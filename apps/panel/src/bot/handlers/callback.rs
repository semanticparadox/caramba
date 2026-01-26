use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, ForceReply, ParseMode, CallbackQuery, ChatId};
use tracing::{info, error};
use crate::AppState;
use crate::bot::utils::escape_md;
use crate::bot::keyboards::{main_menu, terms_keyboard};
use crate::services::pay_service::PaymentType;

pub async fn callback_handler(
    bot: Bot,
    q: CallbackQuery,
    state: AppState
) -> Result<(), teloxide::RequestError> {
    info!("Received callback: {:?}", q.data);
    let user_tg = q.from;
    let tg_id = user_tg.id.0 as i64;

    if let Some(data) = q.data {
        match data.as_str() {
            "set_lang_en" | "set_lang_ru" => {
                let lang = if data.contains("en") { "en" } else { "ru" };
                let _ = bot.answer_callback_query(q.id.clone()).await;

                // Fetch user to get ID
                if let Some(u) = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten() {
                    let _ = state.store_service.update_user_language(u.id, lang).await;
                    
                    // Immediately show terms
                    let terms_text = state.store_service.get_setting("terms_of_service").await.ok().flatten()
                        .unwrap_or_else(|| "Terms of Service...".to_string());
                    
                    // Delete prev message (lang selection) or edit it
                    if let Some(msg) = q.message {
                        let _ = bot.delete_message(msg.chat().id, msg.id()).await;
                        
                        let _ = bot.send_message(msg.chat().id, format!("📜 <b>Terms of Service</b>\n\n{}\n\nPlease accept the terms to continue.", terms_text))
                            .parse_mode(ParseMode::Html)
                            .reply_markup(terms_keyboard())
                            .await
                            .map_err(|e| error!("Failed to send terms after lang choice: {}", e));
                    }
                }
            }

            "accept_terms" => {
                let _ = bot.answer_callback_query(q.id.clone()).await;
                if let Some(u) = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten() {
                        let _ = state.store_service.update_user_terms(u.id).await;
                        
                        if let Some(msg) = q.message {
                            let _ = bot.delete_message(msg.chat().id, msg.id()).await;
                            
                            let welcome_text = format!(
                            "👋 <b>Welcome!</b>\n\n\
                            Use the menu below to manage your VPN subscriptions and digital goods."
                        );
                        let _ = bot.send_message(msg.chat().id, welcome_text)
                            .parse_mode(ParseMode::Html)
                            .reply_markup(main_menu())
                            .await
                            .map_err(|e| error!("Failed to send welcome after terms: {}", e));
                        }
                }
            }

            "decline_terms" => {
                let _ = bot.answer_callback_query(q.id).text("You must accept terms to proceed.").show_alert(true).await;
                // Optional: Ban user or just ignore
            }

            extend if extend.starts_with("extend_sub_") => {
                // Redirect to plans menu
                    let plans = state.store_service.get_active_plans().await.unwrap_or_default();
                    
                    if plans.is_empty() {
                        let _ = bot.answer_callback_query(q.id).text("❌ No active plans available at the moment.").await;
                    } else {
                        let _ = bot.answer_callback_query(q.id).await;
                        let mut response = "💎 *Choose Plan to Extend:*\n\n".to_string();
                        let mut buttons = Vec::new();

                        for plan in plans {
                            response.push_str(&format!(
                                "💎 *{}*\n_{}_\n\n", 
                                escape_md(&plan.name), 
                                escape_md(plan.description.as_deref().unwrap_or("Premium access"))
                            ));

                            let mut duration_row = Vec::new();
                            for dur in plan.durations {
                                let price_major = dur.price / 100;
                                let price_minor = dur.price % 100;
                                duration_row.push(InlineKeyboardButton::callback(
                                    format!("{}d - ${}.{:02}", dur.duration_days, price_major, price_minor),
                                    format!("ext_dur_{}", dur.id)
                                ));
                            }
                            buttons.push(duration_row);
                        }
                        
                        if let Some(msg) = q.message {
                            let _ = bot.send_message(msg.chat().id, response)
                                .parse_mode(ParseMode::MarkdownV2)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await;
                        }
                    }
            }

            "enter_promo" => {
                let _ = bot.answer_callback_query(q.id).await;
                if let Some(msg) = q.message {
                    let _ = bot.send_message(msg.chat().id, "🎟 Enter your Gift Code below:")
                        .reply_markup(ForceReply::new().selective())
                        .await;
                }
            }

            "topup_menu" => {
                let response = "💳 *Choose Top-up Method:*";
                let buttons = vec![
                    vec![InlineKeyboardButton::callback("🪙 CryptoBot", "pay_cryptobot")],
                    vec![InlineKeyboardButton::callback("🔥 NOWPayments (Crypto)", "pay_nowpayments")],
                    vec![InlineKeyboardButton::callback("⭐ Telegram Stars", "pay_stars")],
                ];
                if let Some(msg) = q.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), response)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await;
                }
            }
            
            "pay_cryptobot" => {
                // Ask for amount via buttons or just fixed amounts for now
                let buttons = vec![
                    vec![InlineKeyboardButton::callback("$5", "cb_5"), InlineKeyboardButton::callback("$10", "cb_10")],
                    vec![InlineKeyboardButton::callback("$20", "cb_20"), InlineKeyboardButton::callback("$50", "cb_50")],
                    vec![InlineKeyboardButton::callback("« Back", "topup_menu")],
                ];
                if let Some(msg) = q.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), "🔹 *Select amount for CryptoBot:*").parse_mode(ParseMode::MarkdownV2).reply_markup(InlineKeyboardMarkup::new(buttons)).await;
                }
            }

            cb if cb.starts_with("cb_") => {
                let amount = cb.strip_prefix("cb_").unwrap().parse::<f64>().unwrap_or(0.0);
                let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
                    match state.pay_service.create_cryptobot_invoice(u.id, amount, PaymentType::BalanceTopup).await {
                        Ok(url) => {
                            let buttons = vec![vec![InlineKeyboardButton::url("🔗 Pay with CryptoBot", url.parse().unwrap())]];
                            let _ = bot.answer_callback_query(q.id).await;
                            if let Some(msg) = q.message {
                                let _ = bot.send_message(msg.chat().id, format!("💳 Invoice for *${:.2}* created\\!", amount)).parse_mode(ParseMode::MarkdownV2).reply_markup(InlineKeyboardMarkup::new(buttons)).await;
                            }
                        }
                        Err(e) => {
                            let _ = bot.answer_callback_query(q.id).text(format!("Error: {}", e)).show_alert(true).await;
                        }
                    }
                }
            }

            get_links if get_links.starts_with("get_links_") => {
                    let sub_id = get_links.strip_prefix("get_links_").unwrap_or("0").parse::<i64>().unwrap_or(0);
                    let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                    if let Some(user_tg) = user_db {
                    // Fetch specific subscription
                        if let Ok(subs) = state.store_service.get_user_subscriptions(user_tg.id).await {
                            let sub_opt = subs.iter().find(|s| s.sub.id == sub_id);
                            
                            if let Some(_sub) = sub_opt {
                                // Use the proper link generation service
                                match state.store_service.get_subscription_links(sub_id).await {
                                    Ok(links) => {
                                        if links.is_empty() {
                                            let _ = bot.send_message(ChatId(user_tg.tg_id), "❌ No connection links available for your subscription yet.").await;
                                        } else {
                                            let mut response = "🔗 *Your Connection Links:*\n\n".to_string();
                                            for link in links {
                                                response.push_str(&format!("`{}`\n\n", escape_md(&link)));
                                            }
                                            let _ = bot.send_message(ChatId(user_tg.tg_id), response).parse_mode(ParseMode::MarkdownV2).await;
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to get subscription links: {}", e);
                                        let _ = bot.send_message(ChatId(user_tg.tg_id), "❌ Failed to generate connection links.").await;
                                    }
                                }
                            } else {
                                let _ = bot.answer_callback_query(q.id.clone()).text("❌ Subscription not found").await;
                            }
                        }
                    }
                    let _ = bot.answer_callback_query(q.id.clone()).await;
            }

            activate if activate.starts_with("activate_") => {
                let sub_id = activate.strip_prefix("activate_").unwrap_or("0").parse::<i64>().unwrap_or(0);
                let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                
                if let Some(u) = user_db {
                    match state.store_service.activate_subscription(sub_id, u.id).await {
                        Ok(sub) => {
                            let _ = bot.answer_callback_query(q.id).text("✅ Activated!").await;
                            let orch = state.orchestration_service.clone();
                            tokio::spawn(async move {
                                // Agents pull config automatically - no sync needed
                            });
                            if let Some(msg) = q.message {
                                let _ = bot.send_message(msg.chat().id, format!("🚀 *Subscription Activated!*\nExpires: `{}`", sub.expires_at.format("%Y-%m-%d"))).parse_mode(ParseMode::MarkdownV2).await;
                            }
                        }
                        Err(e) => {
                            error!("Activation failed: {}", e);
                            let _ = bot.answer_callback_query(q.id).text(format!("❌ Error: {}", e)).show_alert(true).await;
                        }
                    }
                }
            }

            "my_gifts" => {
                let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
                    let _ = bot.answer_callback_query(q.id.clone()).await;
                    match state.store_service.get_user_gift_codes(u.id).await {
                        Ok(codes) => {
                            if codes.is_empty() {
                                    if let Some(msg) = q.message {
                                        let _ = bot.send_message(msg.chat().id, "🎁 You have no unredeemed gift codes.").await;
                                    }
                            } else {
                                let mut response = "🎁 *My Gift Codes* \\(Unredeemed\\):\n\n".to_string();
                                for code in codes {
                                    response.push_str(&format!("🎟 `{}`\n   Days: {}\n\n", code.code, code.duration_days.unwrap_or(0)));
                                }
                                if let Some(msg) = q.message {
                                        if let Err(e) = bot.send_message(msg.chat().id, response).parse_mode(ParseMode::MarkdownV2).await {
                                            error!("Failed to send gift codes: {}", e);
                                        }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Fetch gifts error: {}", e);
                            if let Some(msg) = q.message {
                                let _ = bot.send_message(msg.chat().id, "❌ Error: Failed to fetch your gift codes.").await;
                            }
                        }
                    }
                } else {
                        let _ = bot.answer_callback_query(q.id).await;
                }
            }

            edit_note if edit_note.starts_with("edit_note_") => {
                    let sub_id = edit_note.strip_prefix("edit_note_").unwrap_or("0");
                    let _ = bot.answer_callback_query(q.id).await;
                    if let Some(msg) = q.message {
                        let _ = bot.send_message(msg.chat().id, format!("Reply to this message with your note for Subscription #{}.", sub_id))
                        .reply_markup(ForceReply::new().selective())
                        .await;
                    }
            }

            devices if devices.starts_with("devices_") => {
                let sub_id = devices.strip_prefix("devices_").unwrap_or("0").parse::<i64>().unwrap_or(0);
                let _ = bot.answer_callback_query(q.id).await;
                
                if let Some(msg) = q.message {
                    // Get device limit + plan info
                    let device_limit = state.store_service.get_subscription_device_limit(sub_id).await.unwrap_or(3);
                    
                    // Get active IPs (last 15 minutes)
                    let active_ips = state.store_service.get_subscription_active_ips(sub_id).await.unwrap_or_default();
                    
                    let mut response = format!("📱 *CONNECTED DEVICES*\\n\\n");
                    response.push_str(&format!("🔢 *Device Limit:* `{}`\\n", device_limit));
                    response.push_str(&format!("✅ *Active Devices:* `{}`\\n\\n", active_ips.len()));
                    
                    if active_ips.is_empty() {
                        response.push_str("_No devices currently connected\\._\\n\\n");
                        response.push_str("_Devices will appear here when you connect to the VPN\\._");
                    } else {
                        response.push_str("🌐 *Recent Connections:*\\n");
                        for (idx, ip_record) in active_ips.iter().take(10).enumerate() {
                            let time_ago = chrono::Utc::now() - ip_record.last_seen_at;
                            let minutes_ago = time_ago.num_minutes();
                            
                            let time_str = if minutes_ago < 1 {
                                "just now".to_string()
                            } else if minutes_ago < 60 {
                                format!("{} min ago", minutes_ago)
                            } else {
                                format!("{} hr ago", minutes_ago / 60)
                            };
                            
                            response.push_str(&format!("{}\\. `{}` _{}_\\n", idx + 1, escape_md(&ip_record.client_ip), time_str));
                        }
                        
                        if active_ips.len() > device_limit as usize {
                            response.push_str(&format!("\\n⚠️ *Warning:* You have exceeded your device limit\\!"));
                        }
                    }
                    
                    let keyboard = InlineKeyboardMarkup::new(vec![
                        vec![InlineKeyboardButton::callback("« Back to Services", format!("myservices_page_0"))],
                    ]);
                    
                    let _ = bot.send_message(msg.chat().id, response)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(keyboard)
                        .await;
                }
            }

            buy_page if buy_page.starts_with("buy_page_") => {
                let page = buy_page.strip_prefix("buy_page_").unwrap_or("0").parse::<usize>().unwrap_or(0);
                let plans = state.store_service.get_active_plans().await.unwrap_or_default();
                
                if plans.is_empty() {
                    let _ = bot.answer_callback_query(q.id).text("❌ No active plans available.").await;
                } else {
                    let _ = bot.answer_callback_query(q.id).await;
                    let chat_id = q.message.map(|m| m.chat().id).unwrap_or(ChatId(0));
                    if chat_id.0 == 0 { return Ok(()); }

                    let limit = 3;
                    let total_pages = (plans.len() as f64 / limit as f64).ceil() as usize;
                    let page = if page >= total_pages { 0 } else { page };
                    let start = page * limit;
                    let end = std::cmp::min(start + limit, plans.len());
                    let page_plans = &plans[start..end];

                    let _ = bot.send_message(chat_id, format!("💎 *Showcase:* Page {}/{}", page + 1, total_pages)).parse_mode(ParseMode::MarkdownV2).await;

                    for (i, plan) in page_plans.iter().enumerate() {
                        let mut text = format!("💎 *{}*\n\n", escape_md(&plan.name));
                        if let Some(desc) = &plan.description {
                            text.push_str(&format!("_{}_\n", escape_md(desc)));
                        }

                        let mut buttons = Vec::new();
                        let mut duration_row = Vec::new();
                        for dur in &plan.durations {
                            let price_major = dur.price / 100;
                            let price_minor = dur.price % 100;
                            let label = if dur.duration_days == 0 {
                                format!("🚀 Traffic Plan - ${}.{:02}", price_major, price_minor)
                            } else {
                                format!("{}d - ${}.{:02}", dur.duration_days, price_major, price_minor)
                            };
                            duration_row.push(InlineKeyboardButton::callback(
                                label,
                                format!("buy_dur_{}", dur.id)
                            ));
                        }
                        buttons.push(duration_row);

                        let is_last_in_batch = i == (page_plans.len() - 1);
                        if is_last_in_batch && total_pages > 1 {
                            let mut nav_row = Vec::new();
                            if page > 0 {
                                nav_row.push(InlineKeyboardButton::callback("⬅️ Back", format!("buy_page_{}", page - 1)));
                            }
                            if page + 1 < total_pages {
                                nav_row.push(InlineKeyboardButton::callback("Next ➡️", format!("buy_page_{}", page + 1)));
                            }
                            if !nav_row.is_empty() {
                                buttons.push(nav_row);
                            }
                        }

                        let _ = bot.send_message(chat_id, text)
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await;
                    }
                }
            }

            myservices_page if myservices_page.starts_with("myservices_page_") => {
                let page = myservices_page.strip_prefix("myservices_page_").unwrap_or("0").parse::<usize>().unwrap_or(0);
                let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();

                if let Some(user) = user_db {
                    if let Ok(subs) = state.store_service.get_user_subscriptions(user.id).await {
                        // Sort subs (same logic as main handler)
                        let mut sorted_subs = subs.clone();
                        sorted_subs.sort_by(|a, b| match (a.sub.status.as_str(), b.sub.status.as_str()) {
                            ("pending", "active") => std::cmp::Ordering::Less,
                            ("active", "pending") => std::cmp::Ordering::Greater,
                            _ => b.sub.created_at.cmp(&a.sub.created_at),
                        });

                        if !sorted_subs.is_empty() {
                            let total_pages = sorted_subs.len();
                            // Ensure page is valid
                            let page = if page >= total_pages { 0 } else { page };
                            let sub = &sorted_subs[page];

                            let mut response = "🔐 *MY SERVICES*\n\n".to_string();
                            let status_icon = if sub.sub.status == "active" { "✅" } else { "⏳" };
                            response.push_str(&format!("🔹 *Subscription \\#{}/{:}*\n", page + 1, total_pages));
                            response.push_str(&format!("   💎 *Plan:* {}\n", escape_md(&sub.plan_name)));
                            if let Some(desc) = &sub.plan_description {
                                response.push_str(&format!("   _{}_\n", escape_md(desc)));
                            }
                            response.push_str(&format!("   🔑 *Status:* {} `{}`\n", status_icon, sub.sub.status));

                            // Traffic
                            let used_gb = sub.sub.used_traffic as f64 / 1024.0 / 1024.0 / 1024.0;
                            if let Some(limit) = sub.traffic_limit_gb {
                                    if limit == 0 {
                                        response.push_str(&format!("   📊 *Traffic:* `{:.2} GB / ∞`\n", used_gb));
                                    } else {
                                        response.push_str(&format!("   📊 *Traffic:* `{:.2} GB / {} GB`\n", used_gb, limit));
                                    }
                            } else {
                                    response.push_str(&format!("   📊 *Traffic Used:* `{:.2} GB`\n", used_gb));
                            }
                            
                            if sub.sub.status == "active" {
                                response.push_str(&format!("   ⌛ *Expires:* `{}`\n", sub.sub.expires_at.format("%Y-%m-%d")));
                            } else {
                                let duration = sub.sub.expires_at - sub.sub.created_at;
                                response.push_str(&format!("   ⏱ *Duration:* `{} days` \\(starts on activation\\)\n", duration.num_days()));
                            }
                            response.push_str("\n");
                            if let Some(note) = &sub.sub.note {
                                response.push_str(&format!("📝 *Note:* {}\n\n", escape_md(note)));
                            }

                            // Navigation & Actions
                            let mut buttons = Vec::new();
                            
                                // Edit Note Button
                            buttons.push(vec![InlineKeyboardButton::callback("📝 Edit Note", format!("edit_note_{}", sub.sub.id))]);

                            // Action Buttons
                            if sub.sub.status == "active" {
                                buttons.push(vec![
                                    InlineKeyboardButton::callback("🔗 Get Config", format!("get_links_{}", sub.sub.id)),
                                    InlineKeyboardButton::callback("⏳ Extend", format!("extend_sub_{}", sub.sub.id))
                                ]);
                            } else if sub.sub.status == "pending" {
                                buttons.push(vec![
                                    InlineKeyboardButton::callback("▶️ Activate", format!("activate_{}", sub.sub.id)),
                                    InlineKeyboardButton::callback("🎁 Make Gift Code", format!("gift_init_{}", sub.sub.id))
                                ]);
                            }

                            // Navigation Row
                            let mut nav_row = Vec::new();
                            if total_pages > 1 {
                                let prev_page = if page > 0 { page - 1 } else { total_pages - 1 };
                                let next_page = if page < total_pages - 1 { page + 1 } else { 0 };
                                
                                nav_row.push(InlineKeyboardButton::callback("⬅️ Prev", format!("myservices_page_{}", prev_page)));
                                nav_row.push(InlineKeyboardButton::callback(format!("{}/{}", page + 1, total_pages), "ignore"));
                                nav_row.push(InlineKeyboardButton::callback("Next ➡️", format!("myservices_page_{}", next_page)));
                            }
                            if !nav_row.is_empty() {
                                buttons.push(nav_row);
                            }
                            
                            // My Gifts Link
                            buttons.push(vec![InlineKeyboardButton::callback("🎁 My Gift Codes", "my_gifts")]);

                            // Edit the message
                            if let Some(msg) = q.message {
                                let _ = bot.edit_message_text(msg.chat().id, msg.id(), response)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                    }
                }
                let _ = bot.answer_callback_query(q.id).await;
            }

            gift if gift.starts_with("gift_init_") => {
                    let sub_id = gift.strip_prefix("gift_init_").unwrap_or("0").parse::<i64>().unwrap_or(0);
                    let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                    
                    if let Some(u) = user_db {
                        match state.store_service.convert_subscription_to_gift(sub_id, u.id).await {
                            Ok(code) => {
                                let response = format!("🎁 *Gift Code Created!*\n\nCode: `{}`\n\nShare this code with anyone. They can redeem it by sending it to the bot.", code);
                                if let Some(msg) = q.message {
                                    let _ = bot.send_message(msg.chat().id, response).parse_mode(ParseMode::MarkdownV2).await;
                                }
                                let _ = bot.answer_callback_query(q.id).text("✅ Code Generated!").await;
                            },
                            Err(e) => {
                                let _ = bot.answer_callback_query(q.id).text(format!("❌ Error: {}", e)).show_alert(true).await;
                            }
                        }
                    }
            }

            transfer if transfer.starts_with("transfer_init_") => {
                    let sub_id = transfer.strip_prefix("transfer_init_").unwrap_or("0");
                    if let Some(msg) = q.message {
                        let _ = bot.send_message(msg.chat().id, format!("➡️ *Transfer Subscription*\n\nPlease reply to this message with the *Username* of the user you want to transfer Subscription \\#{} to (e.g., @username).", sub_id))
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(ForceReply::new().selective())
                            .await;
                    }
            }

            buy_dur if buy_dur.starts_with("buy_dur_") => {
                let id_str = buy_dur.strip_prefix("buy_dur_").unwrap();
                if let Ok(duration_id) = id_str.parse::<i64>() {
                    let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                    if let Some(u) = user_db {
                        match state.store_service.purchase_plan(u.id, duration_id).await {
                            Ok(_sub) => {
                                let _ = bot.answer_callback_query(q.id).text("✅ Purchase successful!").await;
                                let orch = state.orchestration_service.clone();
                                tokio::spawn(async move {
                                    // Agents pull config automatically - no sync needed
                                });
                                if let Some(msg) = q.message {
                                    let _ = bot.send_message(msg.chat().id, "✅ *Purchase Successful\\!*\n\nYour subscription is now *Pending*.\nGo to *My Services* to activate it when you are ready.").parse_mode(ParseMode::MarkdownV2).await;
                                }
                            }
                            Err(e) => {
                                error!("Purchase failed for user {}: {}", u.id, e);
                                let _ = bot.answer_callback_query(q.id).text(format!("❌ Error: {}", e)).show_alert(true).await;
                            }
                        }
                    } else {
                        error!("User not found for purchase: {}", tg_id);
                    }
                }
            }

            ext_dur if ext_dur.starts_with("ext_dur_") => {
                let id_str = ext_dur.strip_prefix("ext_dur_").unwrap();
                if let Ok(duration_id) = id_str.parse::<i64>() {
                    let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                    if let Some(u) = user_db {
                        match state.store_service.extend_subscription(u.id, duration_id).await {
                            Ok(sub) => {
                                let _ = bot.answer_callback_query(q.id).text("✅ Extension successful!").await;
                                // Sync logic if needed, usually extension doesn't change UUIDs but good to sync
                                let orch = state.orchestration_service.clone();
                                tokio::spawn(async move {
                                    // Agents pull config automatically - no sync needed
                                });

                                if let Some(msg) = q.message {
                                    let _ = bot.send_message(msg.chat().id, format!("✅ *Subscription Extended!*\nNew Expiry: `{}`", sub.expires_at.format("%Y-%m-%d"))).parse_mode(ParseMode::MarkdownV2).await;
                                }
                            }
                            Err(e) => {
                                error!("Extension failed for user {}: {}", u.id, e);
                                let _ = bot.answer_callback_query(q.id).text(format!("❌ Error: {}", e)).show_alert(true).await;
                            }
                        }
                    } else {
                        error!("User not found for extension: {}", tg_id);
                    }
                }
            }
            
            // Store Product Purchase
            buyprod if buyprod.starts_with("buyprod_") => {
                    let prod_id = buyprod.strip_prefix("buyprod_").unwrap().parse::<i64>().unwrap_or(0);
                    let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                    
                    if let Some(u) = user_db {
                        match state.store_service.purchase_product_with_balance(u.id, prod_id).await {
                            Ok(product) => {
                                let _ = bot.answer_callback_query(q.id).text("✅ Paid!").await;
                                if let Some(msg) = q.message {
                                    if let Some(content) = product.content {
                                        let _ = bot.send_message(msg.chat().id, format!("✅ *Purchase Successful!*\n\n📦 *{}*\n\n📋 *Content:*\n`{}`", escape_md(&product.name), escape_md(&content)))
                                            .parse_mode(ParseMode::MarkdownV2)
                                            .await;
                                    } else {
                                        let _ = bot.send_message(msg.chat().id, format!("✅ *Purchase Successful!*\n\n📦 *{}*\n\n(No digital content attached, contact support if expected)", escape_md(&product.name)))
                                            .parse_mode(ParseMode::MarkdownV2)
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                    let _ = bot.answer_callback_query(q.id).text(format!("❌ Failed: {}", e)).show_alert(true).await;
                            }
                        }
                    }
            }

            // Store Browsing
            store if store.starts_with("store_") => {
                let chat_id = q.message.as_ref().map(|m| m.chat().id).unwrap_or(ChatId(0));
                if chat_id.0 == 0 { return Ok(()); } // Safety

                if let Some(cat_id_str) = store.strip_prefix("store_cat_") {
                        if let Ok(cat_id) = cat_id_str.parse::<i64>() {
                            let products = state.store_service.get_products_by_category(cat_id).await.unwrap_or_default();
                            if products.is_empty() {
                                let _ = bot.answer_callback_query(q.id).text("Category is empty").await;
                            } else {
                                let _ = bot.answer_callback_query(q.id).await;
                                // Showcase style: separate message per product
                                for product in products {
                                    let price = product.price as f64 / 100.0;
                                    let text = format!("📦 *{}*\n\n{}\n\n💰 Price: *${:.2}*", 
                                        escape_md(&product.name), 
                                        escape_md(product.description.as_deref().unwrap_or("No description")), 
                                        price
                                    );
                                    let buttons = vec![vec![InlineKeyboardButton::callback(format!("💳 Buy for ${:.2}", price), format!("buyprod_{}", product.id))]];
                                    let _ = bot.send_message(chat_id, text)
                                        .parse_mode(ParseMode::MarkdownV2)
                                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                                        .await;
                                }
                                // Add back button in a small separate message
                                let nav = vec![vec![InlineKeyboardButton::callback("🔙 Back to Categories", "store_home")]];
                                let _ = bot.send_message(chat_id, "---")
                                    .reply_markup(InlineKeyboardMarkup::new(nav))
                                    .await;
                            }
                        }
                } else if let Some(prod_id_str) = store.strip_prefix("store_prod_") {
                        if let Ok(prod_id) = prod_id_str.parse::<i64>() {
                            if let Ok(product) = state.store_service.get_product(prod_id).await {
                                let _ = bot.answer_callback_query(q.id).await;
                                let price = product.price as f64 / 100.0;
                                let text = format!("📦 *{}*\n\n{}\n\n💰 Price: *${:.2}*", 
                                    escape_md(&product.name), 
                                    escape_md(product.description.as_deref().unwrap_or("No description")), 
                                    price
                                );
                                
                                let buttons = vec![
                                    vec![InlineKeyboardButton::callback(format!("💳 Buy for ${:.2}", price), format!("buyprod_{}", product.id))],
                                    vec![InlineKeyboardButton::callback("🔙 Back", format!("store_cat_{}", product.category_id.unwrap_or(0)))],
                                ];
                                
                                let _ = bot.edit_message_text(chat_id, q.message.unwrap().id(), text)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            } else {
                                let _ = bot.answer_callback_query(q.id).text("Product not found").await;
                            }
                        }
                } else if store == "store_home" {
                        let categories = state.store_service.get_categories().await.unwrap_or_default();
                        let mut buttons = Vec::new();
                        for cat in categories {
                            buttons.push(vec![InlineKeyboardButton::callback(cat.name, format!("store_cat_{}", cat.id))]);
                        }
                        let kb = InlineKeyboardMarkup::new(buttons);
                        let _ = bot.edit_message_text(chat_id, q.message.unwrap().id(), "📦 *Digital Store Categories:*")
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(kb)
                            .await;
                }
            }

            "edit_ref_code" => {
                let _ = bot.answer_callback_query(q.id).await;
                if let Some(msg) = q.message {
                    let text = "🔗 *EDIT REFERRAL ALIAS*\n\n\
                        Please reply to this message with your new referral code \\(alias\\)\\.\n\n\
                        *Requirements:*\n\
                        \\- Unique across all users\n\
                        \\- Only letters, numbers, and underscores\n\
                        \\- 3 to 32 characters";
                    
                    if let Err(e) = bot.send_message(msg.chat().id, text)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(ForceReply::new().selective())
                        .await {
                            error!("CRITICAL: Failed to send edit_ref_code prompt: {}", e);
                        }
                }
            }

            "enter_referrer" => {
                let _ = bot.answer_callback_query(q.id).await;
                if let Some(msg) = q.message {
                    let text = "🎁 *Enter Referrer Code*\n\n\
                        Please reply to this message with the referral code of the person who invited you\\.";
                    
                    if let Err(e) = bot.send_message(msg.chat().id, text)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(ForceReply::new().selective())
                        .await {
                            error!("CRITICAL: Failed to send enter_referrer prompt: {}", e);
                        }
                }
            }

            _ => {
                let _ = bot.answer_callback_query(q.id).text("Feature not yet implemented.").await;
            }
        }
    }
    Ok::<_, teloxide::RequestError>(())
}

use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, ForceReply, ParseMode, CallbackQuery, ChatId, LabeledPrice};
use tracing::{info, error};
use crate::AppState;
use crate::bot::utils::escape_md;
use crate::bot::keyboards::{main_menu, terms_keyboard};
use crate::models::payment::PaymentType;

pub async fn callback_handler(
    bot: Bot,
    q: CallbackQuery,
    state: AppState
) -> Result<(), teloxide::RequestError> {
    info!("Received callback: {:?}", q.data);
    let callback_id = q.id.clone();
    let user_tg = q.from;
    let tg_id = user_tg.id.0 as i64;

    if let Some(data) = q.data {
        match data.as_str() {
            "set_lang_en" | "set_lang_ru" => {
                let lang = if data.contains("en") { "en" } else { "ru" };
                let _ = bot.answer_callback_query(callback_id).await;

                // Fetch user to get ID
                let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
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
                let _ = bot.answer_callback_query(callback_id).await;
                let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
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
                            .map(|m| {
                                let state = state.clone();
                                let uid = u.id;
                                tokio::spawn(async move {
                                    let _ = state.store_service.update_last_bot_msg_id(uid, m.id.0).await;
                                });
                            })
                            .map_err(|e| error!("Failed to send welcome after terms: {}", e));
                        }
                }
            }

            "decline_terms" => {
                let _ = bot.answer_callback_query(callback_id).text("You must accept terms to proceed.").show_alert(true).await;
                // Optional: Ban user or just ignore
            }

            extend if extend.starts_with("extend_sub_") => {
                // Redirect to plans menu
                    let plans = state.store_service.get_active_plans().await.unwrap_or_default();
                    
                    if plans.is_empty() {
                        let _ = bot.answer_callback_query(callback_id).text("❌ No active plans available at the moment.").await;
                    } else {
                        let _ = bot.answer_callback_query(callback_id).await;
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
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let _ = bot.send_message(msg.chat().id, "🎟 Enter your Gift Code below:")
                        .reply_markup(ForceReply::new().selective())
                        .await;
                }
            }

            "topup_menu" => {
                let response = "💳 *Choose Top-up Method:*";
                let buttons = vec![
                    vec![InlineKeyboardButton::callback("🪙 Crypto (USDT/TON)", "pay_cryptobot")],
                    vec![InlineKeyboardButton::callback("⚡ Crypto (Altcoins)", "pay_nowpayments")],
                    vec![InlineKeyboardButton::callback("🇷🇺 Cards (RUB/SBP)", "pay_crystal")],
                    vec![InlineKeyboardButton::callback("🌍 Global Cards (USD)", "pay_stripe")],
                    vec![InlineKeyboardButton::callback("⭐️ Telegram Stars", "pay_stars")],
                ];
                if let Some(msg) = q.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), response)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await;
                }
            }

            // Amount Selection Menus
            "pay_cryptobot" => {
                let buttons = make_amount_keyboard("cb");
                if let Some(msg) = q.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), "🔹 *Select amount for CryptoBot:*")
                        .parse_mode(ParseMode::MarkdownV2).reply_markup(buttons).await;
                }
            }
            "pay_nowpayments" => {
                let buttons = make_amount_keyboard("np");
                if let Some(msg) = q.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), "🔹 *Select amount for NOWPayments:*")
                        .parse_mode(ParseMode::MarkdownV2).reply_markup(buttons).await;
                }
            }
            "pay_crystal" => {
                let buttons = make_amount_keyboard("cp");
                if let Some(msg) = q.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), "🔹 *Select amount for CrystalPay (Cards/SBP):*")
                        .parse_mode(ParseMode::MarkdownV2).reply_markup(buttons).await;
                }
            }
            "pay_stripe" => {
                let buttons = make_amount_keyboard("str");
                if let Some(msg) = q.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), "🔹 *Select amount for Stripe:*")
                        .parse_mode(ParseMode::MarkdownV2).reply_markup(buttons).await;
                }
            }
            "pay_stars" => {
                let buttons = make_amount_keyboard("star");
                if let Some(msg) = q.message {
                    let _ = bot.edit_message_text(msg.chat().id, msg.id(), "🔹 *Select amount via Stars:*")
                        .parse_mode(ParseMode::MarkdownV2).reply_markup(buttons).await;
                }
            }

            // Handlers
            cb if cb.starts_with("cb_") => {
                let amount = cb.strip_prefix("cb_").unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
                    match state.pay_service.create_cryptobot_invoice(u.id, amount, PaymentType::BalanceTopup).await {
                        Ok(url) => {
                             let buttons = vec![vec![InlineKeyboardButton::url("🔗 Pay with CryptoBot", url.parse().unwrap())]];
                             let _ = bot.answer_callback_query(callback_id).await;
                             if let Some(msg) = q.message {
                                 let _ = bot.send_message(msg.chat().id, format!("💳 Invoice for *${:.2}* created\\!", amount)).parse_mode(ParseMode::MarkdownV2).reply_markup(InlineKeyboardMarkup::new(buttons)).await;
                             }
                        }
                        Err(e) => {
                             let _ = bot.answer_callback_query(callback_id).text(format!("Error: {}", e)).show_alert(true).await;
                        }
                    }
                }
            }
            np if np.starts_with("np_") => {
                let amount = np.strip_prefix("np_").unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
                    match state.pay_service.create_nowpayments_invoice(u.id, amount, PaymentType::BalanceTopup).await {
                        Ok(url) => {
                             let buttons = vec![vec![InlineKeyboardButton::url("🔗 Pay with NOWPayments", url.parse().unwrap())]];
                             let _ = bot.answer_callback_query(callback_id).await;
                             if let Some(msg) = q.message {
                                 let _ = bot.send_message(msg.chat().id, format!("💳 Invoice for *${:.2}* created\\!", amount)).parse_mode(ParseMode::MarkdownV2).reply_markup(InlineKeyboardMarkup::new(buttons)).await;
                             }
                        }
                        Err(e) => {
                             let _ = bot.answer_callback_query(callback_id).text(format!("Error: {}", e)).show_alert(true).await;
                        }
                    }
                }
            }
            cp if cp.starts_with("cp_") => {
                let amount = cp.strip_prefix("cp_").unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
                    match state.pay_service.create_crystalpay_invoice(u.id, amount, PaymentType::BalanceTopup).await {
                        Ok(url) => {
                             let buttons = vec![vec![InlineKeyboardButton::url("🔗 Pay with Card (CrystalPay)", url.parse().unwrap())]];
                             let _ = bot.answer_callback_query(callback_id).await;
                             if let Some(msg) = q.message {
                                 let _ = bot.send_message(msg.chat().id, format!("💳 Invoice for *${:.2}* created\\!", amount)).parse_mode(ParseMode::MarkdownV2).reply_markup(InlineKeyboardMarkup::new(buttons)).await;
                             }
                        }
                        Err(e) => {
                             let _ = bot.answer_callback_query(callback_id).text(format!("Error: {}", e)).show_alert(true).await;
                        }
                    }
                }
            }
            str_pay if str_pay.starts_with("str_") => {
                let amount = str_pay.strip_prefix("str_").unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
                    match state.pay_service.create_stripe_session(u.id, amount, PaymentType::BalanceTopup).await {
                        Ok(url) => {
                             let buttons = vec![vec![InlineKeyboardButton::url("🔗 Pay with Stripe", url.parse().unwrap())]];
                             let _ = bot.answer_callback_query(callback_id).await;
                             if let Some(msg) = q.message {
                                 let _ = bot.send_message(msg.chat().id, format!("💳 Invoice for *${:.2}* created\\!", amount)).parse_mode(ParseMode::MarkdownV2).reply_markup(InlineKeyboardMarkup::new(buttons)).await;
                             }
                        }
                        Err(e) => {
                             let _ = bot.answer_callback_query(callback_id).text(format!("Error: {}", e)).show_alert(true).await;
                        }
                    }
                }
            }
            
            star if star.starts_with("star_") => {
                let amount_usd = star.strip_prefix("star_").unwrap_or("0").parse::<f64>().unwrap_or(0.0);
                // 1 USD approx 50 XTR (Telegram Stars). Rate varies.
                // Official: 1 XTR ~ $0.02 USD (purchase cost for user usually higher).
                // Let's charge 50 XTR per $1 USD balance.
                let xtr_amount = (amount_usd * 50.0) as u32; 
                
                let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
                    let payload = PaymentType::BalanceTopup.to_payload_string(u.id);
                    let prices = vec![LabeledPrice { label: "Top-up".to_string(), amount: xtr_amount as u32 }];
                    
                    if let Some(msg) = q.message {
                        // Delete menu message
                        let _ = bot.delete_message(msg.chat().id, msg.id()).await;
                        
                        let _ = bot.send_invoice(
                            msg.chat().id,
                            "Balance Top-up",
                            format!("Top-up balance by ${}", amount_usd),
                            payload,
                            "XTR",
                            prices
                        ).await;
                    }
                }
            }

            get_links if get_links.starts_with("get_links_") => {
                    let sub_id = get_links.strip_prefix("get_links_").unwrap_or("0").parse::<i64>().unwrap_or(0);
                    let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
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
                                            
                                            // Add Subscription Page Link
                                            let sub_domain = state.settings.get_or_default("subscription_domain", "").await;
                                            let base_domain = if !sub_domain.is_empty() {
                                                 sub_domain
                                            } else {
                                                 let panel = state.settings.get_or_default("panel_url", "").await;
                                                 if !panel.is_empty() { 
                                                     panel 
                                                 } else { 
                                                     // Try env var, otherwise localhost with a warning
                                                     std::env::var("PANEL_URL").unwrap_or_else(|_| "localhost".to_string())
                                                 }
                                            };
                                            
                                            let is_localhost = base_domain == "localhost";
                                            let base_url = if base_domain.starts_with("http") { 
                                                base_domain 
                                            } else { 
                                                format!("https://{}", base_domain) 
                                            };
                                            
                                            let sub_url = format!("{}/sub/{}", base_url, _sub.sub.subscription_uuid);
                                            
                                            response.push_str(&format!("🌍 *Subscription Page:*\n`{}`\n", escape_md(&sub_url)));
                                            if is_localhost {
                                                response.push_str("⚠️ _Admin: Set PANEL_URL or subscription_domain setting\\!_\n\n");
                                            } else {
                                                response.push_str("\n");
                                            }

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
                                let _ = bot.answer_callback_query(callback_id).text("❌ Subscription not found").await;
                            }
                        }
                    }
            }
            
            get_config if get_config.starts_with("get_config_") => {
                let _sub_id = get_config.strip_prefix("get_config_").unwrap_or("0");
                let _ = bot.answer_callback_query(callback_id).text("Generating profile...").await;
                
                let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
                    match state.store_service.generate_subscription_file(u.id).await {
                        Ok(json_content) => {
                            let data = json_content.into_bytes();
                            let input_file = teloxide::types::InputFile::memory(data).file_name("caramba_v2_profile.json");
                            
                            if let Some(msg) = q.message {
                                let _ = bot.send_document(msg.chat().id, input_file)
                                    .caption("📂 <b>Your CARAMBA Profile</b>\n\nImport this file into Sing-box, Nekobox, or Hiddify.\nIt contains automatic server selection and failover.")
                                    .parse_mode(ParseMode::Html)
                                    .await;
                            }
                        }
                        Err(e) => {
                             error!("Failed to generate config: {}", e);
                             let _ = bot.send_message(ChatId(tg_id), "❌ Failed to generate profile file.").await;
                        }
                    }
                }
            }

            activate if activate.starts_with("activate_") => {
                let sub_id = activate.strip_prefix("activate_").unwrap_or("0").parse::<i64>().unwrap_or(0);
                let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                
                if let Some(u) = user_db {
                    match state.store_service.activate_subscription(sub_id, u.id).await {
                        Ok(sub) => {
                                let _ = bot.answer_callback_query(callback_id).text("✅ Activated!").await;
                            
                            // Trigger instant config update for ALL nodes serving this plan
                            let pubsub = state.pubsub.clone();
                            let pool = state.store_service.get_pool();
                            let plan_id = sub.plan_id;
                            tokio::spawn(async move {
                                // Find all nodes that serve this plan via plan_nodes or plan groups
                                let node_ids: Vec<i64> = sqlx::query_scalar(
                                    "SELECT DISTINCT n.id FROM nodes n
                                     JOIN node_group_members ngm ON n.id = ngm.node_id
                                     JOIN plan_groups pg ON pg.group_id = ngm.group_id
                                     WHERE pg.plan_id = ? AND n.is_enabled = 1"
                                )
                                .bind(plan_id)
                                .fetch_all(&pool)
                                .await
                                .unwrap_or_default();

                                if node_ids.is_empty() {
                                    info!("⚠️ No nodes found for plan {} — skipping auto-sync", plan_id);
                                } else {
                                    info!("🔄 Auto-syncing {} nodes for activated plan {}", node_ids.len(), plan_id);
                                    for nid in node_ids {
                                        if let Err(e) = pubsub.publish(&format!("node_events:{}", nid), "update").await {
                                            error!("Failed to publish node update for {}: {}", nid, e);
                                        }
                                    }
                                }
                            });
                            
                            if let Some(msg) = q.message {
                                let _ = bot.send_message(msg.chat().id, format!("🚀 *Subscription Activated!*\nExpires: `{}`", sub.expires_at.format("%Y-%m-%d"))).parse_mode(ParseMode::MarkdownV2).await;
                            }
                        }
                        Err(e) => {
                            error!("Activation failed: {}", e);
                            let _ = bot.answer_callback_query(callback_id).text(format!("❌ Error: {}", e)).show_alert(true).await;
                        }
                    }
                }
            }

            "my_gifts" => {
                let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                if let Some(u) = user_db {
                    let _ = bot.answer_callback_query(callback_id).await;
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
                        let _ = bot.answer_callback_query(callback_id).await;
                }
            }

            edit_note if edit_note.starts_with("edit_note_") => {
                    let sub_id = edit_note.strip_prefix("edit_note_").unwrap_or("0");
                    let _ = bot.answer_callback_query(callback_id).await;
                    if let Some(msg) = q.message {
                        let _ = bot.send_message(msg.chat().id, format!("Reply to this message with your note for Subscription #{}.", sub_id))
                        .reply_markup(ForceReply::new().selective())
                        .await;
                    }
            }

            devices if devices.starts_with("devices_") => {
                let sub_id = devices.strip_prefix("devices_").unwrap_or("0").parse::<i64>().unwrap_or(0);
                let _ = bot.answer_callback_query(callback_id).await;
                
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

            buy_plan_idx if buy_plan_idx.starts_with("buy_plan_idx_") => {
                let index = buy_plan_idx.strip_prefix("buy_plan_idx_").unwrap_or("0").parse::<usize>().unwrap_or(0);
                let plans = state.store_service.get_active_plans().await.unwrap_or_default();
                
                if plans.is_empty() {
                    let _ = bot.answer_callback_query(callback_id).text("❌ No active plans available.").await;
                } else {
                    let _ = bot.answer_callback_query(callback_id).await;
                    let total_plans = plans.len();
                    // Safety check
                    let index = if index >= total_plans { 0 } else { index };
                    let plan = &plans[index];

                    let mut text = format!("💎 *{}* \\({}/{}\\)\n\n", escape_md(&plan.name), index + 1, total_plans);
                    if let Some(desc) = &plan.description {
                        text.push_str(&format!("_{}_\n", escape_md(desc)));
                    }

                    let mut buttons = Vec::new();
                    
                    // Duration Buttons
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
                     if !duration_row.is_empty() {
                         buttons.push(duration_row);
                    }

                    // Navigation
                    if total_plans > 1 {
                        let mut nav_row = Vec::new();
                        let next_idx = if index + 1 < total_plans { index + 1 } else { 0 };
                        let prev_idx = if index > 0 { index - 1 } else { total_plans - 1 };
                        
                        nav_row.push(InlineKeyboardButton::callback("⬅️", format!("buy_plan_idx_{}", prev_idx)));
                        nav_row.push(InlineKeyboardButton::callback(format!("{}/{}", index + 1, total_plans), "noop"));
                        nav_row.push(InlineKeyboardButton::callback("➡️", format!("buy_plan_idx_{}", next_idx)));
                        buttons.push(nav_row);
                    }

                    if let Some(msg) = q.message {
                        let _ = bot.edit_message_text(msg.chat().id, msg.id(), text)
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
                                    InlineKeyboardButton::callback("🔗 Get Links", format!("get_links_{}", sub.sub.id)),
                                    InlineKeyboardButton::callback("📄 JSON Profile", format!("get_config_{}", sub.sub.id)),
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
                let _ = bot.answer_callback_query(callback_id).await;
            }

            gift if gift.starts_with("gift_init_") => {
                    let sub_id = gift.strip_prefix("gift_init_").unwrap_or("0").parse::<i64>().unwrap_or(0);
                    let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                    
                    if let Some(u) = user_db {
                        match state.store_service.convert_subscription_to_gift(sub_id, u.id).await {
                            Ok(code) => {
                                let response = format!("🎁 *Gift Code Created!*\n\nCode: `{}`\n\nShare this code with anyone. They can redeem it by sending it to the bot.", code);
                                if let Some(msg) = q.message {
                                    let _ = bot.send_message(msg.chat().id, response).parse_mode(ParseMode::MarkdownV2).await;
                                }
                                let _ = bot.answer_callback_query(callback_id).text("✅ Code Generated!").await;
                            },
                            Err(e) => {
                                let _ = bot.answer_callback_query(callback_id).text(format!("❌ Error: {}", e)).show_alert(true).await;
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
                    let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                    if let Some(u) = user_db {
                        match state.store_service.purchase_plan(u.id, duration_id).await {
                            Ok(_sub) => {
                                let _ = bot.answer_callback_query(callback_id).text("✅ Purchase successful!").await;
                                if let Some(msg) = q.message {
                                    let _ = bot.send_message(msg.chat().id, "✅ *Purchase Successful\\!*\n\nYour subscription is now *Pending*.\nGo to *My Services* to activate it when you are ready.").parse_mode(ParseMode::MarkdownV2).await;
                                }
                            }
                            Err(e) => {
                                error!("Purchase failed for user {}: {}", u.id, e);
                                let _ = bot.answer_callback_query(callback_id).text(format!("❌ Error: {}", e)).show_alert(true).await;
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
                    let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                    if let Some(u) = user_db {
                        match state.store_service.extend_subscription(u.id, duration_id).await {
                            Ok(sub) => {
                                let _ = bot.answer_callback_query(callback_id).text("✅ Extension successful!").await;
                                // Agents pull config automatically - no sync needed

                                if let Some(msg) = q.message {
                                    let _ = bot.send_message(msg.chat().id, format!("✅ *Subscription Extended!*\nNew Expiry: `{}`", sub.expires_at.format("%Y-%m-%d"))).parse_mode(ParseMode::MarkdownV2).await;
                                }
                            }
                            Err(e) => {
                                error!("Extension failed for user {}: {}", u.id, e);
                                let _ = bot.answer_callback_query(callback_id).text(format!("❌ Error: {}", e)).show_alert(true).await;
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
                    let user_db: Option<crate::models::store::User> = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                    
                    if let Some(u) = user_db {
                        match state.store_service.purchase_product_with_balance(u.id, prod_id).await {
                            Ok(product) => {
                                let _ = bot.answer_callback_query(callback_id).text("✅ Paid!").await;
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
                                    let _ = bot.answer_callback_query(callback_id).text(format!("❌ Failed: {}", e)).show_alert(true).await;
                            }
                        }
                    }
            }

            // DEVICE MANAGEMENT
            devices if devices.starts_with("devices_") => {
                 let sub_id = devices.strip_prefix("devices_").unwrap().parse::<i64>().unwrap_or(0);
                 let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                 
                 if let Some(u) = user_db {
                      // Verify ownership
                      let user_subs = state.store_service.get_user_subscriptions(u.id).await.unwrap_or_default();
                      if let Some(_sub_details) = user_subs.iter().find(|s| s.sub.id == sub_id) {
                          // Get active IPs
                          let ips = state.store_service.get_subscription_active_ips(sub_id).await.unwrap_or_default();
                          let limit = state.store_service.get_subscription_device_limit(sub_id).await.unwrap_or(0);
                          
                          let mut text = format!("📱 *Active Devices for Subscription \\#{:?}*\n", sub_id);
                          text.push_str(&format!("Limit: `{}/{}` devices\n\n", ips.len(), if limit == 0 { "∞".to_string() } else { limit.to_string() }));
                          
                          if ips.is_empty() {
                              text.push_str("No active sessions detected in the last 15 minutes\\.");
                          } else {
                              for ip in &ips {
                                  // Mask IP slightly for privacy? Or show full? User owns it.
                                  // Show time
                                  let time_ago = chrono::Utc::now() - ip.last_seen_at;
                                  let mins = time_ago.num_minutes();
                                  text.push_str(&format!("• `{}` \\({} mins ago\\)\n", ip.client_ip.replace(".", "\\."), mins));
                              }
                          }

                          let mut buttons = Vec::new();
                          if !ips.is_empty() {
                              buttons.push(vec![InlineKeyboardButton::callback("☠️ Reset Sessions", format!("kill_sessions_{}", sub_id))]);
                          }
                          buttons.push(vec![InlineKeyboardButton::callback("🔙 Back", "myservices_page_0")]);

                          if let Some(msg) = q.message {
                              let _ = bot.edit_message_text(msg.chat().id, msg.id(), text)
                                  .parse_mode(ParseMode::MarkdownV2)
                                  .reply_markup(InlineKeyboardMarkup::new(buttons))
                                  .await;
                          }
                      } else {
                          let _ = bot.answer_callback_query(callback_id.clone()).text("❌ Subscription not found.").await;
                      }
                 }
                 let _ = bot.answer_callback_query(callback_id.clone()).await;
            }

            kill if kill.starts_with("kill_sessions_") => {
                 let sub_id = kill.strip_prefix("kill_sessions_").unwrap().parse::<i64>().unwrap_or(0);
                  let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                 
                 if let Some(u) = user_db {
                      // Verify ownership
                      let user_subs = state.store_service.get_user_subscriptions(u.id).await.unwrap_or_default();
                      if let Some(sub_details) = user_subs.iter().find(|s| s.sub.id == sub_id) {
                           match state.connection_service.kill_subscription_connections(sub_details.sub.id).await {
                                   Ok(_) => {
                                        let _ = bot.answer_callback_query(callback_id).text("✅ Sessions reset successfully!").show_alert(true).await;
                                        // Update the message to remove "Kill" button or showing refreshed list
                                        if let Some(msg) = q.message {
                                            // Trigger refresh by sending "devices_" callback essentially?
                                            // Easier to just edit text.
                                            let _ = bot.send_message(msg.chat().id, "✅ *Sessions Reset*\\n\\nPlease wait a few moments for connections to close\\.").parse_mode(ParseMode::MarkdownV2).await;
                                        }
                                   }
                                   Err(e) => {
                                       let _ = bot.answer_callback_query(callback_id).text(format!("❌ Error: {}", e)).show_alert(true).await;
                                   }
                               }
                          }
                      }
            }
            store if store.starts_with("store_") => {
                let chat_id = q.message.as_ref().map(|m| m.chat().id).unwrap_or(ChatId(0));
                if chat_id.0 == 0 { return Ok(()); } // Safety

                if let Some(cat_id_str) = store.strip_prefix("store_cat_") {
                        if let Ok(cat_id) = cat_id_str.parse::<i64>() {
                            let products = state.store_service.get_products_by_category(cat_id).await.unwrap_or_default();
                            if products.is_empty() {
                                let _ = bot.answer_callback_query(callback_id).text("Category is empty").await;
                            } else {
                                let _ = bot.answer_callback_query(callback_id).await;
                                // Showcase style: separate message per product
                                for product in products {
                                    let price = product.price as f64 / 100.0;
                                    let text = format!("📦 *{}*\n\n{}\n\n💰 Price: *${:.2}*", 
                                        escape_md(&product.name), 
                                        escape_md(product.description.as_deref().unwrap_or("No description")), 
                                        price
                                    );
                                    let buttons = vec![
                                        vec![InlineKeyboardButton::callback(format!("💳 Buy Now (${:.2})", price), format!("buyprod_{}", product.id))],
                                        vec![InlineKeyboardButton::callback("🛒 Add to Cart", format!("add_cart_prod_{}", product.id))]
                                    ];
                                    let _ = bot.send_message(chat_id, text)
                                        .parse_mode(ParseMode::MarkdownV2)
                                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                                        .await;
                                }
                                // Add back button and cart button
                                let nav = vec![
                                    vec![InlineKeyboardButton::callback("🔙 Back to Categories", "store_home")],
                                    vec![InlineKeyboardButton::callback("🛒 View Cart", "view_cart")]
                                ];
                                let _ = bot.send_message(chat_id, "---")
                                    .reply_markup(InlineKeyboardMarkup::new(nav))
                                    .await;
                            }
                        }
                } else if let Some(prod_id_str) = store.strip_prefix("store_prod_") {
                        if let Ok(prod_id) = prod_id_str.parse::<i64>() {
                            if let Ok(product) = state.store_service.get_product(prod_id).await {
                                let _ = bot.answer_callback_query(callback_id).await;
                                let price = product.price as f64 / 100.0;
                                let text = format!("📦 *{}*\n\n{}\n\n💰 Price: *${:.2}*", 
                                    escape_md(&product.name), 
                                    escape_md(product.description.as_deref().unwrap_or("No description")), 
                                    price
                                );
                                
                                let buttons = vec![
                                    vec![InlineKeyboardButton::callback(format!("💳 Buy Now (${:.2})", price), format!("buyprod_{}", product.id))],
                                    vec![InlineKeyboardButton::callback("🛒 Add to Cart", format!("add_cart_prod_{}", product.id))],
                                    vec![InlineKeyboardButton::callback("🔙 Back", format!("store_cat_{}", product.category_id.unwrap_or(0)))],
                                ];
                                
                                let _ = bot.edit_message_text(chat_id, q.message.unwrap().id(), text)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            } else {
                                let _ = bot.answer_callback_query(callback_id).text("Product not found").await;
                            }
                        }
                } else if store == "store_home" {
                        let categories: Vec<crate::models::store::StoreCategory> = state.catalog_service.get_categories().await.unwrap_or_default();
                        let mut buttons = Vec::new();
                        for cat in categories {
                            buttons.push(vec![InlineKeyboardButton::callback(cat.name, format!("store_cat_{}", cat.id))]);
                        }
                        // View Cart
                        buttons.push(vec![InlineKeyboardButton::callback("🛒 View Cart", "view_cart")]);

                        let kb = InlineKeyboardMarkup::new(buttons);
                        let _ = bot.edit_message_text(chat_id, q.message.unwrap().id(), "📦 *Digital Store Categories:*")
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(kb)
                            .await;
                }
            }

            // Cart Actions
            "view_cart" => {
                 let _ = bot.answer_callback_query(callback_id).await;
                 let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                 if let Some(user) = user_db {
                     let cart_items = state.store_service.get_user_cart(user.id).await.unwrap_or_default();
                     
                     let text = if cart_items.is_empty() {
                         "🛒 Your cart is empty.".to_string()
                     } else {
                         let mut total_price: i64 = 0;
                         let mut t = "🛒 *YOUR SHOPPING CART*\n\n".to_string();
                         
                         for item in &cart_items {
                             let price_major = item.price / 100;
                             let price_minor = item.price % 100;
                             t.push_str(&format!("• *{}* \\(x{}\\) - ${}.{:02}\n", escape_md(&item.product_name), item.quantity, price_major, price_minor));
                             total_price += item.price * item.quantity;
                         }

                         let total_major = total_price / 100;
                         let total_minor = total_price % 100;
                         t.push_str(&format!("\n💰 *TOTAL: ${}.{:02}*", total_major, total_minor));
                         t
                     };

                     let buttons = if cart_items.is_empty() {
                         vec![vec![InlineKeyboardButton::callback("📦 Return to Store", "store_home")]]
                     } else {
                         vec![
                             vec![InlineKeyboardButton::callback("✅ Checkout", "cart_checkout")],
                             vec![InlineKeyboardButton::callback("🗑️ Clear Cart", "cart_clear")],
                             vec![InlineKeyboardButton::callback("📦 Continue Shopping", "store_home")]
                         ]
                     };
                     
                     if let Some(msg) = q.message {
                         let _ = bot.send_message(msg.chat().id, text)
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await;
                     }
                 }
            }

            "cart_clear" => {
                 let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                 if let Some(user) = user_db {
                     let _ = state.store_service.clear_cart(user.id).await;
                     let _ = bot.answer_callback_query(callback_id).text("🗑️ Cart cleared").await;
                     if let Some(msg) = q.message {
                         let _ = bot.edit_message_text(msg.chat().id, msg.id(), "🛒 Your cart is empty.")
                             .reply_markup(InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback("📦 Return to Store", "store_home")]]))
                             .await;
                     }
                 }
            }

            "cart_checkout" => {
                 let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                 if let Some(user) = user_db {
                     match state.store_service.checkout_cart(user.id).await {
                         Ok(notes) => {
                             let _ = bot.answer_callback_query(callback_id).text("✅ Checkout successful!").await;
                             let mut response = "✅ *Order Processed Successfully\\!*\n\n".to_string();
                             for note in notes {
                                 response.push_str(&format!("{}\n", escape_md(&note)));
                             }
                             if let Some(msg) = q.message {
                                 let _ = bot.send_message(msg.chat().id, response).parse_mode(ParseMode::MarkdownV2).await;
                                 let _ = bot.delete_message(msg.chat().id, msg.id()).await; // Delete cart msg
                             }
                         },
                         Err(e) => {
                             let _ = bot.answer_callback_query(callback_id).text(format!("❌ Failed: {}", e)).show_alert(true).await;
                         }
                     }
                 }
            }

            add_cart if add_cart.starts_with("add_cart_prod_") => {
                 let prod_id = add_cart.strip_prefix("add_cart_prod_").unwrap().parse::<i64>().unwrap_or(0);
                 let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                 if let Some(user) = user_db {
                     match state.store_service.add_to_cart(user.id, prod_id, 1).await {
                         Ok(_) => {
                             let _ = bot.answer_callback_query(callback_id).text("🛒 Added to cart!").await;
                         },
                         Err(e) => {
                             let _ = bot.answer_callback_query(callback_id).text(format!("❌ Error: {}", e)).await;
                         }
                     }
                 }
            }
            "edit_ref_code" => {
                let _ = bot.answer_callback_query(callback_id).await;
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
                let _ = bot.answer_callback_query(callback_id).await;
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

            // === Quick Wins: Auto-Renewal Toggle ===
            toggle if toggle.starts_with("toggle_renew_") => {
                let sub_id: i64 = toggle.strip_prefix("toggle_renew_").and_then(|s| s.parse().ok()).unwrap_or(0);
                let _ = bot.answer_callback_query(callback_id).await;
                
                match state.store_service.toggle_auto_renewal(sub_id).await {
                    Ok(new_state) => {
                        let status_text = if new_state {
                            "✅ *Auto\\-Renewal Enabled*\n\nYour subscription will automatically renew 24h before expiration if you have sufficient balance\\."
                        } else {
                            "🔴 *Auto\\-Renewal Disabled*\n\nYou'll need to manually renew your subscription when it expires\\."
                        };
                        
                        if let Some(msg) = q.message {
                            let _ = bot.send_message(msg.chat().id, status_text)
                                .parse_mode(ParseMode::MarkdownV2)
                                .await;
                        }
                    }
                    Err(e) => {
                        error!("Failed to toggle auto-renewal: {}", e);
                        if let Some(msg) = q.message {
                            let _ = bot.send_message(msg.chat().id, "❌ Failed to update setting\\. Please try again\\.")
                                .parse_mode(ParseMode::MarkdownV2)
                                .await;
                        }
                    }
                }
            }


            _ => {
                let _ = bot.answer_callback_query(callback_id).text("Feature not yet implemented.").await;
            }
        }
    }
    Ok::<_, teloxide::RequestError>(())
}

fn make_amount_keyboard(prefix: &str) -> InlineKeyboardMarkup {
    let amounts = vec![5, 10, 20, 50];
    let mut buttons = Vec::new();
    
    // 2x2 grid
    for chunk in amounts.chunks(2) {
        let mut row = Vec::new();
        for &amt in chunk {
             row.push(InlineKeyboardButton::callback(format!("${}", amt), format!("{}_{}", prefix, amt)));
        }
        buttons.push(row);
    }
    buttons.push(vec![InlineKeyboardButton::callback("« Back", "topup_menu")]);
    InlineKeyboardMarkup::new(buttons)
}

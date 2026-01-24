use teloxide::{
    prelude::*,
    types::{
        InlineKeyboardButton, InlineKeyboardMarkup, CallbackQuery, ParseMode, Message,
        KeyboardMarkup, KeyboardButton, Update, ForceReply,
    },
    dptree,
};
use tracing::{info, error};
use crate::services::pay_service::PaymentType;


fn escape_md(text: &str) -> String {
    text.replace(".", "\\.")
        .replace("-", "\\-")
        .replace("_", "\\_")
        .replace("*", "\\*")
        .replace("[", "\\[")
        .replace("]", "\\]")
        .replace("(", "\\(")
        .replace(")", "\\)")
        .replace("~", "\\~")
        .replace("`", "\\`")
        .replace(">", "\\>")
        .replace("#", "\\#")
        .replace("+", "\\+")
        .replace("=", "\\=")
        .replace("|", "\\|")
        .replace("{", "\\{")
        .replace("}", "\\}")
        .replace("!", "\\!")
}

fn main_menu() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![KeyboardButton::new("🛍 Buy Subscription"), KeyboardButton::new("🔐 My Services")],
        vec![KeyboardButton::new("📦 Digital Store"), KeyboardButton::new("👤 My Profile")],
        vec![KeyboardButton::new("🎁 Bonuses / Referral"), KeyboardButton::new("❓ Support")],
    ])
    .resize_keyboard()
}

fn language_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("🇺🇸 English", "set_lang_en"),
            InlineKeyboardButton::callback("🇷🇺 Русский", "set_lang_ru"),
        ]
    ])
}

fn terms_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("✅ Accept", "accept_terms"),
            InlineKeyboardButton::callback("❌ Decline", "decline_terms"),
        ]
    ])
}

pub async fn run_bot(bot: Bot, mut shutdown_signal: tokio::sync::broadcast::Receiver<()>, state: crate::AppState) {
    info!("Starting refined bot dispatcher...");

    let handler = Update::filter_message().endpoint(
        |bot: Bot, msg: Message, state: crate::AppState| async move {
            info!("Received message: {:?}", msg.text());
            let tg_id = msg.chat.id.0 as i64;
//... (rest of handler)
            
            if let Some(text) = msg.text() {
                // 1. Resolve User (Handle /start upsert or fetch existing)
                let user_res = if text.starts_with("/start") {
                    let start_param = text.strip_prefix("/start ").unwrap_or("");
                    let referrer_id = if !start_param.is_empty() {
                        state.store_service.resolve_referrer_id(start_param).await.ok().flatten()
                    } else {
                        None
                    };

                    let user_name = msg.from.as_ref().map(|u| u.full_name()).unwrap_or_else(|| "User".to_string());
                    // Upsert returns User
                    state.store_service.upsert_user(
                        tg_id, 
                        msg.from.as_ref().and_then(|u| u.username.as_deref()),
                        Some(&user_name),
                        referrer_id
                    ).await.ok()
                } else {
                    state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten()
                };

                // 2. State Machine Checks
                if let Some(user) = user_res {
                    if user.is_banned {
                        let _ = bot.send_message(msg.chat.id, "🚫 *Access Denied*\n\nYour account has been banned\\.").parse_mode(ParseMode::MarkdownV2).await;
                        return Ok(());
                    }

                    if user.language_code.is_none() {
                        let _ = bot.send_message(msg.chat.id, "🌐 <b>Please select your language / Пожалуйста, выберите язык:</b>")
                            .parse_mode(ParseMode::Html)
                            .reply_markup(language_keyboard())
                            .await
                            .map_err(|e| error!("Failed to send language choice: {}", e));
                        return Ok(());
                    }

                    // Check Terms
                    if user.terms_accepted_at.is_none() {
                        if !text.starts_with("/start") {
                             let _ = state.store_service.increment_warning_count(user.id).await;
                             if user.warning_count >= 5 {
                                 let _ = state.store_service.ban_user(user.id).await;
                                 let _ = bot.send_message(msg.chat.id, "🚫 <b>Account Banned</b> due to spam/botting.")
                                    .parse_mode(ParseMode::Html).await;
                                 return Ok(());
                             }
                        }
                        let terms_text = state.store_service.get_setting("terms_of_service").await.ok().flatten()
                            .unwrap_or_else(|| "Terms of Service...".to_string());
                        
                        let _ = bot.send_message(msg.chat.id, format!("📜 <b>Terms of Service</b>\n\n{}\n\nPlease accept the terms to continue.", terms_text))
                            .parse_mode(ParseMode::Html)
                            .reply_markup(terms_keyboard())
                            .await
                            .map_err(|e| error!("Failed to send terms: {}", e));
                        return Ok(());
                    }

                    // Auto-update profile if changed (only if fully engaged)
                    if let Some(u) = msg.from.as_ref() {
                        let new_full_name = u.full_name();
                        let new_username = u.username.as_deref();
                        let name_changed = user.full_name.as_deref() != Some(new_full_name.as_str());
                        let username_changed = user.username.as_deref() != new_username;

                        if name_changed || username_changed {
                             let _ = state.store_service.upsert_user(tg_id, new_username, Some(new_full_name.as_str()), None).await;
                        }
                    }

                    // If we just started, show welcome
                    if text.starts_with("/start") {
                         let user_name = msg.from.as_ref().map(|u| u.full_name()).unwrap_or_else(|| "User".to_string());
                         let welcome_text = format!(
                            "👋 <b>Hello, {}!</b>\n\n\
                            Use the menu below to manage your VPN subscriptions and digital goods.",
                            user_name
                        );
                        let _ = bot.send_message(msg.chat.id, welcome_text)
                            .parse_mode(ParseMode::Html)
                            .reply_markup(main_menu())
                            .await
                            .map_err(|e| error!("Failed to send welcome on /start: {}", e));
                        return Ok(());
                    }
                } else if !text.starts_with("/start") {
                    // Non-start message from unknown user? ignore or ask to start
                    return Ok(());
                }

                // 3. Normal Message Processing (User is verified)
                // Check for Reply to Transfer or Note
                if let Some(reply) = msg.reply_to_message() {
                    // ... (existing reply logic) ...
                    if let Some(reply_text) = reply.text() {
                         info!("Processing reply to message with text: [{}]", reply_text);
                         info!("User reply body: [{}]", text);
                         // Note Update
                        if reply_text.contains("with your note for Subscription #") {
                            // ... copy existing note logic ...
                             if let Some(start_idx) = reply_text.find('#') {
                                 let id_part = &reply_text[start_idx + 1..];
                                 let id_str = id_part.trim_end_matches('.'); 
                                 if let Ok(sub_id) = id_str.parse::<i64>() {
                                     // ...
                                     let _ = state.store_service.update_subscription_note(sub_id, text.to_string()).await;
                                     let _ = bot.send_message(msg.chat.id, "✅ Note updated!").await;
                                     return Ok(());
                                 }
                             }
                        }
                        // Transfer
                         if reply_text.contains("Transfer Subscription") && reply_text.contains("Subscription #") {
                             if let Some(start) = reply_text.find("Subscription #") {
                                // ... copy existing logic ...
                                let rest = &reply_text[start + "Subscription #".len()..];
                                let id_str = rest.split_whitespace().next().unwrap_or("0");
                                if let Ok(sub_id) = id_str.parse::<i64>() {
                                    if let Some(u) = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten() {
                                        match state.store_service.transfer_subscription(sub_id, u.id, text).await {
                                            Ok(_) => { let _ = bot.send_message(msg.chat.id, format!("✅ Subscription \\#{} transferred to {} successfully\\!", sub_id, escape_md(text))).parse_mode(ParseMode::MarkdownV2).await; }
                                            Err(e) => { let _ = bot.send_message(msg.chat.id, format!("❌ Transfer failed: {}", escape_md(&e.to_string()))).parse_mode(ParseMode::MarkdownV2).await; }
                                        }
                                    }
                                    return Ok(());
                                }
                             }
                         }
                         // Gift Code
                         if reply_text.contains("🎟 Enter your Gift Code") || reply_text.contains("🎟 Enter your Promo Code") {
                              let code = text.trim();
                              if let Some(u) = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten() {
                                  if code.starts_with("EXA-GIFT-") {
                                      match state.store_service.redeem_gift_code(u.id, code).await {
                                          Ok(_sub) => { let _ = bot.send_message(msg.chat.id, "✅ *Code Redeemed\\!*\n\nCheck *My Services*\\.").parse_mode(ParseMode::MarkdownV2).await; },
                                          Err(e) => { let _ = bot.send_message(msg.chat.id, format!("❌ Redemption Failed: {}", escape_md(&e.to_string()))).parse_mode(ParseMode::MarkdownV2).await; }
                                      }
                                  } else {
                                      let _ = bot.send_message(msg.chat.id, "❌ Invalid code format\\.").parse_mode(ParseMode::MarkdownV2).await;
                                  }
                              }
                              return Ok(());
                         }

                         // Edit Referral Code Alias
                         if reply_text.contains("EDIT REFERRAL ALIAS") {
                             let new_code = text.trim();
                             
                             // Basic validation
                             if new_code.len() < 3 || new_code.len() > 32 {
                                 let _ = bot.send_message(msg.chat.id, "❌ *Invalid Length*\n\nReferral alias must be between 3 and 32 characters\\.").parse_mode(ParseMode::MarkdownV2).await;
                                 return Ok(());
                             }

                             if !new_code.chars().all(|c| c.is_alphanumeric() || c == '_') {
                                 let _ = bot.send_message(msg.chat.id, "❌ *Invalid Characters*\n\nReferral alias can only contain letters, numbers, and underscores\\.").parse_mode(ParseMode::MarkdownV2).await;
                                 return Ok(());
                             }

                             if let Some(u) = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten() {
                                 match state.store_service.update_user_referral_code(u.id, new_code).await {
                                     Ok(_) => { 
                                         let bot_me = bot.get_me().await.ok();
                                         let bot_username = bot_me.and_then(|m| m.username.clone()).unwrap_or_else(|| "bot".to_string());
                                         let new_link = format!("https://t.me/{}?start={}", bot_username, new_code);
                                         
                                         let response = format!(
                                             "✅ *Referral Alias Updated\\!*\n\n\
                                             Your new data:\n\
                                             Code: `{}`\n\
                                             Link: `{}`", 
                                             new_code.replace("`", "\\`").replace("\\", "\\\\"),
                                             new_link.replace("`", "\\`").replace("\\", "\\\\")
                                         );
                                         if let Err(e) = bot.send_message(msg.chat.id, response).parse_mode(ParseMode::MarkdownV2).await {
                                             error!("Failed to send alias update confirmation: {}", e);
                                         }
                                     },
                                     Err(_e) => { 
                                         let _ = bot.send_message(msg.chat.id, "❌ *Update Failed*\n\nThis alias might already be taken or invalid\\.").parse_mode(ParseMode::MarkdownV2).await; 
                                     }
                                 }
                             }
                             return Ok(());
                         }

                         // Enter Referrer Code
                         if reply_text.contains("Enter Referrer Code") {
                             let ref_code = text.trim();
                             if let Some(u) = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten() {
                                 match state.store_service.set_user_referrer(u.id, ref_code).await {
                                     Ok(_) => { let _ = bot.send_message(msg.chat.id, "✅ *Referrer Linked\\!*\n\nYou've successfully set your referrer\\.").parse_mode(ParseMode::MarkdownV2).await; },
                                     Err(e) => { let _ = bot.send_message(msg.chat.id, format!("❌ Linking Failed: {}", escape_md(&e.to_string()))).parse_mode(ParseMode::MarkdownV2).await; }
                                 }
                             }
                             return Ok(());
                         }
                    }
                }

                // Commands and Menus
                match text {
                    // /start is already handled above in flow
                    "📦 Digital Store" => {
                        // ... copy existing store logic ... 
                         let categories = state.store_service.get_categories().await.unwrap_or_default();
                         if categories.is_empty() {
                             let _ = bot.send_message(msg.chat.id, "❌ The store is currently empty.").await;
                         } else {
                             let mut buttons = Vec::new();
                             for cat in categories {
                                 buttons.push(vec![InlineKeyboardButton::callback(cat.name, format!("store_cat_{}", cat.id))]);
                             }
                             let kb = InlineKeyboardMarkup::new(buttons);
                             let _ = bot.send_message(msg.chat.id, "📦 *Welcome to the Digital Store*\\nSelect a category to browse:")
                                .parse_mode(ParseMode::MarkdownV2)
                                .reply_markup(kb)
                                .await;
                         }
                    }
                    "/enter_promo" | "🎁 Redeem Code" => {
                        let _ = bot.send_message(msg.chat.id, "🎟 *Redeem Gift Code*\n\nPlease reply to this message with your code (e.g., `EXA-GIFT-XYZ`).")
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(ForceReply::new().selective())
                            .await;
                    }

                    "🛍 Buy Subscription" | "/plans" => {
                        let plans = state.store_service.get_active_plans().await.unwrap_or_default();
                        
                        if plans.is_empty() {
                            let _ = bot.send_message(msg.chat.id, "❌ No active plans available at the moment.").await;
                        } else {
                            let page = 0;
                            let limit = 3;
                            let total_pages = (plans.len() as f64 / limit as f64).ceil() as usize;
                            let start = page * limit;
                            let end = std::cmp::min(start + limit, plans.len());
                            let page_plans = &plans[start..end];

                            let _ = bot.send_message(msg.chat.id, format!("💎 *Showcase:* Page {}/{}", page + 1, total_pages)).parse_mode(ParseMode::MarkdownV2).await;

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

                                // If it's the last plan on the last page or just the last plan of the batch, we add navigation if needed
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

                                let _ = bot.send_message(msg.chat.id, text)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                    }

                    "👤 My Profile" | "/profile" => {
                        let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                        
                        if let Some(user) = user_db {
                            let price_major = user.balance / 100;
                            let price_minor = user.balance % 100;
                            
                            let response = format!(
                                "👤 *USER PROFILE*\n\n\
                                🆔 ID: `{}`\n\
                                💰 Balance: `${}.{:02}`\n\n\
                                _Use 'My Services' to manage subscriptions and products\\._",
                                user.tg_id, price_major, price_minor
                            );

                            let mut buttons = Vec::new();
                            buttons.push(vec![InlineKeyboardButton::callback("💳 Top-up Balance", "topup_menu")]);

                            let _ = bot.send_message(msg.chat.id, response)
                                .parse_mode(ParseMode::MarkdownV2)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await;
                        }
                    }

                    "🔐 My Services" | "/services" => {
                        let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                        
                        if let Some(user) = user_db {
                            let mut response = "🔐 *MY SERVICES*\n\n".to_string();

                            // 1. Subscriptions
                            let subs = match state.store_service.get_user_subscriptions(user.id).await {
                                Ok(s) => s,
                                Err(e) => {
                                    error!("Failed to fetch subs for user {}: {}", user.id, e);
                                    Vec::new()
                                }
                            };
                            
                            // Sort subs by status (Active first)
                            let mut sorted_subs = subs.clone();
                            sorted_subs.sort_by(|a, b| match (a.sub.status.as_str(), b.sub.status.as_str()) {
                                ("pending", "active") => std::cmp::Ordering::Less,
                                ("active", "pending") => std::cmp::Ordering::Greater,
                                _ => b.sub.created_at.cmp(&a.sub.created_at),
                            });

                            if sorted_subs.is_empty() {
                                response.push_str("📡 VPN Status: ❌ *No Subscriptions*\n\n");
                                let _ = bot.send_message(msg.chat.id, response).parse_mode(ParseMode::MarkdownV2).await;
                            } else {
                                // Default to page 0
                                let page = 0;
                                let total_pages = sorted_subs.len();
                                let sub = &sorted_subs[page];

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
                                    let duration = sub.sub.expires_at - sub.sub.created_at;
                                    if duration.num_days() == 0 {
                                        response.push_str("   ⌛ *Expires:* `No expiration` \\(Traffic Plan\\)\n");
                                    } else {
                                        response.push_str(&format!("   ⌛ *Expires:* `{}`\n", sub.sub.expires_at.format("%Y-%m-%d")));
                                    }
                                } else {
                                    let duration = sub.sub.expires_at - sub.sub.created_at;
                                    if duration.num_days() == 0 {
                                        response.push_str("   ⏱ *Duration:* `No expiration` \\(Traffic Plan\\)\n");
                                    } else {
                                        response.push_str(&format!("   ⏱ *Duration:* `{} days` \\(starts on activation\\)\n", duration.num_days()));
                                    }
                                }
                                response.push_str("\n");
                                if let Some(note) = &sub.sub.note {
                                    response.push_str(&format!("📝 *Note:* {}\n\n", escape_md(note)));
                                }

                                // Navigation & Actions
                                let mut buttons = Vec::new();

                                // Edit Note Button
                                buttons.push(vec![InlineKeyboardButton::callback("📝 Edit Note", format!("edit_note_{}", sub.sub.id))]);

                                // Connected Devices Button (for active subscriptions)
                                if sub.sub.status == "active" {
                                    buttons.push(vec![InlineKeyboardButton::callback("📱 Connected Devices", format!("devices_{}", sub.sub.id))]);
                                }

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

                                let _ = bot.send_message(msg.chat.id, response)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                    }

                    "🎁 Bonuses / Referral" | "/referral" => {
                        let user_db = state.store_service.get_user_by_tg_id(tg_id).await.ok().flatten();
                        if let Some(user) = user_db {
                            let bot_me = bot.get_me().await.ok();
                            let bot_username = bot_me.and_then(|m| m.username.clone()).unwrap_or_else(|| "bot".to_string());
                            
                            // Use referral_code (alias) if exists, fallback to tg_id
                            let ref_code = user.referral_code.clone().unwrap_or_else(|| user.tg_id.to_string());
                            let ref_link = format!("https://t.me/{}?start={}", bot_username, ref_code);
                            
                            let ref_count = state.store_service.get_referral_count(user.id).await.unwrap_or(0);
                            let ref_earnings = state.store_service.get_user_referral_earnings(user.id).await.unwrap_or(0);
                            let earnings_major = ref_earnings / 100;
                            let earnings_minor = ref_earnings % 100;
                            
                            let response = format!(
                                "🎁 *BONUS PROGRAM*\n\n\
                                🤝 *Invite friends and earn money\\!*\n\
                                You get *10%* from *EVERY* purchase your friends make\\.\n\n\
                                📊 *Your Statistics:*\n\
                                👥 Referrals joined: *{}*\n\
                                💰 Total earned: *${}\\.{:02}*\n\n\
                                🔗 *Your Promo Data:*\n\
                                Code: `{}`\n\
                                Link: `{}`\n\n\
                                _Share your link or code to start earning\\!_",
                                ref_count,
                                earnings_major, earnings_minor,
                                ref_code.replace("`", "\\`").replace("\\", "\\\\"),
                                ref_link.replace("`", "\\`").replace("\\", "\\\\")
                            );

                            let mut buttons = Vec::new();
                            buttons.push(vec![InlineKeyboardButton::callback("🎟 Enter Promo Code", "enter_promo")]);
                            
                            // Add Referral Management Buttons
                            buttons.push(vec![InlineKeyboardButton::callback("🔗 Edit My Code (Alias)", "edit_ref_code")]);
                            if user.referrer_id.is_none() {
                                buttons.push(vec![InlineKeyboardButton::callback("🎁 Enter Referrer Code", "enter_referrer")]);
                            }

                            let _ = bot.send_message(msg.chat.id, response)
                                .parse_mode(ParseMode::MarkdownV2)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await;
                        }
                    }

                    "❓ Support" => {
                        let support_username = state.settings.get_or_default("support_url", "").await;
                        
                        if support_username.is_empty() {
                             let _ = bot.send_message(msg.chat.id, "❌ Support contact is not configured yet.").await;
                        } else {
                            // Sanitize username (remove @ if present)
                            let clean_username = support_username.trim_start_matches('@');
                            let url = format!("https://t.me/{}", clean_username);
                            
                            let kb = InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::url("💬 Contact Support", url.parse().unwrap())
                            ]]);

                            let _ = bot.send_message(msg.chat.id, "Need help? Click the button below to contact support:")
                                .reply_markup(kb)
                                .await;
                        }
                    }

                    _ => {
                         // Fallback or handle promo code input if we implement state
                    }
                }
            }
            Ok::<_, teloxide::RequestError>(())
        },
    );

    let callback_handler = Update::filter_callback_query().endpoint(
        |bot: Bot, q: CallbackQuery, state: crate::AppState| async move {
            info!("Received callback: {:?}", q.data);
            let user_tg = q.from;
// ... (rest of callback handler)

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
                                 
                                 if let Some(sub) = sub_opt {
                                     let nodes = state.store_service.get_active_nodes().await.unwrap_or_default();
                                     if nodes.is_empty() {
                                         let _ = bot.send_message(ChatId(user_tg.tg_id), "❌ No nodes available for your plan yet.").await;
                                     } else {
                                        let mut response = "🔗 *Your Connection Links:*\n\n".to_string();
                                        for node in nodes {
                                            if let (Some(pub_key), Some(short_id), Some(uuid)) = (&node.reality_pub, &node.short_id, &sub.sub.vless_uuid) {
                                                let link = format!(
                                                    "vless://{}@{}:443?encryption=none&flow=xtls-rprx-vision&security=reality&sni=google.com&fp=chrome&pbk={}&sid={}#EXA-{}-{}",
                                                    uuid, node.ip, pub_key, short_id, escape_md(&node.name), escape_md(&node.ip)
                                                );
                                                response.push_str(&format!("📍 *{}*\n`{}`\n\n", escape_md(&node.name), escape_md(&link)));
                                            }
                                        }
                                        let _ = bot.send_message(ChatId(user_tg.tg_id), response).parse_mode(ParseMode::MarkdownV2).await;
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
                                        let _ = orch.sync_all_nodes().await;
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
                                            response.push_str(&format!("🎟 `{}`\n   Days: {}\n\n", code.code, code.duration_days));
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
                                         // Optionally refresh the My Services view by triggering the same page (or removing the sub from view)
                                         // Ideally we would re-render page 0 or edit the current message.
                                         // For simplicity, we just send the code. The user can refresh My Services manually or we could try to edit.
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
                                            let _ = orch.sync_all_nodes().await;
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
                                            let _ = orch.sync_all_nodes().await;
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
        },
    );

    let mut dispatcher = Dispatcher::builder(bot, dptree::entry().branch(handler).branch(callback_handler))
        .dependencies(dptree::deps![state])
        .default_handler(|upd: std::sync::Arc<Update>| async move {
            info!("Unhandled update: {:?}", upd);
        })
        .enable_ctrlc_handler()
        .build();

    tokio::select! {
        _ = dispatcher.dispatch() => {
            info!("Bot dispatcher exited naturally");
        }
        _ = shutdown_signal.recv() => {
            info!("Bot received shutdown signal, stopping...");
        }
    }
}

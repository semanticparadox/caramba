use crate::bot::keyboards::{language_keyboard, main_menu, terms_keyboard};
use crate::bot::utils::{escape_md, register_bot_message};
use crate::models::store::{
    CartItem, DetailedSubscription, Plan, StoreCategory, SubscriptionIpTracking, User,
};
use crate::AppState;
use anyhow::Result as AnyhowResult;
use teloxide::prelude::*;
use teloxide::types::{ForceReply, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};
use tracing::{error, info};

// Note: LoggingService is now in state

pub async fn message_handler(
    bot: Bot,
    msg: Message,
    state: AppState,
) -> Result<(), teloxide::RequestError> {
    info!("Received message: {:?}", msg.text());
    let tg_id = msg.chat.id.0 as i64;

    if let Some(payment) = msg.successful_payment() {
        let amount_xtr = payment.total_amount as f64;
        // 1 USD approx 50 XTR
        let amount_usd = amount_xtr / 50.0;
        info!(
            "Processing Stars Payment: {} XTR (${:.2})",
            amount_xtr, amount_usd
        );

        match state
            .pay_service
            .process_any_payment(
                amount_usd,
                "stars",
                Some(payment.provider_payment_charge_id.clone()),
                &payment.invoice_payload,
            )
            .await
        {
            Ok(_) => {
                // Log successful payment
                let _ = state
                    .logging_service
                    .log_user(
                        Some(tg_id),
                        "payment_stars",
                        &format!(
                            "Stars payment successful: {} XTR (${:.2})",
                            amount_xtr, amount_usd
                        ),
                        None,
                    )
                    .await;

                let _ = bot
                    .send_message(msg.chat.id, "✅ Payment successful! Balance updated.")
                    .await;
            }
            Err(e) => {
                error!("Stars payment processing failed: {}", e);
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        "❌ Error processing payment. Please contact support.",
                    )
                    .await;
            }
        }
        return Ok(());
    }

    if let Some(text) = msg.text() {
        // 1. Resolve User (Handle /start upsert or fetch existing)
        let user_res: Option<User> = if text.starts_with("/start") {
            let start_param = text.strip_prefix("/start ").unwrap_or("");
            let referrer_id_res: AnyhowResult<Option<i64>> = if !start_param.is_empty() {
                state.store_service.resolve_referrer_id(start_param).await
            } else {
                Ok(None)
            };
            let referrer_id = referrer_id_res.ok().flatten();

            let user_name = msg
                .from
                .as_ref()
                .map(|u| u.full_name())
                .unwrap_or_else(|| "User".to_string());
            // Upsert returns User
            let user_res_inner: AnyhowResult<Option<User>> = state
                .store_service
                .upsert_user(
                    tg_id,
                    msg.from.as_ref().and_then(|u| u.username.as_deref()),
                    Some(&user_name),
                    referrer_id,
                )
                .await;

            match user_res_inner {
                Ok(Some(u)) => {
                    // Log user /start command
                    let _ = state
                        .logging_service
                        .log_user(
                            Some(tg_id),
                            "bot_start",
                            &format!("User {} executed /start command", tg_id),
                            None,
                        )
                        .await;

                    Some(u)
                }
                Ok(None) => None, // Should not happen on upsert unless error
                Err(e) => {
                    error!("Failed to upsert user on /start: {:?}", e);
                    None
                }
            }
        } else {
            let res: AnyhowResult<Option<User>> =
                state.store_service.get_user_by_tg_id(tg_id).await;
            res.ok().flatten()
        };

        // 2. State Machine Checks
        if let Some(user) = &user_res {
            if user.is_banned {
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        "🚫 *Access Denied*\n\nYour account has been banned\\.",
                    )
                    .parse_mode(ParseMode::MarkdownV2)
                    .await;
                return Ok(());
            }

            if user.language_code.is_none() {
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        "🌐 <b>Please select your language / Пожалуйста, выберите язык:</b>",
                    )
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
                        let _ = bot
                            .send_message(
                                msg.chat.id,
                                "🚫 <b>Account Banned</b> due to spam/botting.",
                            )
                            .parse_mode(ParseMode::Html)
                            .await;
                        return Ok(());
                    }
                }
                let terms_text: String = state
                    .settings
                    .get_or_default("terms_of_service", "Terms of Service...")
                    .await;

                let _ = bot.send_message(msg.chat.id, format!("📜 <b>Terms of Service</b>\n\n{}\n\nPlease accept the terms to continue.", terms_text))
                    .parse_mode(ParseMode::Html)
                    .reply_markup(terms_keyboard())
                    .await
                    .map(move |m| {
                         let state = state.clone();
                         let bot = bot.clone();
                         let uid = user.id;
                         tokio::spawn(async move {
                             register_bot_message(bot, &state, uid, &m).await;
                         });
                    })
                    .map_err(|e| error!("Failed to send terms: {}", e));
                return Ok(());
            }

            // --- Dissolving Effect: Delete previous bot message & this command ---
            let _ = bot.delete_message(msg.chat.id, msg.id).await;
            // ---------------------------------------------------------------------

            // Auto-update profile if changed (only if fully engaged)
            if let Some(u) = msg.from.as_ref() {
                let new_full_name = u.full_name();
                let new_username = u.username.as_deref();
                let name_changed = user.full_name.as_deref() != Some(new_full_name.as_str());
                let username_changed = user.username.as_deref() != new_username;

                if name_changed || username_changed {
                    let _ = state
                        .store_service
                        .upsert_user(tg_id, new_username, Some(new_full_name.as_str()), None)
                        .await;
                }
            }

            // If we just started, show welcome
            if text.starts_with("/start") {
                let user_name = msg
                    .from
                    .as_ref()
                    .map(|u| u.full_name())
                    .unwrap_or_else(|| "User".to_string());
                let welcome_text = format!(
                    "👋 <b>Hello, {}!</b>\n\n\
                    Use the menu below to manage your VPN subscriptions and digital goods.",
                    user_name
                );
                let bot_for_task = bot.clone();
                let state_for_task = state.clone();
                let _ = bot
                    .send_message(msg.chat.id, welcome_text)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(main_menu())
                    .await
                    .map(move |m| {
                        let uid = user.id;
                        tokio::spawn(async move {
                            register_bot_message(bot_for_task, &state_for_task, uid, &m).await;
                        });
                    })
                    .map_err(|e| error!("Failed to send welcome on /start: {}", e));

                // Set persistent menu button
                let web_app_url = state.settings.get_or_default("mini_app_url", "").await;
                if !web_app_url.is_empty() {
                    let _ = bot
                        .set_chat_menu_button()
                        .chat_id(msg.chat.id)
                        .menu_button(teloxide::types::MenuButton::WebApp {
                            text: "🚀 Open App".to_string(),
                            web_app: teloxide::types::WebAppInfo {
                                url: web_app_url.parse().unwrap(),
                            },
                        })
                        .await;
                }

                return Ok(());
            }
        } else if !text.starts_with("/start") {
            // Non-start message from unknown user? ignore or ask to start
            return Ok(());
        }

        // 3. Normal Message Processing (User is verified)
        // Check for Reply to Transfer or Note
        if let Some(reply) = msg.reply_to_message() {
            if let Some(reply_text) = reply.text() {
                info!("Processing reply to message with text: [{}]", reply_text);
                info!("User reply body: [{}]", text);
                // Note Update
                if let Some(start_idx) = reply_text.find('#') {
                    let id_part = &reply_text[start_idx + 1..];
                    let id_str = id_part.trim_end_matches('.');
                    if let Ok(sub_id) = id_str.parse::<i64>() {
                        let _ = state
                            .store_service
                            .update_subscription_note(sub_id, text.to_string())
                            .await;
                        let _ = bot.send_message(msg.chat.id, "✅ Note updated!").await;
                        return Ok(());
                    }
                }
                // Transfer
                if reply_text.contains("Transfer Subscription")
                    && reply_text.contains("Subscription #")
                {
                    if let Some(start) = reply_text.find("Subscription #") {
                        let rest = &reply_text[start + "Subscription #".len()..];
                        let id_str = rest.split_whitespace().next().unwrap_or("0");
                        if let Ok(sub_id) = id_str.parse::<i64>() {
                            let user_db_res: AnyhowResult<Option<User>> =
                                state.store_service.get_user_by_tg_id(tg_id).await;
                            if let Ok(Some(u)) = user_db_res {
                                match state
                                    .store_service
                                    .transfer_subscription(sub_id, u.id, text)
                                    .await
                                {
                                    Ok(_) => {
                                        let _ = bot.send_message(msg.chat.id, format!("✅ Subscription \\#{} transferred to {} successfully\\!", sub_id, escape_md(text))).parse_mode(ParseMode::MarkdownV2).await;
                                    }
                                    Err(e) => {
                                        let _ = bot
                                            .send_message(
                                                msg.chat.id,
                                                format!(
                                                    "❌ Transfer failed: {}",
                                                    escape_md(&e.to_string())
                                                ),
                                            )
                                            .parse_mode(ParseMode::MarkdownV2)
                                            .await;
                                    }
                                }
                            }
                            return Ok(());
                        }
                    }
                }

                // Gift Code
                if reply_text.contains("🎟 Enter your Gift Code")
                    || reply_text.contains("🎟 Enter your Promo Code")
                {
                    let code = text.trim();
                    let user_db_res: AnyhowResult<Option<User>> =
                        state.store_service.get_user_by_tg_id(tg_id).await;
                    if let Ok(Some(u)) = user_db_res {
                        match state.promo_service.redeem_code(u.id, code).await {
                            Ok(res_msg) => {
                                let _ = bot
                                    .send_message(
                                        msg.chat.id,
                                        format!("✅ *Success\\!*\n\n{}", escape_md(&res_msg)),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                            Err(e) => {
                                let _ = bot
                                    .send_message(
                                        msg.chat.id,
                                        format!(
                                            "❌ Redemption Failed: {}",
                                            escape_md(&e.to_string())
                                        ),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
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

                    let user_db_res: AnyhowResult<Option<User>> =
                        state.store_service.get_user_by_tg_id(tg_id).await;
                    if let Ok(Some(u)) = user_db_res {
                        match state
                            .store_service
                            .update_user_referral_code(u.id, new_code)
                            .await
                        {
                            Ok(_) => {
                                let bot_me = bot.get_me().await.ok();
                                let bot_username = bot_me
                                    .and_then(|m| m.username.clone())
                                    .unwrap_or_else(|| "bot".to_string());
                                let new_link =
                                    format!("https://t.me/{}?start={}", bot_username, new_code);

                                let response = format!(
                                    "✅ *Referral Alias Updated\\!*\n\n\
                                        Your new data:\n\
                                        Code: `{}`\n\
                                        Link: `{}`",
                                    new_code.replace("`", "\\`").replace("\\", "\\\\"),
                                    new_link.replace("`", "\\`").replace("\\", "\\\\")
                                );
                                if let Err(e) = bot
                                    .send_message(msg.chat.id, response)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await
                                {
                                    error!("Failed to send alias update confirmation: {}", e);
                                }
                            }
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
                    let user_db_res: AnyhowResult<Option<User>> =
                        state.store_service.get_user_by_tg_id(tg_id).await;
                    if let Ok(Some(u)) = user_db_res {
                        match state.store_service.set_user_referrer(u.id, ref_code).await {
                            Ok(_) => {
                                let _ = bot.send_message(msg.chat.id, "✅ *Referrer Linked\\!*\n\nYou've successfully set your referrer\\.").parse_mode(ParseMode::MarkdownV2).await;
                            }
                            Err(e) => {
                                let _ = bot
                                    .send_message(
                                        msg.chat.id,
                                        format!("❌ Linking Failed: {}", escape_md(&e.to_string())),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
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
                let categories_res: AnyhowResult<Vec<StoreCategory>> =
                    state.store_service.get_categories().await;
                let categories = categories_res.unwrap_or_default();
                if categories.is_empty() {
                    let _ = bot
                        .send_message(msg.chat.id, "❌ The store is currently empty.")
                        .reply_markup(main_menu())
                        .await;
                } else {
                    let mut buttons = Vec::new();
                    for cat in categories {
                        buttons.push(vec![InlineKeyboardButton::callback(
                            cat.name,
                            format!("store_cat_{}", cat.id),
                        )]);
                    }

                    // Add "View Cart" button to store menu
                    buttons.push(vec![InlineKeyboardButton::callback(
                        "🛒 View Cart",
                        "view_cart",
                    )]);

                    let kb = InlineKeyboardMarkup::new(buttons);
                    let _ = bot
                        .send_message(
                            msg.chat.id,
                            "📦 *Welcome to the Digital Store*\\nSelect a category to browse:",
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(kb)
                        .await;
                }
            }
            "🛒 My Cart" | "/cart" => {
                if let Some(user) = &user_res {
                    let cart_items_res: AnyhowResult<Vec<CartItem>> =
                        state.store_service.get_user_cart(user.id).await;
                    let cart_items = cart_items_res.unwrap_or_default();

                    if cart_items.is_empty() {
                        let _ = bot
                            .send_message(msg.chat.id, "🛒 Your cart is empty.")
                            .await;
                    } else {
                        let mut total_price: i64 = 0;
                        let mut text = "🛒 *YOUR SHOPPING CART*\n\n".to_string();

                        for item in &cart_items {
                            let price_major = item.price / 100;
                            let price_minor = item.price % 100;
                            text.push_str(&format!(
                                "• *{}* - ${}.{:02}\n",
                                escape_md(&item.product_name),
                                price_major,
                                price_minor
                            ));
                            total_price += item.price * item.quantity;
                        }

                        let total_major = total_price / 100;
                        let total_minor = total_price % 100;
                        text.push_str(&format!(
                            "\n💰 *TOTAL: ${}.{:02}*",
                            total_major, total_minor
                        ));

                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "✅ Checkout",
                                "cart_checkout",
                            )],
                            vec![InlineKeyboardButton::callback(
                                "🗑️ Clear Cart",
                                "cart_clear",
                            )],
                        ];

                        let _ = bot
                            .send_message(msg.chat.id, text)
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await
                            .map(move |m| {
                                let state = state.clone();
                                let bot = bot.clone();
                                let uid = user.id;
                                tokio::spawn(async move {
                                    register_bot_message(bot, &state, uid, &m).await;
                                });
                            });
                    }
                }
            }
            "/enter_promo" | "🎁 Redeem Code" => {
                let _ = bot.send_message(msg.chat.id, "🎟 *Redeem Gift Code*\n\nPlease reply to this message with your code (e.g., `EXA-GIFT-XYZ`).")
                    .parse_mode(ParseMode::MarkdownV2)
                    .reply_markup(ForceReply::new().selective())
                    .await;
            }

            "🛍 Buy Subscription" | "/plans" => {
                let plans_res: AnyhowResult<Vec<Plan>> =
                    state.store_service.get_active_plans().await;
                let plans = plans_res.unwrap_or_default();

                if plans.is_empty() {
                    let _ = bot
                        .send_message(msg.chat.id, "❌ No active plans available at the moment.")
                        .reply_markup(main_menu())
                        .await;
                } else {
                    let total_plans = plans.len();
                    let index = 0;
                    let plan = &plans[index];

                    let mut text = format!(
                        "💎 *{}* \\({}/{}\\)\n\n",
                        escape_md(&plan.name),
                        index + 1,
                        total_plans
                    );
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
                            format!(
                                "{}d - ${}.{:02}",
                                dur.duration_days, price_major, price_minor
                            )
                        };
                        duration_row.push(InlineKeyboardButton::callback(
                            label,
                            format!("buy_dur_{}", dur.id),
                        ));
                    }
                    if !duration_row.is_empty() {
                        buttons.push(duration_row);
                    }

                    // Navigation
                    if total_plans > 1 {
                        let mut nav_row = Vec::new();
                        let next_idx = if index + 1 < total_plans {
                            index + 1
                        } else {
                            0
                        };
                        let prev_idx = if index > 0 {
                            index - 1
                        } else {
                            total_plans - 1
                        };

                        nav_row.push(InlineKeyboardButton::callback(
                            "⬅️",
                            format!("buy_plan_idx_{}", prev_idx),
                        ));
                        nav_row.push(InlineKeyboardButton::callback(
                            format!("{}/{}", index + 1, total_plans),
                            "noop",
                        ));
                        nav_row.push(InlineKeyboardButton::callback(
                            "➡️",
                            format!("buy_plan_idx_{}", next_idx),
                        ));
                        buttons.push(nav_row);
                    }

                    let _ = bot
                        .send_message(msg.chat.id, text)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await
                        .map(move |m| {
                            if let Some(user) = &user_res {
                                let state = state.clone();
                                let bot = bot.clone();
                                let uid = user.id;
                                tokio::spawn(async move {
                                    register_bot_message(bot, &state, uid, &m).await;
                                });
                            }
                        });
                }
            }

            "👤 My Profile" | "/profile" => {
                if let Some(user) = &user_res {
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
                    buttons.push(vec![InlineKeyboardButton::callback(
                        "💳 Top-up Balance",
                        "topup_menu",
                    )]);

                    let _ = bot
                        .send_message(msg.chat.id, response)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await
                        .map(move |m| {
                            let state = state.clone();
                            let bot = bot.clone();
                            let uid = user.id;
                            tokio::spawn(async move {
                                register_bot_message(bot, &state, uid, &m).await;
                            });
                        });
                }
            }

            "🔐 My Services" | "/services" => {
                if let Some(user) = &user_res {
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
                    sorted_subs.sort_by(|a, b| {
                        match (a.sub.status.as_str(), b.sub.status.as_str()) {
                            ("pending", "active") => std::cmp::Ordering::Less,
                            ("active", "pending") => std::cmp::Ordering::Greater,
                            _ => b.sub.created_at.cmp(&a.sub.created_at),
                        }
                    });

                    if sorted_subs.is_empty() {
                        response.push_str("📡 VPN Status: ❌ *No Subscriptions*\n\n");
                        let _ = bot
                            .send_message(msg.chat.id, response)
                            .parse_mode(ParseMode::MarkdownV2)
                            .await
                            .map(move |m| {
                                let state = state.clone();
                                let bot = bot.clone();
                                let uid = user.id;
                                tokio::spawn(async move {
                                    register_bot_message(bot, &state, uid, &m).await;
                                });
                            });
                    } else {
                        // Default to page 0
                        let page = 0;
                        let total_pages = sorted_subs.len();
                        let sub = &sorted_subs[page];

                        let status_icon = if sub.sub.status == "active" {
                            "✅"
                        } else {
                            "⏳"
                        };
                        response.push_str(&format!(
                            "🔹 *Subscription \\#{}/{:}*\n",
                            page + 1,
                            total_pages
                        ));
                        response
                            .push_str(&format!("   💎 *Plan:* {}\n", escape_md(&sub.plan_name)));
                        if let Some(desc) = &sub.plan_description {
                            response.push_str(&format!("   _{}_\n", escape_md(desc)));
                        }
                        response.push_str(&format!(
                            "   🔑 *Status:* {} `{}`\n",
                            status_icon, sub.sub.status
                        ));

                        // Traffic
                        let used_gb = sub.sub.used_traffic as f64 / 1024.0 / 1024.0 / 1024.0;
                        if let Some(limit) = sub.traffic_limit_gb {
                            if limit == 0 {
                                response.push_str(&format!(
                                    "   📊 *Traffic:* `{:.2} GB / ∞`\n",
                                    used_gb
                                ));
                            } else {
                                response.push_str(&format!(
                                    "   📊 *Traffic:* `{:.2} GB / {} GB`\n",
                                    used_gb, limit
                                ));
                            }
                        } else {
                            response
                                .push_str(&format!("   📊 *Traffic Used:* `{:.2} GB`\n", used_gb));
                        }

                        if sub.sub.status == "active" {
                            let duration = sub.sub.expires_at - sub.sub.created_at;
                            if duration.num_days() == 0 {
                                response.push_str(
                                    "   ⌛ *Expires:* `No expiration` \\(Traffic Plan\\)\n",
                                );
                            } else {
                                response.push_str(&format!(
                                    "   ⌛ *Expires:* `{}`\n",
                                    sub.sub.expires_at.format("%Y-%m-%d")
                                ));
                            }
                        } else {
                            let duration = sub.sub.expires_at - sub.sub.created_at;
                            if duration.num_days() == 0 {
                                response.push_str(
                                    "   ⏱ *Duration:* `No expiration` \\(Traffic Plan\\)\n",
                                );
                            } else {
                                response.push_str(&format!(
                                    "   ⏱ *Duration:* `{} days` \\(starts on activation\\)\n",
                                    duration.num_days()
                                ));
                            }
                        }
                        response.push_str("\n");
                        if let Some(note) = &sub.sub.note {
                            response.push_str(&format!("📝 *Note:* {}\n\n", escape_md(note)));
                        }

                        // Navigation & Actions
                        let mut buttons = Vec::new();

                        // Edit Note Button
                        buttons.push(vec![InlineKeyboardButton::callback(
                            "📝 Edit Note",
                            format!("edit_note_{}", sub.sub.id),
                        )]);

                        // Connected Devices Button (for active subscriptions)
                        if sub.sub.status == "active" {
                            buttons.push(vec![InlineKeyboardButton::callback(
                                "📱 Connected Devices",
                                format!("devices_{}", sub.sub.id),
                            )]);
                        }

                        // Action Buttons
                        if sub.sub.status == "active" {
                            buttons.push(vec![
                                InlineKeyboardButton::callback(
                                    "🔗 Get Config",
                                    format!("get_links_{}", sub.sub.id),
                                ),
                                InlineKeyboardButton::callback(
                                    "⏳ Extend",
                                    format!("extend_sub_{}", sub.sub.id),
                                ),
                            ]);
                        } else if sub.sub.status == "pending" {
                            buttons.push(vec![
                                InlineKeyboardButton::callback(
                                    "▶️ Activate",
                                    format!("activate_{}", sub.sub.id),
                                ),
                                InlineKeyboardButton::callback(
                                    "🎁 Make Gift Code",
                                    format!("gift_init_{}", sub.sub.id),
                                ),
                            ]);
                        }

                        // Navigation Row
                        let mut nav_row = Vec::new();
                        if total_pages > 1 {
                            let prev_page = if page > 0 { page - 1 } else { total_pages - 1 };
                            let next_page = if page < total_pages - 1 { page + 1 } else { 0 };

                            nav_row.push(InlineKeyboardButton::callback(
                                "⬅️ Prev",
                                format!("myservices_page_{}", prev_page),
                            ));
                            nav_row.push(InlineKeyboardButton::callback(
                                format!("{}/{}", page + 1, total_pages),
                                "ignore",
                            ));
                            nav_row.push(InlineKeyboardButton::callback(
                                "Next ➡️",
                                format!("myservices_page_{}", next_page),
                            ));
                        }
                        if !nav_row.is_empty() {
                            buttons.push(nav_row);
                        }

                        // My Gifts Link
                        buttons.push(vec![InlineKeyboardButton::callback(
                            "🎁 My Gift Codes",
                            "my_gifts",
                        )]);

                        let _ = bot
                            .send_message(msg.chat.id, response)
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await
                            .map(move |m| {
                                let state = state.clone();
                                let bot = bot.clone();
                                let uid = user.id;
                                tokio::spawn(async move {
                                    register_bot_message(bot, &state, uid, &m).await;
                                });
                            });
                    }
                }
            }

            "🎁 Bonuses / Referral" | "/referral" => {
                if let Some(user) = &user_res {
                    let bot_me = bot.get_me().await.ok();
                    let bot_username = bot_me
                        .and_then(|m| m.username.clone())
                        .unwrap_or_else(|| "bot".to_string());

                    // Use referral_code (alias) if exists, fallback to tg_id
                    let ref_code = user
                        .referral_code
                        .clone()
                        .unwrap_or_else(|| user.tg_id.to_string());
                    let ref_link = format!("https://t.me/{}?start={}", bot_username, ref_code);

                    let ref_count: i64 = state
                        .store_service
                        .get_referral_count(user.id)
                        .await
                        .unwrap_or(0);
                    let ref_earnings: i64 = state
                        .store_service
                        .get_user_referral_earnings(user.id)
                        .await
                        .unwrap_or(0);
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
                        earnings_major,
                        earnings_minor,
                        ref_code.replace("`", "\\`").replace("\\", "\\\\"),
                        ref_link.replace("`", "\\`").replace("\\", "\\\\")
                    );

                    let mut buttons = Vec::new();
                    buttons.push(vec![InlineKeyboardButton::callback(
                        "🎟 Enter Promo Code",
                        "enter_promo",
                    )]);

                    // Add Referral Management Buttons
                    buttons.push(vec![InlineKeyboardButton::callback(
                        "🔗 Edit My Code (Alias)",
                        "edit_ref_code",
                    )]);
                    if user.referrer_id.is_none() {
                        buttons.push(vec![InlineKeyboardButton::callback(
                            "🎁 Enter Referrer Code",
                            "enter_referrer",
                        )]);
                    }

                    let _ = bot
                        .send_message(msg.chat.id, response)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await
                        .map(move |m| {
                            let state = state.clone();
                            let bot = bot.clone();
                            let uid = user.id;
                            tokio::spawn(async move {
                                register_bot_message(bot, &state, uid, &m).await;
                            });
                        });
                }
            }

            "❓ Support" => {
                let support_username = state.settings.get_or_default("support_url", "").await;

                if support_username.is_empty() {
                    let _ = bot
                        .send_message(msg.chat.id, "❌ Support contact is not configured yet.")
                        .reply_markup(main_menu())
                        .await;
                } else {
                    // Sanitize username (remove @ if present)
                    let clean_username = support_username.trim_start_matches('@');
                    let url = format!("https://t.me/{}", clean_username);

                    let kb = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::url(
                        "💬 Contact Support",
                        url.parse().unwrap(),
                    )]]);

                    let _ = bot
                        .send_message(
                            msg.chat.id,
                            "Need help? Click the button below to contact support:",
                        )
                        .reply_markup(kb)
                        .await;
                }
            }

            "/devices" | "📱 My Devices" => {
                if let Some(u) = &user_res {
                    if let Ok(subs) = state.store_service.get_user_subscriptions(u.id).await {
                        let active_subs: Vec<DetailedSubscription> = subs
                            .into_iter()
                            .filter(|s| s.sub.status == "active")
                            .collect();

                        if active_subs.is_empty() {
                            let _ = bot
                                .send_message(msg.chat.id, "❌ You have no active subscriptions.")
                                .reply_markup(main_menu())
                                .await;
                        } else if active_subs.len() == 1 {
                            // Auto-show active devices for the only subscription
                            let sub = &active_subs[0];
                            // Delegate to reuse logic
                            let ips_res: AnyhowResult<Vec<SubscriptionIpTracking>> = state
                                .store_service
                                .get_subscription_active_ips(sub.sub.id)
                                .await;
                            let ips = ips_res.unwrap_or_default();
                            let limit_res: AnyhowResult<i64> = state
                                .store_service
                                .get_subscription_device_limit(sub.sub.id)
                                .await;
                            let limit = limit_res.unwrap_or(0);

                            let mut text = format!(
                                "📱 *Active Devices for Subscription \\#{:?}*\n",
                                sub.sub.id
                            );
                            text.push_str(&format!(
                                "Limit: `{}/{}` devices\n\n",
                                ips.len(),
                                if limit == 0 {
                                    "∞".to_string()
                                } else {
                                    limit.to_string()
                                }
                            ));

                            if ips.is_empty() {
                                text.push_str(
                                    "No active sessions detected in the last 15 minutes\\.",
                                );
                            } else {
                                for ip in &ips {
                                    // Robust time handling
                                    let duration =
                                        chrono::Utc::now().signed_duration_since(ip.last_seen_at);
                                    let mins = duration.num_minutes();
                                    text.push_str(&format!(
                                        "• `{}` \\({} mins ago\\)\n",
                                        ip.client_ip.replace(".", "\\."),
                                        mins
                                    ));
                                }
                            }

                            let mut buttons = Vec::new();
                            if !ips.is_empty() {
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    "☠️ Reset Sessions",
                                    format!("kill_sessions_{}", sub.sub.id),
                                )]);
                            }

                            let _ = bot
                                .send_message(msg.chat.id, text)
                                .parse_mode(ParseMode::MarkdownV2)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await
                                .map(move |m| {
                                    let state = state.clone();
                                    let bot = bot.clone();
                                    let uid = u.id;
                                    tokio::spawn(async move {
                                        register_bot_message(bot, &state, uid, &m).await;
                                    });
                                });
                        } else {
                            // Multiple active subs - ask which one
                            let mut buttons = Vec::new();
                            for sub in active_subs {
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    format!("Plan: {} (#{})", sub.plan_name, sub.sub.id),
                                    format!("devices_{}", sub.sub.id),
                                )]);
                            }

                            let _ = bot
                                .send_message(
                                    msg.chat.id,
                                    "📱 *Select a subscription to view devices:*",
                                )
                                .parse_mode(ParseMode::MarkdownV2)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await;
                        }
                    }
                }
            }
            _ => {
                // Ignore unknown commands
            }
        }
    }

    Ok(())
}

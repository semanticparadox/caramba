use crate::bot::handlers::admin::handle_admin_text;
use crate::bot::keyboards::{language_keyboard, main_menu, terms_keyboard};
use crate::bot::translations::{t, tf};
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
    let tg_id = msg.chat.id.0;

    if let Some(payment) = msg.successful_payment() {
        // Идемпотентный ключ — charge_id гарантированно уникален для каждого платежа.
        // Передаём его панели, чтобы та могла игнорировать повторные события от Telegram.
        let charge_id = payment.provider_payment_charge_id.clone();
        let amount_xtr = payment.total_amount as f64;
        // 1 USD ≈ 50 XTR (курс Telegram Stars)
        let amount_usd = amount_xtr / 50.0;

        info!(
            tg_id,
            charge_id = %charge_id,
            amount_xtr,
            amount_usd,
            payload = %payment.invoice_payload,
            "Processing Stars payment"
        );

        match state
            .pay_service
            .process_any_payment(
                amount_usd,
                "stars",
                Some(charge_id.clone()),
                &payment.invoice_payload,
            )
            .await
        {
            Ok(_) => {
                let _ = state
                    .logging_service
                    .log_user(
                        Some(tg_id),
                        "payment_stars",
                        &format!(
                            "Stars payment successful: {} XTR (${:.2}) charge={}",
                            amount_xtr, amount_usd, charge_id
                        ),
                        None,
                    )
                    .await;

                let pay_lang = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|u| u.language_code.clone());
                let _ = bot
                    .send_message(msg.chat.id, t(pay_lang.as_deref(), "msg.payment_success"))
                    .await;
            }
            Err(e) => {
                error!(tg_id, charge_id = %charge_id, error = %e, "Stars payment processing failed");
                let pay_lang = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|u| u.language_code.clone());
                let _ = bot
                    .send_message(msg.chat.id, t(pay_lang.as_deref(), "msg.payment_error"))
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

                    // Если пользователь зарегистрировался по реферальной ссылке —
                    // запрашиваем начисление signup-бонусов.
                    // Метод идемпотентен: повторный вызов не дублирует начисление.
                    if let Some(r_id) = referrer_id {
                        if state.api_client.has_token() {
                            let referred_id = u.id;
                            let api = state.api_client.clone();
                            tokio::spawn(async move {
                                if let Err(e) =
                                    api.apply_referral_signup_bonus(r_id, referred_id).await
                                {
                                    tracing::warn!(
                                        "Failed to apply referral signup bonus (referrer={} referred={}): {}",
                                        r_id, referred_id, e
                                    );
                                }
                            });
                        }
                    }

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
                let lang = user.language_code.as_deref();
                let _ = bot
                    .send_message(msg.chat.id, t(lang, "msg.access_denied"))
                    .parse_mode(ParseMode::MarkdownV2)
                    .await;
                return Ok(());
            }

            if user.language_code.is_none() {
                let _ = bot
                    .send_message(msg.chat.id, t(None, "msg.select_language"))
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
                    // warning_count — значение до инкремента; после него становится +1
                    const MAX_WARNINGS: i32 = 5;
                    let current_warning = user.warning_count + 1;
                    if user.warning_count >= MAX_WARNINGS {
                        let _ = state.store_service.ban_user(user.id).await;
                        let lang = user.language_code.as_deref();
                        let _ = bot
                            .send_message(msg.chat.id, t(lang, "msg.account_banned"))
                            .parse_mode(ParseMode::Html)
                            .await;
                        return Ok(());
                    }
                    // Уведомляем пользователя о предупреждении, чтобы он понимал что происходит
                    let lang = user.language_code.as_deref();
                    let _ = bot
                        .send_message(
                            msg.chat.id,
                            tf(
                                lang,
                                "msg.tos_warning",
                                &[&current_warning.to_string(), &MAX_WARNINGS.to_string()],
                            ),
                        )
                        .parse_mode(ParseMode::Html)
                        .await
                        .map_err(|e| error!("Failed to send TOS warning: {}", e));
                }
                let lang = user.language_code.as_deref();
                let terms_text: String = state
                    .settings
                    .get_or_default("terms_of_service", "Terms of Service...")
                    .await;

                let _ = bot
                    .send_message(
                        msg.chat.id,
                        format!(
                            "{}\n\n{}\n\n{}",
                            t(lang, "msg.terms_header"),
                            terms_text,
                            t(lang, "msg.terms_accept_prompt")
                        ),
                    )
                    .parse_mode(ParseMode::Html)
                    .reply_markup(terms_keyboard(lang))
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
                let lang = user.language_code.as_deref();
                let user_name = msg
                    .from
                    .as_ref()
                    .map(|u| u.full_name())
                    .unwrap_or_else(|| "User".to_string());
                let welcome_text = tf(lang, "msg.hello", &[&user_name]);
                let bot_for_task = bot.clone();
                let state_for_task = state.clone();
                let _ = bot
                    .send_message(msg.chat.id, welcome_text)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(main_menu(lang))
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
                    if let Ok(url) = web_app_url.parse() {
                        let _ = bot
                            .set_chat_menu_button()
                            .chat_id(msg.chat.id)
                            .menu_button(teloxide::types::MenuButton::WebApp {
                                text: t(lang, "msg.menu_button").to_string(),
                                web_app: teloxide::types::WebAppInfo { url },
                            })
                            .await;
                    }
                }

                return Ok(());
            }
        } else if !text.starts_with("/start") {
            // Non-start message from unknown user? ignore or ask to start
            return Ok(());
        }

        // --- Admin FSM: обрабатываем текстовые вводы в FSM-состояниях администратора ---
        // Проверяем ДО стандартной обработки команд, чтобы не мешать нормальному флоу.
        if state.admin_service.is_admin(tg_id).await
            && handle_admin_text(&bot, &msg, tg_id, text, &state).await
        {
            return Ok(());
        }

        // --- Admin Commands ---
        if text.starts_with("/admin")
            || text.starts_with("/stats")
            || text.starts_with("/gift ")
            || text.starts_with("/promo ")
            || text.starts_with("/ban ")
            || text.starts_with("/unban ")
        {
            let is_admin = is_admin_tg_id(&state, tg_id).await;
            if !is_admin {
                // /admin — тихий отказ (не раскрываем команду).
                // Другие admin-команды — явный отказ.
                if text.starts_with("/admin") {
                    return Ok(());
                }
                let _ = bot.send_message(msg.chat.id, "Access denied.").await;
                return Ok(());
            }

            if text == "/admin" || text == "/admin@" || text.starts_with("/admin ") {
                // Делегируем новому обработчику с inline-клавиатурой
                let _ = crate::bot::handlers::admin::handle_admin_command(
                    bot.clone(),
                    msg.clone(),
                    state.clone(),
                )
                .await;
                return Ok(());
            }

            if text == "/stats" {
                return handle_admin_stats(&bot, &msg, &state).await;
            }

            if text.starts_with("/gift ") {
                return handle_admin_gift(&bot, &msg, &state, text).await;
            }

            if text.starts_with("/promo ") {
                return handle_admin_promo(&bot, &msg, &state, text).await;
            }

            if text.starts_with("/ban ") {
                let username = text
                    .strip_prefix("/ban ")
                    .unwrap_or("")
                    .trim()
                    .trim_start_matches('@');
                if username.is_empty() {
                    let _ = bot.send_message(msg.chat.id, "Usage: /ban @username").await;
                } else {
                    match state.store_service.ban_user_by_username(username).await {
                        Ok(_) => {
                            let _ = bot
                                .send_message(msg.chat.id, format!("Banned @{}", username))
                                .await;
                        }
                        Err(e) => {
                            error!(username, error = %e, "Admin /ban failed");
                            let _ = bot
                                .send_message(
                                    msg.chat.id,
                                    format!("Failed to ban @{}. Check logs.", username),
                                )
                                .await;
                        }
                    }
                }
                return Ok(());
            }

            if text.starts_with("/unban ") {
                let username = text
                    .strip_prefix("/unban ")
                    .unwrap_or("")
                    .trim()
                    .trim_start_matches('@');
                if username.is_empty() {
                    let _ = bot
                        .send_message(msg.chat.id, "Usage: /unban @username")
                        .await;
                } else {
                    match state.store_service.unban_user_by_username(username).await {
                        Ok(_) => {
                            let _ = bot
                                .send_message(msg.chat.id, format!("Unbanned @{}", username))
                                .await;
                        }
                        Err(e) => {
                            error!(username, error = %e, "Admin /unban failed");
                            let _ = bot
                                .send_message(
                                    msg.chat.id,
                                    format!("Failed to unban @{}. Check logs.", username),
                                )
                                .await;
                        }
                    }
                }
                return Ok(());
            }
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
                        let note_lang = state
                            .store_service
                            .get_user_by_tg_id(tg_id)
                            .await
                            .ok()
                            .flatten()
                            .and_then(|u| u.language_code.clone());
                        let _ = bot
                            .send_message(msg.chat.id, t(note_lang.as_deref(), "msg.note_updated"))
                            .await;
                        return Ok(());
                    }
                }
                // Transfer (detect both EN and RU prompt markers)
                let is_transfer = (reply_text.contains("Transfer Subscription")
                    && reply_text.contains("Subscription #"))
                    || (reply_text.contains("Передача подписки")
                        && reply_text.contains("подписки #"));
                if is_transfer {
                    // Extract sub ID: look for "Subscription #" or "подписки #"
                    let marker = if reply_text.contains("Subscription #") {
                        "Subscription #"
                    } else {
                        "подписки #"
                    };
                    if let Some(start) = reply_text.find(marker) {
                        let rest = &reply_text[start + marker.len()..];
                        let id_str = rest
                            .split(|c: char| !c.is_ascii_digit())
                            .next()
                            .unwrap_or("0");
                        if let Ok(sub_id) = id_str.parse::<i64>() {
                            let user_db_res: AnyhowResult<Option<User>> =
                                state.store_service.get_user_by_tg_id(tg_id).await;
                            if let Ok(Some(u)) = user_db_res {
                                let lang = u.language_code.as_deref();
                                match state
                                    .store_service
                                    .transfer_subscription(sub_id, u.id, text)
                                    .await
                                {
                                    Ok(_) => {
                                        let _ = bot
                                            .send_message(
                                                msg.chat.id,
                                                tf(
                                                    lang,
                                                    "msg.transfer_success",
                                                    &[&sub_id.to_string(), &escape_md(text)],
                                                ),
                                            )
                                            .parse_mode(ParseMode::MarkdownV2)
                                            .await;
                                    }
                                    Err(e) => {
                                        let _ = bot
                                            .send_message(
                                                msg.chat.id,
                                                tf(
                                                    lang,
                                                    "msg.transfer_failed",
                                                    &[&escape_md(&e.to_string())],
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

                // Gift Code (detect both EN and RU prompt markers)
                if reply_text.contains("🎟 Enter your Gift Code")
                    || reply_text.contains("🎟 Enter your Promo Code")
                    || reply_text.contains("🎟 Введите подарочный код")
                    || reply_text.contains("🎟 Активировать подарочный код")
                {
                    let code = text.trim();
                    let user_db_res: AnyhowResult<Option<User>> =
                        state.store_service.get_user_by_tg_id(tg_id).await;
                    if let Ok(Some(u)) = user_db_res {
                        let lang = u.language_code.as_deref();
                        match state.promo_service.redeem_code(u.id, code).await {
                            Ok(res_msg) => {
                                let _ = bot
                                    .send_message(
                                        msg.chat.id,
                                        tf(lang, "msg.redemption_success", &[&escape_md(&res_msg)]),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                            Err(e) => {
                                let _ = bot
                                    .send_message(
                                        msg.chat.id,
                                        tf(
                                            lang,
                                            "msg.redemption_failed",
                                            &[&escape_md(&e.to_string())],
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
                    let ref_lang = state
                        .store_service
                        .get_user_by_tg_id(tg_id)
                        .await
                        .ok()
                        .flatten()
                        .and_then(|u| u.language_code.clone());
                    if new_code.len() < 3 || new_code.len() > 32 {
                        let _ = bot
                            .send_message(
                                msg.chat.id,
                                t(ref_lang.as_deref(), "msg.alias_invalid_length"),
                            )
                            .parse_mode(ParseMode::MarkdownV2)
                            .await;
                        return Ok(());
                    }

                    if !new_code.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        let _ = bot
                            .send_message(
                                msg.chat.id,
                                t(ref_lang.as_deref(), "msg.alias_invalid_chars"),
                            )
                            .parse_mode(ParseMode::MarkdownV2)
                            .await;
                        return Ok(());
                    }

                    let user_db_res: AnyhowResult<Option<User>> =
                        state.store_service.get_user_by_tg_id(tg_id).await;
                    if let Ok(Some(u)) = user_db_res {
                        let lang = u.language_code.as_deref();
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

                                let response = tf(
                                    lang,
                                    "msg.alias_updated",
                                    &[
                                        &new_code.replace('`', "\\`").replace('\\', "\\\\"),
                                        &new_link.replace('`', "\\`").replace('\\', "\\\\"),
                                    ],
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
                                let _ = bot
                                    .send_message(msg.chat.id, t(lang, "msg.alias_taken"))
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                        }
                    }
                    return Ok(());
                }

                // Enter Referrer Code (detect both EN and RU)
                if reply_text.contains("Enter Referrer Code") || reply_text.contains("Код реферера")
                {
                    let ref_code = text.trim();
                    let user_db_res: AnyhowResult<Option<User>> =
                        state.store_service.get_user_by_tg_id(tg_id).await;
                    if let Ok(Some(u)) = user_db_res {
                        let lang = u.language_code.as_deref();
                        match state.store_service.set_user_referrer(u.id, ref_code).await {
                            Ok(_) => {
                                let _ = bot
                                    .send_message(msg.chat.id, t(lang, "msg.referrer_linked"))
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                            Err(e) => {
                                let _ = bot
                                    .send_message(
                                        msg.chat.id,
                                        tf(
                                            lang,
                                            "msg.linking_failed",
                                            &[&escape_md(&e.to_string())],
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

        // Commands and Menus
        // Get user lang for translations
        let lang = user_res.as_ref().and_then(|u| u.language_code.as_deref());

        if text == t(lang, "kb.digital_store") || text == t(Some("en"), "kb.digital_store") {
            let categories_res: AnyhowResult<Vec<StoreCategory>> =
                state.store_service.get_categories().await;
            let categories = categories_res.unwrap_or_default();
            if categories.is_empty() {
                let _ = bot
                    .send_message(msg.chat.id, t(lang, "msg.store_empty"))
                    .reply_markup(main_menu(lang))
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
                    t(lang, "kb.view_cart"),
                    "view_cart",
                )]);

                let kb = InlineKeyboardMarkup::new(buttons);
                let _ = bot
                    .send_message(msg.chat.id, t(lang, "msg.store_welcome"))
                    .parse_mode(ParseMode::MarkdownV2)
                    .reply_markup(kb)
                    .await;
            }
        } else if text == "🛒 My Cart"
            || text == "/cart"
            || text == t(lang, "kb.view_cart")
            || text == t(Some("en"), "kb.view_cart")
        {
            if let Some(user) = &user_res {
                let cart_items_res: AnyhowResult<Vec<CartItem>> =
                    state.store_service.get_user_cart(user.id).await;
                let cart_items = cart_items_res.unwrap_or_default();

                if cart_items.is_empty() {
                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "msg.cart_empty"))
                        .await;
                } else {
                    let mut total_price: i64 = 0;
                    let mut text = t(lang, "msg.cart_header").to_string();

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
                            t(lang, "kb.checkout"),
                            "cart_checkout",
                        )],
                        vec![InlineKeyboardButton::callback(
                            t(lang, "kb.clear_cart"),
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
        } else if text == "/enter_promo" || text == "🎁 Redeem Code" {
            let _ = bot
                .send_message(msg.chat.id, t(lang, "msg.redeem_gift"))
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(ForceReply::new().selective())
                .await;
        } else if text == t(lang, "kb.buy_sub")
            || text == t(Some("en"), "kb.buy_sub")
            || text == "/plans"
        {
            let plans_res: AnyhowResult<Vec<Plan>> = state.store_service.get_active_plans().await;
            let plans = plans_res.unwrap_or_default();

            if plans.is_empty() {
                let _ = bot
                    .send_message(msg.chat.id, t(lang, "msg.no_plans"))
                    .reply_markup(main_menu(lang))
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
                        format!(
                            "🚀 {} - ${}.{:02}",
                            t(lang, "msg.traffic_plan"),
                            price_major,
                            price_minor
                        )
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
        } else if text == t(lang, "kb.my_profile")
            || text == t(Some("en"), "kb.my_profile")
            || text == "/profile"
        {
            if let Some(user) = &user_res {
                let price_major = user.balance / 100;
                let price_minor = user.balance % 100;

                let balance_str = format!("{}.{:02}", price_major, price_minor);
                let response = tf(
                    lang,
                    "msg.user_profile",
                    &[&user.tg_id.to_string(), &balance_str],
                );

                let buttons = vec![vec![InlineKeyboardButton::callback(
                    t(lang, "kb.topup"),
                    "topup_menu",
                )]];

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
        } else if text == t(lang, "kb.my_services")
            || text == t(Some("en"), "kb.my_services")
            || text == "/services"
        {
            if let Some(user) = &user_res {
                let mut response = t(lang, "msg.my_services").to_string();

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
                sorted_subs.sort_by(
                    |a, b| match (a.sub.status.as_str(), b.sub.status.as_str()) {
                        ("pending", "active") => std::cmp::Ordering::Less,
                        ("active", "pending") => std::cmp::Ordering::Greater,
                        _ => b.sub.created_at.cmp(&a.sub.created_at),
                    },
                );

                if sorted_subs.is_empty() {
                    response.push_str(t(lang, "msg.no_subscriptions"));
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
                    response.push_str(&format!(
                        "   💎 *{}* {}\n",
                        t(lang, "msg.plan"),
                        escape_md(&sub.plan_name)
                    ));
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
                                "   📊 *{}* `{:.2} GB / ∞`\n",
                                t(lang, "msg.traffic"),
                                used_gb
                            ));
                        } else {
                            response.push_str(&format!(
                                "   📊 *{}* `{:.2} GB / {} GB`\n",
                                t(lang, "msg.traffic"),
                                used_gb,
                                limit
                            ));
                        }
                    } else {
                        response.push_str(&format!(
                            "   📊 *{}* `{:.2} GB`\n",
                            t(lang, "msg.traffic_used"),
                            used_gb
                        ));
                    }

                    if sub.sub.status == "active" {
                        let duration = sub.sub.expires_at - sub.sub.created_at;
                        if duration.num_days() == 0 {
                            response.push_str(&format!(
                                "   ⌛ *{}* `{}` \\({}\\)\n",
                                t(lang, "msg.expires"),
                                t(lang, "msg.no_expiration"),
                                t(lang, "msg.traffic_plan")
                            ));
                        } else {
                            response.push_str(&format!(
                                "   ⌛ *{}* `{}`\n",
                                t(lang, "msg.expires"),
                                sub.sub.expires_at.format("%Y-%m-%d")
                            ));
                        }
                    } else {
                        let duration = sub.sub.expires_at - sub.sub.created_at;
                        if duration.num_days() == 0 {
                            response.push_str(&format!(
                                "   ⏱ *{}* `{}` \\({}\\)\n",
                                t(lang, "msg.duration"),
                                t(lang, "msg.no_expiration"),
                                t(lang, "msg.traffic_plan")
                            ));
                        } else {
                            response.push_str(&format!(
                                "   ⏱ *{}* `{} {}` \\({}\\)\n",
                                t(lang, "msg.duration"),
                                duration.num_days(),
                                t(lang, "msg.days"),
                                t(lang, "msg.starts_on_activation")
                            ));
                        }
                    }
                    response.push('\n');
                    if let Some(note) = &sub.sub.note {
                        response.push_str(&format!("📝 *Note:* {}\n\n", escape_md(note)));
                    }

                    // Navigation & Actions
                    let mut buttons = Vec::new();

                    // Edit Note Button
                    buttons.push(vec![InlineKeyboardButton::callback(
                        t(lang, "kb.edit_note"),
                        format!("edit_note_{}", sub.sub.id),
                    )]);

                    // Connected Devices Button (for active subscriptions)
                    if sub.sub.status == "active" {
                        buttons.push(vec![InlineKeyboardButton::callback(
                            t(lang, "kb.devices"),
                            format!("devices_{}", sub.sub.id),
                        )]);
                    }

                    // Action Buttons
                    if sub.sub.status == "active" {
                        buttons.push(vec![
                            InlineKeyboardButton::callback(
                                t(lang, "kb.get_config"),
                                format!("get_links_{}", sub.sub.id),
                            ),
                            InlineKeyboardButton::callback(
                                t(lang, "kb.extend"),
                                format!("extend_sub_{}", sub.sub.id),
                            ),
                        ]);
                    } else if sub.sub.status == "pending" {
                        buttons.push(vec![
                            InlineKeyboardButton::callback(
                                t(lang, "kb.activate"),
                                format!("activate_{}", sub.sub.id),
                            ),
                            InlineKeyboardButton::callback(
                                t(lang, "kb.make_gift"),
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
                            t(lang, "kb.prev"),
                            format!("myservices_page_{}", prev_page),
                        ));
                        nav_row.push(InlineKeyboardButton::callback(
                            format!("{}/{}", page + 1, total_pages),
                            "ignore",
                        ));
                        nav_row.push(InlineKeyboardButton::callback(
                            t(lang, "kb.next"),
                            format!("myservices_page_{}", next_page),
                        ));
                    }
                    if !nav_row.is_empty() {
                        buttons.push(nav_row);
                    }

                    // My Gifts Link
                    buttons.push(vec![InlineKeyboardButton::callback(
                        t(lang, "kb.my_gifts"),
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
        } else if text == t(lang, "kb.bonuses")
            || text == t(Some("en"), "kb.bonuses")
            || text == "/referral"
        {
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

                let mut response = t(lang, "msg.bonus_header").to_string();
                response.push_str(&tf(lang, "msg.your_stats", &[]));
                response.push_str(&tf(lang, "msg.referrals_joined", &[&ref_count.to_string()]));
                response.push_str(&tf(
                    lang,
                    "msg.total_earned",
                    &[
                        &earnings_major.to_string(),
                        &format!("{:02}", earnings_minor),
                    ],
                ));
                response.push_str(&tf(
                    lang,
                    "msg.promo_data",
                    &[
                        &ref_code.replace('`', "\\`").replace('\\', "\\\\"),
                        &ref_link.replace('`', "\\`").replace('\\', "\\\\"),
                    ],
                ));

                let mut buttons = Vec::new();
                buttons.push(vec![InlineKeyboardButton::callback(
                    t(lang, "kb.enter_promo"),
                    "enter_promo",
                )]);

                // Add Referral Management Buttons
                buttons.push(vec![InlineKeyboardButton::callback(
                    t(lang, "kb.edit_ref_code"),
                    "edit_ref_code",
                )]);
                if user.referrer_id.is_none() {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        t(lang, "kb.enter_referrer"),
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
        } else if text == t(lang, "kb.guides") || text == t(Some("en"), "kb.guides") {
            match crate::bot::keyboards::guides_keyboard(&state.settings, lang).await {
                Some(kb) => {
                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "msg.guides_prompt"))
                        .reply_markup(kb)
                        .await;
                }
                None => {
                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "msg.guides_missing"))
                        .reply_markup(main_menu(lang))
                        .await;
                }
            }
        } else if text == t(lang, "kb.support") || text == t(Some("en"), "kb.support") {
            let support_username = state.settings.get_or_default("support_url", "").await;

            if support_username.is_empty() {
                let _ = bot
                    .send_message(msg.chat.id, t(lang, "msg.support_not_configured"))
                    .reply_markup(main_menu(lang))
                    .await;
            } else {
                // Sanitize username (remove @ if present)
                let clean_username = support_username.trim_start_matches('@');
                let url = format!("https://t.me/{}", clean_username);

                match url.parse::<reqwest::Url>() {
                    Ok(parsed_url) => {
                        let kb = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::url(
                            t(lang, "kb.contact_support"),
                            parsed_url,
                        )]]);
                        let _ = bot
                            .send_message(msg.chat.id, t(lang, "msg.support_prompt"))
                            .reply_markup(kb)
                            .await;
                    }
                    Err(e) => {
                        error!(support_username = %clean_username, error = %e, "Invalid support URL; falling back to text");
                        let _ = bot
                            .send_message(msg.chat.id, t(lang, "msg.support_not_configured"))
                            .reply_markup(main_menu(lang))
                            .await;
                    }
                }
            }
        } else if text == "/devices" || text == "📱 My Devices" {
            if let Some(u) = &user_res {
                if let Ok(subs) = state.store_service.get_user_subscriptions(u.id).await {
                    let active_subs: Vec<DetailedSubscription> = subs
                        .into_iter()
                        .filter(|s| s.sub.status == "active")
                        .collect();

                    if active_subs.is_empty() {
                        let _ = bot
                            .send_message(msg.chat.id, t(lang, "msg.no_active_subs"))
                            .reply_markup(main_menu(lang))
                            .await;
                    } else if active_subs.len() == 1 {
                        // Auto-show active devices for the only subscription
                        let sub = &active_subs[0];
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

                        let mut text =
                            tf(lang, "msg.devices_for_sub", &[&format!("{:?}", sub.sub.id)]);
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
                            text.push_str(t(lang, "msg.no_sessions"));
                        } else {
                            for ip in &ips {
                                let duration =
                                    chrono::Utc::now().signed_duration_since(ip.last_seen_at);
                                let mins = duration.num_minutes();
                                text.push_str(&format!(
                                    "• `{}` \\({} {}\\)\n",
                                    ip.client_ip.replace(".", "\\."),
                                    mins,
                                    t(lang, "msg.mins_ago")
                                ));
                            }
                        }

                        let mut buttons = Vec::new();
                        if !ips.is_empty() {
                            buttons.push(vec![InlineKeyboardButton::callback(
                                t(lang, "kb.reset_sessions"),
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
                                format!(
                                    "{} {} (#{})",
                                    t(lang, "msg.plan"),
                                    sub.plan_name,
                                    sub.sub.id
                                ),
                                format!("devices_{}", sub.sub.id),
                            )]);
                        }

                        let _ = bot
                            .send_message(msg.chat.id, t(lang, "msg.select_sub_devices"))
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await;
                    }
                }
            }
        } else {
            // Ignore unknown commands
        }
    }

    Ok(())
}

// ============================================================================
// Admin Command Helpers
// ============================================================================

async fn is_admin_tg_id(state: &AppState, tg_id: i64) -> bool {
    let admin_ids_str = state
        .settings
        .get_or_default("admin_notification_tg_ids", "")
        .await;
    admin_ids_str
        .split(',')
        .any(|s| s.trim().parse::<i64>().ok() == Some(tg_id))
}

async fn handle_admin_stats(
    bot: &Bot,
    msg: &Message,
    state: &AppState,
) -> Result<(), teloxide::RequestError> {
    let stats = state.store_service.get_system_stats().await;
    match stats {
        Ok(s) => {
            let text = format!(
                "📊 <b>System Stats</b>\n\n\
                Nodes: {} active\n\
                Users: {}\n\
                Active subs: {}\n\
                Revenue: ${:.2}\n\
                Traffic (30d): {} GB",
                s.active_nodes, s.total_users, s.active_subs, s.total_revenue, s.traffic_30d_gb,
            );
            let _ = bot
                .send_message(msg.chat.id, text)
                .parse_mode(ParseMode::Html)
                .await;
        }
        Err(e) => {
            error!(error = %e, "Admin /stats failed");
            let _ = bot
                .send_message(msg.chat.id, "Failed to fetch stats. Check logs.")
                .await;
        }
    }
    Ok(())
}

async fn handle_admin_gift(
    bot: &Bot,
    msg: &Message,
    state: &AppState,
    text: &str,
) -> Result<(), teloxide::RequestError> {
    // /gift @username 30d
    let parts: Vec<&str> = text.splitn(3, ' ').collect();
    if parts.len() < 3 {
        let _ = bot
            .send_message(msg.chat.id, "Usage: /gift @username 30d")
            .await;
        return Ok(());
    }
    let username = parts[1].trim_start_matches('@');
    let duration_str = parts[2].trim();
    let days: i64 = duration_str.trim_end_matches('d').parse().unwrap_or(0);
    if days <= 0 {
        let _ = bot
            .send_message(msg.chat.id, "Invalid duration. Use e.g. 30d")
            .await;
        return Ok(());
    }

    match state
        .store_service
        .admin_gift_subscription(username, days)
        .await
    {
        Ok(sub_id) => {
            let _ = bot
                .send_message(
                    msg.chat.id,
                    format!(
                        "Gift sub #{} created for @{} ({} days)",
                        sub_id, username, days
                    ),
                )
                .await;
        }
        Err(e) => {
            error!(username, error = %e, "Admin /gift failed");
            let _ = bot
                .send_message(
                    msg.chat.id,
                    format!("Failed to gift sub to @{}. Check logs.", username),
                )
                .await;
        }
    }
    Ok(())
}

async fn handle_admin_promo(
    bot: &Bot,
    msg: &Message,
    state: &AppState,
    text: &str,
) -> Result<(), teloxide::RequestError> {
    let parts: Vec<&str> = text.splitn(5, ' ').collect();
    // /promo list
    if parts.len() >= 2 && parts[1] == "list" {
        match state.promo_service.list_promos().await {
            Ok(promos) => {
                if promos.is_empty() {
                    let _ = bot.send_message(msg.chat.id, "No active promos.").await;
                } else {
                    let lines: Vec<String> = promos
                        .iter()
                        .map(|p| format!("<code>{}</code> — {} uses", p.code, p.use_count))
                        .collect();
                    let _ = bot
                        .send_message(
                            msg.chat.id,
                            format!("📋 <b>Active Promos</b>\n\n{}", lines.join("\n")),
                        )
                        .parse_mode(ParseMode::Html)
                        .await;
                }
            }
            Err(e) => {
                error!(error = %e, "Admin /promo list failed");
                let _ = bot
                    .send_message(msg.chat.id, "Failed to list promos. Check logs.")
                    .await;
            }
        }
        return Ok(());
    }
    // /promo create CODE balance 500
    if parts.len() >= 5 && parts[1] == "create" {
        let code = parts[2];
        let promo_type = parts[3];
        let value: i64 = parts[4].parse().unwrap_or(0);
        if value <= 0 {
            let _ = bot.send_message(msg.chat.id, "Invalid value.").await;
            return Ok(());
        }
        match state
            .promo_service
            .create_promo(code, promo_type, value)
            .await
        {
            Ok(_) => {
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        format!("Promo {} created ({} = {})", code, promo_type, value),
                    )
                    .await;
            }
            Err(e) => {
                error!(code, promo_type, value, error = %e, "Admin /promo create failed");
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        format!("Failed to create promo {}. Check logs.", code),
                    )
                    .await;
            }
        }
        return Ok(());
    }

    let _ = bot
        .send_message(
            msg.chat.id,
            "Usage:\n/promo list\n/promo create CODE balance 500",
        )
        .await;
    Ok(())
}

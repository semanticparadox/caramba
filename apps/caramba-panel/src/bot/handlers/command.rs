use crate::AppState;
use crate::bot::keyboards::{language_keyboard, main_menu, terms_keyboard};
use crate::bot::translations::{Lang, contains_any_lang, lang_for, matches_any_lang, t, tf};
use crate::bot::utils::{escape_html, register_bot_message};
use crate::services::logging_service::LoggingService;
use teloxide::prelude::*;
use teloxide::types::{ChatId, ForceReply, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};
use tracing::{error, info};

/// Пункт меню, к которому свелось входящее сообщение.
///
/// Reply-клавиатура присылает подпись кнопки обычным текстом, а подписи теперь
/// локализованы — значит распознавать надо и русский, и английский вариант.
/// Плюс старые английские подписи: у пользователей, открывших бота до
/// локализации, клавиатура остаётся отрисованной со старым текстом, пока они не
/// нажмут /start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Store,
    Cart,
    RedeemCode,
    BuyPlans,
    Profile,
    Services,
    Referral,
    Support,
    Guides,
    Devices,
    Leaderboard,
    Login,
}

fn menu_action(text: &str) -> Option<MenuAction> {
    use MenuAction::*;

    // Слэш-команды — язык не важен.
    match text {
        "/cart" => return Some(Cart),
        "/enter_promo" => return Some(RedeemCode),
        "/plans" => return Some(BuyPlans),
        "/profile" => return Some(Profile),
        "/services" => return Some(Services),
        "/referral" => return Some(Referral),
        "/devices" => return Some(Devices),
        "/leaderboard" => return Some(Leaderboard),
        "/login" => return Some(Login),
        _ => {}
    }

    // Устаревшие английские подписи с уже отрисованных клавиатур.
    match text {
        "📦 Digital Store" => return Some(Store),
        "🛒 My Cart" => return Some(Cart),
        "🎁 Redeem Code" => return Some(RedeemCode),
        "🛍 Buy Subscription" => return Some(BuyPlans),
        "👤 My Profile" => return Some(Profile),
        "🔐 My Services" => return Some(Services),
        "🎁 Bonuses / Referral" => return Some(Referral),
        "❓ Support" => return Some(Support),
        "📱 My Devices" => return Some(Devices),
        "🏆 Leaderboard" => return Some(Leaderboard),
        "🔑 Open in app" | "🔑 Войти в приложение" => return Some(Login),
        _ => {}
    }

    // Актуальные подписи — на любом поддерживаемом языке.
    for (key, action) in [
        ("menu.store", Store),
        ("cart.view", Cart),
        ("promo.enter_code_btn", RedeemCode),
        ("menu.buy", BuyPlans),
        ("menu.profile", Profile),
        ("menu.services", Services),
        ("menu.referral", Referral),
        ("menu.support", Support),
        ("menu.guides", Guides),
        ("menu.open_app", Login),
    ] {
        if matches_any_lang(text, key) {
            return Some(action);
        }
    }

    None
}

/// Форматирует сумму в минорных единицах (центах) как "12.34".
pub fn money(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, (cents % 100).abs())
}

/// Главное меню с текущими настройками отображения.
async fn menu_markup(state: &AppState, lang: Lang) -> teloxide::types::KeyboardMarkup {
    let app_mode = state
        .settings
        .get_or_default("bot_buttons_mode", "full")
        .await
        == "app_only";
    let always_support = state
        .settings
        .get_bool_or_default("bot_support_button_always_on", true)
        .await;
    main_menu(lang, app_mode, always_support)
}

/// Первое число после `#` в тексте — id подписки из нашей же подсказки.
///
/// Язык подсказки на разбор не влияет: `#42` выглядит одинаково на всех языках,
/// поэтому парсер не привязан к английской фразе «Subscription #».
fn first_hash_id(text: &str) -> Option<i64> {
    let rest = text.split_once('#')?.1;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().ok()
}

/// Fulfil a Telegram Stars charge that belongs to a `payment_sessions` row
/// (invoice payload `sess:{uuid}`, produced by `StarsProvider`).
///
/// Stars have no HTTP webhook, so this handler is the Stars equivalent of
/// `MarketplaceService::handle_webhook` — including its amount gate: we compare
/// the Stars Telegram says were actually paid against the Stars the session was
/// priced at, using the SAME conversion the invoice was built with (so there is
/// no rounding drift), and refuse to fulfil an underpayment. A refusal leaves
/// the session `pending` (never mutates it) so the operator can inspect or
/// refund, exactly like the `CompletedWithAmount` gate for other providers.
///
/// On success we do NOT send our own confirmation: `fulfill_payment` already
/// DMs the user a localized "payment received" message with an Open-App button,
/// and duplicating it here would double-message every Mini App purchase.
async fn fulfill_stars_session_payment(
    bot: &Bot,
    chat_id: ChatId,
    state: &AppState,
    session_id: uuid::Uuid,
    paid_stars: i64,
    charge_id: &str,
) {
    let tg_id = chat_id.0;
    let lang = crate::bot::utils::lang_by_tg_id(state, tg_id).await;

    let session = match state
        .marketplace_service
        .session_repo
        .get_by_id(session_id)
        .await
    {
        Ok(Some(session)) => session,
        Ok(None) => {
            error!(
                "Stars payment for unknown session {} (charge {}, {} XTR, tg_id {})",
                session_id, charge_id, paid_stars, tg_id
            );
            alert_admins_stars(
                state,
                &format!(
                    "🚨 *Stars payment for an unknown session*\n\nSession: `{}`\nCharge: `{}`\nPaid: `{}` XTR\nUser: `{}`\n\nThe user was charged but no payment session exists — refund manually\\.",
                    session_id, charge_id, paid_stars, tg_id
                ),
            );
            let _ = bot.send_message(chat_id, t(lang, "pay.unmatched")).await;
            return;
        }
        Err(e) => {
            error!(
                "Failed to load payment session {} for Stars charge {}: {}",
                session_id, charge_id, e
            );
            let _ = bot
                .send_message(chat_id, t(lang, "pay.error_contact_support"))
                .await;
            return;
        }
    };

    // Amount gate — mirrors MarketplaceService's CompletedWithAmount check.
    // Exact-or-over is accepted; only genuine underpayment is refused.
    //
    // The expectation comes from the amount FROZEN into the session when the
    // invoice was created, so retuning `stars_per_usd` mid-flight cannot make an
    // honestly paid invoice look underpaid. The live rate is only the fallback
    // for sessions created before that field existed.
    let stars_rate = crate::services::payment::stars::stars_per_usd(&state.settings).await;
    let expected_stars =
        match crate::services::payment::stars::expected_session_star_amount(&session, stars_rate) {
            Ok(stars) => stars,
            Err(e) => {
                error!(
                    "Cannot price session {} in Stars (charge {}): {}",
                    session_id, charge_id, e
                );
                let _ = bot
                    .send_message(chat_id, t(lang, "pay.error_contact_support"))
                    .await;
                return;
            }
        };

    if paid_stars < expected_stars {
        error!(
            session_id = %session_id,
            charge_id,
            paid_stars,
            expected_stars,
            expected_amount = session.amount,
            expected_currency = %session.currency,
            "Rejecting Stars fulfillment: underpaid invoice (possible tampering)"
        );
        alert_admins_stars(
            state,
            &format!(
                "🚨 *Stars payment amount mismatch*\n\nSession: `{}`\nCharge: `{}`\nPaid: `{}` XTR\nExpected: `{}` XTR\nUser: `{}`\n\nFulfillment was refused and the session left pending — inspect or refund manually\\.",
                session_id, charge_id, paid_stars, expected_stars, tg_id
            ),
        );
        let _ = bot
            .send_message(chat_id, t(lang, "pay.amount_mismatch"))
            .await;
        return;
    }

    match state.marketplace_service.fulfill_payment(session_id).await {
        Ok(()) => {
            info!(
                "Fulfilled Stars payment: session {} ({} XTR, charge {}, tg_id {})",
                session_id, paid_stars, charge_id, tg_id
            );
            let _ = LoggingService::log_user(
                &state.pool,
                Some(tg_id),
                "payment_stars",
                &format!(
                    "Stars payment fulfilled: {} XTR, session {}",
                    paid_stars, session_id
                ),
                None,
            )
            .await;
        }
        Err(e) => {
            error!(
                "Stars fulfillment failed for session {} (charge {}): {}",
                session_id, charge_id, e
            );
            alert_admins_stars(
                state,
                &format!(
                    "🚨 *Stars fulfillment failed*\n\nSession: `{}`\nCharge: `{}`\nUser: `{}`\n\nThe user paid but the resource was not granted\\.",
                    session_id, charge_id, tg_id
                ),
            );
            let _ = bot
                .send_message(chat_id, t(lang, "pay.error_contact_support"))
                .await;
        }
    }
}

/// Fire-and-forget admin alert; never blocks the payment handler.
fn alert_admins_stars(state: &AppState, message: &str) {
    let pool = state.pool.clone();
    let bot_manager = state.bot_manager.clone();
    let message = message.to_string();
    tokio::spawn(async move {
        bot_manager.notify_admins(&pool, &message).await;
    });
}

pub async fn message_handler(
    bot: Bot,
    msg: Message,
    state: AppState,
) -> Result<(), teloxide::RequestError> {
    info!("Received message: {:?}", msg.text());
    let tg_id = msg.chat.id.0;

    if let Some(payment) = msg.successful_payment() {
        let lang = crate::bot::utils::lang_by_tg_id(&state, tg_id).await;
        // Two payload dialects arrive here:
        //   * `sess:{uuid}`  — Mini App / marketplace checkout (StarsProvider).
        //                      Fulfilled through MarketplaceService so plan
        //                      DURATIONS, referral rewards, the atomic
        //                      pending→completed claim and the success DM all
        //                      behave exactly as for every other provider.
        //   * `{user}:bal|ord|sub:{id}` — legacy bot-native flows (balance
        //                      top-up keyboard, /buy). Unchanged.
        // The two can never be confused: the legacy form always starts with a
        // decimal user id, never with `sess:`.
        if let Some(session_id) =
            crate::services::payment::stars::parse_session_invoice_payload(&payment.invoice_payload)
        {
            fulfill_stars_session_payment(
                &bot,
                msg.chat.id,
                &state,
                session_id,
                payment.total_amount as i64,
                &payment.provider_payment_charge_id,
            )
            .await;
            return Ok(());
        }

        let amount_xtr = payment.total_amount as f64;
        // Same rate as the invoice side, via the shared helper (exact integer
        // cents rather than a float division by a hardcoded 50.0). Legacy
        // bot-native payloads carry no frozen amount, so the live setting is
        // the only rate available here.
        let stars_rate = crate::services::payment::stars::stars_per_usd(&state.settings).await;
        let amount_usd = crate::services::payment::stars::stars_to_usd_cents(
            payment.total_amount as i64,
            stars_rate,
        ) as f64
            / 100.0;
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
                let _ = LoggingService::log_user(
                    &state.pool,
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
                    .send_message(msg.chat.id, t(lang, "pay.topup_success"))
                    .await;
            }
            Err(e) => {
                error!("Stars payment processing failed: {}", e);
                let _ = bot
                    .send_message(msg.chat.id, t(lang, "pay.error_contact_support"))
                    .await;
            }
        }
        return Ok(());
    }

    if let Some(text) = msg.text() {
        // 1. Resolve User (Handle /start upsert or fetch existing)
        let user_res = if text.starts_with("/start") {
            let start_param = text.strip_prefix("/start ").unwrap_or("");
            let referrer_id: Option<i64> = if !start_param.is_empty() {
                state
                    .store_service
                    .resolve_referrer_id(start_param)
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };

            let user_name = msg
                .from
                .as_ref()
                .map(|u| u.full_name())
                .unwrap_or_else(|| "User".to_string());
            // Upsert returns (User, was_new). was_new gates first-touch signup
            // attribution side effects (partner code stamping) so they fire
            // exactly once, on genuine signup.
            let user_res_inner = state
                .store_service
                .upsert_user_with_new_flag(
                    tg_id,
                    msg.from.as_ref().and_then(|u| u.username.as_deref()),
                    Some(&user_name),
                    referrer_id,
                )
                .await;

            match user_res_inner {
                Ok((u, was_new)) => {
                    // Stamp the partner code this user signed up through, so
                    // per-code stats (signups/conversions/balance) are derivable.
                    // Only on genuine new signups, and only once (the deep-link
                    // start_param may be a partner code). resolve_partner_code_id
                    // also bumps the code's `clicks`, so call it once per signup.
                    if was_new
                        && !start_param.is_empty()
                        && let Ok(Some(code_id)) = state
                            .store_service
                            .resolve_partner_code_id(start_param)
                            .await
                    {
                        let _ = state
                            .store_service
                            .set_signup_partner_code(u.id, code_id)
                            .await;
                    }
                    // Log user /start command
                    let _ = LoggingService::log_user(
                        &state.pool,
                        Some(tg_id),
                        "bot_start",
                        &format!("User {} executed /start command", tg_id),
                        None,
                    )
                    .await;

                    // Notify referrer about new referral (only for genuinely new users)
                    if let Some(r_id) = referrer_id
                        && r_id != u.id
                    {
                        let referrer_tg_id: Option<i64> =
                            sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                                .bind(r_id)
                                .fetch_optional(&state.pool)
                                .await
                                .unwrap_or(None);
                        if let Some(ref_tg_id) = referrer_tg_id {
                            // Язык РЕФЕРРЕРА, а не того, кто только что нажал /start.
                            let ref_lang =
                                crate::bot::utils::lang_by_tg_id(&state, ref_tg_id).await;
                            let payload = crate::bot_manager::NotificationPayload::plain(t(
                                ref_lang,
                                "referral.new_referral_dm",
                            ));
                            let _ = state
                                .bot_manager
                                .send_rich_notification(ref_tg_id, payload)
                                .await;
                        }
                    }

                    Some(u)
                }
                Err(e) => {
                    error!("Failed to upsert user on /start: {:?}", e);
                    None
                }
            }
        } else {
            let user: Option<caramba_db::models::store::User> = state
                .store_service
                .get_user_by_tg_id(tg_id)
                .await
                .ok()
                .flatten();
            user
        };

        // Язык для всех ответов ниже: users.language_code → настройка
        // `default_language` → ru (см. bot::translations::resolve_lang).
        let lang = lang_for(
            &state.settings,
            user_res.as_ref().and_then(|u| u.language_code.as_deref()),
        )
        .await;

        // 2. State Machine Checks
        if let Some(user) = user_res {
            if user.is_banned {
                let _ = bot
                    .send_message(msg.chat.id, t(lang, "error.banned"))
                    .parse_mode(ParseMode::Html)
                    .await;
                return Ok(());
            }

            if user.language_code.is_none() {
                // НАМЕРЕННО двуязычно: язык ещё не выбран, и это единственное
                // сообщение, которое пользователь обязан понять на любом языке.
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
                            .send_message(msg.chat.id, t(lang, "error.banned_spam"))
                            .parse_mode(ParseMode::Html)
                            .await;
                        return Ok(());
                    }
                }
                // Текст соглашения задаёт оператор — переводить его мы не можем.
                // Отдаём БЕЗ экранирования, как и раньше: в текущих настройках
                // там может лежать размеченный HTML, и экранирование сломало бы
                // вёрстку у действующих операторов. Локализуем только обвязку.
                // (Обратная сторона: голый `&` или `<` в соглашении уронит
                //  отправку — это давняя особенность, не трогаем её здесь.)
                let terms_text: String = state
                    .store_service
                    .get_setting("terms_of_service")
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| t(lang, "terms.placeholder").to_string());

                let _ = bot
                    .send_message(
                        msg.chat.id,
                        format!(
                            "{}\n\n{}\n\n{}",
                            t(lang, "terms.title"),
                            terms_text,
                            t(lang, "terms.prompt")
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
            // --- Dissolving Effect: History Handled by register_bot_message ---
            // Old deletion logic removed to support "Keep 3" history.
            // ---------------------------------------------------------------------
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
                let welcome_text = tf(lang, "welcome.start", &[&escape_html(&user_name)]);
                let bot_for_task = bot.clone();
                let state_for_task = state.clone();
                let bot_buttons_mode = state
                    .settings
                    .get_or_default("bot_buttons_mode", "full")
                    .await;
                let app_mode = bot_buttons_mode == "app_only";
                let always_support = state
                    .settings
                    .get_bool_or_default("bot_support_button_always_on", true)
                    .await;

                let _ = bot
                    .send_message(msg.chat.id, welcome_text)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(main_menu(lang, app_mode, always_support))
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
                            text: t(lang, "menu.launch_app").to_string(),
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
        //
        // Ответы на наши подсказки распознаются по КОРОТКОМУ МАРКЕРУ, который мы
        // сами вставили в текст подсказки, и проверяются на обоих языках: юзер
        // мог получить подсказку по-русски, переключить язык и ответить.
        // Маркеры не содержат разметки, поэтому совпадают с тем, что Telegram
        // реально отрисовал в чате.
        if let Some(reply) = msg.reply_to_message()
            && let Some(reply_text) = reply.text()
        {
            info!("Processing reply to message with text: [{}]", reply_text);
            info!("User reply body: [{}]", text);

            // Transfer — проверяем ПЕРЕД заметкой: обе подсказки содержат "#id",
            // и общая проверка по '#' раньше перехватывала переносы.
            if contains_any_lang(reply_text, "transfer.marker") {
                if let Some(sub_id) = first_hash_id(reply_text) {
                    let user_db: Option<caramba_db::models::store::User> = state
                        .store_service
                        .get_user_by_tg_id(tg_id)
                        .await
                        .ok()
                        .flatten();
                    if let Some(u) = user_db {
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
                                            "transfer.ok",
                                            &[&sub_id.to_string(), &escape_html(text)],
                                        ),
                                    )
                                    .parse_mode(ParseMode::Html)
                                    .await;
                            }
                            Err(e) => {
                                let _ = bot
                                    .send_message(
                                        msg.chat.id,
                                        tf(
                                            lang,
                                            "transfer.failed",
                                            &[&escape_html(&e.to_string())],
                                        ),
                                    )
                                    .parse_mode(ParseMode::Html)
                                    .await;
                            }
                        }
                    }
                }
                return Ok(());
            }

            // Note Update
            if contains_any_lang(reply_text, "services.note_prompt_marker")
                && let Some(sub_id) = first_hash_id(reply_text)
            {
                let _ = state
                    .store_service
                    .update_subscription_note(sub_id, text.to_string())
                    .await;
                let _ = bot
                    .send_message(msg.chat.id, t(lang, "services.note_updated"))
                    .await;
                return Ok(());
            }

            // Gift / Promo Code
            if contains_any_lang(reply_text, "promo.redeem_marker") {
                let code = text.trim();
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    match state.promo_service.redeem_code(u.id, code).await {
                        Ok(res_msg) => {
                            let _ = bot
                                .send_message(
                                    msg.chat.id,
                                    tf(lang, "promo.redeem_ok", &[&escape_html(&res_msg)]),
                                )
                                .parse_mode(ParseMode::Html)
                                .await;
                        }
                        Err(e) => {
                            let _ = bot
                                .send_message(
                                    msg.chat.id,
                                    tf(
                                        lang,
                                        "promo.redeem_failed",
                                        &[&escape_html(&e.to_string())],
                                    ),
                                )
                                .parse_mode(ParseMode::Html)
                                .await;
                        }
                    }
                }
                return Ok(());
            }

            // Edit Referral Code Alias
            if contains_any_lang(reply_text, "referral.alias_marker") {
                let new_code = text.trim();

                // Basic validation
                if new_code.len() < 3 || new_code.len() > 32 {
                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "referral.alias_bad_length"))
                        .parse_mode(ParseMode::Html)
                        .await;
                    return Ok(());
                }

                if !new_code.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "referral.alias_bad_chars"))
                        .parse_mode(ParseMode::Html)
                        .await;
                    return Ok(());
                }

                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
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
                                "referral.alias_updated",
                                &[&escape_html(new_code), &escape_html(&new_link)],
                            );
                            if let Err(e) = bot
                                .send_message(msg.chat.id, response)
                                .parse_mode(ParseMode::Html)
                                .await
                            {
                                error!("Failed to send alias update confirmation: {}", e);
                            }
                        }
                        Err(_e) => {
                            let _ = bot
                                .send_message(msg.chat.id, t(lang, "referral.alias_update_failed"))
                                .parse_mode(ParseMode::Html)
                                .await;
                        }
                    }
                }
                return Ok(());
            }

            // Enter Referrer Code
            if contains_any_lang(reply_text, "referral.referrer_marker") {
                let ref_code = text.trim();
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    match state.store_service.set_user_referrer(u.id, ref_code).await {
                        Ok(_) => {
                            let _ = bot
                                .send_message(msg.chat.id, t(lang, "referral.referrer_linked"))
                                .parse_mode(ParseMode::Html)
                                .await;

                            // Notify referrer about new referral
                            let referrer_tg_id: Option<i64> = sqlx::query_scalar(
                                    "SELECT u2.tg_id FROM users u1 JOIN users u2 ON u2.id = u1.referrer_id WHERE u1.id = $1"
                                )
                                .bind(u.id)
                                .fetch_optional(&state.pool)
                                .await
                                .unwrap_or(None);
                            if let Some(ref_tg_id) = referrer_tg_id {
                                // Язык реферрера, а не текущего пользователя.
                                let ref_lang =
                                    crate::bot::utils::lang_by_tg_id(&state, ref_tg_id).await;
                                let payload = crate::bot_manager::NotificationPayload::plain(t(
                                    ref_lang,
                                    "referral.new_referral_dm",
                                ));
                                let _ = state
                                    .bot_manager
                                    .send_rich_notification(ref_tg_id, payload)
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .send_message(
                                    msg.chat.id,
                                    tf(
                                        lang,
                                        "referral.referrer_link_failed",
                                        &[&escape_html(&e.to_string())],
                                    ),
                                )
                                .parse_mode(ParseMode::Html)
                                .await;
                        }
                    }
                }
                return Ok(());
            }
        }

        // Admin Commands
        if text.starts_with("/admin") {
            // Verify Admin
            // Admins table stores usernames; resolve Telegram user by tg_id, then match by username.
            let is_admin: bool = sqlx::query_scalar(
                r#"
                SELECT EXISTS(
                    SELECT 1
                    FROM admins a
                    JOIN users u ON u.username = a.username
                    WHERE u.tg_id = $1
                )
                "#,
            )
            .bind(tg_id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(false);

            if !is_admin {
                // Silent ignore or "Unknown command"
                return Ok(());
            }

            if text.starts_with("/admin sni") {
                let parts: Vec<&str> = text.split_whitespace().collect();
                if parts.len() > 2 && parts[2] == "logs" {
                    // /admin sni logs
                    #[derive(sqlx::FromRow)]
                    struct SniLogView {
                        name: String,
                        old_sni: String,
                        new_sni: String,
                        reason: Option<String>,
                        rotated_at: chrono::DateTime<chrono::Utc>,
                    }

                    let logs: Vec<SniLogView> = sqlx::query_as(
                        "SELECT n.name, l.old_sni, l.new_sni, l.reason, l.rotated_at 
                          FROM sni_rotation_log l
                          JOIN nodes n ON l.node_id = n.id
                          ORDER BY l.rotated_at DESC LIMIT 10",
                    )
                    .fetch_all(&state.pool)
                    .await
                    .unwrap_or_default();

                    if logs.is_empty() {
                        let _ = bot
                            .send_message(msg.chat.id, "📜 No SNI rotations found.")
                            .await;
                    } else {
                        let mut response = "📜 <b>Recent SNI Rotations</b>\n\n".to_string();
                        for log in logs {
                            response.push_str(&format!(
                                "🔄 <b>{}</b> (Node: {})\n",
                                log.rotated_at.format("%Y-%m-%d %H:%M"),
                                log.name
                            ));
                            response.push_str(&format!("   {} → {}\n", log.old_sni, log.new_sni));
                            response.push_str(&format!(
                                "   Reason: {}\n\n",
                                log.reason.unwrap_or_else(|| "none".to_string())
                            ));
                        }
                        let _ = bot
                            .send_message(msg.chat.id, response)
                            .parse_mode(ParseMode::Html)
                            .await;
                    }
                } else {
                    // /admin sni (Status)
                    let pool: Vec<(String, i32, i32)> = sqlx::query_as("SELECT domain, tier, health_score FROM sni_pool ORDER BY tier ASC, health_score DESC")
                         .fetch_all(&state.pool)
                         .await
                         .unwrap_or_default();

                    let mut response = "📊 <b>SNI Pool Status</b>\n\n".to_string();
                    for (domain, tier, score) in pool {
                        let icon = if score > 80 {
                            "✅"
                        } else if score > 50 {
                            "⚠️"
                        } else {
                            "❌"
                        };
                        response.push_str(&format!(
                            "{} <b>{}</b> (T{}, Score: {})\n",
                            icon, domain, tier, score
                        ));
                    }

                    let _ = bot
                        .send_message(msg.chat.id, response)
                        .parse_mode(ParseMode::Html)
                        .await;
                }
                return Ok(());
            }
        }

        // Commands and Menus.
        // Подписи reply-кнопок приходят обычным текстом, поэтому сводим их к
        // MenuAction (распознаётся на обоих языках + старые английские подписи).
        let Some(action) = menu_action(text) else {
            // Неизвестный текст — молча игнорируем (как и раньше).
            return Ok(());
        };

        match action {
            MenuAction::Store => {
                let categories: Vec<caramba_db::models::store::StoreCategory> = state
                    .catalog_service
                    .get_categories()
                    .await
                    .unwrap_or_default();
                if categories.is_empty() {
                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "store.empty"))
                        .reply_markup(menu_markup(&state, lang).await)
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
                        t(lang, "cart.view"),
                        "view_cart",
                    )]);

                    let kb = InlineKeyboardMarkup::new(buttons);
                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "store.welcome"))
                        .parse_mode(ParseMode::Html)
                        .reply_markup(kb)
                        .await;
                }
            }

            MenuAction::Cart => {
                if let Ok(Some(user)) = state.store_service.get_user_by_tg_id(tg_id).await {
                    let cart_items: Vec<caramba_db::models::store::CartItem> = state
                        .store_service
                        .get_user_cart(user.id)
                        .await
                        .unwrap_or_default();

                    if cart_items.is_empty() {
                        let _ = bot.send_message(msg.chat.id, t(lang, "cart.empty")).await;
                    } else {
                        let mut total_price: i64 = 0;
                        let mut text = format!("{}\n\n", t(lang, "cart.title"));

                        for item in &cart_items {
                            text.push_str(&format!(
                                "• <b>{}</b> — ${}\n",
                                escape_html(&item.product_name),
                                money(item.price)
                            ));
                            total_price += item.price * item.quantity;
                        }

                        text.push('\n');
                        text.push_str(&tf(lang, "cart.total", &[&money(total_price)]));

                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                t(lang, "cart.checkout"),
                                "cart_checkout",
                            )],
                            vec![InlineKeyboardButton::callback(
                                t(lang, "cart.clear"),
                                "cart_clear",
                            )],
                        ];

                        let _ = bot
                            .send_message(msg.chat.id, text)
                            .parse_mode(ParseMode::Html)
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

            MenuAction::RedeemCode => {
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        tf(
                            lang,
                            "promo.redeem_prompt",
                            &[t(lang, "promo.redeem_marker")],
                        ),
                    )
                    .parse_mode(ParseMode::Html)
                    .reply_markup(ForceReply::new().selective())
                    .await;
            }

            MenuAction::BuyPlans => {
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                let plans: Vec<caramba_db::models::store::Plan> = state
                    .store_service
                    .get_active_plans()
                    .await
                    .unwrap_or_default();

                if plans.is_empty() {
                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "plans.none"))
                        .reply_markup(menu_markup(&state, lang).await)
                        .await;
                } else {
                    let (text, buttons) =
                        crate::bot::handlers::callback::render_plan_page(lang, &plans, 0);

                    let _ = bot
                        .send_message(msg.chat.id, text)
                        .parse_mode(ParseMode::Html)
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await
                        .map(move |m| {
                            if let Some(user) = user_db {
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

            MenuAction::Profile => {
                if let Ok(Some(user)) = state.store_service.get_user_by_tg_id(tg_id).await {
                    let response = tf(
                        lang,
                        "profile.card",
                        &[&user.tg_id.to_string(), &money(user.balance)],
                    );

                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        t(lang, "profile.topup_btn"),
                        "topup_menu",
                    )]];

                    let _ = bot
                        .send_message(msg.chat.id, response)
                        .parse_mode(ParseMode::Html)
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

            MenuAction::Services => {
                if let Ok(Some(user)) = state.store_service.get_user_by_tg_id(tg_id).await {
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
                        let response = format!(
                            "{}\n\n{}",
                            t(lang, "services.title"),
                            t(lang, "services.none")
                        );
                        let _ = bot
                            .send_message(msg.chat.id, response)
                            .parse_mode(ParseMode::Html)
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
                        let (response, buttons) =
                            crate::bot::handlers::callback::render_services_page(
                                lang,
                                &sorted_subs,
                                0,
                            );

                        let _ = bot
                            .send_message(msg.chat.id, response)
                            .parse_mode(ParseMode::Html)
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

            MenuAction::Referral => {
                if let Ok(Some(user)) = state.store_service.get_user_by_tg_id(tg_id).await {
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
                    let ref_earnings = state
                        .store_service
                        .get_user_referral_earnings(user.id)
                        .await
                        .unwrap_or(0);

                    let response = tf(
                        lang,
                        "referral.card",
                        &[
                            &ref_count.to_string(),
                            &money(ref_earnings),
                            &escape_html(&ref_code),
                            &escape_html(&ref_link),
                        ],
                    );

                    let mut buttons = vec![
                        vec![InlineKeyboardButton::callback(
                            t(lang, "promo.enter_code_btn"),
                            "enter_promo",
                        )],
                        vec![InlineKeyboardButton::callback(
                            t(lang, "referral.edit_alias_btn"),
                            "edit_ref_code",
                        )],
                    ];
                    if user.referrer_id.is_none() {
                        buttons.push(vec![InlineKeyboardButton::callback(
                            t(lang, "referral.enter_referrer_btn"),
                            "enter_referrer",
                        )]);
                    }

                    let _ = bot
                        .send_message(msg.chat.id, response)
                        .parse_mode(ParseMode::Html)
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

            MenuAction::Guides => {
                match crate::bot::keyboards::guides_keyboard(&state.settings, lang).await {
                    Some(kb) => {
                        let _ = bot
                            .send_message(msg.chat.id, t(lang, "guides.prompt"))
                            .reply_markup(kb)
                            .await;
                    }
                    None => {
                        let _ = bot
                            .send_message(msg.chat.id, t(lang, "guides.missing"))
                            .reply_markup(menu_markup(&state, lang).await)
                            .await;
                    }
                }
            }

            MenuAction::Support => {
                let support_username = state.settings.get_or_default("support_url", "").await;

                if support_username.is_empty() {
                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "support.not_configured"))
                        .reply_markup(menu_markup(&state, lang).await)
                        .await;
                } else {
                    // Sanitize username (remove @ if present)
                    let clean_username = support_username.trim_start_matches('@');
                    let url = format!("https://t.me/{}", clean_username);

                    let kb = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::url(
                        t(lang, "support.contact_btn"),
                        url.parse().unwrap(),
                    )]]);

                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "support.prompt"))
                        .reply_markup(kb)
                        .await;
                }
            }

            MenuAction::Devices => {
                if let Ok(Some(u)) = state.store_service.get_user_by_tg_id(tg_id).await
                    && let Ok(subs) = state.store_service.get_user_subscriptions(u.id).await
                {
                    let active_subs: Vec<caramba_db::models::store::SubscriptionWithDetails> = subs
                        .into_iter()
                        .filter(|s| s.sub.status == "active")
                        .collect();

                    if active_subs.is_empty() {
                        let _ = bot
                            .send_message(msg.chat.id, t(lang, "services.no_active"))
                            .reply_markup(menu_markup(&state, lang).await)
                            .await;
                    } else if active_subs.len() == 1 {
                        // Auto-show active devices for the only subscription
                        let sub = &active_subs[0];

                        let active_ips = state
                            .store_service
                            .get_subscription_active_ips(sub.sub.id)
                            .await
                            .unwrap_or_default();
                        let limit: i64 = state
                            .store_service
                            .get_subscription_device_limit(sub.sub.id)
                            .await
                            .unwrap_or(0)
                            .into();

                        let (text, buttons) = crate::bot::handlers::callback::render_devices_page(
                            lang,
                            sub.sub.id,
                            &active_ips,
                            limit,
                            None,
                        );

                        let _ = bot
                            .send_message(msg.chat.id, text)
                            .parse_mode(ParseMode::Html)
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
                        // Multiple subs, show selection
                        let mut buttons = Vec::new();
                        for sub in active_subs {
                            let label = format!("{} (#{})", sub.plan_name, sub.sub.id);
                            buttons.push(vec![InlineKeyboardButton::callback(
                                label,
                                format!("devices_{}", sub.sub.id),
                            )]);
                        }

                        let _ = bot
                            .send_message(msg.chat.id, t(lang, "devices.select_subscription"))
                            .parse_mode(ParseMode::Html)
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
                    }
                }
            }

            MenuAction::Leaderboard => {
                use crate::services::referral_service::ReferralService;

                let leaderboard = ReferralService::get_leaderboard(&state.pool, 10)
                    .await
                    .unwrap_or_default();

                if leaderboard.is_empty() {
                    let _ = bot
                        .send_message(msg.chat.id, t(lang, "leaderboard.empty"))
                        .parse_mode(ParseMode::Html)
                        .await;
                } else {
                    let mut text = format!("{}\n\n", t(lang, "leaderboard.header"));
                    for entry in leaderboard {
                        let medal = entry.medal.unwrap_or_else(|| "👤".to_string());
                        text.push_str(&format!(
                            "{} <b>{}</b> — {} {}\n",
                            medal,
                            escape_html(&entry.username),
                            entry.referral_count,
                            t(lang, "leaderboard.refs_suffix")
                        ));
                    }
                    text.push('\n');
                    text.push_str(t(lang, "leaderboard.footer"));

                    let _ = bot
                        .send_message(msg.chat.id, text)
                        .parse_mode(ParseMode::Html)
                        .await;
                }
            }

            MenuAction::Login => {
                // Одноразовый код для входа в standalone-приложение (Flutter + Go-ядро).
                send_login_code(&bot, &state, msg.chat.id, tg_id).await;
            }
        }
    }
    Ok::<_, teloxide::RequestError>(())
}

/// Генерирует одноразовый 6-значный код для входа в приложение и отправляет его
/// пользователю. Код кладётся в Redis по ключу "app:logincode:{code}" => tg_id
/// (TTL 300с, одноразовый). Перезаписывает предыдущий активный код этого юзера.
///
/// Общая логика для команды /login и инлайн-кнопки «Получить код для входа».
pub async fn send_login_code(bot: &Bot, state: &AppState, chat_id: ChatId, tg_id: i64) {
    use rand::Rng;

    let lang = crate::bot::utils::lang_by_tg_id(state, tg_id).await;

    // Перетираем предыдущий активный код пользователя, чтобы валидным был только
    // один. Ключ обратного индекса tg_id -> code хранит текущий код юзера.
    let user_index_key = format!("app:logincode:user:{}", tg_id);
    if let Ok(Some(prev_code)) = state.redis.get(&user_index_key).await {
        let _ = state
            .redis
            .del(&format!("app:logincode:{}", prev_code))
            .await;
    }

    // 6 цифр, ведущие нули допустимы (000000..=999999).
    let code: String = format!("{:06}", rand::rng().random_range(0..1_000_000u32));
    let code_key = format!("app:logincode:{}", code);

    // TTL 300с (5 минут), single-use. Значение — tg_id строкой.
    if let Err(e) = state.redis.set(&code_key, &tg_id.to_string(), 300).await {
        error!("Failed to store login code in Redis: {}", e);
        let _ = bot
            .send_message(chat_id, t(lang, "login.code_failed"))
            .await;
        return;
    }
    // Обратный индекс с тем же TTL — чтобы при следующем /login перетереть код.
    let _ = state.redis.set(&user_index_key, &code, 300).await;

    let text = tf(lang, "login.code", &[&code]);
    let _ = bot
        .send_message(chat_id, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(crate::bot::keyboards::login_code_keyboard(lang))
        .await
        .map_err(|e| error!("Failed to send login code: {}", e));
}

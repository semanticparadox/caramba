use crate::bot::handlers::admin::{
    handle_broadcast_confirm, send_admin_stats, send_ticket_detail, send_ticket_list,
    AdminFsmState,
};
use crate::bot::keyboards::admin::{
    broadcast_category_keyboard, broadcast_segment_keyboard, broadcast_severity_keyboard,
    ticket_status_keyboard,
};
use crate::bot::keyboards::make_amount_keyboard;
use crate::bot::keyboards::{main_menu, terms_keyboard};
use crate::bot::translations::{t, tf, tf_brand, DEFAULT_BRAND};
use crate::bot::utils::escape_md;
use crate::models::payment::PaymentType;
use crate::models::store::{DetailedSubscription, GiftCode, Plan, SubscriptionIpTracking, User};
use crate::AppState;
use reqwest::Url;
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, ChatId, ForceReply, InlineKeyboardButton, InlineKeyboardMarkup, LabeledPrice,
    ParseMode,
};
use tracing::{error, info};
// Distinct import for anyhow result
use anyhow::Result as AnyhowResult;

pub async fn callback_handler(
    bot: Bot,
    q: CallbackQuery,
    state: AppState,
) -> Result<(), teloxide::RequestError> {
    info!("Received callback: {:?}", q.data);
    let callback_id = q.id.clone();
    let user_tg = q.from;
    let tg_id = user_tg.id.0 as i64;

    if let Some(data) = q.data {
        match data.as_str() {
            "set_lang_en" | "set_lang_ru" => {
                let chosen_lang = if data.contains("en") { "en" } else { "ru" };
                let _ = bot.answer_callback_query(callback_id).await;

                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let _ = state.store_service.update_user_language(u.id, chosen_lang).await;
                    let lang = Some(chosen_lang);

                    let terms_text = state
                        .settings
                        .get_or_default("terms_of_service", "Terms of Service...")
                        .await;

                    if let Some(msg) = q.message {
                        let _ = bot.delete_message(msg.chat().id, msg.id()).await;

                        let _ = bot.send_message(msg.chat().id, format!("{}\n\n{}\n\n{}", t(lang, "msg.terms_header"), terms_text, t(lang, "msg.terms_accept_prompt")))
                            .parse_mode(ParseMode::Html)
                            .reply_markup(terms_keyboard(lang))
                            .await
                            .map_err(|e| error!("Failed to send terms after lang choice: {}", e));
                    }
                }
            }

            "accept_terms" => {
                let _ = bot.answer_callback_query(callback_id).await;
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let _ = state.store_service.update_user_terms(u.id).await;
                    let lang = u.language_code.as_deref();

                    // Автоматически выдаём бесплатную подписку при первом принятии условий.
                    // Если бесплатного плана нет в системе — просто пропускаем без ошибки.
                    match state.store_service.create_free_subscription(u.id).await {
                        Ok((sub_id, false)) => {
                            info!(
                                user_id = u.id,
                                subscription_id = sub_id,
                                "Бесплатная подписка создана для нового пользователя"
                            );
                        }
                        Ok((_, true)) => {
                            // У пользователя уже была бесплатная подписка — ничего не делаем
                        }
                        Err(e) => {
                            // Бесплатного плана нет или ошибка API — не блокируем регистрацию
                            error!("Не удалось создать бесплатную подписку для user {}: {}", u.id, e);
                        }
                    }

                    if let Some(msg) = q.message {
                        let _ = bot.delete_message(msg.chat().id, msg.id()).await;

                        let welcome_text = t(lang, "msg.welcome").to_string();
                        let _ = bot
                            .send_message(msg.chat().id, welcome_text)
                            .parse_mode(ParseMode::Html)
                            .reply_markup(main_menu(lang))
                            .await
                            .map(|m| {
                                let state = state.clone();
                                let uid = u.id;
                                tokio::spawn(async move {
                                    let _ = state
                                        .store_service
                                        .update_last_bot_msg_id(uid, m.id.0)
                                        .await;
                                });
                            })
                            .map_err(|e| error!("Failed to send welcome after terms: {}", e));
                    }
                }
            }

            "decline_terms" => {
                // Look up user lang for decline message
                let decline_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot
                    .answer_callback_query(callback_id)
                    .text(t(decline_lang.as_deref(), "msg.must_accept_terms"))
                    .show_alert(true)
                    .await;
            }

            extend if extend.starts_with("extend_sub_") => {
                let ext_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let text_plans: Vec<Plan> = state
                    .store_service
                    .get_active_plans()
                    .await
                    .unwrap_or_default();

                if text_plans.is_empty() {
                    let _ = bot
                        .answer_callback_query(callback_id)
                        .text(t(ext_lang.as_deref(), "msg.no_plans"))
                        .await;
                } else {
                    let _ = bot.answer_callback_query(callback_id).await;
                    let mut response = t(ext_lang.as_deref(), "msg.choose_plan_extend").to_string();
                    let mut buttons = Vec::new();

                    for plan in text_plans {
                        response.push_str(&format!(
                            "💎 *{}*\n_{}_\n\n",
                            escape_md(&plan.name),
                            escape_md(plan.description.as_deref().unwrap_or(t(ext_lang.as_deref(), "msg.premium_access")))
                        ));

                        let mut duration_row = Vec::new();
                        for dur in plan.durations {
                            let price_major = dur.price / 100;
                            let price_minor = dur.price % 100;
                            duration_row.push(InlineKeyboardButton::callback(
                                format!(
                                    "{}d - ${}.{:02}",
                                    dur.duration_days, price_major, price_minor
                                ),
                                format!("ext_dur_{}", dur.id),
                            ));
                        }
                        buttons.push(duration_row);
                    }

                    if let Some(msg) = q.message {
                        let _ = bot
                            .send_message(msg.chat().id, response)
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await;
                    }
                }
            }

            "enter_promo" => {
                let promo_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let _ = bot
                        .send_message(msg.chat().id, t(promo_lang.as_deref(), "msg.enter_gift_code"))
                        .reply_markup(ForceReply::new().selective())
                        .await;
                }
            }

            "topup_menu" => {
                let topup_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let response = t(topup_lang.as_deref(), "msg.topup_menu");
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        t(topup_lang.as_deref(), "kb.pay_cryptobot"),
                        "pay_cryptobot",
                    )],
                    vec![InlineKeyboardButton::callback(
                        t(topup_lang.as_deref(), "kb.pay_nowpayments"),
                        "pay_nowpayments",
                    )],
                    vec![InlineKeyboardButton::callback(
                        t(topup_lang.as_deref(), "kb.pay_crystal"),
                        "pay_crystal",
                    )],
                    vec![InlineKeyboardButton::callback(
                        t(topup_lang.as_deref(), "kb.pay_stripe"),
                        "pay_stripe",
                    )],
                    vec![InlineKeyboardButton::callback(
                        t(topup_lang.as_deref(), "kb.pay_stars"),
                        "pay_stars",
                    )],
                ];
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(msg.chat().id, msg.id(), response)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await;
                }
            }

            "pay_cryptobot" => {
                let pay_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let buttons = make_amount_keyboard("cb");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(
                            msg.chat().id,
                            msg.id(),
                            t(pay_lang.as_deref(), "msg.select_amount_cryptobot"),
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_nowpayments" => {
                let pay_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let buttons = make_amount_keyboard("np");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(
                            msg.chat().id,
                            msg.id(),
                            t(pay_lang.as_deref(), "msg.select_amount_nowpayments"),
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_crystal" => {
                let pay_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let buttons = make_amount_keyboard("cp");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(
                            msg.chat().id,
                            msg.id(),
                            t(pay_lang.as_deref(), "msg.select_amount_crystal"),
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_stripe" => {
                let pay_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let buttons = make_amount_keyboard("str");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(
                            msg.chat().id,
                            msg.id(),
                            t(pay_lang.as_deref(), "msg.select_amount_stripe"),
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_stars" => {
                let pay_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let buttons = make_amount_keyboard("star");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(msg.chat().id, msg.id(), t(pay_lang.as_deref(), "msg.select_amount_stars"))
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(buttons)
                        .await;
                }
            }

            cb if cb.starts_with("cb_") => {
                let amount = cb
                    .strip_prefix("cb_")
                    .unwrap_or("0")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                match user_res {
                    Ok(Some(u)) => {
                        let lang = u.language_code.as_deref();
                        match state
                            .pay_service
                            .create_cryptobot_invoice(u.id, amount, PaymentType::BalanceTopup)
                            .await
                        {
                            Ok(url) => {
                                match url.parse::<Url>() {
                                    Ok(parsed_url) => {
                                        let buttons = vec![vec![InlineKeyboardButton::url(
                                            t(lang, "msg.pay_cryptobot"),
                                            parsed_url,
                                        )]];
                                        let _ = bot.answer_callback_query(callback_id).await;
                                        if let Some(msg) = q.message {
                                            let _ = bot
                                                .send_message(
                                                    msg.chat().id,
                                                    tf(lang, "msg.invoice_created", &[&format!("{:.2}", amount)]),
                                                )
                                                .parse_mode(ParseMode::MarkdownV2)
                                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                                .await;
                                        }
                                    }
                                    Err(e) => {
                                        error!(url = %url, error = %e, "Cryptobot вернул невалидный URL для оплаты");
                                        let _ = bot
                                            .answer_callback_query(callback_id)
                                            .text(t(lang, "msg.payment_error"))
                                            .show_alert(true)
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!(tg_id, error = %e, "Ошибка создания инвойса CryptoBot");
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(lang, "msg.payment_error"))
                                    .show_alert(true)
                                    .await;
                            }
                        }
                    }
                    _ => {
                        // Пользователь не найден или панель недоступна — подтверждаем callback
                        let _ = bot
                            .answer_callback_query(callback_id)
                            .text(t(None, "msg.payment_error"))
                            .show_alert(true)
                            .await;
                    }
                }
            }
            np if np.starts_with("np_") => {
                let amount = np
                    .strip_prefix("np_")
                    .unwrap_or("0")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                match user_res {
                    Ok(Some(u)) => {
                        let lang = u.language_code.as_deref();
                        match state
                            .pay_service
                            .create_nowpayments_invoice(u.id, amount, PaymentType::BalanceTopup)
                            .await
                        {
                            Ok(url) => {
                                match url.parse::<Url>() {
                                    Ok(parsed_url) => {
                                        let buttons = vec![vec![InlineKeyboardButton::url(
                                            t(lang, "msg.pay_nowpayments"),
                                            parsed_url,
                                        )]];
                                        let _ = bot.answer_callback_query(callback_id).await;
                                        if let Some(msg) = q.message {
                                            let _ = bot
                                                .send_message(
                                                    msg.chat().id,
                                                    tf(lang, "msg.invoice_created", &[&format!("{:.2}", amount)]),
                                                )
                                                .parse_mode(ParseMode::MarkdownV2)
                                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                                .await;
                                        }
                                    }
                                    Err(e) => {
                                        error!(url = %url, error = %e, "NOWPayments вернул невалидный URL для оплаты");
                                        let _ = bot
                                            .answer_callback_query(callback_id)
                                            .text(t(lang, "msg.payment_error"))
                                            .show_alert(true)
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!(tg_id, error = %e, "Ошибка создания инвойса NOWPayments");
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(lang, "msg.payment_error"))
                                    .show_alert(true)
                                    .await;
                            }
                        }
                    }
                    _ => {
                        let _ = bot
                            .answer_callback_query(callback_id)
                            .text(t(None, "msg.payment_error"))
                            .show_alert(true)
                            .await;
                    }
                }
            }
            cp if cp.starts_with("cp_") => {
                let amount = cp
                    .strip_prefix("cp_")
                    .unwrap_or("0")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                match user_res {
                    Ok(Some(u)) => {
                        let lang = u.language_code.as_deref();
                        match state
                            .pay_service
                            .create_crystalpay_invoice(u.id, amount, PaymentType::BalanceTopup)
                            .await
                        {
                            Ok(url) => {
                                match url.parse::<Url>() {
                                    Ok(parsed_url) => {
                                        let buttons = vec![vec![InlineKeyboardButton::url(
                                            t(lang, "msg.pay_crystal"),
                                            parsed_url,
                                        )]];
                                        let _ = bot.answer_callback_query(callback_id).await;
                                        if let Some(msg) = q.message {
                                            let _ = bot
                                                .send_message(
                                                    msg.chat().id,
                                                    tf(lang, "msg.invoice_created", &[&format!("{:.2}", amount)]),
                                                )
                                                .parse_mode(ParseMode::MarkdownV2)
                                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                                .await;
                                        }
                                    }
                                    Err(e) => {
                                        error!(url = %url, error = %e, "CrystalPay вернул невалидный URL для оплаты");
                                        let _ = bot
                                            .answer_callback_query(callback_id)
                                            .text(t(lang, "msg.payment_error"))
                                            .show_alert(true)
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!(tg_id, error = %e, "Ошибка создания инвойса CrystalPay");
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(lang, "msg.payment_error"))
                                    .show_alert(true)
                                    .await;
                            }
                        }
                    }
                    _ => {
                        let _ = bot
                            .answer_callback_query(callback_id)
                            .text(t(None, "msg.payment_error"))
                            .show_alert(true)
                            .await;
                    }
                }
            }
            str_pay if str_pay.starts_with("str_") => {
                let amount = str_pay
                    .strip_prefix("str_")
                    .unwrap_or("0")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                match user_res {
                    Ok(Some(u)) => {
                        let lang = u.language_code.as_deref();
                        match state
                            .pay_service
                            .create_stripe_session(u.id, amount, PaymentType::BalanceTopup)
                            .await
                        {
                            Ok(url) => {
                                match url.parse::<Url>() {
                                    Ok(parsed_url) => {
                                        let buttons = vec![vec![InlineKeyboardButton::url(
                                            t(lang, "msg.pay_stripe"),
                                            parsed_url,
                                        )]];
                                        let _ = bot.answer_callback_query(callback_id).await;
                                        if let Some(msg) = q.message {
                                            let _ = bot
                                                .send_message(
                                                    msg.chat().id,
                                                    tf(lang, "msg.invoice_created", &[&format!("{:.2}", amount)]),
                                                )
                                                .parse_mode(ParseMode::MarkdownV2)
                                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                                .await;
                                        }
                                    }
                                    Err(e) => {
                                        error!(url = %url, error = %e, "Stripe вернул невалидный URL для оплаты");
                                        let _ = bot
                                            .answer_callback_query(callback_id)
                                            .text(t(lang, "msg.payment_error"))
                                            .show_alert(true)
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!(tg_id, error = %e, "Ошибка создания сессии Stripe");
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(lang, "msg.payment_error"))
                                    .show_alert(true)
                                    .await;
                            }
                        }
                    }
                    _ => {
                        let _ = bot
                            .answer_callback_query(callback_id)
                            .text(t(None, "msg.payment_error"))
                            .show_alert(true)
                            .await;
                    }
                }
            }

            star if star.starts_with("star_") => {
                let amount_usd = star
                    .strip_prefix("star_")
                    .unwrap_or("0")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let xtr_amount = (amount_usd * 50.0) as u32;

                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                match user_res {
                    Ok(Some(u)) => {
                        let lang = u.language_code.as_deref();
                        let payload = PaymentType::BalanceTopup.to_payload_string(u.id);
                        let prices = vec![LabeledPrice {
                            label: t(lang, "msg.balance_topup").to_string(),
                            amount: xtr_amount,
                        }];

                        if let Some(msg) = q.message {
                            let _ = bot.delete_message(msg.chat().id, msg.id()).await;
                            let _ = bot.answer_callback_query(callback_id).await;

                            let _ = bot
                                .send_invoice(
                                    msg.chat().id,
                                    t(lang, "msg.balance_topup"),
                                    tf(lang, "msg.topup_description", &[&format!("{}", amount_usd)]),
                                    payload,
                                    "XTR",
                                    prices,
                                )
                                .await;
                        } else {
                            let _ = bot.answer_callback_query(callback_id).await;
                        }
                    }
                    _ => {
                        let _ = bot
                            .answer_callback_query(callback_id)
                            .text(t(None, "msg.payment_error"))
                            .show_alert(true)
                            .await;
                    }
                }
            }

            get_links if get_links.starts_with("get_links_") => {
                let sub_id = get_links
                    .strip_prefix("get_links_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let links_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot
                    .answer_callback_query(callback_id.clone())
                    .text(t(links_lang.as_deref(), "msg.fetching_links"))
                    .await;

                let links_res: AnyhowResult<Vec<String>> =
                    state.store_service.get_subscription_links(sub_id).await;
                match links_res {
                    Ok(links) => {
                        if links.is_empty() {
                            let _ = bot
                                .send_message(ChatId(tg_id), t(links_lang.as_deref(), "msg.no_links"))
                                .await;
                        } else {
                            let mut response = t(links_lang.as_deref(), "msg.your_links").to_string();
                            for link in links {
                                response.push_str(&format!("`{}`\n\n", escape_md(&link)));
                            }
                            let _ = bot
                                .send_message(ChatId(tg_id), response)
                                .parse_mode(ParseMode::MarkdownV2)
                                .await;
                        }
                    }
                    Err(e) => {
                        error!("Links error: {}", e);
                        let _ = bot
                            .send_message(ChatId(tg_id), t(links_lang.as_deref(), "msg.links_error"))
                            .await;
                    }
                }
            }

            get_config if get_config.starts_with("get_config_") => {
                // sub_id используется для генерации профиля конкретной подписки.
                // Ранее sub_id игнорировался (баг: всегда генерировался профиль пользователя).
                let sub_id = get_config
                    .strip_prefix("get_config_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);

                let cfg_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot
                    .answer_callback_query(callback_id)
                    .text(t(cfg_lang.as_deref(), "msg.generating_profile"))
                    .await;

                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let lang = u.language_code.as_deref();
                    // Генерируем профиль для конкретной подписки (sub_id).
                    // Если sub_id == 0 или подписка не найдена — панель вернёт ошибку.
                    match state.store_service.generate_subscription_file_for_sub(u.id, sub_id).await {
                        Ok(json_content) => {
                            let data: Vec<u8> = json_content.into_bytes();
                            let input_file = teloxide::types::InputFile::memory(data)
                                .file_name("caramba_v2_profile.json");

                            if let Some(msg) = q.message {
                                let raw_brand = state
                                    .settings
                                    .get_or_default("brand_name", DEFAULT_BRAND)
                                    .await;
                                // get_or_default only falls back when the key is
                                // absent; a stored empty/whitespace brand_name
                                // (e.g. set via the panel write path) would
                                // otherwise substitute a blank {brand}. Trim and
                                // fall back to the platform default here too.
                                let trimmed = raw_brand.trim();
                                let brand = if trimmed.is_empty() {
                                    DEFAULT_BRAND
                                } else {
                                    trimmed
                                };
                                let _ = bot.send_document(msg.chat().id, input_file)
                                    .caption(tf_brand(lang, "msg.your_profile_file", brand, &[]))
                                    .parse_mode(ParseMode::Html)
                                    .await;
                            }
                        }
                        Err(e) => {
                            error!(tg_id, sub_id, error = %e, "Ошибка генерации профиля подписки");
                            let _ = bot
                                .send_message(ChatId(tg_id), t(lang, "msg.profile_error"))
                                .await;
                        }
                    }
                }
            }

            activate if activate.starts_with("activate_") => {
                let sub_id = activate
                    .strip_prefix("activate_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);

                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let lang = u.language_code.as_deref();
                    match state
                        .store_service
                        .activate_subscription(sub_id, u.id)
                        .await
                    {
                        Ok(sub) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.activated"))
                                .await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        tf(lang, "msg.sub_activated", &[&sub.expires_at.format("%Y-%m-%d").to_string()]),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                        }
                        Err(e) => {
                            error!(tg_id, error = %e, "Ошибка активации подписки");
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.payment_error"))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            "my_gifts" => {
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let lang = u.language_code.as_deref();
                    let _ = bot.answer_callback_query(callback_id).await;
                    let codes_res: AnyhowResult<Vec<GiftCode>> =
                        state.store_service.get_user_gift_codes(u.id).await;
                    match codes_res {
                        Ok(codes) => {
                            if codes.is_empty() {
                                if let Some(msg) = q.message {
                                    let _ = bot
                                        .send_message(
                                            msg.chat().id,
                                            t(lang, "msg.no_gift_codes"),
                                        )
                                        .await;
                                }
                            } else {
                                let mut response =
                                    t(lang, "msg.my_gift_codes_header").to_string();
                                for code in codes {
                                    response.push_str(&format!(
                                        "🎟 `{}`\n   Days: {}\n\n",
                                        code.code,
                                        code.duration_days.unwrap_or(0)
                                    ));
                                }
                                if let Some(msg) = q.message {
                                    let _ = bot
                                        .send_message(msg.chat().id, response)
                                        .parse_mode(ParseMode::MarkdownV2)
                                        .await;
                                }
                            }
                        }
                        Err(_e) => {
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, t(lang, "msg.gift_error"))
                                    .await;
                            }
                        }
                    }
                }
            }

            edit_note if edit_note.starts_with("edit_note_") => {
                let sub_id = edit_note.strip_prefix("edit_note_").unwrap_or("0");
                let note_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let _ = bot
                        .send_message(
                            msg.chat().id,
                            tf(note_lang.as_deref(), "msg.reply_with_note", &[sub_id]),
                        )
                        .reply_markup(ForceReply::new().selective())
                        .await;
                }
            }

            devices if devices.starts_with("devices_") => {
                let sub_id = devices
                    .strip_prefix("devices_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let dev_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot.answer_callback_query(callback_id).await;

                let limit_res: AnyhowResult<i64> = state
                    .store_service
                    .get_subscription_device_limit(sub_id)
                    .await;
                let limit = limit_res.unwrap_or(3);
                let ips_res: AnyhowResult<Vec<SubscriptionIpTracking>> = state
                    .store_service
                    .get_subscription_active_ips(sub_id)
                    .await;
                let ips = ips_res.unwrap_or_default();

                let mut response = t(dev_lang.as_deref(), "msg.devices_header").to_string();
                response.push_str(&tf(dev_lang.as_deref(), "msg.device_limit", &[&limit.to_string()]));
                response.push_str(&tf(dev_lang.as_deref(), "msg.active_devices", &[&ips.len().to_string()]));

                if ips.is_empty() {
                    response.push_str(t(dev_lang.as_deref(), "msg.no_devices"));
                } else {
                    response.push_str(t(dev_lang.as_deref(), "msg.recent_connections"));
                    for (idx, ip_record) in ips.iter().take(10).enumerate() {
                        let duration =
                            chrono::Utc::now().signed_duration_since(ip_record.last_seen_at);
                        let minutes_ago = duration.num_minutes();
                        response.push_str(&format!(
                            "{}\\. `{}` _{} {}_\\n",
                            idx + 1,
                            escape_md(&ip_record.client_ip),
                            minutes_ago,
                            t(dev_lang.as_deref(), "msg.min_ago")
                        ));
                    }
                }

                let keyboard =
                    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                        t(dev_lang.as_deref(), "kb.back_services"),
                        format!("myservices_page_0"),
                    )]]);

                if let Some(msg) = q.message {
                    let _ = bot
                        .send_message(msg.chat().id, response)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(keyboard)
                        .await;
                }
            }

            buy_plan_idx if buy_plan_idx.starts_with("buy_plan_idx_") => {
                let bp_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let index = buy_plan_idx
                    .strip_prefix("buy_plan_idx_")
                    .unwrap_or("0")
                    .parse::<usize>()
                    .unwrap_or(0);
                let plans: Vec<Plan> = state
                    .store_service
                    .get_active_plans()
                    .await
                    .unwrap_or_default();

                if !plans.is_empty() {
                    let total_plans = plans.len();
                    let index = if index >= total_plans { 0 } else { index };
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
                            format!("🚀 {} - ${}.{:02}", t(bp_lang.as_deref(), "msg.traffic_plan"), price_major, price_minor)
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

                    if let Some(msg) = q.message {
                        let _ = bot
                            .edit_message_text(msg.chat().id, msg.id(), text)
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await;
                    }
                } else {
                    let _ = bot
                        .answer_callback_query(callback_id)
                        .text(t(bp_lang.as_deref(), "msg.no_plans_short"))
                        .await;
                }
            }

            myservices_page if myservices_page.starts_with("myservices_page_") => {
                let page = myservices_page
                    .strip_prefix("myservices_page_")
                    .unwrap_or("0")
                    .parse::<usize>()
                    .unwrap_or(0);

                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(user)) = user_res {
                    let lang = user.language_code.as_deref();
                    let subs_res: AnyhowResult<Vec<DetailedSubscription>> =
                        state.store_service.get_user_subscriptions(user.id).await;
                    if let Ok(subs) = subs_res {
                        // Sort subs
                        let mut sorted_subs: Vec<DetailedSubscription> = subs.clone();
                        sorted_subs.sort_by(|a, b| {
                            match (a.sub.status.as_str(), b.sub.status.as_str()) {
                                ("pending", "active") => std::cmp::Ordering::Less,
                                ("active", "pending") => std::cmp::Ordering::Greater,
                                _ => b.sub.created_at.cmp(&a.sub.created_at),
                            }
                        });

                        if !sorted_subs.is_empty() {
                            let total_pages = sorted_subs.len();
                            let page = if page >= total_pages { 0 } else { page };
                            let sub = &sorted_subs[page];

                            let mut response = t(lang, "msg.my_services").to_string();
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
                                t(lang, "msg.plan"), escape_md(&sub.plan_name)
                            ));
                            if let Some(desc) = &sub.plan_description {
                                response.push_str(&format!("   _{}_\n", escape_md(desc)));
                            }
                            response.push_str(&format!(
                                "   🔑 *Status:* {} `{}`\n",
                                status_icon, sub.sub.status
                            ));

                            let used_gb = sub.sub.used_traffic as f64 / 1024.0 / 1024.0 / 1024.0;
                            if let Some(limit) = sub.traffic_limit_gb {
                                if limit == 0 {
                                    response.push_str(&format!(
                                        "   📊 *{}* `{:.2} GB / ∞`\n",
                                        t(lang, "msg.traffic"), used_gb
                                    ));
                                } else {
                                    response.push_str(&format!(
                                        "   📊 *{}* `{:.2} GB / {} GB`\n",
                                        t(lang, "msg.traffic"), used_gb, limit
                                    ));
                                }
                            } else {
                                response.push_str(&format!(
                                    "   📊 *{}* `{:.2} GB`\n",
                                    t(lang, "msg.traffic_used"), used_gb
                                ));
                            }

                            if sub.sub.status == "active" {
                                response.push_str(&format!(
                                    "   ⌛ *{}* `{}`\n",
                                    t(lang, "msg.expires"), sub.sub.expires_at.format("%Y-%m-%d")
                                ));
                            } else {
                                let duration = sub.sub.expires_at - sub.sub.created_at;
                                response.push_str(&format!(
                                    "   ⏱ *{}* `{} {}` \\({}\\)\n",
                                    t(lang, "msg.duration"), duration.num_days(), t(lang, "msg.days"), t(lang, "msg.starts_on_activation")
                                ));
                            }
                            response.push_str("\n");
                            if let Some(note) = &sub.sub.note {
                                response.push_str(&format!("📝 *Note:* {}\n\n", escape_md(note)));
                            }

                            let mut buttons = Vec::new();
                            buttons.push(vec![InlineKeyboardButton::callback(
                                t(lang, "kb.edit_note"),
                                format!("edit_note_{}", sub.sub.id),
                            )]);

                            if sub.sub.status == "active" {
                                buttons.push(vec![
                                    InlineKeyboardButton::callback(
                                        t(lang, "kb.get_links"),
                                        format!("get_links_{}", sub.sub.id),
                                    ),
                                    InlineKeyboardButton::callback(
                                        t(lang, "kb.json_profile"),
                                        format!("get_config_{}", sub.sub.id),
                                    ),
                                    InlineKeyboardButton::callback(
                                        t(lang, "kb.extend"),
                                        format!("extend_sub_{}", sub.sub.id),
                                    ),
                                ]);
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    t(lang, "kb.devices_short"),
                                    format!("devices_{}", sub.sub.id),
                                )]);
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
                            buttons.push(vec![InlineKeyboardButton::callback(
                                t(lang, "kb.my_gifts"),
                                "my_gifts",
                            )]);

                            if let Some(msg) = q.message {
                                let _ = bot
                                    .edit_message_text(msg.chat().id, msg.id(), response)
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
                let sub_id = gift
                    .strip_prefix("gift_init_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let lang = u.language_code.as_deref();
                    match state
                        .store_service
                        .convert_subscription_to_gift(sub_id, u.id)
                        .await
                    {
                        Ok(code) => {
                            let response = tf(lang, "msg.gift_created", &[&code]);
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, response)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.code_generated"))
                                .await;
                        }
                        Err(e) => {
                            error!(tg_id, error = %e, "Ошибка конвертации подписки в подарок");
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.payment_error"))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            transfer if transfer.starts_with("transfer_init_") => {
                let sub_id = transfer.strip_prefix("transfer_init_").unwrap_or("0");
                let tr_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let _ = bot.send_message(msg.chat().id, tf(tr_lang.as_deref(), "msg.transfer_prompt", &[sub_id])).reply_markup(ForceReply::new().selective()).await;
                }
            }

            buy_dur if buy_dur.starts_with("buy_dur_") => {
                // Показываем выбор: для себя или как подарок
                let id = buy_dur
                    .strip_prefix("buy_dur_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let lang = u.language_code.as_deref();
                    let _ = bot.answer_callback_query(callback_id).await;
                    if let Some(msg) = q.message {
                        let buttons = vec![vec![
                            InlineKeyboardButton::callback(
                                t(lang, "btn.buy_for_myself"),
                                format!("buy_self_{}", id),
                            ),
                            InlineKeyboardButton::callback(
                                t(lang, "btn.buy_as_gift"),
                                format!("buy_gift_{}", id),
                            ),
                        ]];
                        let _ = bot
                            .send_message(msg.chat().id, t(lang, "msg.buy_for_whom"))
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await;
                    }
                }
            }

            buy_self if buy_self.starts_with("buy_self_") => {
                // Покупка для себя — подписка сразу активна
                let id = buy_self
                    .strip_prefix("buy_self_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let lang = u.language_code.as_deref();
                    match state.store_service.purchase_plan(u.id, id, false).await {
                        Ok(resp) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.purchase_success_short"))
                                .await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, t(lang, "msg.purchase_success"))
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                            let _ = resp; // serde_json::Value, уже обработан панелью
                        }
                        Err(e) => {
                            error!(tg_id, error = %e, "Ошибка покупки тарифа для себя");
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.payment_error"))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            buy_gift if buy_gift.starts_with("buy_gift_") => {
                // Покупка как подарок — возвращается gift code
                let id = buy_gift
                    .strip_prefix("buy_gift_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let lang = u.language_code.as_deref();
                    match state.store_service.purchase_plan(u.id, id, true).await {
                        Ok(resp) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.code_generated"))
                                .await;
                            if let Some(msg) = q.message {
                                let code = resp
                                    .get("gift_code")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("—")
                                    .to_string();
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        tf(lang, "msg.gift_created", &[&code]),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                        }
                        Err(e) => {
                            error!(tg_id, error = %e, "Ошибка покупки подарочного кода");
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.payment_error"))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            ext_dur if ext_dur.starts_with("ext_dur_") => {
                let id = ext_dur
                    .strip_prefix("ext_dur_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let lang = u.language_code.as_deref();
                    match state.store_service.extend_subscription(u.id, id).await {
                        Ok(_) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.extended"))
                                .await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, t(lang, "msg.sub_extended"))
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                        }
                        Err(e) => {
                            error!(tg_id, error = %e, "Ошибка продления подписки");
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.payment_error"))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            scat if scat.starts_with("store_cat_") => {
                let cat_id = scat
                    .strip_prefix("store_cat_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let scat_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot.answer_callback_query(callback_id).await;

                match state.store_service.get_products_by_category(cat_id).await {
                    Ok(prods) => {
                        if prods.is_empty() {
                            let _ = bot
                                .send_message(ChatId(tg_id), t(scat_lang.as_deref(), "msg.no_products"))
                                .await;
                        } else {
                            let mut buttons = Vec::new();
                            let mut text = t(scat_lang.as_deref(), "msg.products").to_string();
                            for p in prods {
                                let major = p.price / 100;
                                let minor = p.price % 100;
                                text.push_str(&format!(
                                    "• *{}* - ${}.{:02}\n",
                                    escape_md(&p.name),
                                    major,
                                    minor
                                ));
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    tf(scat_lang.as_deref(), "msg.view_product", &[&p.name]),
                                    format!("view_prod_{}", p.id),
                                )]);
                            }
                            buttons.push(vec![InlineKeyboardButton::callback(
                                t(scat_lang.as_deref(), "kb.back_categories"),
                                "📦 Digital Store",
                            )]);

                            if let Some(msg) = q.message {
                                let _ = bot
                                    .edit_message_text(msg.chat().id, msg.id(), text)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        error!(tg_id, error = %e, "Ошибка загрузки товаров категории");
                        let _ = bot
                            .send_message(ChatId(tg_id), t(scat_lang.as_deref(), "msg.payment_error"))
                            .await;
                    }
                }
            }

            vprod if vprod.starts_with("view_prod_") => {
                let prod_id = vprod
                    .strip_prefix("view_prod_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let vp_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot.answer_callback_query(callback_id).await;

                let text = t(vp_lang.as_deref(), "msg.product_details");
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        t(vp_lang.as_deref(), "kb.buy_now"),
                        format!("buy_prod_{}", prod_id),
                    )],
                    vec![InlineKeyboardButton::callback(t(vp_lang.as_deref(), "kb.back"), "📦 Digital Store")],
                ];
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(msg.chat().id, msg.id(), text)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await;
                }
            }

            bprod if bprod.starts_with("buy_prod_") => {
                let prod_id = bprod
                    .strip_prefix("buy_prod_")
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let lang = u.language_code.as_deref();
                    match state
                        .store_service
                        .purchase_product_with_balance(u.id, prod_id)
                        .await
                    {
                        Ok(p) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.purchase_product_success"))
                                .await;
                            let content = p
                                .content
                                .unwrap_or_else(|| t(lang, "msg.no_content").to_string());
                            let text = format!(
                                "✅ *Success!* Purchased: *{}*\n\n📋 *Content:*\n`{}`",
                                escape_md(&p.name),
                                escape_md(&content)
                            );
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, text)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                        }
                        Err(e) => {
                            error!(tg_id, error = %e, "Ошибка покупки цифрового товара");
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "msg.payment_error"))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            // ------------------------------------------------------------------
            // "noop" / "ignore" — кнопки-заглушки (счётчик пагинации).
            // Обязательно подтверждаем, чтобы у пользователя не крутился спиннер.
            // ------------------------------------------------------------------
            "noop" | "ignore" => {
                let _ = bot.answer_callback_query(callback_id).await;
            }

            // ------------------------------------------------------------------
            // "📦 Digital Store" — возврат в корневое меню магазина.
            // Эта строка используется как callback_data в кнопках «Назад»
            // внутри магазина (store_cat_ и view_prod_).
            // ------------------------------------------------------------------
            "📦 Digital Store" => {
                let scat_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot.answer_callback_query(callback_id).await;

                let categories_res = state.store_service.get_categories().await;
                let categories = categories_res.unwrap_or_default();

                if categories.is_empty() {
                    if let Some(msg) = q.message {
                        let _ = bot
                            .edit_message_text(msg.chat().id, msg.id(), t(scat_lang.as_deref(), "msg.store_empty"))
                            .await;
                    }
                } else {
                    let mut buttons = Vec::new();
                    for cat in categories {
                        buttons.push(vec![InlineKeyboardButton::callback(
                            cat.name,
                            format!("store_cat_{}", cat.id),
                        )]);
                    }
                    buttons.push(vec![InlineKeyboardButton::callback(
                        t(scat_lang.as_deref(), "kb.view_cart"),
                        "view_cart",
                    )]);
                    if let Some(msg) = q.message {
                        let _ = bot
                            .edit_message_text(msg.chat().id, msg.id(), t(scat_lang.as_deref(), "msg.store_welcome"))
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await;
                    }
                }
            }

            // ------------------------------------------------------------------
            // view_cart — показываем содержимое корзины из inline-контекста
            // ------------------------------------------------------------------
            "view_cart" => {
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let lang = u.language_code.as_deref();
                    let _ = bot.answer_callback_query(callback_id).await;

                    let cart_items = state.store_service.get_user_cart(u.id).await.unwrap_or_default();
                    if cart_items.is_empty() {
                        if let Some(msg) = q.message {
                            let _ = bot
                                .edit_message_text(msg.chat().id, msg.id(), t(lang, "msg.cart_empty"))
                                .await;
                        }
                    } else {
                        let mut total_price: i64 = 0;
                        let mut text = t(lang, "msg.cart_header").to_string();
                        for item in &cart_items {
                            let price_major = item.price / 100;
                            let price_minor = item.price % 100;
                            text.push_str(&format!(
                                "• *{}* \\- ${}\\.{:02}\\n",
                                escape_md(&item.product_name),
                                price_major,
                                price_minor
                            ));
                            total_price += item.price * item.quantity;
                        }
                        let total_major = total_price / 100;
                        let total_minor = total_price % 100;
                        text.push_str(&format!("\\n💰 *TOTAL: ${}\\.{:02}*", total_major, total_minor));

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
                        if let Some(msg) = q.message {
                            let _ = bot
                                .edit_message_text(msg.chat().id, msg.id(), text)
                                .parse_mode(ParseMode::MarkdownV2)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await;
                        }
                    }
                } else {
                    let _ = bot.answer_callback_query(callback_id).await;
                }
            }

            // ------------------------------------------------------------------
            // cart_checkout — finalize cart through user balance via panel.
            // ------------------------------------------------------------------
            "cart_checkout" => {
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                let user = user_res.ok().flatten();
                let lang = user.as_ref().and_then(|u| u.language_code.clone());
                if let Some(u) = user {
                    match state.store_service.checkout_cart(u.id).await {
                        Ok(_) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang.as_deref(), "msg.payment_success"))
                                .show_alert(true)
                                .await;
                        }
                        Err(e) => {
                            tracing::warn!(user_id = u.id, error = %e, "checkout_cart failed");
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang.as_deref(), "msg.payment_error"))
                                .show_alert(true)
                                .await;
                        }
                    }
                } else {
                    let _ = bot.answer_callback_query(callback_id).await;
                }
            }

            // ------------------------------------------------------------------
            // cart_clear — empty user's cart via panel.
            // ------------------------------------------------------------------
            "cart_clear" => {
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                let user = user_res.ok().flatten();
                let lang = user.as_ref().and_then(|u| u.language_code.clone());
                if let Some(u) = user {
                    match state.store_service.clear_cart(u.id).await {
                        Ok(_) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang.as_deref(), "msg.cart_cleared"))
                                .await;
                        }
                        Err(e) => {
                            tracing::warn!(user_id = u.id, error = %e, "clear_cart failed");
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang.as_deref(), "msg.payment_error"))
                                .show_alert(true)
                                .await;
                        }
                    }
                } else {
                    let _ = bot.answer_callback_query(callback_id).await;
                }
            }

            // ------------------------------------------------------------------
            // edit_ref_code — запрашиваем новый алиас реферального кода
            // ------------------------------------------------------------------
            "edit_ref_code" => {
                let ref_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let _ = bot
                        .send_message(msg.chat().id, t(ref_lang.as_deref(), "msg.edit_ref_alias"))
                        .reply_markup(ForceReply::new().selective())
                        .await;
                }
            }

            // ------------------------------------------------------------------
            // enter_referrer — запрашиваем код реферера
            // ------------------------------------------------------------------
            "enter_referrer" => {
                let ref_lang = state.store_service.get_user_by_tg_id(tg_id).await
                    .ok().flatten().and_then(|u| u.language_code.clone());
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let _ = bot
                        .send_message(msg.chat().id, t(ref_lang.as_deref(), "msg.enter_referrer_code"))
                        .reply_markup(ForceReply::new().selective())
                        .await;
                }
            }

            // ------------------------------------------------------------------
            // kill_sessions_{sub_id} — disconnect all active devices on this subscription.
            // ------------------------------------------------------------------
            kill_sessions if kill_sessions.starts_with("kill_sessions_") => {
                let sub_id: i64 = kill_sessions
                    .strip_prefix("kill_sessions_")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                let user = user_res.ok().flatten();
                let ks_lang = user.as_ref().and_then(|u| u.language_code.clone());
                match (user, sub_id > 0) {
                    (Some(u), true) => {
                        match state.store_service.kill_subscription_sessions(sub_id, u.id).await {
                            Ok(_) => {
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(ks_lang.as_deref(), "msg.sessions_killed"))
                                    .show_alert(true)
                                    .await;
                            }
                            Err(e) => {
                                tracing::warn!(user_id = u.id, sub_id, error = %e, "kill_subscription_sessions failed");
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(ks_lang.as_deref(), "msg.payment_error"))
                                    .show_alert(true)
                                    .await;
                            }
                        }
                    }
                    _ => {
                        let _ = bot.answer_callback_query(callback_id).await;
                    }
                }
            }

            // ==================================================================
            // Административные callbacks (prefix "adm:")
            // Каждый handler проверяет is_admin перед выполнением действия.
            // ==================================================================
            adm if adm.starts_with("adm:") => {
                let _ = bot.answer_callback_query(callback_id.clone()).await;

                // Проверка прав — двойное ограждение (первое — на уровне команды /admin)
                if !state.admin_service.is_admin(tg_id).await {
                    let _ = bot
                        .answer_callback_query(callback_id)
                        .text(t(None, "admin.tickets.no_admin"))
                        .show_alert(false)
                        .await;
                    return Ok(());
                }

                let chat_id = q.message.as_ref().map(|m| m.chat().id).unwrap_or(ChatId(tg_id));
                let msg_id = q.message.as_ref().map(|m| m.id());

                match adm {
                    // --------------------------------------------------------
                    // Главное меню
                    // --------------------------------------------------------
                    "adm:menu" => {
                        let open_count = state
                            .admin_service
                            .list_tickets(Some("open"), 100, 0)
                            .await
                            .map(|v| v.len())
                            .unwrap_or(0);
                        let kb = crate::bot::keyboards::admin::admin_main_menu(open_count);
                        if let Some(mid) = msg_id {
                            let _ = bot
                                .edit_message_text(chat_id, mid, t(None, "admin.menu.title"))
                                .parse_mode(ParseMode::Html)
                                .reply_markup(kb)
                                .await;
                        } else {
                            let _ = bot
                                .send_message(chat_id, t(None, "admin.menu.title"))
                                .parse_mode(ParseMode::Html)
                                .reply_markup(kb)
                                .await;
                        }
                    }

                    // --------------------------------------------------------
                    // Статистика
                    // --------------------------------------------------------
                    "adm:stats" => {
                        let _ = send_admin_stats(&bot, chat_id, &state).await;
                    }

                    // --------------------------------------------------------
                    // Модерация (placeholder)
                    // --------------------------------------------------------
                    "adm:moderation" => {
                        let _ = bot
                            .answer_callback_query(callback_id)
                            .text("Скоро...")
                            .show_alert(false)
                            .await;
                    }

                    // --------------------------------------------------------
                    // Закрыть меню
                    // --------------------------------------------------------
                    "adm:close" => {
                        if let Some(mid) = msg_id {
                            let _ = bot.delete_message(chat_id, mid).await;
                        }
                    }

                    // --------------------------------------------------------
                    // Noop для счётчиков в пагинации
                    // --------------------------------------------------------
                    "adm:noop" => {}

                    // --------------------------------------------------------
                    // Список тикетов: adm:tickets:list:<page>:<filter>
                    // --------------------------------------------------------
                    tickets_list if tickets_list.starts_with("adm:tickets:list:") => {
                        let rest = tickets_list.strip_prefix("adm:tickets:list:").unwrap_or("0:open");
                        let mut parts = rest.splitn(2, ':');
                        let page: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                        let filter = parts.next().unwrap_or("open");
                        let _ = send_ticket_list(&bot, chat_id, msg_id, &state, page, filter).await;
                    }

                    // --------------------------------------------------------
                    // Детали тикета: adm:ticket:<id>
                    // --------------------------------------------------------
                    ticket_detail if ticket_detail.starts_with("adm:ticket:") => {
                        let ticket_id: i64 = ticket_detail
                            .strip_prefix("adm:ticket:")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        if ticket_id > 0 {
                            let _ = send_ticket_detail(&bot, chat_id, msg_id, &state, ticket_id).await;
                        }
                    }

                    // --------------------------------------------------------
                    // Начать ответ на тикет: adm:reply:<ticket_id>
                    // --------------------------------------------------------
                    reply if reply.starts_with("adm:reply:") => {
                        let ticket_id: i64 = reply
                            .strip_prefix("adm:reply:")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        if ticket_id > 0 {
                            // Устанавливаем FSM-состояние ожидания текста
                            state
                                .admin_fsm
                                .set(tg_id, AdminFsmState::ReplyTo { ticket_id })
                                .await;
                            let _ = bot
                                .send_message(chat_id, t(None, "admin.reply.prompt"))
                                .await;
                        }
                    }

                    // --------------------------------------------------------
                    // Назначить тикет на себя: adm:assign:<ticket_id>
                    // --------------------------------------------------------
                    assign if assign.starts_with("adm:assign:") => {
                        let ticket_id: i64 = assign
                            .strip_prefix("adm:assign:")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        if ticket_id > 0 {
                            match state.admin_service.assign_ticket(ticket_id, tg_id).await {
                                Ok(_) => {
                                    let _ = bot
                                        .send_message(chat_id, t(None, "admin.assign.success"))
                                        .await;
                                    let _ = send_ticket_detail(&bot, chat_id, msg_id, &state, ticket_id).await;
                                }
                                Err(e) => {
                                    error!(ticket_id, tg_id, error = %e, "Failed to assign ticket");
                                    let _ = bot
                                        .send_message(chat_id, "Не удалось назначить тикет.")
                                        .await;
                                }
                            }
                        }
                    }

                    // --------------------------------------------------------
                    // Меню смены статуса: adm:status:<ticket_id>
                    // --------------------------------------------------------
                    status_menu if status_menu.starts_with("adm:status:") => {
                        let ticket_id: i64 = status_menu
                            .strip_prefix("adm:status:")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        if ticket_id > 0 {
                            let kb = ticket_status_keyboard(ticket_id);
                            let text = format!("Выберите новый статус для тикета #{}:", ticket_id);
                            if let Some(mid) = msg_id {
                                let _ = bot
                                    .edit_message_text(chat_id, mid, text)
                                    .reply_markup(kb)
                                    .await;
                            } else {
                                let _ = bot
                                    .send_message(chat_id, text)
                                    .reply_markup(kb)
                                    .await;
                            }
                        }
                    }

                    // --------------------------------------------------------
                    // Установить статус: adm:setstatus:<ticket_id>:<status>
                    // --------------------------------------------------------
                    setstatus if setstatus.starts_with("adm:setstatus:") => {
                        let rest = setstatus.strip_prefix("adm:setstatus:").unwrap_or("");
                        let mut parts = rest.splitn(2, ':');
                        let ticket_id: i64 =
                            parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                        let new_status = parts.next().unwrap_or("");
                        if ticket_id > 0 && !new_status.is_empty() {
                            match state
                                .admin_service
                                .change_ticket_status(ticket_id, tg_id, new_status)
                                .await
                            {
                                Ok(_) => {
                                    let _ = bot
                                        .send_message(chat_id, t(None, "admin.status.changed"))
                                        .await;
                                    let _ = send_ticket_detail(&bot, chat_id, msg_id, &state, ticket_id).await;
                                }
                                Err(e) => {
                                    error!(ticket_id, new_status, tg_id, error = %e, "Failed to change ticket status");
                                    let _ = bot
                                        .send_message(chat_id, "Не удалось изменить статус.")
                                        .await;
                                }
                            }
                        }
                    }

                    // --------------------------------------------------------
                    // Broadcast: старт
                    // --------------------------------------------------------
                    "adm:bcast:start" => {
                        // Очищаем любой предыдущий черновик
                        state.admin_fsm.clear(tg_id).await;
                        let text = t(None, "admin.broadcast.step_segment");
                        let kb = broadcast_segment_keyboard();
                        if let Some(mid) = msg_id {
                            let _ = bot
                                .edit_message_text(chat_id, mid, text)
                                .reply_markup(kb)
                                .await;
                        } else {
                            let _ = bot
                                .send_message(chat_id, text)
                                .reply_markup(kb)
                                .await;
                        }
                    }

                    // --------------------------------------------------------
                    // Broadcast: выбор сегмента adm:bcast:seg:<segment>
                    // --------------------------------------------------------
                    seg if seg.starts_with("adm:bcast:seg:") => {
                        let segment = seg.strip_prefix("adm:bcast:seg:").unwrap_or("all");
                        // Сохраняем сегмент в FSM (временно в BcastAwaitTitle с пустыми полями)
                        // Будем строить состояние шаг за шагом через промежуточные варианты.
                        // На этом этапе сегмент известен, категория и тяжесть ещё нет —
                        // переходим к выбору категории через отдельный callback.
                        // Запоминаем сегмент в FSM как BcastAwaitTitle с пустыми category/severity
                        state
                            .admin_fsm
                            .set(
                                tg_id,
                                AdminFsmState::BcastAwaitTitle {
                                    segment: segment.to_string(),
                                    category: String::new(), // заполнится на шаге 2
                                    severity: String::new(), // заполнится на шаге 3
                                },
                            )
                            .await;
                        let text = t(None, "admin.broadcast.step_category");
                        let kb = broadcast_category_keyboard();
                        if let Some(mid) = msg_id {
                            let _ = bot
                                .edit_message_text(chat_id, mid, text)
                                .reply_markup(kb)
                                .await;
                        } else {
                            let _ = bot
                                .send_message(chat_id, text)
                                .reply_markup(kb)
                                .await;
                        }
                    }

                    // --------------------------------------------------------
                    // Broadcast: выбор категории adm:bcast:cat:<category>
                    // --------------------------------------------------------
                    cat if cat.starts_with("adm:bcast:cat:") => {
                        let category = cat.strip_prefix("adm:bcast:cat:").unwrap_or("other");
                        // Извлекаем текущее состояние, дополняем category
                        let current = state.admin_fsm.get(tg_id).await;
                        if let Some(AdminFsmState::BcastAwaitTitle { segment, .. }) = current {
                            state
                                .admin_fsm
                                .set(
                                    tg_id,
                                    AdminFsmState::BcastAwaitTitle {
                                        segment,
                                        category: category.to_string(),
                                        severity: String::new(),
                                    },
                                )
                                .await;
                            let text = t(None, "admin.broadcast.step_severity");
                            let kb = broadcast_severity_keyboard();
                            if let Some(mid) = msg_id {
                                let _ = bot
                                    .edit_message_text(chat_id, mid, text)
                                    .reply_markup(kb)
                                    .await;
                            } else {
                                let _ = bot
                                    .send_message(chat_id, text)
                                    .reply_markup(kb)
                                    .await;
                            }
                        } else {
                            // Состояние потеряно — начинаем заново
                            let _ = bot
                                .send_message(chat_id, "Состояние потеряно. Начните заново: /admin")
                                .await;
                        }
                    }

                    // --------------------------------------------------------
                    // Broadcast: выбор severity adm:bcast:sev:<severity>
                    // --------------------------------------------------------
                    sev if sev.starts_with("adm:bcast:sev:") => {
                        let severity = sev.strip_prefix("adm:bcast:sev:").unwrap_or("info");
                        let current = state.admin_fsm.get(tg_id).await;
                        if let Some(AdminFsmState::BcastAwaitTitle { segment, category, .. }) = current {
                            // Переходим к вводу заголовка (текстовый ввод)
                            state
                                .admin_fsm
                                .set(
                                    tg_id,
                                    AdminFsmState::BcastAwaitTitle {
                                        segment,
                                        category,
                                        severity: severity.to_string(),
                                    },
                                )
                                .await;
                            let _ = bot
                                .send_message(chat_id, t(None, "admin.broadcast.step_title"))
                                .await;
                        } else {
                            let _ = bot
                                .send_message(chat_id, "Состояние потеряно. Начните заново: /admin")
                                .await;
                        }
                    }

                    // --------------------------------------------------------
                    // Broadcast: подтверждение отправки
                    // --------------------------------------------------------
                    "adm:bcast:confirm" => {
                        let _ = handle_broadcast_confirm(&bot, chat_id, msg_id, tg_id, &state).await;
                    }

                    // --------------------------------------------------------
                    // Broadcast: отмена
                    // --------------------------------------------------------
                    "adm:bcast:cancel" => {
                        state.admin_fsm.clear(tg_id).await;
                        let text = t(None, "admin.broadcast.cancelled");
                        if let Some(mid) = msg_id {
                            let _ = bot
                                .edit_message_text(chat_id, mid, text)
                                .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                                    InlineKeyboardButton::callback("В меню", "adm:menu"),
                                ]]))
                                .await;
                        } else {
                            let _ = bot.send_message(chat_id, text).await;
                        }
                    }

                    // --------------------------------------------------------
                    // Бренд: меню
                    // --------------------------------------------------------
                    "adm:brand:menu" => {
                        // Возврат в меню бренда отменяет любой незавершённый ввод.
                        state.admin_fsm.clear(tg_id).await;
                        crate::bot::handlers::brand::send_brand_menu(
                            &bot, chat_id, msg_id, &state,
                        )
                        .await;
                    }

                    // --------------------------------------------------------
                    // Бренд: переключить enabled
                    // --------------------------------------------------------
                    "adm:brand:toggle" => {
                        crate::bot::handlers::brand::toggle_brand_enabled(
                            &bot, chat_id, msg_id, &state,
                        )
                        .await;
                    }

                    // --------------------------------------------------------
                    // Бренд: ввод значения поля adm:brand:set:<field>
                    // --------------------------------------------------------
                    set_field if set_field.starts_with("adm:brand:set:") => {
                        let field_id = set_field
                            .strip_prefix("adm:brand:set:")
                            .unwrap_or("");
                        if let Some(field) =
                            crate::bot::handlers::brand::BrandField::from_id(field_id)
                        {
                            crate::bot::handlers::brand::prompt_brand_field(
                                &bot, chat_id, tg_id, field, &state,
                            )
                            .await;
                        }
                    }

                    // --------------------------------------------------------
                    // Неизвестный adm: callback — игнорируем
                    // --------------------------------------------------------
                    _ => {
                        info!(adm, "Unknown admin callback");
                    }
                }
            }

            _ => {
                // Неизвестный callback — всё равно подтверждаем, чтобы спиннер исчез.
                let _ = bot.answer_callback_query(callback_id).await;
            }
        }
    }
    Ok(())
}

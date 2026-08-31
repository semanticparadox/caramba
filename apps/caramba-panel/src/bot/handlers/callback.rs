use crate::AppState;
use crate::bot::handlers::command::money;
use crate::bot::keyboards::{main_menu, terms_keyboard};
use crate::bot::translations::{Lang, t, tf};
use crate::bot::utils::escape_html;
use caramba_db::models::payment::PaymentType;
use caramba_db::models::store::{Plan, SubscriptionWithDetails};
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, ChatId, ForceReply, InlineKeyboardButton, InlineKeyboardMarkup, LabeledPrice,
    ParseMode,
};
use tracing::{error, info};

/// Карточка тарифа со стрелками навигации.
///
/// Общая для меню «Купить подписку» и для листания инлайн-кнопками — иначе
/// подписи кнопок и текст расходятся при каждой правке.
pub fn render_plan_page(
    lang: Lang,
    plans: &[Plan],
    index: usize,
) -> (String, Vec<Vec<InlineKeyboardButton>>) {
    let total_plans = plans.len();
    let index = if index >= total_plans { 0 } else { index };
    let plan = &plans[index];

    let mut text = tf(
        lang,
        "plans.header",
        &[
            &escape_html(&plan.name),
            &(index + 1).to_string(),
            &total_plans.to_string(),
        ],
    );
    text.push_str("\n\n");
    if let Some(desc) = &plan.description {
        text.push_str(&format!("<i>{}</i>\n", escape_html(desc)));
    }

    let mut buttons = Vec::new();

    let mut duration_row = Vec::new();
    for dur in &plan.durations {
        let price = money(dur.price);
        let label = if dur.duration_days == 0 {
            tf(lang, "plans.traffic_label", &[&price])
        } else {
            tf(
                lang,
                "plans.duration_label",
                &[&dur.duration_days.to_string(), &price],
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

    if total_plans > 1 {
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
        buttons.push(vec![
            InlineKeyboardButton::callback("⬅️", format!("buy_plan_idx_{}", prev_idx)),
            InlineKeyboardButton::callback(format!("{}/{}", index + 1, total_plans), "noop"),
            InlineKeyboardButton::callback("➡️", format!("buy_plan_idx_{}", next_idx)),
        ]);
    }

    (text, buttons)
}

/// Карточка одной подписки + навигация. Раньше этот блок был скопирован в
/// command.rs и callback.rs с разными наборами кнопок; теперь один рендер, и
/// набор кнопок одинаков независимо от того, как пользователь сюда попал.
pub fn render_services_page(
    lang: Lang,
    subs: &[SubscriptionWithDetails],
    page: usize,
) -> (String, Vec<Vec<InlineKeyboardButton>>) {
    let total_pages = subs.len();
    let page = if page >= total_pages { 0 } else { page };
    let sub = &subs[page];
    let is_active = sub.sub.status == "active";

    let mut response = format!("{}\n\n", t(lang, "services.title"));
    response.push_str(&tf(
        lang,
        "services.item_header",
        &[&(page + 1).to_string(), &total_pages.to_string()],
    ));
    response.push('\n');
    response.push_str(&format!(
        "   💎 <b>{}:</b> {}\n",
        t(lang, "services.plan"),
        escape_html(&sub.plan_name)
    ));
    if let Some(desc) = &sub.plan_description {
        response.push_str(&format!("   <i>{}</i>\n", escape_html(desc)));
    }
    let status_icon = if is_active { "✅" } else { "⏳" };
    let status_text = if is_active {
        t(lang, "services.status_active")
    } else {
        t(lang, "services.status_pending")
    };
    response.push_str(&format!(
        "   🔑 <b>{}:</b> {} {}\n",
        t(lang, "services.status"),
        status_icon,
        status_text
    ));

    // Traffic
    let used_gb = sub.sub.used_traffic as f64 / 1024.0 / 1024.0 / 1024.0;
    match sub.traffic_limit_gb {
        Some(0) => response.push_str(&format!(
            "   📊 <b>{}:</b> <code>{:.2} GB / ∞</code>\n",
            t(lang, "services.traffic"),
            used_gb
        )),
        Some(limit) => response.push_str(&format!(
            "   📊 <b>{}:</b> <code>{:.2} GB / {} GB</code>\n",
            t(lang, "services.traffic"),
            used_gb,
            limit
        )),
        None => response.push_str(&format!(
            "   📊 <b>{}:</b> <code>{:.2} GB</code>\n",
            t(lang, "services.traffic_used"),
            used_gb
        )),
    }

    let duration = sub.sub.expires_at - sub.sub.created_at;
    if is_active {
        if duration.num_days() == 0 {
            response.push_str(&format!(
                "   ⌛ <b>{}:</b> {}\n",
                t(lang, "services.expires"),
                t(lang, "services.no_expiry")
            ));
        } else {
            response.push_str(&format!(
                "   ⌛ <b>{}:</b> <code>{}</code>\n",
                t(lang, "services.expires"),
                sub.sub.expires_at.format("%Y-%m-%d")
            ));
        }
    } else if duration.num_days() == 0 {
        response.push_str(&format!(
            "   ⏱ <b>{}:</b> {}\n",
            t(lang, "services.duration"),
            t(lang, "services.no_expiry")
        ));
    } else {
        response.push_str(&format!(
            "   ⏱ <b>{}:</b> {}\n",
            t(lang, "services.duration"),
            tf(
                lang,
                "services.days_on_activation",
                &[&duration.num_days().to_string()]
            )
        ));
    }

    response.push('\n');
    if let Some(note) = &sub.sub.note {
        response.push_str(&format!(
            "📝 <b>{}:</b> {}\n\n",
            t(lang, "services.note"),
            escape_html(note)
        ));
    }

    let mut buttons = vec![vec![InlineKeyboardButton::callback(
        t(lang, "services.edit_note"),
        format!("edit_note_{}", sub.sub.id),
    )]];

    if is_active {
        buttons.push(vec![InlineKeyboardButton::callback(
            t(lang, "services.devices_btn"),
            format!("devices_{}", sub.sub.id),
        )]);
        buttons.push(vec![
            InlineKeyboardButton::callback(
                t(lang, "services.get_links"),
                format!("get_links_{}", sub.sub.id),
            ),
            InlineKeyboardButton::callback(
                t(lang, "services.json_profile"),
                format!("get_config_{}", sub.sub.id),
            ),
            InlineKeyboardButton::callback(
                t(lang, "services.extend"),
                format!("extend_sub_{}", sub.sub.id),
            ),
        ]);
    } else if sub.sub.status == "pending" {
        buttons.push(vec![
            InlineKeyboardButton::callback(
                t(lang, "services.activate"),
                format!("activate_{}", sub.sub.id),
            ),
            InlineKeyboardButton::callback(
                t(lang, "services.make_gift"),
                format!("gift_init_{}", sub.sub.id),
            ),
        ]);
    }

    if total_pages > 1 {
        let prev_page = if page > 0 { page - 1 } else { total_pages - 1 };
        let next_page = if page < total_pages - 1 { page + 1 } else { 0 };
        buttons.push(vec![
            InlineKeyboardButton::callback(
                t(lang, "services.prev"),
                format!("myservices_page_{}", prev_page),
            ),
            InlineKeyboardButton::callback(format!("{}/{}", page + 1, total_pages), "ignore"),
            InlineKeyboardButton::callback(
                t(lang, "services.next"),
                format!("myservices_page_{}", next_page),
            ),
        ]);
    }

    buttons.push(vec![InlineKeyboardButton::callback(
        t(lang, "services.my_gifts"),
        "my_gifts",
    )]);

    (response, buttons)
}

/// Список активных устройств подписки.
///
/// `back` — callback_data кнопки «назад» (None — кнопку не показывать; так
/// вызывается из команды /devices, где возвращаться некуда).
pub fn render_devices_page(
    lang: Lang,
    sub_id: i64,
    ips: &[caramba_db::models::store::SubscriptionIpTracking],
    limit: i64,
    back: Option<&str>,
) -> (String, Vec<Vec<InlineKeyboardButton>>) {
    let mut text = tf(lang, "devices.header", &[&sub_id.to_string()]);
    text.push('\n');
    text.push_str(&tf(
        lang,
        "devices.limit_line",
        &[
            &ips.len().to_string(),
            &if limit == 0 {
                "∞".to_string()
            } else {
                limit.to_string()
            },
        ],
    ));
    text.push_str("\n\n");

    if ips.is_empty() {
        text.push_str(t(lang, "devices.none_recent"));
    } else {
        for ip in ips {
            let mins = (chrono::Utc::now() - ip.last_seen_at).num_minutes();
            let ago = if mins < 1 {
                t(lang, "devices.just_now").to_string()
            } else if mins < 60 {
                tf(lang, "devices.mins_ago", &[&mins.to_string()])
            } else {
                tf(lang, "devices.hours_ago", &[&(mins / 60).to_string()])
            };
            text.push_str(&format!(
                "• <code>{}</code> <i>({})</i>\n",
                escape_html(&ip.client_ip),
                ago
            ));
        }
        if limit > 0 && ips.len() > limit as usize {
            text.push('\n');
            text.push_str(t(lang, "devices.over_limit"));
        }
    }

    let mut buttons = Vec::new();
    if !ips.is_empty() {
        buttons.push(vec![InlineKeyboardButton::callback(
            t(lang, "devices.reset"),
            format!("kill_sessions_{}", sub_id),
        )]);
    }
    if let Some(back) = back {
        buttons.push(vec![InlineKeyboardButton::callback(
            t(lang, "devices.back"),
            back.to_string(),
        )]);
    }

    (text, buttons)
}

pub async fn callback_handler(
    bot: Bot,
    q: CallbackQuery,
    state: AppState,
) -> Result<(), teloxide::RequestError> {
    info!("Received callback: {:?}", q.data);
    let callback_id = q.id.clone();
    let user_tg = q.from;
    let tg_id = user_tg.id.0 as i64;
    // Язык для всех ответов этого колбэка: users.language_code → настройка
    // `default_language` → ru.
    let lang = crate::bot::utils::lang_by_tg_id(&state, tg_id).await;

    if let Some(data) = q.data {
        match data.as_str() {
            "get_login_code" => {
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    crate::bot::handlers::command::send_login_code(
                        &bot,
                        &state,
                        msg.chat().id,
                        tg_id,
                    )
                    .await;
                }
            }

            "set_lang_en" | "set_lang_ru" => {
                // Выбор пользователя перекрывает всё, что мы разрешили выше.
                let chosen = if data.contains("en") {
                    Lang::En
                } else {
                    Lang::Ru
                };
                let _ = bot.answer_callback_query(callback_id).await;

                // Fetch user to get ID
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    let _ = state
                        .store_service
                        .update_user_language(u.id, chosen.as_str())
                        .await;

                    // Immediately show terms — уже на только что выбранном языке.
                    let terms_text = state
                        .store_service
                        .get_setting("terms_of_service")
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| t(chosen, "terms.placeholder").to_string());

                    // Delete prev message (lang selection) or edit it
                    if let Some(msg) = q.message {
                        let _ = bot.delete_message(msg.chat().id, msg.id()).await;

                        let _ = bot
                            .send_message(
                                msg.chat().id,
                                format!(
                                    "{}\n\n{}\n\n{}",
                                    t(chosen, "terms.title"),
                                    terms_text,
                                    t(chosen, "terms.prompt")
                                ),
                            )
                            .parse_mode(ParseMode::Html)
                            .reply_markup(terms_keyboard(chosen))
                            .await
                            .map_err(|e| error!("Failed to send terms after lang choice: {}", e));
                    }
                }
            }

            "accept_terms" => {
                let _ = bot.answer_callback_query(callback_id).await;
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    let _ = state.store_service.update_user_terms(u.id).await;

                    if let Some(msg) = q.message {
                        let _ = bot.delete_message(msg.chat().id, msg.id()).await;

                        let _ = bot
                            .send_message(msg.chat().id, t(lang, "welcome.after_terms"))
                            .parse_mode(ParseMode::Html)
                            .reply_markup(main_menu(
                                lang,
                                state
                                    .settings
                                    .get_or_default("bot_buttons_mode", "full")
                                    .await
                                    == "app_only",
                                state
                                    .settings
                                    .get_bool_or_default("bot_support_button_always_on", true)
                                    .await,
                            ))
                            .await
                            .map(|m| {
                                let state = state.clone();
                                let uid = u.id;
                                tokio::spawn(async move {
                                    let _ = state
                                        .store_service
                                        .update_last_bot_msg_id(uid, m.id.0.into())
                                        .await;
                                });
                            })
                            .map_err(|e| error!("Failed to send welcome after terms: {}", e));
                    }
                }
            }

            "decline_terms" => {
                let _ = bot
                    .answer_callback_query(callback_id)
                    .text(t(lang, "terms.must_accept"))
                    .show_alert(true)
                    .await;
                // Optional: Ban user or just ignore
            }

            extend if extend.starts_with("extend_sub_") => {
                // Redirect to plans menu
                let plans = state
                    .catalog_service
                    .get_active_plans()
                    .await
                    .unwrap_or_default();

                if plans.is_empty() {
                    let _ = bot
                        .answer_callback_query(callback_id)
                        .text(t(lang, "plans.none"))
                        .await;
                } else {
                    let _ = bot.answer_callback_query(callback_id).await;
                    let mut response = format!("{}\n\n", t(lang, "plans.extend_header"));
                    let mut buttons = Vec::new();

                    for plan in plans {
                        response.push_str(&format!(
                            "💎 <b>{}</b>\n<i>{}</i>\n\n",
                            escape_html(&plan.name),
                            escape_html(
                                plan.description
                                    .as_deref()
                                    .unwrap_or(t(lang, "plans.default_description"))
                            )
                        ));

                        let mut duration_row = Vec::new();
                        for dur in plan.durations {
                            duration_row.push(InlineKeyboardButton::callback(
                                tf(
                                    lang,
                                    "plans.duration_label",
                                    &[&dur.duration_days.to_string(), &money(dur.price)],
                                ),
                                format!("ext_dur_{}", dur.id),
                            ));
                        }
                        buttons.push(duration_row);
                    }

                    if let Some(msg) = q.message {
                        let _ = bot
                            .send_message(msg.chat().id, response)
                            .parse_mode(ParseMode::Html)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await;
                    }
                }
            }

            "enter_promo" => {
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let _ = bot
                        .send_message(
                            msg.chat().id,
                            tf(
                                lang,
                                "promo.redeem_prompt_short",
                                &[t(lang, "promo.redeem_marker")],
                            ),
                        )
                        .reply_markup(ForceReply::new().selective())
                        .await;
                }
            }

            "topup_menu" => {
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        t(lang, "topup.method_cryptobot"),
                        "pay_cryptobot",
                    )],
                    vec![InlineKeyboardButton::callback(
                        t(lang, "topup.method_nowpayments"),
                        "pay_nowpayments",
                    )],
                    vec![InlineKeyboardButton::callback(
                        t(lang, "topup.method_crystal"),
                        "pay_crystal",
                    )],
                    vec![InlineKeyboardButton::callback(
                        t(lang, "topup.method_stripe"),
                        "pay_stripe",
                    )],
                    vec![InlineKeyboardButton::callback(
                        t(lang, "topup.method_stars"),
                        "pay_stars",
                    )],
                ];
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(msg.chat().id, msg.id(), t(lang, "topup.choose_method"))
                        .parse_mode(ParseMode::Html)
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await;
                }
            }

            // Amount Selection Menus
            "pay_cryptobot" => {
                let buttons = make_amount_keyboard(lang, "cb");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(
                            msg.chat().id,
                            msg.id(),
                            t(lang, "topup.amount_cryptobot"),
                        )
                        .parse_mode(ParseMode::Html)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_nowpayments" => {
                let buttons = make_amount_keyboard(lang, "np");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(
                            msg.chat().id,
                            msg.id(),
                            t(lang, "topup.amount_nowpayments"),
                        )
                        .parse_mode(ParseMode::Html)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_crystal" => {
                let buttons = make_amount_keyboard(lang, "cp");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(msg.chat().id, msg.id(), t(lang, "topup.amount_crystal"))
                        .parse_mode(ParseMode::Html)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_stripe" => {
                let buttons = make_amount_keyboard(lang, "str");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(msg.chat().id, msg.id(), t(lang, "topup.amount_stripe"))
                        .parse_mode(ParseMode::Html)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_stars" => {
                let buttons = make_amount_keyboard(lang, "star");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(msg.chat().id, msg.id(), t(lang, "topup.amount_stars"))
                        .parse_mode(ParseMode::Html)
                        .reply_markup(buttons)
                        .await;
                }
            }

            // Handlers
            cb if cb.starts_with("cb_") => {
                let amount = cb
                    .strip_prefix("cb_")
                    .unwrap_or("0")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    match state
                        .pay_service
                        .create_cryptobot_invoice(u.id, amount, PaymentType::BalanceTopup)
                        .await
                    {
                        Ok(url) => {
                            let buttons = vec![vec![InlineKeyboardButton::url(
                                t(lang, "topup.pay_cryptobot"),
                                url.parse().unwrap(),
                            )]];
                            let _ = bot.answer_callback_query(callback_id).await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        tf(
                                            lang,
                                            "topup.invoice_created",
                                            &[&format!("{:.2}", amount)],
                                        ),
                                    )
                                    .parse_mode(ParseMode::Html)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }
            np if np.starts_with("np_") => {
                let amount = np
                    .strip_prefix("np_")
                    .unwrap_or("0")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    match state
                        .pay_service
                        .create_nowpayments_invoice(u.id, amount, PaymentType::BalanceTopup)
                        .await
                    {
                        Ok(url) => {
                            let buttons = vec![vec![InlineKeyboardButton::url(
                                t(lang, "topup.pay_nowpayments"),
                                url.parse().unwrap(),
                            )]];
                            let _ = bot.answer_callback_query(callback_id).await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        tf(
                                            lang,
                                            "topup.invoice_created",
                                            &[&format!("{:.2}", amount)],
                                        ),
                                    )
                                    .parse_mode(ParseMode::Html)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }
            cp if cp.starts_with("cp_") => {
                let amount = cp
                    .strip_prefix("cp_")
                    .unwrap_or("0")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    match state
                        .pay_service
                        .create_crystalpay_invoice(u.id, amount, PaymentType::BalanceTopup)
                        .await
                    {
                        Ok(url) => {
                            let buttons = vec![vec![InlineKeyboardButton::url(
                                t(lang, "topup.pay_crystal"),
                                url.parse().unwrap(),
                            )]];
                            let _ = bot.answer_callback_query(callback_id).await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        tf(
                                            lang,
                                            "topup.invoice_created",
                                            &[&format!("{:.2}", amount)],
                                        ),
                                    )
                                    .parse_mode(ParseMode::Html)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }
            str_pay if str_pay.starts_with("str_") => {
                let amount = str_pay
                    .strip_prefix("str_")
                    .unwrap_or("0")
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    match state
                        .pay_service
                        .create_stripe_session(u.id, amount, PaymentType::BalanceTopup)
                        .await
                    {
                        Ok(url) => {
                            let buttons = vec![vec![InlineKeyboardButton::url(
                                t(lang, "topup.pay_stripe"),
                                url.parse().unwrap(),
                            )]];
                            let _ = bot.answer_callback_query(callback_id).await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        tf(
                                            lang,
                                            "topup.invoice_created",
                                            &[&format!("{:.2}", amount)],
                                        ),
                                    )
                                    .parse_mode(ParseMode::Html)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            // Direct plan purchase via Telegram Stars.
            // Callback format: "stars_plan_{plan_id}_{duration_id}".
            // Unlike the balance-topup "star_" path below, this builds the invoice with
            // PaymentType::SubscriptionPurchase(plan_id) so the resulting payload is
            // "{user_id}:sub:{plan_id}". On successful_payment, process_any_payment routes
            // "sub" -> process_subscription_purchase, activating the plan directly instead
            // of merely crediting balance. The pre_checkout handler already validates "sub"
            // payloads (ownership / ban / amount), so no extra gating is needed here.
            stars_plan if stars_plan.starts_with("stars_plan_") => {
                // Parse the two trailing ids: plan_id then duration_id.
                let rest = stars_plan.strip_prefix("stars_plan_").unwrap_or("");
                let parts: Vec<&str> = rest.split('_').collect();
                if parts.len() != 2 {
                    let _ = bot
                        .answer_callback_query(callback_id)
                        .text(t(lang, "checkout.invalid_plan"))
                        .show_alert(true)
                        .await;
                    return Ok(());
                }
                let plan_id: i64 = parts[0].parse().unwrap_or(0);
                let duration_id: i64 = parts[1].parse().unwrap_or(0);

                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    // Price comes from the selected duration (minor units / cents).
                    let duration_opt = state
                        .catalog_service
                        .get_plan_duration_by_id(duration_id)
                        .await
                        .unwrap_or(None);
                    let duration = match duration_opt {
                        // Guard against a duration belonging to a different plan than the
                        // one encoded in the callback (stale/forged button).
                        Some(d) if d.plan_id == plan_id => d,
                        _ => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "checkout.invalid_duration"))
                                .show_alert(true)
                                .await;
                            return Ok(());
                        }
                    };

                    let amount_usd = duration.price as f64 / 100.0;
                    // Same XTR/USD rate as the balance-topup path and command.rs
                    // (successful_payment converts back via amount_xtr / 50.0).
                    // 50 XTR per $1 USD. ceil so we never undercharge by rounding.
                    let xtr_amount = (amount_usd * 50.0).ceil() as u32;
                    if xtr_amount == 0 {
                        let _ = bot
                            .answer_callback_query(callback_id)
                            .text(t(lang, "checkout.stars_unavailable"))
                            .show_alert(true)
                            .await;
                        return Ok(());
                    }

                    // Payload "{user_id}:sub:{plan_id}" -> routed to subscription purchase.
                    let payload =
                        PaymentType::SubscriptionPurchase(plan_id).to_payload_string(u.id);
                    let prices = vec![LabeledPrice {
                        label: t(lang, "checkout.stars_line_item").to_string(),
                        amount: xtr_amount,
                    }];

                    let _ = bot.answer_callback_query(callback_id).await;
                    if let Some(msg) = q.message {
                        // Delete menu message
                        let _ = bot.delete_message(msg.chat().id, msg.id()).await;

                        let _ = bot
                            .send_invoice(
                                msg.chat().id,
                                t(lang, "checkout.stars_invoice_title"),
                                tf(
                                    lang,
                                    "checkout.stars_invoice_desc",
                                    &[&format!("{:.2}", amount_usd)],
                                ),
                                payload,
                                "XTR",
                                prices,
                            )
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
                // 1 USD approx 50 XTR (Telegram Stars). Rate varies.
                // Official: 1 XTR ~ $0.02 USD (purchase cost for user usually higher).
                // Let's charge 50 XTR per $1 USD balance.
                let xtr_amount = (amount_usd * 50.0) as u32;

                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    let payload = PaymentType::BalanceTopup.to_payload_string(u.id);
                    let prices = vec![LabeledPrice {
                        label: t(lang, "topup.title").to_string(),
                        amount: xtr_amount,
                    }];

                    if let Some(msg) = q.message {
                        // Delete menu message
                        let _ = bot.delete_message(msg.chat().id, msg.id()).await;

                        let _ = bot
                            .send_invoice(
                                msg.chat().id,
                                t(lang, "topup.title"),
                                tf(lang, "topup.invoice_desc", &[&format!("{:.2}", amount_usd)]),
                                payload,
                                "XTR",
                                prices,
                            )
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
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(user_tg) = user_db {
                    // Fetch specific subscription
                    if let Ok(subs) = state.store_service.get_user_subscriptions(user_tg.id).await {
                        let sub_opt = subs.iter().find(|s| s.sub.id == sub_id);

                        if let Some(_sub) = sub_opt {
                            // Use the proper link generation service
                            match state.store_service.get_subscription_links(sub_id).await {
                                Ok(links) => {
                                    if links.is_empty() {
                                        let _ = bot
                                            .send_message(
                                                ChatId(user_tg.tg_id),
                                                t(lang, "services.links_none"),
                                            )
                                            .await;
                                    } else {
                                        let mut response =
                                            format!("{}\n\n", t(lang, "services.links_header"));

                                        // Add Subscription Page Link
                                        let sub_domain = state
                                            .settings
                                            .get_or_default("subscription_domain", "")
                                            .await;
                                        let base_domain = if !sub_domain.is_empty() {
                                            sub_domain
                                        } else {
                                            let panel = state
                                                .settings
                                                .get_or_default("panel_url", "")
                                                .await;
                                            if !panel.is_empty() {
                                                panel
                                            } else {
                                                // Try env var, otherwise localhost with a warning
                                                std::env::var("PANEL_URL")
                                                    .unwrap_or_else(|_| "localhost".to_string())
                                            }
                                        };

                                        let is_localhost = base_domain == "localhost";
                                        let base_url = if base_domain.starts_with("http") {
                                            base_domain
                                        } else {
                                            format!("https://{}", base_domain)
                                        };

                                        let sub_url = format!(
                                            "{}/sub/{}",
                                            base_url, _sub.sub.subscription_uuid
                                        );

                                        response.push_str(&format!(
                                            "{}\n<code>{}</code>\n",
                                            t(lang, "services.links_page"),
                                            escape_html(&sub_url)
                                        ));
                                        if is_localhost {
                                            // Только для админа: PANEL_URL не задан.
                                            // Намеренно английский — это не текст
                                            // для покупателя.
                                            response.push_str("⚠️ <i>Admin: set PANEL_URL or the subscription_domain setting.</i>\n\n");
                                        } else {
                                            response.push('\n');
                                        }

                                        for link in links {
                                            response.push_str(&format!(
                                                "<code>{}</code>\n\n",
                                                escape_html(&link)
                                            ));
                                        }
                                        let _ = bot
                                            .send_message(ChatId(user_tg.tg_id), response)
                                            .parse_mode(ParseMode::Html)
                                            .await;
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to get subscription links: {}", e);
                                    let _ = bot
                                        .send_message(
                                            ChatId(user_tg.tg_id),
                                            t(lang, "services.links_failed"),
                                        )
                                        .await;
                                }
                            }
                        } else {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "services.not_found"))
                                .await;
                        }
                    }
                }
            }

            get_config if get_config.starts_with("get_config_") => {
                let _sub_id = get_config.strip_prefix("get_config_").unwrap_or("0");
                let _ = bot
                    .answer_callback_query(callback_id)
                    .text(t(lang, "services.profile_generating"))
                    .await;

                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    match state.store_service.generate_subscription_file(u.id).await {
                        Ok(json_content) => {
                            let data = json_content.into_bytes();
                            let input_file = teloxide::types::InputFile::memory(data)
                                .file_name("caramba_v2_profile.json");

                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_document(msg.chat().id, input_file)
                                    .caption(t(lang, "services.profile_caption"))
                                    .parse_mode(ParseMode::Html)
                                    .await;
                            }
                        }
                        Err(e) => {
                            error!("Failed to generate config: {}", e);
                            let _ = bot
                                .send_message(ChatId(tg_id), t(lang, "services.profile_failed"))
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
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();

                if let Some(u) = user_db {
                    match state
                        .store_service
                        .activate_subscription(sub_id, u.id)
                        .await
                    {
                        Ok(sub) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "services.activated_toast"))
                                .await;

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
                                     WHERE pg.plan_id = $1 AND n.is_enabled = TRUE",
                                )
                                .bind(plan_id)
                                .fetch_all(&pool)
                                .await
                                .unwrap_or_default();

                                if node_ids.is_empty() {
                                    info!(
                                        "⚠️ No nodes found for plan {} — skipping auto-sync",
                                        plan_id
                                    );
                                } else {
                                    info!(
                                        "🔄 Auto-syncing {} nodes for activated plan {}",
                                        node_ids.len(),
                                        plan_id
                                    );
                                    for nid in node_ids {
                                        if let Err(e) = pubsub
                                            .publish(&format!("node_events:{}", nid), "update")
                                            .await
                                        {
                                            error!(
                                                "Failed to publish node update for {}: {}",
                                                nid, e
                                            );
                                        }
                                    }
                                }
                            });

                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        tf(
                                            lang,
                                            "services.activated",
                                            &[&sub.expires_at.format("%Y-%m-%d").to_string()],
                                        ),
                                    )
                                    .parse_mode(ParseMode::Html)
                                    .await;
                            }
                        }
                        Err(e) => {
                            error!("Activation failed: {}", e);
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            "my_gifts" => {
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(u) = user_db {
                    let _ = bot.answer_callback_query(callback_id).await;
                    match state.store_service.get_user_gift_codes(u.id).await {
                        Ok(codes) => {
                            if codes.is_empty() {
                                if let Some(msg) = q.message {
                                    let _ =
                                        bot.send_message(msg.chat().id, t(lang, "gift.none")).await;
                                }
                            } else {
                                let mut response = format!("{}\n\n", t(lang, "gift.list_header"));
                                for code in codes {
                                    response.push_str(&format!(
                                        "🎟 <code>{}</code>\n   {}: {}\n\n",
                                        escape_html(&code.code),
                                        t(lang, "gift.days"),
                                        code.duration_days.unwrap_or(0)
                                    ));
                                }
                                if let Some(msg) = q.message
                                    && let Err(e) = bot
                                        .send_message(msg.chat().id, response)
                                        .parse_mode(ParseMode::Html)
                                        .await
                                {
                                    error!("Failed to send gift codes: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Fetch gifts error: {}", e);
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, t(lang, "gift.fetch_failed"))
                                    .await;
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
                    let _ = bot
                        .send_message(
                            msg.chat().id,
                            tf(
                                lang,
                                "services.note_prompt",
                                &[t(lang, "services.note_prompt_marker"), sub_id],
                            ),
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
                let _ = bot.answer_callback_query(callback_id).await;

                if let Some(msg) = q.message {
                    // Владение проверяем до показа: иначе любой мог бы прочитать
                    // чужие адреса, подставив id в callback_data.
                    let owns = match state.store_service.get_user_by_tg_id(tg_id).await {
                        Ok(Some(u)) => state
                            .store_service
                            .get_user_subscriptions(u.id)
                            .await
                            .unwrap_or_default()
                            .iter()
                            .any(|s| s.sub.id == sub_id),
                        _ => false,
                    };

                    if !owns {
                        let _ = bot
                            .send_message(msg.chat().id, t(lang, "services.not_found"))
                            .await;
                        return Ok(());
                    }

                    let limit: i64 = state
                        .store_service
                        .get_subscription_device_limit(sub_id)
                        .await
                        .unwrap_or(0)
                        .into();
                    let active_ips = state
                        .store_service
                        .get_subscription_active_ips(sub_id)
                        .await
                        .unwrap_or_default();

                    let (text, buttons) = render_devices_page(
                        lang,
                        sub_id,
                        &active_ips,
                        limit,
                        Some("myservices_page_0"),
                    );

                    let _ = bot
                        .send_message(msg.chat().id, text)
                        .parse_mode(ParseMode::Html)
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await;
                }
            }

            buy_plan_idx if buy_plan_idx.starts_with("buy_plan_idx_") => {
                let index = buy_plan_idx
                    .strip_prefix("buy_plan_idx_")
                    .unwrap_or("0")
                    .parse::<usize>()
                    .unwrap_or(0);
                let plans = state
                    .catalog_service
                    .get_active_plans()
                    .await
                    .unwrap_or_default();

                if plans.is_empty() {
                    let _ = bot
                        .answer_callback_query(callback_id)
                        .text(t(lang, "plans.none"))
                        .await;
                } else {
                    let _ = bot.answer_callback_query(callback_id).await;
                    let (text, buttons) = render_plan_page(lang, &plans, index);

                    if let Some(msg) = q.message {
                        let _ = bot
                            .edit_message_text(msg.chat().id, msg.id(), text)
                            .parse_mode(ParseMode::Html)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await;
                    }
                }
            }

            myservices_page if myservices_page.starts_with("myservices_page_") => {
                let page = myservices_page
                    .strip_prefix("myservices_page_")
                    .unwrap_or("0")
                    .parse::<usize>()
                    .unwrap_or(0);
                let user_db = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();

                if let Some(user) = user_db
                    && let Ok(subs) = state.store_service.get_user_subscriptions(user.id).await
                {
                    // Sort subs (same logic as the menu handler)
                    let mut sorted_subs = subs.clone();
                    sorted_subs.sort_by(|a, b| {
                        match (a.sub.status.as_str(), b.sub.status.as_str()) {
                            ("pending", "active") => std::cmp::Ordering::Less,
                            ("active", "pending") => std::cmp::Ordering::Greater,
                            _ => b.sub.created_at.cmp(&a.sub.created_at),
                        }
                    });

                    if !sorted_subs.is_empty() {
                        let (response, buttons) = render_services_page(lang, &sorted_subs, page);

                        if let Some(msg) = q.message {
                            let _ = bot
                                .edit_message_text(msg.chat().id, msg.id(), response)
                                .parse_mode(ParseMode::Html)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await;
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
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();

                if let Some(u) = user_db {
                    match state
                        .store_service
                        .convert_subscription_to_gift(sub_id, u.id)
                        .await
                    {
                        Ok(code) => {
                            let response = tf(lang, "gift.created", &[&escape_html(&code)]);
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, response)
                                    .parse_mode(ParseMode::Html)
                                    .await;
                            }
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "gift.created_toast"))
                                .await;
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            transfer if transfer.starts_with("transfer_init_") => {
                let sub_id = transfer.strip_prefix("transfer_init_").unwrap_or("0");
                if let Some(msg) = q.message {
                    let _ = bot
                        .send_message(
                            msg.chat().id,
                            tf(
                                lang,
                                "transfer.prompt",
                                &[t(lang, "transfer.marker"), sub_id],
                            ),
                        )
                        .parse_mode(ParseMode::Html)
                        .reply_markup(ForceReply::new().selective())
                        .await;
                }
            }

            buy_dur if buy_dur.starts_with("buy_dur_") => {
                let id_str = buy_dur.strip_prefix("buy_dur_").unwrap();
                if let Ok(duration_id) = id_str.parse::<i64>() {
                    let user_db: Option<caramba_db::models::store::User> = state
                        .store_service
                        .get_user_by_tg_id(tg_id)
                        .await
                        .ok()
                        .flatten();
                    if let Some(u) = user_db {
                        let mut buttons: Vec<Vec<InlineKeyboardButton>> = Vec::new();

                        // Balance Payment Option
                        if u.balance > 0 {
                            buttons.push(vec![InlineKeyboardButton::callback(
                                tf(lang, "checkout.pay_balance", &[&money(u.balance)]),
                                format!("pay_dur_balance_{}", duration_id),
                            )]);
                        }

                        if state
                            .settings
                            .get_or_default("manual_enabled", "false")
                            .await
                            == "true"
                        {
                            buttons.push(vec![InlineKeyboardButton::callback(
                                t(lang, "checkout.pay_manual"),
                                format!("pay_dur_manual_{}", duration_id),
                            )]);
                        }

                        if state
                            .settings
                            .get_or_default("stars_enabled", "false")
                            .await
                            == "true"
                        {
                            buttons.push(vec![InlineKeyboardButton::callback(
                                t(lang, "checkout.pay_stars"),
                                format!("pay_dur_stars_{}", duration_id),
                            )]);
                        }

                        if !state
                            .settings
                            .get_or_default("cryptobot_token", "")
                            .await
                            .is_empty()
                        {
                            buttons.push(vec![InlineKeyboardButton::callback(
                                t(lang, "checkout.pay_cryptobot"),
                                format!("pay_dur_cryptobot_{}", duration_id),
                            )]);
                        }

                        if !state
                            .settings
                            .get_or_default("nowpayments_key", "")
                            .await
                            .is_empty()
                        {
                            buttons.push(vec![InlineKeyboardButton::callback(
                                t(lang, "checkout.pay_nowpayments"),
                                format!("pay_dur_nowpayments_{}", duration_id),
                            )]);
                        }

                        // Always add a Back button to prevent empty keyboards and allow navigation
                        buttons.push(vec![InlineKeyboardButton::callback(
                            t(lang, "checkout.back_to_plans"),
                            "buy_subscription",
                        )]);

                        let _ = bot
                            .answer_callback_query(callback_id)
                            .text(t(lang, "checkout.choose_method_toast"))
                            .await;

                        if let Some(msg) = q.message {
                            let _ = bot
                                .edit_message_text(
                                    msg.chat().id,
                                    msg.id(),
                                    t(lang, "checkout.choose_method"),
                                )
                                .parse_mode(ParseMode::Html)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await;
                        }
                    } else {
                        error!("User not found for purchase: {}", tg_id);
                    }
                }
            }

            // New Payment Dispatcher
            pay_dur if pay_dur.starts_with("pay_dur_") => {
                let parts: Vec<&str> = pay_dur
                    .strip_prefix("pay_dur_")
                    .unwrap_or("")
                    .split("_")
                    .collect();
                if parts.len() == 2 {
                    let provider = parts[0];
                    let duration_id: i64 = parts[1].parse().unwrap_or(0);

                    let user_db: Option<caramba_db::models::store::User> = state
                        .store_service
                        .get_user_by_tg_id(tg_id)
                        .await
                        .ok()
                        .flatten();
                    if let Some(u) = user_db {
                        // Fetch actual price and plan info
                        let duration_opt = state
                            .catalog_service
                            .get_plan_duration_by_id(duration_id)
                            .await
                            .unwrap_or(None);
                        let duration = match duration_opt {
                            Some(d) => d,
                            None => {
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(lang, "checkout.invalid_duration"))
                                    .show_alert(true)
                                    .await;
                                return Ok(());
                            }
                        };

                        // Telegram Stars is a native invoice (XTR), not a redirect URL, so it
                        // cannot go through marketplace_service.create_session (which yields a
                        // pay-URL). Build the Stars invoice directly with a "{user_id}:sub:{plan}"
                        // payload so successful_payment -> process_any_payment activates the plan.
                        if provider == "stars" {
                            let amount_usd = duration.price as f64 / 100.0;
                            // Shared rate helper (services::payment::stars) — the single
                            // source of truth for USD↔XTR across every Stars path.
                            let stars_rate =
                                crate::services::payment::stars::stars_per_usd(&state.settings)
                                    .await;
                            let xtr_amount = crate::services::payment::stars::usd_cents_to_stars(
                                duration.price,
                                stars_rate,
                            )
                            .clamp(0, u32::MAX as i64)
                                as u32;
                            if xtr_amount == 0 {
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(lang, "checkout.stars_unavailable"))
                                    .show_alert(true)
                                    .await;
                                return Ok(());
                            }

                            let payload = PaymentType::SubscriptionPurchase(duration.plan_id)
                                .to_payload_string(u.id);
                            let prices = vec![LabeledPrice {
                                label: t(lang, "checkout.stars_line_item").to_string(),
                                amount: xtr_amount,
                            }];

                            let _ = bot.answer_callback_query(callback_id).await;
                            if let Some(msg) = q.message {
                                let _ = bot.delete_message(msg.chat().id, msg.id()).await;
                                let _ = bot
                                    .send_invoice(
                                        msg.chat().id,
                                        t(lang, "checkout.stars_invoice_title"),
                                        tf(
                                            lang,
                                            "checkout.stars_invoice_desc",
                                            &[&format!("{:.2}", amount_usd)],
                                        ),
                                        payload,
                                        "XTR",
                                        prices,
                                    )
                                    .await;
                            }
                            return Ok(());
                        }

                        let amount = duration.price;
                        let currency = "USD";
                        let product_id = duration.plan_id;
                        let metadata = serde_json::json!({
                            "type": "plan",
                            "duration_days": duration.duration_days
                        });

                        match state
                            .marketplace_service
                            .create_session(
                                &u,
                                product_id,
                                provider,
                                amount,
                                currency,
                                Some(metadata),
                            )
                            .await
                        {
                            Ok((session, invoice_payload)) => {
                                if provider == "balance" {
                                    // Charge the wallet FIRST with an atomic, conditional
                                    // deduction so a user can never spend more than they hold
                                    // (`balance >= $1` makes it a no-op on insufficient funds).
                                    // Only fulfill after a successful charge; refund if
                                    // fulfillment fails. Mirrors the Mini App path in
                                    // api/client.rs and fixes the old fulfill-then-deduct
                                    // TOCTOU double-spend / free-fulfillment bug.
                                    let charged = sqlx::query(
                                        "UPDATE users SET balance = balance - $1 WHERE id = $2 AND balance >= $1",
                                    )
                                    .bind(amount)
                                    .bind(u.id)
                                    .execute(&state.pool)
                                    .await;

                                    match charged {
                                        Ok(res) if res.rows_affected() == 1 => {}
                                        Ok(_) => {
                                            let _ = state
                                                .marketplace_service
                                                .mark_session_failed(session.id)
                                                .await;
                                            let _ = bot
                                                .answer_callback_query(callback_id)
                                                .text(t(lang, "checkout.insufficient_balance"))
                                                .show_alert(true)
                                                .await;
                                            return Ok(());
                                        }
                                        Err(_) => {
                                            let _ = state
                                                .marketplace_service
                                                .mark_session_failed(session.id)
                                                .await;
                                            let _ = bot
                                                .answer_callback_query(callback_id)
                                                .text(t(lang, "checkout.charge_failed"))
                                                .show_alert(true)
                                                .await;
                                            return Ok(());
                                        }
                                    }

                                    if let Err(e) =
                                        state.marketplace_service.fulfill_payment(session.id).await
                                    {
                                        // Refund so the user isn't billed for nothing.
                                        let _ = sqlx::query(
                                            "UPDATE users SET balance = balance + $1 WHERE id = $2",
                                        )
                                        .bind(amount)
                                        .bind(u.id)
                                        .execute(&state.pool)
                                        .await;
                                        let _ = bot
                                            .answer_callback_query(callback_id)
                                            .text(tf(
                                                lang,
                                                "checkout.fulfillment_failed",
                                                &[&e.to_string()],
                                            ))
                                            .show_alert(true)
                                            .await;
                                        return Ok(());
                                    }

                                    let _ = bot
                                        .answer_callback_query(callback_id)
                                        .text(t(lang, "checkout.paid_toast"))
                                        .await;
                                    if let Some(msg) = q.message {
                                        let _ = bot
                                            .send_message(
                                                msg.chat().id,
                                                t(lang, "checkout.balance_paid"),
                                            )
                                            .parse_mode(ParseMode::Html)
                                            .await;
                                    }
                                    return Ok(());
                                }

                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(lang, "checkout.invoice_toast"))
                                    .await;

                                if let Some(msg) = q.message {
                                    if provider == "manual" {
                                        let _ = bot
                                            .send_message(
                                                msg.chat().id,
                                                tf(
                                                    lang,
                                                    "checkout.manual",
                                                    &[&escape_html(&invoice_payload)],
                                                ),
                                            )
                                            .parse_mode(ParseMode::Html)
                                            .await;
                                    } else {
                                        let buttons = vec![vec![InlineKeyboardButton::url(
                                            t(lang, "checkout.pay_now"),
                                            invoice_payload
                                                .parse()
                                                .unwrap_or("https://example.com".parse().unwrap()),
                                        )]];
                                        let _ = bot
                                            .send_message(
                                                msg.chat().id,
                                                t(lang, "checkout.invoice_ready"),
                                            )
                                            .parse_mode(ParseMode::Html)
                                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                    .show_alert(true)
                                    .await;
                            }
                        }
                    }
                }
            }

            ext_dur if ext_dur.starts_with("ext_dur_") => {
                let id_str = ext_dur.strip_prefix("ext_dur_").unwrap();
                if let Ok(duration_id) = id_str.parse::<i64>() {
                    let user_db: Option<caramba_db::models::store::User> = state
                        .store_service
                        .get_user_by_tg_id(tg_id)
                        .await
                        .ok()
                        .flatten();
                    if let Some(u) = user_db {
                        match state
                            .store_service
                            .extend_subscription(u.id, duration_id)
                            .await
                        {
                            Ok(sub) => {
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(lang, "services.extended_toast"))
                                    .await;
                                // Agents pull config automatically - no sync needed

                                if let Some(msg) = q.message {
                                    let _ = bot
                                        .send_message(
                                            msg.chat().id,
                                            tf(
                                                lang,
                                                "services.extended",
                                                &[&sub.expires_at.format("%Y-%m-%d").to_string()],
                                            ),
                                        )
                                        .parse_mode(ParseMode::Html)
                                        .await;
                                }
                            }
                            Err(e) => {
                                error!("Extension failed for user {}: {}", u.id, e);
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                    .show_alert(true)
                                    .await;
                            }
                        }
                    } else {
                        error!("User not found for extension: {}", tg_id);
                    }
                }
            }

            // Store Product Purchase
            buyprod if buyprod.starts_with("buyprod_") => {
                let prod_id = buyprod
                    .strip_prefix("buyprod_")
                    .unwrap()
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_db: Option<caramba_db::models::store::User> = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();

                if let Some(u) = user_db {
                    match state
                        .store_service
                        .purchase_product_with_balance(u.id, prod_id)
                        .await
                    {
                        Ok(product) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "store.purchase_ok_toast"))
                                .await;
                            if let Some(msg) = q.message {
                                let text = match product.content {
                                    Some(content) => tf(
                                        lang,
                                        "store.purchase_ok",
                                        &[&escape_html(&product.name), &escape_html(&content)],
                                    ),
                                    None => tf(
                                        lang,
                                        "store.purchase_ok_no_content",
                                        &[&escape_html(&product.name)],
                                    ),
                                };
                                let _ = bot
                                    .send_message(msg.chat().id, text)
                                    .parse_mode(ParseMode::Html)
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            // DEVICE MANAGEMENT
            devices if devices.starts_with("devices_") => {
                let sub_id = devices
                    .strip_prefix("devices_")
                    .unwrap()
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_db = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();

                if let Some(u) = user_db {
                    // Verify ownership
                    let user_subs = state
                        .store_service
                        .get_user_subscriptions(u.id)
                        .await
                        .unwrap_or_default();
                    if let Some(_sub_details) = user_subs.iter().find(|s| s.sub.id == sub_id) {
                        // Get active IPs
                        let ips = state
                            .store_service
                            .get_subscription_active_ips(sub_id)
                            .await
                            .unwrap_or_default();
                        let limit = state
                            .store_service
                            .get_subscription_device_limit(sub_id)
                            .await
                            .unwrap_or(0);

                        let mut text =
                            format!("📱 *Active Devices for Subscription \\#{:?}*\n", sub_id);
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
                            text.push_str("No active sessions detected in the last 15 minutes\\.");
                        } else {
                            for ip in &ips {
                                // Mask IP slightly for privacy? Or show full? User owns it.
                                // Show time
                                let time_ago = chrono::Utc::now() - ip.last_seen_at;
                                let mins = time_ago.num_minutes();
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
                                format!("kill_sessions_{}", sub_id),
                            )]);
                        }
                        buttons.push(vec![InlineKeyboardButton::callback(
                            "🔙 Back",
                            "myservices_page_0",
                        )]);

                        if let Some(msg) = q.message {
                            let _ = bot
                                .edit_message_text(msg.chat().id, msg.id(), text)
                                .parse_mode(ParseMode::MarkdownV2)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await;
                        }
                    } else {
                        let _ = bot
                            .answer_callback_query(callback_id.clone())
                            .text("❌ Subscription not found.")
                            .await;
                    }
                }
                let _ = bot.answer_callback_query(callback_id.clone()).await;
            }

            kill if kill.starts_with("kill_sessions_") => {
                let sub_id = kill
                    .strip_prefix("kill_sessions_")
                    .unwrap()
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_db = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();

                if let Some(u) = user_db {
                    // Verify ownership
                    let user_subs = state
                        .store_service
                        .get_user_subscriptions(u.id)
                        .await
                        .unwrap_or_default();
                    if let Some(sub_details) = user_subs.iter().find(|s| s.sub.id == sub_id) {
                        match state
                            .connection_service
                            .kill_subscription_connections(sub_details.sub.id)
                            .await
                        {
                            Ok(_) => {
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(t(lang, "devices.reset_toast"))
                                    .show_alert(true)
                                    .await;
                                // Update the message to remove "Kill" button or showing refreshed list
                                if let Some(msg) = q.message {
                                    // Trigger refresh by sending "devices_" callback essentially?
                                    // Easier to just edit text.
                                    let _ = bot
                                        .send_message(msg.chat().id, t(lang, "devices.reset_done"))
                                        .parse_mode(ParseMode::Html)
                                        .await;
                                }
                            }
                            Err(e) => {
                                let _ = bot
                                    .answer_callback_query(callback_id)
                                    .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                    .show_alert(true)
                                    .await;
                            }
                        }
                    }
                }
            }
            store if store.starts_with("store_") => {
                let chat_id = q.message.as_ref().map(|m| m.chat().id).unwrap_or(ChatId(0));
                if chat_id.0 == 0 {
                    return Ok(());
                } // Safety

                if let Some(cat_id_str) = store.strip_prefix("store_cat_") {
                    if let Ok(cat_id) = cat_id_str.parse::<i64>() {
                        let products = state
                            .store_service
                            .get_products_by_category(cat_id)
                            .await
                            .unwrap_or_default();
                        if products.is_empty() {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "store.category_empty"))
                                .await;
                        } else {
                            let _ = bot.answer_callback_query(callback_id).await;
                            // Showcase style: separate message per product
                            for product in products {
                                let text = format!(
                                    "📦 <b>{}</b>\n\n{}\n\n{} <b>${}</b>",
                                    escape_html(&product.name),
                                    escape_html(
                                        product
                                            .description
                                            .as_deref()
                                            .unwrap_or(t(lang, "store.no_description"))
                                    ),
                                    t(lang, "store.price"),
                                    money(product.price)
                                );
                                let buttons = vec![
                                    vec![InlineKeyboardButton::callback(
                                        tf(lang, "store.buy_now", &[&money(product.price)]),
                                        format!("buyprod_{}", product.id),
                                    )],
                                    vec![InlineKeyboardButton::callback(
                                        t(lang, "store.add_to_cart"),
                                        format!("add_cart_prod_{}", product.id),
                                    )],
                                ];
                                let _ = bot
                                    .send_message(chat_id, text)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                            // Add back button and cart button
                            let nav = vec![
                                vec![InlineKeyboardButton::callback(
                                    t(lang, "store.back_to_categories"),
                                    "store_home",
                                )],
                                vec![InlineKeyboardButton::callback(
                                    t(lang, "cart.view"),
                                    "view_cart",
                                )],
                            ];
                            let _ = bot
                                .send_message(chat_id, "---")
                                .reply_markup(InlineKeyboardMarkup::new(nav))
                                .await;
                        }
                    }
                } else if let Some(prod_id_str) = store.strip_prefix("store_prod_") {
                    if let Ok(prod_id) = prod_id_str.parse::<i64>() {
                        if let Ok(product) = state.store_service.get_product(prod_id).await {
                            let _ = bot.answer_callback_query(callback_id).await;
                            let text = format!(
                                "📦 <b>{}</b>\n\n{}\n\n{} <b>${}</b>",
                                escape_html(&product.name),
                                escape_html(
                                    product
                                        .description
                                        .as_deref()
                                        .unwrap_or(t(lang, "store.no_description"))
                                ),
                                t(lang, "store.price"),
                                money(product.price)
                            );

                            let buttons = vec![
                                vec![InlineKeyboardButton::callback(
                                    tf(lang, "store.buy_now", &[&money(product.price)]),
                                    format!("buyprod_{}", product.id),
                                )],
                                vec![InlineKeyboardButton::callback(
                                    t(lang, "store.add_to_cart"),
                                    format!("add_cart_prod_{}", product.id),
                                )],
                                vec![InlineKeyboardButton::callback(
                                    t(lang, "store.back"),
                                    format!("store_cat_{}", product.category_id.unwrap_or(0)),
                                )],
                            ];

                            let _ = bot
                                .edit_message_text(chat_id, q.message.unwrap().id(), text)
                                .parse_mode(ParseMode::MarkdownV2)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await;
                        } else {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "store.product_not_found"))
                                .await;
                        }
                    }
                } else if store == "store_home" {
                    let categories: Vec<caramba_db::models::store::StoreCategory> = state
                        .catalog_service
                        .get_categories()
                        .await
                        .unwrap_or_default();
                    let mut buttons = Vec::new();
                    for cat in categories {
                        buttons.push(vec![InlineKeyboardButton::callback(
                            cat.name,
                            format!("store_cat_{}", cat.id),
                        )]);
                    }
                    // View Cart
                    buttons.push(vec![InlineKeyboardButton::callback(
                        t(lang, "cart.view"),
                        "view_cart",
                    )]);

                    let kb = InlineKeyboardMarkup::new(buttons);
                    let _ = bot
                        .edit_message_text(
                            chat_id,
                            q.message.unwrap().id(),
                            t(lang, "store.categories"),
                        )
                        .parse_mode(ParseMode::Html)
                        .reply_markup(kb)
                        .await;
                }
            }

            // Cart Actions
            "view_cart" => {
                let _ = bot.answer_callback_query(callback_id).await;
                let user_db = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(user) = user_db {
                    let cart_items = state
                        .store_service
                        .get_user_cart(user.id)
                        .await
                        .unwrap_or_default();

                    let text = if cart_items.is_empty() {
                        t(lang, "cart.empty").to_string()
                    } else {
                        let mut total_price: i64 = 0;
                        let mut body = format!("{}\n\n", t(lang, "cart.title"));

                        for item in &cart_items {
                            body.push_str(&format!(
                                "• <b>{}</b> (x{}) — ${}\n",
                                escape_html(&item.product_name),
                                item.quantity,
                                money(item.price)
                            ));
                            total_price += item.price * item.quantity;
                        }

                        body.push('\n');
                        body.push_str(&tf(lang, "cart.total", &[&money(total_price)]));
                        body
                    };

                    let buttons = if cart_items.is_empty() {
                        vec![vec![InlineKeyboardButton::callback(
                            t(lang, "cart.return_to_store"),
                            "store_home",
                        )]]
                    } else {
                        vec![
                            vec![InlineKeyboardButton::callback(
                                t(lang, "cart.checkout"),
                                "cart_checkout",
                            )],
                            vec![InlineKeyboardButton::callback(
                                t(lang, "cart.clear"),
                                "cart_clear",
                            )],
                            vec![InlineKeyboardButton::callback(
                                t(lang, "cart.continue_shopping"),
                                "store_home",
                            )],
                        ]
                    };

                    if let Some(msg) = q.message {
                        let _ = bot
                            .send_message(msg.chat().id, text)
                            .parse_mode(ParseMode::Html)
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await;
                    }
                }
            }

            "cart_clear" => {
                let user_db = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(user) = user_db {
                    let _ = state.store_service.clear_cart(user.id).await;
                    let _ = bot
                        .answer_callback_query(callback_id)
                        .text(t(lang, "cart.cleared_toast"))
                        .await;
                    if let Some(msg) = q.message {
                        let _ = bot
                            .edit_message_text(msg.chat().id, msg.id(), t(lang, "cart.empty"))
                            .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::callback(
                                    t(lang, "cart.return_to_store"),
                                    "store_home",
                                ),
                            ]]))
                            .await;
                    }
                }
            }

            "cart_checkout" => {
                let user_db = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(user) = user_db {
                    match state.store_service.checkout_cart(user.id).await {
                        Ok(notes) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "cart.checkout_ok_toast"))
                                .await;
                            let mut response = format!("{}\n\n", t(lang, "cart.checkout_ok"));
                            for note in notes {
                                response.push_str(&format!("{}\n", escape_html(&note)));
                            }
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, response)
                                    .parse_mode(ParseMode::Html)
                                    .await;
                                let _ = bot.delete_message(msg.chat().id, msg.id()).await;
                                // Delete cart msg
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            add_cart if add_cart.starts_with("add_cart_prod_") => {
                let prod_id = add_cart
                    .strip_prefix("add_cart_prod_")
                    .unwrap()
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_db = state
                    .store_service
                    .get_user_by_tg_id(tg_id)
                    .await
                    .ok()
                    .flatten();
                if let Some(user) = user_db {
                    match state.store_service.add_to_cart(user.id, prod_id, 1).await {
                        Ok(_) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(t(lang, "cart.added_toast"))
                                .await;
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(tf(lang, "checkout.failed", &[&e.to_string()]))
                                .await;
                        }
                    }
                }
            }
            "edit_ref_code" => {
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let text = tf(
                        lang,
                        "referral.alias_prompt",
                        &[t(lang, "referral.alias_marker")],
                    );

                    if let Err(e) = bot
                        .send_message(msg.chat().id, text)
                        .parse_mode(ParseMode::Html)
                        .reply_markup(ForceReply::new().selective())
                        .await
                    {
                        error!("CRITICAL: Failed to send edit_ref_code prompt: {}", e);
                    }
                }
            }

            "enter_referrer" => {
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let text = tf(
                        lang,
                        "referral.referrer_prompt",
                        &[t(lang, "referral.referrer_marker")],
                    );

                    if let Err(e) = bot
                        .send_message(msg.chat().id, text)
                        .parse_mode(ParseMode::Html)
                        .reply_markup(ForceReply::new().selective())
                        .await
                    {
                        error!("CRITICAL: Failed to send enter_referrer prompt: {}", e);
                    }
                }
            }

            // === Quick Wins: Auto-Renewal Toggle ===
            toggle if toggle.starts_with("toggle_renew_") => {
                let sub_id: i64 = toggle
                    .strip_prefix("toggle_renew_")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let _ = bot.answer_callback_query(callback_id).await;

                match state.store_service.toggle_auto_renewal(sub_id).await {
                    Ok(new_state) => {
                        let status_text = if new_state {
                            t(lang, "renew.enabled")
                        } else {
                            t(lang, "renew.disabled")
                        };

                        if let Some(msg) = q.message {
                            let _ = bot
                                .send_message(msg.chat().id, status_text)
                                .parse_mode(ParseMode::Html)
                                .await;
                        }
                    }
                    Err(e) => {
                        error!("Failed to toggle auto-renewal: {}", e);
                        if let Some(msg) = q.message {
                            let _ = bot
                                .send_message(msg.chat().id, t(lang, "renew.toggle_failed"))
                                .parse_mode(ParseMode::Html)
                                .await;
                        }
                    }
                }
            }

            _ => {
                let _ = bot
                    .answer_callback_query(callback_id)
                    .text(t(lang, "error.not_implemented"))
                    .await;
            }
        }
    }
    Ok::<_, teloxide::RequestError>(())
}

fn make_amount_keyboard(lang: Lang, prefix: &str) -> InlineKeyboardMarkup {
    let amounts = [5, 10, 20, 50];
    let mut buttons = Vec::new();

    // 2x2 grid
    for chunk in amounts.chunks(2) {
        let mut row = Vec::new();
        for &amt in chunk {
            row.push(InlineKeyboardButton::callback(
                format!("${}", amt),
                format!("{}_{}", prefix, amt),
            ));
        }
        buttons.push(row);
    }
    buttons.push(vec![InlineKeyboardButton::callback(
        t(lang, "topup.back"),
        "topup_menu",
    )]);
    InlineKeyboardMarkup::new(buttons)
}

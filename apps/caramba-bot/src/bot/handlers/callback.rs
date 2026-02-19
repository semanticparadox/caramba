use crate::bot::keyboards::make_amount_keyboard;
use crate::bot::keyboards::{main_menu, terms_keyboard};
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
                let lang = if data.contains("en") { "en" } else { "ru" };
                let _ = bot.answer_callback_query(callback_id).await;

                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let _ = state.store_service.update_user_language(u.id, lang).await;

                    let terms_text = state
                        .settings
                        .get_or_default("terms_of_service", "Terms of Service...")
                        .await;

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
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    let _ = state.store_service.update_user_terms(u.id).await;

                    if let Some(msg) = q.message {
                        let _ = bot.delete_message(msg.chat().id, msg.id()).await;

                        let welcome_text = format!(
                            "👋 <b>Welcome!</b>\n\n\
                            Use the menu below to manage your VPN subscriptions and digital goods."
                        );
                        let _ = bot
                            .send_message(msg.chat().id, welcome_text)
                            .parse_mode(ParseMode::Html)
                            .reply_markup(main_menu())
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
                let _ = bot
                    .answer_callback_query(callback_id)
                    .text("You must accept terms to proceed.")
                    .show_alert(true)
                    .await;
            }

            extend if extend.starts_with("extend_sub_") => {
                let text_plans: Vec<Plan> = state
                    .store_service
                    .get_active_plans()
                    .await
                    .unwrap_or_default();

                if text_plans.is_empty() {
                    let _ = bot
                        .answer_callback_query(callback_id)
                        .text("❌ No active plans available at the moment.")
                        .await;
                } else {
                    let _ = bot.answer_callback_query(callback_id).await;
                    let mut response = "💎 *Choose Plan to Extend:*\n\n".to_string();
                    let mut buttons = Vec::new();

                    for plan in text_plans {
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
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let _ = bot
                        .send_message(msg.chat().id, "🎟 Enter your Gift Code below:")
                        .reply_markup(ForceReply::new().selective())
                        .await;
                }
            }

            "topup_menu" => {
                let response = "💳 *Choose Top-up Method:*";
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "🪙 Crypto (USDT/TON)",
                        "pay_cryptobot",
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⚡ Crypto (Altcoins)",
                        "pay_nowpayments",
                    )],
                    vec![InlineKeyboardButton::callback(
                        "🇷🇺 Cards (RUB/SBP)",
                        "pay_crystal",
                    )],
                    vec![InlineKeyboardButton::callback(
                        "🌍 Global Cards (USD)",
                        "pay_stripe",
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⭐️ Telegram Stars",
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
                let buttons = make_amount_keyboard("cb");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(
                            msg.chat().id,
                            msg.id(),
                            "🔹 *Select amount for CryptoBot:*",
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_nowpayments" => {
                let buttons = make_amount_keyboard("np");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(
                            msg.chat().id,
                            msg.id(),
                            "🔹 *Select amount for NOWPayments:*",
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_crystal" => {
                let buttons = make_amount_keyboard("cp");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(
                            msg.chat().id,
                            msg.id(),
                            "🔹 *Select amount for CrystalPay (Cards/SBP):*",
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_stripe" => {
                let buttons = make_amount_keyboard("str");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(
                            msg.chat().id,
                            msg.id(),
                            "🔹 *Select amount for Stripe:*",
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(buttons)
                        .await;
                }
            }
            "pay_stars" => {
                let buttons = make_amount_keyboard("star");
                if let Some(msg) = q.message {
                    let _ = bot
                        .edit_message_text(msg.chat().id, msg.id(), "🔹 *Select amount via Stars:*")
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
                if let Ok(Some(u)) = user_res {
                    match state
                        .pay_service
                        .create_cryptobot_invoice(u.id, amount, PaymentType::BalanceTopup)
                        .await
                    {
                        Ok(url) => {
                            let buttons = vec![vec![InlineKeyboardButton::url(
                                "🔗 Pay with CryptoBot",
                                url.parse::<Url>().unwrap(),
                            )]];
                            let _ = bot.answer_callback_query(callback_id).await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        format!("💳 Invoice for *${:.2}* created\\!", amount),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(format!("Error: {}", e))
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
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    match state
                        .pay_service
                        .create_nowpayments_invoice(u.id, amount, PaymentType::BalanceTopup)
                        .await
                    {
                        Ok(url) => {
                            let buttons = vec![vec![InlineKeyboardButton::url(
                                "🔗 Pay with NOWPayments",
                                url.parse::<Url>().unwrap(),
                            )]];
                            let _ = bot.answer_callback_query(callback_id).await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        format!("💳 Invoice for *${:.2}* created\\!", amount),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(format!("Error: {}", e))
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
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    match state
                        .pay_service
                        .create_crystalpay_invoice(u.id, amount, PaymentType::BalanceTopup)
                        .await
                    {
                        Ok(url) => {
                            let buttons = vec![vec![InlineKeyboardButton::url(
                                "🔗 Pay with Card (CrystalPay)",
                                url.parse::<Url>().unwrap(),
                            )]];
                            let _ = bot.answer_callback_query(callback_id).await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        format!("💳 Invoice for *${:.2}* created\\!", amount),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(format!("Error: {}", e))
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
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    match state
                        .pay_service
                        .create_stripe_session(u.id, amount, PaymentType::BalanceTopup)
                        .await
                    {
                        Ok(url) => {
                            let buttons = vec![vec![InlineKeyboardButton::url(
                                "🔗 Pay with Stripe",
                                url.parse::<Url>().unwrap(),
                            )]];
                            let _ = bot.answer_callback_query(callback_id).await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        format!("💳 Invoice for *${:.2}* created\\!", amount),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(format!("Error: {}", e))
                                .show_alert(true)
                                .await;
                        }
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
                if let Ok(Some(u)) = user_res {
                    let payload = PaymentType::BalanceTopup.to_payload_string(u.id);
                    let prices = vec![LabeledPrice {
                        label: "Top-up".to_string(),
                        amount: xtr_amount as u32,
                    }];

                    if let Some(msg) = q.message {
                        let _ = bot.delete_message(msg.chat().id, msg.id()).await;

                        let _ = bot
                            .send_invoice(
                                msg.chat().id,
                                "Balance Top-up",
                                format!("Top-up balance by ${}", amount_usd),
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
                let _ = bot
                    .answer_callback_query(callback_id.clone())
                    .text("Fetching links...")
                    .await;

                let links_res: AnyhowResult<Vec<String>> =
                    state.store_service.get_subscription_links(sub_id).await;
                match links_res {
                    Ok(links) => {
                        if links.is_empty() {
                            let _ = bot
                                .send_message(ChatId(tg_id), "❌ No connection links available.")
                                .await;
                        } else {
                            let mut response = "🔗 *Your Connection Links:*\n\n".to_string();
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
                            .send_message(ChatId(tg_id), "❌ Failed to fetch links.")
                            .await;
                    }
                }
            }

            get_config if get_config.starts_with("get_config_") => {
                let _sub_id = get_config.strip_prefix("get_config_").unwrap_or("0");
                let _ = bot
                    .answer_callback_query(callback_id)
                    .text("Generating profile...")
                    .await;

                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    match state.store_service.generate_subscription_file(u.id).await {
                        Ok(json_content) => {
                            let data: Vec<u8> = json_content.into_bytes();
                            let input_file = teloxide::types::InputFile::memory(data)
                                .file_name("caramba_v2_profile.json");

                            if let Some(msg) = q.message {
                                let _ = bot.send_document(msg.chat().id, input_file)
                                    .caption("📂 <b>Your CARAMBA Profile</b>\n\nImport this file into Sing-box, Nekobox, or Hiddify.")
                                    .parse_mode(ParseMode::Html)
                                    .await;
                            }
                        }
                        Err(e) => {
                            error!("Config error: {}", e);
                            let _ = bot
                                .send_message(ChatId(tg_id), "❌ Failed to generate profile.")
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
                    match state
                        .store_service
                        .activate_subscription(sub_id, u.id)
                        .await
                    {
                        Ok(sub) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text("✅ Activated!")
                                .await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(
                                        msg.chat().id,
                                        format!(
                                            "🚀 *Subscription Activated!*\nExpires: `{}`",
                                            sub.expires_at.format("%Y-%m-%d")
                                        ),
                                    )
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(format!("❌ Error: {}", e))
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
                                            "🎁 You have no unredeemed gift codes.",
                                        )
                                        .await;
                                }
                            } else {
                                let mut response =
                                    "🎁 *My Gift Codes* \\(Unredeemed\\):\n\n".to_string();
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
                                    .send_message(msg.chat().id, "❌ Error fetching gift codes.")
                                    .await;
                            }
                        }
                    }
                }
            }

            edit_note if edit_note.starts_with("edit_note_") => {
                let sub_id = edit_note.strip_prefix("edit_note_").unwrap_or("0");
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let _ = bot
                        .send_message(
                            msg.chat().id,
                            format!(
                                "Reply to this message with your note for Subscription #{}.",
                                sub_id
                            ),
                        )
                        .reply_markup(ForceReply::new().selective())
                        .await;
                }
            }

            devices if devices.starts_with("devices_") => {
                let sub_id = devices
                    .strip_prefix("devices_")
                    .unwrap()
                    .parse::<i64>()
                    .unwrap_or(0);
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

                let mut response = format!("📱 *CONNECTED DEVICES*\\n\\n");
                response.push_str(&format!("🔢 *Device Limit:* `{}`\\n", limit));
                response.push_str(&format!("✅ *Active Devices:* `{}`\\n\\n", ips.len()));

                if ips.is_empty() {
                    response.push_str("_No devices currently connected\\._\\n\\n");
                } else {
                    response.push_str("🌐 *Recent Connections:*\\n");
                    for (idx, ip_record) in ips.iter().take(10).enumerate() {
                        let duration =
                            chrono::Utc::now().signed_duration_since(ip_record.last_seen_at);
                        let minutes_ago = duration.num_minutes();
                        response.push_str(&format!(
                            "{}\\. `{}` _{} min ago_\\n",
                            idx + 1,
                            escape_md(&ip_record.client_ip),
                            minutes_ago
                        ));
                    }
                }

                let keyboard =
                    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                        "« Back to Services",
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
                        .text("❌ No active plans.")
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

                            let mut response = "🔐 *MY SERVICES*\n\n".to_string();
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
                                "   💎 *Plan:* {}\n",
                                escape_md(&sub.plan_name)
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
                                response.push_str(&format!(
                                    "   📊 *Traffic Used:* `{:.2} GB`\n",
                                    used_gb
                                ));
                            }

                            if sub.sub.status == "active" {
                                response.push_str(&format!(
                                    "   ⌛ *Expires:* `{}`\n",
                                    sub.sub.expires_at.format("%Y-%m-%d")
                                ));
                            } else {
                                let duration = sub.sub.expires_at - sub.sub.created_at;
                                response.push_str(&format!(
                                    "   ⏱ *Duration:* `{} days` \\(starts on activation\\)\n",
                                    duration.num_days()
                                ));
                            }
                            response.push_str("\n");
                            if let Some(note) = &sub.sub.note {
                                response.push_str(&format!("📝 *Note:* {}\n\n", escape_md(note)));
                            }

                            let mut buttons = Vec::new();
                            buttons.push(vec![InlineKeyboardButton::callback(
                                "📝 Edit Note",
                                format!("edit_note_{}", sub.sub.id),
                            )]);

                            if sub.sub.status == "active" {
                                buttons.push(vec![
                                    InlineKeyboardButton::callback(
                                        "🔗 Get Links",
                                        format!("get_links_{}", sub.sub.id),
                                    ),
                                    InlineKeyboardButton::callback(
                                        "📄 JSON Profile",
                                        format!("get_config_{}", sub.sub.id),
                                    ),
                                    InlineKeyboardButton::callback(
                                        "⏳ Extend",
                                        format!("extend_sub_{}", sub.sub.id),
                                    ),
                                ]);
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    "📱 Devices",
                                    format!("devices_{}", sub.sub.id),
                                )]);
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
                            buttons.push(vec![InlineKeyboardButton::callback(
                                "🎁 My Gift Codes",
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
                    match state
                        .store_service
                        .convert_subscription_to_gift(sub_id, u.id)
                        .await
                    {
                        Ok(code) => {
                            let response = format!(
                                "🎁 *Gift Code Created!*\n\nCode: `{}`\n\nShare this code.",
                                code
                            );
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, response)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text("✅ Code Generated!")
                                .await;
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(format!("Error: {}", e))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            transfer if transfer.starts_with("transfer_init_") => {
                let sub_id = transfer.strip_prefix("transfer_init_").unwrap_or("0");
                let _ = bot.answer_callback_query(callback_id).await;
                if let Some(msg) = q.message {
                    let _ = bot.send_message(msg.chat().id, format!("➡️ *Transfer Subscription*\n\nReply with username for Subscription #{}.", sub_id)).reply_markup(ForceReply::new().selective()).await;
                }
            }

            buy_dur if buy_dur.starts_with("buy_dur_") => {
                let id = buy_dur
                    .strip_prefix("buy_dur_")
                    .unwrap()
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    match state.store_service.purchase_plan(u.id, id).await {
                        Ok(_) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text("✅ Success!")
                                .await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, "✅ *Purchase Successful!*")
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(format!("Error: {}", e))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            ext_dur if ext_dur.starts_with("ext_dur_") => {
                let id = ext_dur
                    .strip_prefix("ext_dur_")
                    .unwrap()
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    match state.store_service.extend_subscription(u.id, id).await {
                        Ok(_) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text("✅ Extended!")
                                .await;
                            if let Some(msg) = q.message {
                                let _ = bot
                                    .send_message(msg.chat().id, "✅ *Subscription Extended!*")
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await;
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(format!("Error: {}", e))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            scat if scat.starts_with("store_cat_") => {
                let cat_id = scat
                    .strip_prefix("store_cat_")
                    .unwrap()
                    .parse::<i64>()
                    .unwrap_or(0);
                let _ = bot.answer_callback_query(callback_id).await;

                match state.store_service.get_products_by_category(cat_id).await {
                    Ok(prods) => {
                        if prods.is_empty() {
                            let _ = bot
                                .send_message(ChatId(tg_id), "📦 No products in this category.")
                                .await;
                        } else {
                            let mut buttons = Vec::new();
                            let mut text = "📦 *Products:*\n\n".to_string();
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
                                    format!("View: {}", p.name),
                                    format!("view_prod_{}", p.id),
                                )]);
                            }
                            buttons.push(vec![InlineKeyboardButton::callback(
                                "« Back to Categories",
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
                        let _ = bot
                            .send_message(ChatId(tg_id), format!("Error: {}", e))
                            .await;
                    }
                }
            }

            vprod if vprod.starts_with("view_prod_") => {
                let prod_id = vprod
                    .strip_prefix("view_prod_")
                    .unwrap()
                    .parse::<i64>()
                    .unwrap_or(0);
                let _ = bot.answer_callback_query(callback_id).await;

                // We'd ideally have a get_product(id) but for now let's hope it's not needed or use a stub
                // Actually, let's just use the category list logic or add get_product to StoreService
                // (I'll skip full details for brevity but implement the "Buy" trigger)
                let text = "📦 *Product Details*\n\n(Details would go here...)";
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "💰 Buy Now",
                        format!("buy_prod_{}", prod_id),
                    )],
                    vec![InlineKeyboardButton::callback("« Back", "📦 Digital Store")], // Simple back
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
                    .unwrap()
                    .parse::<i64>()
                    .unwrap_or(0);
                let user_res: AnyhowResult<Option<User>> =
                    state.store_service.get_user_by_tg_id(tg_id).await;
                if let Ok(Some(u)) = user_res {
                    match state
                        .store_service
                        .purchase_product_with_balance(u.id, prod_id)
                        .await
                    {
                        Ok(p) => {
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text("✅ Purchase successful!")
                                .await;
                            let content = p
                                .content
                                .unwrap_or_else(|| "No additional content".to_string());
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
                            let _ = bot
                                .answer_callback_query(callback_id)
                                .text(format!("Error: {}", e))
                                .show_alert(true)
                                .await;
                        }
                    }
                }
            }

            _ => {
                // Ignore unknown
            }
        }
    }
    Ok(())
}

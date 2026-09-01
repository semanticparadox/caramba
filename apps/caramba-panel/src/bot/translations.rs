//! Переводы пользовательских строк панельного бота и уведомлений.
//!
//! EXA ROBOT продаётся русскоязычной аудитории, поэтому русский — язык по
//! умолчанию везде, включая уведомления. Английский остаётся как явный выбор
//! пользователя.
//!
//! # Порядок разрешения языка
//!
//! `users.language_code` (если это поддерживаемое значение) → настройка
//! `default_language` (если это поддерживаемое значение) → [`Lang::Ru`].
//!
//! Единственная точка входа — [`resolve_lang`] (чистая) и [`lang_for`]
//! (читает настройку). Никаких локальных `if is_ru` по файлам.
//!
//! # Формат
//!
//! Все строки с разметкой написаны под **HTML** parse mode (нужно экранировать
//! только `& < >`, см. [`crate::bot::utils::escape_html`]) — русский текст
//! постоянно содержит `.`, `!`, `-`, `(`, `)`, которые MarkdownV2 требует
//! экранировать, и одна пропущенная обратная косая черта роняет отправку
//! сообщения целиком. Строки, отправляемые в plain-режиме (`answer_callback_query`,
//! всплывающие алерты, сообщения без `parse_mode`), разметки не содержат.

use crate::settings::SettingsService;

/// Значение, возвращаемое [`t`] для неизвестного ключа. Видно в чате, поэтому
/// его появление — баг, который ловится тестом [`tests::every_key_has_both_languages`].
const MISSING: &str = "???";

/// То же значение для тестов соседних модулей: реестр уведомлений проверяет,
/// что его ключи не разъехались с этой таблицей, и сравнивать ему нужно именно
/// с [`MISSING`].
#[cfg(test)]
pub const MISSING_FOR_TESTS: &str = MISSING;

/// Поддерживаемые языки интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
}

impl Lang {
    /// Язык по умолчанию, если ничего не удалось разрешить.
    pub const DEFAULT: Lang = Lang::Ru;

    /// Разбирает код языка. Принимаются как чистые коды (`ru`, `en`), так и
    /// полные теги Telegram/BCP-47 (`ru-RU`, `en_US`). Неподдерживаемые языки
    /// (`de`, `zh`, мусор, пустая строка) дают `None`, чтобы вызывающий код мог
    /// перейти к следующему шагу разрешения.
    pub fn parse(code: &str) -> Option<Lang> {
        let code = code.trim().to_ascii_lowercase();
        if code.starts_with("ru") {
            Some(Lang::Ru)
        } else if code.starts_with("en") {
            Some(Lang::En)
        } else {
            None
        }
    }

    /// Канонический код для хранения в `users.language_code`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::Ru => "ru",
            Lang::En => "en",
        }
    }
}

/// Ключ настройки с языком по умолчанию для пользователей без выбранного языка.
pub const DEFAULT_LANGUAGE_SETTING: &str = "default_language";

/// Чистое разрешение языка: язык пользователя → настройка `default_language` → `ru`.
///
/// Оба аргумента — «сырые» значения из БД, они могут быть `None`, пустыми или
/// содержать неподдерживаемый код; в этом случае используется следующий шаг.
pub fn resolve_lang(user_lang: Option<&str>, default_setting: Option<&str>) -> Lang {
    user_lang
        .and_then(Lang::parse)
        .or_else(|| default_setting.and_then(Lang::parse))
        .unwrap_or(Lang::DEFAULT)
}

/// [`resolve_lang`], читающий `default_language` из настроек панели.
pub async fn lang_for(settings: &SettingsService, user_lang: Option<&str>) -> Lang {
    // Быстрый путь: язык пользователя валиден — настройка не нужна.
    if let Some(lang) = user_lang.and_then(Lang::parse) {
        return lang;
    }
    let default = settings.get(DEFAULT_LANGUAGE_SETTING).await;
    resolve_lang(None, default.as_deref())
}

/// Читает `default_language` напрямую из таблицы `settings`.
///
/// Для мест без доступа к [`SettingsService`] (фоновые сервисы, у которых на
/// руках только пул). Порядок разрешения от этого не меняется — значение
/// по-прежнему скармливается в [`resolve_lang`] вторым аргументом.
pub async fn default_language_setting(pool: &sqlx::PgPool) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = $1")
        .bind(DEFAULT_LANGUAGE_SETTING)
        .fetch_optional(pool)
        .await
        .unwrap_or(None)
}

/// Таблица переводов. Макрос генерирует и [`t`], и [`KEYS`] из одного списка,
/// поэтому ключ физически невозможно добавить только в один язык.
macro_rules! translations {
    ($( $key:literal => { ru: $ru:expr, en: $en:expr } ),* $(,)?) => {
        /// Все известные ключи. Выводится из той же таблицы, что и [`t`],
        /// поэтому список нельзя рассинхронизировать с переводами. Вне тестов
        /// не используется — но именно на нём держится проверка полноты.
        #[cfg_attr(not(test), allow(dead_code))]
        pub const KEYS: &[&str] = &[ $($key),* ];

        /// Возвращает перевод по ключу. Неизвестный ключ даёт [`MISSING`].
        pub fn t(lang: Lang, key: &str) -> &'static str {
            match (key, lang) {
                $(
                    ($key, Lang::Ru) => $ru,
                    ($key, Lang::En) => $en,
                )*
                _ => MISSING,
            }
        }
    };
}

translations! {
    // =========================================================================
    // Платёжный поток Mini App (DM после checkout)
    // =========================================================================
    // {0} = название тарифа/товара, {1} = сумма, {2} = валюта
    "invoice_created" => {
        ru: "Счёт на оплату — {0}, {1} {2}. Оплатите по кнопке; после оплаты вернитесь в приложение.",
        en: "Payment invoice — {0}, {1} {2}. Pay via the button below; return to the app once you're done."
    },
    "pay_button" => { ru: "💳 Оплатить", en: "💳 Pay" },
    "payment_success_sub" => {
        ru: "✅ Оплата получена — подписка активирована.",
        en: "✅ Payment received — your subscription is active."
    },
    "payment_success_order" => {
        ru: "✅ Оплата получена — заказ оплачен.",
        en: "✅ Payment received — your order is paid."
    },
    "open_app_button" => { ru: "📱 Открыть приложение", en: "📱 Open App" },
    // {0} = числовой id
    "label_order" => { ru: "заказ №{0}", en: "order #{0}" },
    "label_subscription" => { ru: "подписка", en: "subscription" },

    // =========================================================================
    // Кнопки главного меню (reply keyboard).
    // ВАЖНО: нажатие такой кнопки приходит боту обычным текстом, поэтому эти
    // же строки распознаются в command.rs::menu_action на обоих языках.
    // =========================================================================
    "menu.buy" => { ru: "🛍 Купить подписку", en: "🛍 Buy Subscription" },
    "menu.services" => { ru: "🔐 Мои подписки", en: "🔐 My Services" },
    "menu.store" => { ru: "📦 Магазин", en: "📦 Digital Store" },
    "menu.profile" => { ru: "👤 Профиль", en: "👤 My Profile" },
    "menu.referral" => { ru: "🎁 Бонусы и рефералы", en: "🎁 Bonuses / Referral" },
    "menu.support" => { ru: "❓ Поддержка", en: "❓ Support" },
    "menu.open_app" => { ru: "🔑 Войти в приложение", en: "🔑 Open in app" },
    "menu.launch_app" => { ru: "🚀 Запустить", en: "🚀 Launch" },

    // =========================================================================
    // Онбординг: язык, условия, приветствие
    // =========================================================================
    "terms.title" => { ru: "📜 <b>Пользовательское соглашение</b>", en: "📜 <b>Terms of Service</b>" },
    "terms.prompt" => {
        ru: "Примите условия, чтобы продолжить.",
        en: "Please accept the terms to continue."
    },
    "terms.placeholder" => {
        ru: "Текст соглашения ещё не настроен.",
        en: "Terms of Service have not been configured yet."
    },
    "terms.accept" => { ru: "✅ Принимаю", en: "✅ Accept" },
    "terms.decline" => { ru: "❌ Отказываюсь", en: "❌ Decline" },
    "terms.must_accept" => {
        ru: "Без принятия условий продолжить нельзя.",
        en: "You must accept the terms to proceed."
    },

    // {0} = имя пользователя (уже экранировано под HTML)
    "welcome.start" => {
        ru: "👋 <b>Привет, {0}!</b>\n\n\
             Добро пожаловать в EXA ROBOT — ваш персональный VPN-сервис.\n\n\
             📱 <b>Как подключиться:</b>\n\
             1. Откройте Mini App кнопкой «Запустить» ниже\n\
             2. Выберите и оплатите подходящий тариф\n\
             3. Нажмите «Подключить в Hiddify» или «Подключить в Happ» — конфигурация импортируется автоматически\n\
             4. Также вы можете нажать «Скопировать ссылку» и вставить её вручную в любое совместимое VPN-приложение (Hiddify, Happ, Streisand, V2rayNG, Koala Clash и др.)\n\n\
             🌍 <b>Обход блокировок (Relay):</b>\n\
             Если в вашей стране действуют интернет-блокировки или белые списки — \
             откройте раздел «Выбрать сервер» и включите Relay для вашей страны. \
             Relay направит трафик через промежуточный сервер в вашем регионе, \
             что позволяет обойти ограничения.\n\
             После любых изменений настроек обновите подписку в VPN-приложении \
             (потяните вниз или удалите и добавьте профиль заново).\n\n\
             📲 <b>Скачать приложение:</b>\n\
             • Android/iOS — <a href=\"https://hiddify.com\">Hiddify</a>\n\
             • Windows/macOS/Linux — <a href=\"https://github.com/coolcoala/koala-clash\">Koala Clash</a>",
        en: "👋 <b>Hi, {0}!</b>\n\n\
             Welcome to EXA ROBOT — your personal VPN service.\n\n\
             📱 <b>How to connect:</b>\n\
             1. Open the Mini App with the «Launch» button below\n\
             2. Pick a plan and pay for it\n\
             3. Tap «Connect in Hiddify» or «Connect in Happ» — the config is imported automatically\n\
             4. You can also tap «Copy link» and paste it manually into any compatible VPN app (Hiddify, Happ, Streisand, V2rayNG, Koala Clash and others)\n\n\
             🌍 <b>Bypassing blocks (Relay):</b>\n\
             If your country filters the internet or uses allow-lists — open «Choose server» \
             and enable Relay for your country. Relay routes traffic through an intermediate \
             server in your region, which gets around the restrictions.\n\
             After changing any settings, refresh the subscription in your VPN app \
             (pull down, or remove and re-add the profile).\n\n\
             📲 <b>Download the app:</b>\n\
             • Android/iOS — <a href=\"https://hiddify.com\">Hiddify</a>\n\
             • Windows/macOS/Linux — <a href=\"https://github.com/coolcoala/koala-clash\">Koala Clash</a>"
    },
    // Первое уведомление в инбокс Mini App после регистрации. {0} = имя.
    // Отправляется через notifications_svc.create → DM в MarkdownV2, поэтому
    // текст намеренно без разметки (экранирование делает сам сервис).
    "welcome.notif_title" => {
        ru: "Добро пожаловать в EXA ROBOT!",
        en: "Welcome to EXA ROBOT!"
    },
    "welcome.notif_body" => {
        ru: "Здравствуйте, {0}!\n\nВыберите тариф во вкладке «Подписка», подключите устройство в «Устройствах» — и трафик пойдёт. Если что-то не работает, откройте тикет в поддержке: отвечаем быстро.",
        en: "Hi {0}!\n\nPick a plan in Subscription, connect a device in Devices, and you're online. If anything's off, open a support ticket — we reply fast."
    },

    "welcome.after_terms" => {
        ru: "👋 <b>Добро пожаловать!</b>\n\nВыберите пункт меню ниже, чтобы управлять подписками и покупками.",
        en: "👋 <b>Welcome!</b>\n\nUse the menu below to manage your VPN subscriptions and digital goods."
    },

    // =========================================================================
    // Общие ошибки и доступ
    // =========================================================================
    "error.banned" => {
        ru: "🚫 <b>Доступ закрыт</b>\n\nВаш аккаунт заблокирован.",
        en: "🚫 <b>Access denied</b>\n\nYour account has been banned."
    },
    "error.banned_spam" => {
        ru: "🚫 <b>Аккаунт заблокирован</b> за спам.",
        en: "🚫 <b>Account banned</b> due to spam/botting."
    },
    "error.generic" => {
        ru: "❌ Что-то пошло не так. Попробуйте ещё раз.",
        en: "❌ Something went wrong. Please try again."
    },
    "error.not_implemented" => {
        ru: "Эта кнопка пока не работает.",
        en: "Feature not yet implemented."
    },

    // =========================================================================
    // Оплата (бот-нативные потоки)
    // =========================================================================
    "pay.topup_success" => {
        ru: "✅ Оплата прошла — баланс пополнен.",
        en: "✅ Payment successful! Balance updated."
    },
    "pay.error_contact_support" => {
        ru: "❌ Не удалось обработать платёж. Напишите в поддержку.",
        en: "❌ Error processing payment. Please contact support."
    },
    "pay.unmatched" => {
        ru: "❌ Платёж получен, но мы не смогли сопоставить его с заказом. Напишите в поддержку.",
        en: "❌ We received your payment but could not match it to an order. Please contact support."
    },
    "pay.amount_mismatch" => {
        ru: "❌ Сумма платежа не совпадает со счётом. Заказ не активирован — напишите в поддержку.",
        en: "❌ The paid amount does not match the invoice. Your order was not activated — please contact support."
    },
    // Pre-checkout: Telegram показывает эти строки во всплывающем окне (plain text).
    "precheckout.bad_amount" => {
        ru: "Некорректная сумма платежа. Попробуйте ещё раз.",
        en: "Invalid payment amount. Please try again."
    },
    "precheckout.bad_payload" => {
        ru: "Платёжная сессия повреждена. Начните заново.",
        en: "Malformed payment session. Please start over."
    },
    "precheckout.session_missing" => {
        ru: "Платёжная сессия не найдена. Начните заново.",
        en: "Payment session not found. Please start over."
    },
    "precheckout.session_foreign" => {
        ru: "Эта платёжная сессия принадлежит другому аккаунту.",
        en: "Payment session does not match your account."
    },
    "precheckout.session_closed" => {
        ru: "Счёт больше не действителен. Начните заново.",
        en: "This invoice is no longer payable. Please start over."
    },
    "precheckout.not_stars" => {
        ru: "Этот счёт нельзя оплатить звёздами.",
        en: "This invoice is not payable with Stars."
    },
    "precheckout.amount_mismatch" => {
        ru: "Некорректная сумма платежа. Начните заново.",
        en: "Invalid payment amount. Please start over."
    },
    "precheckout.restricted" => {
        ru: "Ваш аккаунт ограничен.",
        en: "Your account is restricted."
    },
    "precheckout.no_account" => {
        ru: "Аккаунт не найден. Отправьте /start и попробуйте снова.",
        en: "Account not found. Send /start and try again."
    },
    "precheckout.unavailable" => {
        ru: "Сервис временно недоступен. Попробуйте ещё раз.",
        en: "Service temporarily unavailable. Please try again."
    },

    // Пополнение баланса
    "topup.choose_method" => { ru: "💳 <b>Выберите способ оплаты</b>", en: "💳 <b>Choose a top-up method</b>" },
    "topup.method_cryptobot" => { ru: "🪙 Крипта (USDT/TON)", en: "🪙 Crypto (USDT/TON)" },
    "topup.method_nowpayments" => { ru: "⚡ Крипта (альткоины)", en: "⚡ Crypto (altcoins)" },
    "topup.method_crystal" => { ru: "🇷🇺 Карты (RUB/СБП)", en: "🇷🇺 Cards (RUB/SBP)" },
    "topup.method_stripe" => { ru: "🌍 Карты (USD)", en: "🌍 Global cards (USD)" },
    "topup.method_stars" => { ru: "⭐️ Telegram Stars", en: "⭐️ Telegram Stars" },
    "topup.amount_cryptobot" => { ru: "🔹 <b>Сумма пополнения через CryptoBot:</b>", en: "🔹 <b>Select amount for CryptoBot:</b>" },
    "topup.amount_nowpayments" => { ru: "🔹 <b>Сумма пополнения через NOWPayments:</b>", en: "🔹 <b>Select amount for NOWPayments:</b>" },
    "topup.amount_crystal" => { ru: "🔹 <b>Сумма пополнения картой / СБП:</b>", en: "🔹 <b>Select amount for CrystalPay (cards/SBP):</b>" },
    "topup.amount_stripe" => { ru: "🔹 <b>Сумма пополнения через Stripe:</b>", en: "🔹 <b>Select amount for Stripe:</b>" },
    "topup.amount_stars" => { ru: "🔹 <b>Сумма пополнения звёздами:</b>", en: "🔹 <b>Select amount via Stars:</b>" },
    // {0} = сумма в долларах, например "10.00"
    "topup.invoice_created" => { ru: "💳 Счёт на <b>${0}</b> создан!", en: "💳 Invoice for <b>${0}</b> created!" },
    "topup.pay_cryptobot" => { ru: "🔗 Оплатить в CryptoBot", en: "🔗 Pay with CryptoBot" },
    "topup.pay_nowpayments" => { ru: "🔗 Оплатить в NOWPayments", en: "🔗 Pay with NOWPayments" },
    "topup.pay_crystal" => { ru: "🔗 Оплатить картой", en: "🔗 Pay with card" },
    "topup.pay_stripe" => { ru: "🔗 Оплатить через Stripe", en: "🔗 Pay with Stripe" },
    "topup.title" => { ru: "Пополнение баланса", en: "Balance top-up" },
    // {0} = сумма
    "topup.invoice_desc" => { ru: "Пополнение баланса на ${0}", en: "Top up balance by ${0}" },
    "topup.back" => { ru: "« Назад", en: "« Back" },

    // Выбор способа оплаты тарифа
    "checkout.choose_method" => {
        ru: "💳 <b>Способ оплаты</b>\n\nВыберите, как хотите оплатить подписку:",
        en: "💳 <b>Select payment method</b>\n\nChoose how you would like to pay for your VPN subscription:"
    },
    "checkout.choose_method_toast" => { ru: "Выберите способ оплаты.", en: "Please select a payment method." },
    // {0} = баланс, например "12.34"
    "checkout.pay_balance" => { ru: "💰 С баланса (${0})", en: "💰 Pay with balance (${0})" },
    "checkout.pay_manual" => { ru: "💳 Картой / вручную", en: "💳 Pay with card/manual" },
    "checkout.pay_stars" => { ru: "⭐️ Telegram Stars", en: "⭐️ Pay with Telegram Stars" },
    "checkout.pay_cryptobot" => { ru: "🪙 CryptoBot", en: "🪙 Pay with CryptoBot" },
    "checkout.pay_nowpayments" => { ru: "🪙 NOWPayments", en: "🪙 Pay with NOWPayments" },
    "checkout.back_to_plans" => { ru: "⬅️ К тарифам", en: "⬅️ Back to plans" },
    "checkout.invalid_duration" => { ru: "❌ Некорректный срок подписки.", en: "❌ Invalid duration." },
    "checkout.invalid_plan" => { ru: "❌ Некорректный тариф.", en: "❌ Invalid plan selection." },
    "checkout.stars_unavailable" => {
        ru: "❌ Этот тариф нельзя оплатить звёздами.",
        en: "❌ This plan cannot be paid with Stars."
    },
    "checkout.insufficient_balance" => { ru: "❌ Недостаточно средств на балансе", en: "❌ Insufficient balance" },
    "checkout.charge_failed" => {
        ru: "❌ Не удалось списать с баланса, попробуйте ещё раз",
        en: "❌ Balance charge failed, please try again"
    },
    // {0} = техническая причина (остаётся на английском — это внутренняя ошибка)
    "checkout.fulfillment_failed" => { ru: "❌ Не удалось выдать подписку: {0}", en: "❌ Fulfillment failed: {0}" },
    "checkout.failed" => { ru: "❌ Ошибка: {0}", en: "❌ Failed: {0}" },
    "checkout.paid_toast" => { ru: "✅ Оплачено!", en: "✅ Payment successful!" },
    "checkout.balance_paid" => {
        ru: "✅ <b>Подписка активирована!</b>\n\nОплата с баланса прошла успешно. Подписка уже работает.",
        en: "✅ <b>Subscription activated!</b>\n\nYour payment via account balance was successful. Your subscription is now active."
    },
    "checkout.invoice_toast" => { ru: "Счёт сформирован!", en: "Invoice generated!" },
    // {0} = ссылка/реквизиты
    "checkout.manual" => {
        ru: "💳 <b>Оплата вручную</b>\n\nПереведите сумму по нашим реквизитам и пришлите скриншот сюда: {0}",
        en: "💳 <b>Manual payment</b>\n\nPlease send your payment to our details and upload a screenshot to {0}"
    },
    "checkout.invoice_ready" => {
        ru: "🧾 <b>Счёт готов</b>\n\nНажмите кнопку ниже, чтобы оплатить.",
        en: "🧾 <b>Invoice generated</b>\n\nPlease click the button below to complete your payment."
    },
    "checkout.pay_now" => { ru: "🔗 Оплатить", en: "🔗 Pay now" },
    "checkout.stars_invoice_title" => { ru: "Подписка VPN", en: "VPN Subscription" },
    // {0} = сумма
    "checkout.stars_invoice_desc" => { ru: "Тариф на сумму ${0}", en: "Subscription plan for ${0}" },
    "checkout.stars_line_item" => { ru: "Подписка", en: "Subscription" },

    // =========================================================================
    // Тарифы
    // =========================================================================
    "plans.none" => { ru: "❌ Сейчас нет доступных тарифов.", en: "❌ No active plans available at the moment." },
    // {0} = название, {1} = текущий индекс, {2} = всего
    "plans.header" => { ru: "💎 <b>{0}</b> ({1}/{2})", en: "💎 <b>{0}</b> ({1}/{2})" },
    // {0} = цена, например "4.99"
    "plans.traffic_label" => { ru: "🚀 Пакет трафика — ${0}", en: "🚀 Traffic plan — ${0}" },
    // {0} = дни, {1} = цена
    "plans.duration_label" => { ru: "{0} дн — ${1}", en: "{0}d — ${1}" },
    "plans.extend_header" => { ru: "💎 <b>Выберите тариф для продления:</b>", en: "💎 <b>Choose a plan to extend:</b>" },
    "plans.default_description" => { ru: "Премиум-доступ", en: "Premium access" },

    // =========================================================================
    // Профиль
    // =========================================================================
    // {0} = telegram id, {1} = баланс
    "profile.card" => {
        ru: "👤 <b>ПРОФИЛЬ</b>\n\n🆔 ID: <code>{0}</code>\n💰 Баланс: <code>${1}</code>\n\n<i>Подписками и товарами можно управлять в разделе «Мои подписки».</i>",
        en: "👤 <b>USER PROFILE</b>\n\n🆔 ID: <code>{0}</code>\n💰 Balance: <code>${1}</code>\n\n<i>Use «My Services» to manage subscriptions and products.</i>"
    },
    "profile.topup_btn" => { ru: "💳 Пополнить баланс", en: "💳 Top up balance" },

    // =========================================================================
    // Мои подписки
    // =========================================================================
    "services.title" => { ru: "🔐 <b>МОИ ПОДПИСКИ</b>", en: "🔐 <b>MY SERVICES</b>" },
    "services.none" => { ru: "📡 Статус VPN: ❌ <b>подписок нет</b>", en: "📡 VPN status: ❌ <b>no subscriptions</b>" },
    // {0} = номер, {1} = всего
    "services.item_header" => { ru: "🔹 <b>Подписка {0}/{1}</b>", en: "🔹 <b>Subscription {0}/{1}</b>" },
    "services.plan" => { ru: "Тариф", en: "Plan" },
    "services.status" => { ru: "Статус", en: "Status" },
    "services.status_active" => { ru: "активна", en: "active" },
    "services.status_pending" => { ru: "ожидает активации", en: "pending" },
    "services.traffic" => { ru: "Трафик", en: "Traffic" },
    "services.traffic_used" => { ru: "Использовано", en: "Traffic used" },
    "services.expires" => { ru: "Действует до", en: "Expires" },
    "services.duration" => { ru: "Срок", en: "Duration" },
    "services.no_expiry" => { ru: "без ограничения по времени (пакет трафика)", en: "no expiration (traffic plan)" },
    // {0} = количество дней
    "services.days_on_activation" => { ru: "{0} дн. (отсчёт с момента активации)", en: "{0} days (starts on activation)" },
    "services.note" => { ru: "Заметка", en: "Note" },
    "services.edit_note" => { ru: "📝 Изменить заметку", en: "📝 Edit note" },
    "services.devices_btn" => { ru: "📱 Подключённые устройства", en: "📱 Connected devices" },
    "services.get_config" => { ru: "🔗 Получить конфиг", en: "🔗 Get config" },
    "services.get_links" => { ru: "🔗 Получить ссылки", en: "🔗 Get links" },
    "services.json_profile" => { ru: "📄 JSON-профиль", en: "📄 JSON profile" },
    "services.extend" => { ru: "⏳ Продлить", en: "⏳ Extend" },
    "services.activate" => { ru: "▶️ Активировать", en: "▶️ Activate" },
    "services.make_gift" => { ru: "🎁 Сделать подарочный код", en: "🎁 Make gift code" },
    "services.prev" => { ru: "⬅️ Назад", en: "⬅️ Prev" },
    "services.next" => { ru: "Вперёд ➡️", en: "Next ➡️" },
    "services.my_gifts" => { ru: "🎁 Мои подарочные коды", en: "🎁 My gift codes" },
    "services.not_found" => { ru: "❌ Подписка не найдена", en: "❌ Subscription not found" },
    "services.no_active" => { ru: "❌ У вас нет активных подписок.", en: "❌ You have no active subscriptions." },
    "services.activated_toast" => { ru: "✅ Активировано!", en: "✅ Activated!" },
    // {0} = дата
    "services.activated" => {
        ru: "🚀 <b>Подписка активирована!</b>\nДействует до: <code>{0}</code>",
        en: "🚀 <b>Subscription activated!</b>\nExpires: <code>{0}</code>"
    },
    "services.extended_toast" => { ru: "✅ Подписка продлена!", en: "✅ Extension successful!" },
    // {0} = дата
    "services.extended" => {
        ru: "✅ <b>Подписка продлена!</b>\nНовая дата окончания: <code>{0}</code>",
        en: "✅ <b>Subscription extended!</b>\nNew expiry: <code>{0}</code>"
    },
    "services.note_updated" => { ru: "✅ Заметка сохранена!", en: "✅ Note updated!" },
    // Маркер для распознавания ответа на запрос заметки. Без разметки.
    "services.note_prompt_marker" => { ru: "Заметка к подписке", en: "Note for subscription" },
    // {0} = маркер, {1} = id подписки
    "services.note_prompt" => {
        ru: "{0} #{1}.\n\nОтветьте на это сообщение текстом заметки.",
        en: "{0} #{1}.\n\nReply to this message with your note."
    },
    "services.links_none" => {
        ru: "❌ Для этой подписки пока нет ссылок подключения.",
        en: "❌ No connection links available for your subscription yet."
    },
    "services.links_header" => { ru: "🔗 <b>Ваши ссылки подключения:</b>", en: "🔗 <b>Your connection links:</b>" },
    "services.links_page" => { ru: "🌍 <b>Страница подписки:</b>", en: "🌍 <b>Subscription page:</b>" },
    "services.links_failed" => { ru: "❌ Не удалось сформировать ссылки подключения.", en: "❌ Failed to generate connection links." },
    "services.profile_generating" => { ru: "Формируем профиль…", en: "Generating profile…" },
    "services.profile_caption" => {
        ru: "📂 <b>Ваш профиль CARAMBA</b>\n\nИмпортируйте этот файл в Sing-box, Nekobox или Hiddify.\nВ нём уже есть автоподбор сервера и переключение при сбоях.",
        en: "📂 <b>Your CARAMBA profile</b>\n\nImport this file into Sing-box, Nekobox, or Hiddify.\nIt contains automatic server selection and failover."
    },
    "services.profile_failed" => { ru: "❌ Не удалось собрать файл профиля.", en: "❌ Failed to generate profile file." },

    // Автопродление
    "renew.enabled" => {
        ru: "✅ <b>Автопродление включено</b>\n\nПодписка продлится сама за 24 часа до окончания, если на балансе хватит средств.",
        en: "✅ <b>Auto-renewal enabled</b>\n\nYour subscription will automatically renew 24h before expiration if you have sufficient balance."
    },
    "renew.disabled" => {
        ru: "🔴 <b>Автопродление выключено</b>\n\nПродлевать подписку придётся вручную.",
        en: "🔴 <b>Auto-renewal disabled</b>\n\nYou'll need to manually renew your subscription when it expires."
    },
    "renew.toggle_failed" => {
        ru: "❌ Не удалось изменить настройку. Попробуйте ещё раз.",
        en: "❌ Failed to update setting. Please try again."
    },

    // =========================================================================
    // Устройства
    // =========================================================================
    // {0} = id подписки
    "devices.header" => { ru: "📱 <b>Активные устройства подписки #{0}</b>", en: "📱 <b>Active devices for subscription #{0}</b>" },
    // {0} = использовано, {1} = лимит
    "devices.limit_line" => { ru: "Лимит: <code>{0}/{1}</code> устройств", en: "Limit: <code>{0}/{1}</code> devices" },
    "devices.none_recent" => {
        ru: "За последние 15 минут активных подключений не было.",
        en: "No active sessions detected in the last 15 minutes."
    },
    // {0} = минут назад
    "devices.mins_ago" => { ru: "{0} мин назад", en: "{0} min ago" },
    // {0} = часов назад
    "devices.hours_ago" => { ru: "{0} ч назад", en: "{0} hr ago" },
    "devices.just_now" => { ru: "только что", en: "just now" },
    "devices.title" => { ru: "📱 <b>ПОДКЛЮЧЁННЫЕ УСТРОЙСТВА</b>", en: "📱 <b>CONNECTED DEVICES</b>" },
    "devices.limit_label" => { ru: "🔢 <b>Лимит устройств:</b>", en: "🔢 <b>Device limit:</b>" },
    "devices.active_label" => { ru: "✅ <b>Активных устройств:</b>", en: "✅ <b>Active devices:</b>" },
    "devices.none_connected" => {
        ru: "<i>Сейчас подключённых устройств нет.</i>\n\n<i>Они появятся здесь, как только вы подключитесь к VPN.</i>",
        en: "<i>No devices currently connected.</i>\n\n<i>Devices will appear here when you connect to the VPN.</i>"
    },
    "devices.recent_header" => { ru: "🌐 <b>Последние подключения:</b>", en: "🌐 <b>Recent connections:</b>" },
    "devices.over_limit" => { ru: "⚠️ <b>Внимание:</b> лимит устройств превышен!", en: "⚠️ <b>Warning:</b> you have exceeded your device limit!" },
    "devices.reset" => { ru: "☠️ Сбросить сессии", en: "☠️ Reset sessions" },
    "devices.back_to_services" => { ru: "« К подпискам", en: "« Back to services" },
    "devices.back" => { ru: "🔙 Назад", en: "🔙 Back" },
    "devices.reset_toast" => { ru: "✅ Сессии сброшены!", en: "✅ Sessions reset successfully!" },
    "devices.reset_done" => {
        ru: "✅ <b>Сессии сброшены</b>\n\nПодождите немного, пока соединения закроются.",
        en: "✅ <b>Sessions reset</b>\n\nPlease wait a few moments for connections to close."
    },
    "devices.select_subscription" => {
        ru: "📱 <b>Выберите подписку, чтобы посмотреть её сессии:</b>",
        en: "📱 <b>Select a subscription to manage active sessions:</b>"
    },
    // Уведомление о блокировке нового устройства. {0} = IP, {1} = лимит
    "devices.blocked_dm" => {
        ru: "📵 <b>Достигнут лимит устройств</b>\n\nНовое устройство (<code>{0}</code>) пыталось подключиться к вашей подписке, но было заблокировано.\nЛимит: <b>{1}</b> устр.\n\nОтключите одно из старых устройств в приложении, чтобы подключить новое.",
        en: "📵 <b>Device limit reached</b>\n\nA new device (<code>{0}</code>) tried to connect to your subscription but was blocked.\nLimit: <b>{1}</b> device(s).\n\nRemove an existing device in the Mini App to connect a new one."
    },

    // =========================================================================
    // Магазин и корзина
    // =========================================================================
    "store.empty" => { ru: "❌ Магазин пока пуст.", en: "❌ The store is currently empty." },
    "store.welcome" => {
        ru: "📦 <b>Магазин</b>\n\nВыберите категорию:",
        en: "📦 <b>Digital store</b>\n\nSelect a category to browse:"
    },
    "store.categories" => { ru: "📦 <b>Категории магазина:</b>", en: "📦 <b>Digital store categories:</b>" },
    "store.category_empty" => { ru: "В этой категории пусто", en: "Category is empty" },
    "store.product_not_found" => { ru: "Товар не найден", en: "Product not found" },
    "store.no_description" => { ru: "Без описания", en: "No description" },
    "store.price" => { ru: "💰 Цена:", en: "💰 Price:" },
    // {0} = цена
    "store.buy_now" => { ru: "💳 Купить (${0})", en: "💳 Buy now (${0})" },
    "store.add_to_cart" => { ru: "🛒 В корзину", en: "🛒 Add to cart" },
    "store.back_to_categories" => { ru: "🔙 К категориям", en: "🔙 Back to categories" },
    "store.back" => { ru: "🔙 Назад", en: "🔙 Back" },
    "store.purchase_ok_toast" => { ru: "✅ Оплачено!", en: "✅ Paid!" },
    // {0} = название товара, {1} = содержимое
    "store.purchase_ok" => {
        ru: "✅ <b>Покупка совершена!</b>\n\n📦 <b>{0}</b>\n\n📋 <b>Содержимое:</b>\n<code>{1}</code>",
        en: "✅ <b>Purchase successful!</b>\n\n📦 <b>{0}</b>\n\n📋 <b>Content:</b>\n<code>{1}</code>"
    },
    // {0} = название товара
    "store.purchase_ok_no_content" => {
        ru: "✅ <b>Покупка совершена!</b>\n\n📦 <b>{0}</b>\n\n(Цифрового содержимого нет — напишите в поддержку, если ожидали его.)",
        en: "✅ <b>Purchase successful!</b>\n\n📦 <b>{0}</b>\n\n(No digital content attached, contact support if expected.)"
    },
    "cart.view" => { ru: "🛒 Корзина", en: "🛒 View cart" },
    "cart.empty" => { ru: "🛒 Корзина пуста.", en: "🛒 Your cart is empty." },
    "cart.title" => { ru: "🛒 <b>ВАША КОРЗИНА</b>", en: "🛒 <b>YOUR SHOPPING CART</b>" },
    // {0} = сумма
    "cart.total" => { ru: "💰 <b>ИТОГО: ${0}</b>", en: "💰 <b>TOTAL: ${0}</b>" },
    "cart.checkout" => { ru: "✅ Оформить", en: "✅ Checkout" },
    "cart.clear" => { ru: "🗑️ Очистить корзину", en: "🗑️ Clear cart" },
    "cart.cleared_toast" => { ru: "🗑️ Корзина очищена", en: "🗑️ Cart cleared" },
    "cart.return_to_store" => { ru: "📦 Вернуться в магазин", en: "📦 Return to store" },
    "cart.continue_shopping" => { ru: "📦 Продолжить покупки", en: "📦 Continue shopping" },
    "cart.added_toast" => { ru: "🛒 Добавлено в корзину!", en: "🛒 Added to cart!" },
    "cart.checkout_ok_toast" => { ru: "✅ Заказ оформлен!", en: "✅ Checkout successful!" },
    "cart.checkout_ok" => { ru: "✅ <b>Заказ успешно оформлен!</b>", en: "✅ <b>Order processed successfully!</b>" },

    // =========================================================================
    // Подарочные и промокоды
    // =========================================================================
    // Маркер: подставляется в текст запроса и по нему же распознаётся ответ.
    "promo.redeem_marker" => { ru: "Введите подарочный код", en: "Enter your gift code" },
    // {0} = маркер
    "promo.redeem_prompt" => {
        ru: "🎟 <b>{0}</b>\n\nОтветьте на это сообщение своим кодом (например, <code>EXA-GIFT-XYZ</code>).",
        en: "🎟 <b>{0}</b>\n\nReply to this message with your code (e.g. <code>EXA-GIFT-XYZ</code>)."
    },
    // {0} = маркер
    "promo.redeem_prompt_short" => { ru: "🎟 {0}:", en: "🎟 {0}:" },
    // {0} = сообщение сервиса
    "promo.redeem_ok" => { ru: "✅ <b>Готово!</b>\n\n{0}", en: "✅ <b>Success!</b>\n\n{0}" },
    // {0} = техническая причина
    "promo.redeem_failed" => { ru: "❌ Код не принят: {0}", en: "❌ Redemption failed: {0}" },
    "promo.enter_code_btn" => { ru: "🎟 Ввести промокод", en: "🎟 Enter promo code" },
    "gift.none" => { ru: "🎁 У вас нет неиспользованных подарочных кодов.", en: "🎁 You have no unredeemed gift codes." },
    "gift.list_header" => { ru: "🎁 <b>Мои подарочные коды</b> (неиспользованные):", en: "🎁 <b>My gift codes</b> (unredeemed):" },
    "gift.days" => { ru: "Дней", en: "Days" },
    "gift.fetch_failed" => { ru: "❌ Не удалось загрузить ваши подарочные коды.", en: "❌ Failed to fetch your gift codes." },
    "gift.created_toast" => { ru: "✅ Код создан!", en: "✅ Code generated!" },
    // {0} = код
    "gift.created" => {
        ru: "🎁 <b>Подарочный код создан!</b>\n\nКод: <code>{0}</code>\n\nПередайте его кому угодно — код активируется отправкой боту.",
        en: "🎁 <b>Gift code created!</b>\n\nCode: <code>{0}</code>\n\nShare this code with anyone. They can redeem it by sending it to the bot."
    },

    // =========================================================================
    // Рефералы и бонусы
    // =========================================================================
    // {0} = число рефералов, {1} = заработано, {2} = код, {3} = ссылка
    "referral.card" => {
        ru: "🎁 <b>БОНУСНАЯ ПРОГРАММА</b>\n\n\
             🤝 <b>Приглашайте друзей и зарабатывайте!</b>\n\
             Вы получаете <b>10%</b> с <b>каждой</b> покупки приглашённого.\n\n\
             📊 <b>Ваша статистика:</b>\n\
             👥 Пришло по ссылке: <b>{0}</b>\n\
             💰 Всего заработано: <b>${1}</b>\n\n\
             🔗 <b>Ваши данные:</b>\n\
             Код: <code>{2}</code>\n\
             Ссылка: <code>{3}</code>\n\n\
             <i>Делитесь ссылкой или кодом — и получайте выплаты.</i>",
        en: "🎁 <b>BONUS PROGRAM</b>\n\n\
             🤝 <b>Invite friends and earn money!</b>\n\
             You get <b>10%</b> from <b>every</b> purchase your friends make.\n\n\
             📊 <b>Your statistics:</b>\n\
             👥 Referrals joined: <b>{0}</b>\n\
             💰 Total earned: <b>${1}</b>\n\n\
             🔗 <b>Your promo data:</b>\n\
             Code: <code>{2}</code>\n\
             Link: <code>{3}</code>\n\n\
             <i>Share your link or code to start earning.</i>"
    },
    "referral.edit_alias_btn" => { ru: "🔗 Изменить свой код", en: "🔗 Edit my code (alias)" },
    "referral.enter_referrer_btn" => { ru: "🎁 Ввести код пригласившего", en: "🎁 Enter referrer code" },
    "referral.alias_marker" => { ru: "Изменение реферального кода", en: "Edit referral alias" },
    // {0} = маркер
    "referral.alias_prompt" => {
        ru: "🔗 <b>{0}</b>\n\nОтветьте на это сообщение новым кодом.\n\n<b>Требования:</b>\n— уникальный среди всех пользователей\n— только латинские буквы, цифры и подчёркивание\n— от 3 до 32 символов",
        en: "🔗 <b>{0}</b>\n\nReply to this message with your new referral code.\n\n<b>Requirements:</b>\n— unique across all users\n— letters, numbers and underscores only\n— 3 to 32 characters"
    },
    "referral.alias_bad_length" => {
        ru: "❌ <b>Неверная длина</b>\n\nКод должен быть от 3 до 32 символов.",
        en: "❌ <b>Invalid length</b>\n\nReferral alias must be between 3 and 32 characters."
    },
    "referral.alias_bad_chars" => {
        ru: "❌ <b>Недопустимые символы</b>\n\nВ коде можно использовать только латинские буквы, цифры и подчёркивание.",
        en: "❌ <b>Invalid characters</b>\n\nReferral alias can only contain letters, numbers, and underscores."
    },
    // {0} = код, {1} = ссылка
    "referral.alias_updated" => {
        ru: "✅ <b>Код обновлён!</b>\n\nНовые данные:\nКод: <code>{0}</code>\nСсылка: <code>{1}</code>",
        en: "✅ <b>Referral alias updated!</b>\n\nYour new data:\nCode: <code>{0}</code>\nLink: <code>{1}</code>"
    },
    "referral.alias_update_failed" => {
        ru: "❌ <b>Не сохранилось</b>\n\nТакой код уже занят или содержит недопустимые символы.",
        en: "❌ <b>Update failed</b>\n\nThis alias might already be taken or invalid."
    },
    "referral.referrer_marker" => { ru: "Код пригласившего", en: "Enter referrer code" },
    // {0} = маркер
    "referral.referrer_prompt" => {
        ru: "🎁 <b>{0}</b>\n\nОтветьте на это сообщение кодом того, кто вас пригласил.",
        en: "🎁 <b>{0}</b>\n\nReply to this message with the referral code of the person who invited you."
    },
    "referral.referrer_linked" => {
        ru: "✅ <b>Пригласивший записан!</b>\n\nКод успешно применён.",
        en: "✅ <b>Referrer linked!</b>\n\nYou've successfully set your referrer."
    },
    // {0} = техническая причина
    "referral.referrer_link_failed" => { ru: "❌ Не удалось привязать код: {0}", en: "❌ Linking failed: {0}" },
    "referral.new_referral_dm" => {
        ru: "👤 Новый реферал! По вашей ссылке зарегистрировался пользователь.",
        en: "👤 New referral! Someone just signed up through your link."
    },
    // {0} = сумма
    "referral.bonus_dm" => {
        ru: "🎉 <b>Реферальный бонус</b>\n\nДруг пришёл по вашей ссылке — на баланс зачислено <b>+${0}</b>.",
        en: "🎉 <b>Referral bonus</b>\n\nYour friend joined via your referral link — <b>+${0}</b> added to your balance."
    },
    // {0} = сумма
    "referral.welcome_bonus_dm" => {
        ru: "🎁 <b>Приветственный бонус</b>\n\nНа ваш баланс зачислено <b>+${0}</b> за регистрацию по приглашению.",
        en: "🎁 <b>Welcome bonus</b>\n\n<b>+${0}</b> has been added to your balance as a referral welcome gift."
    },
    "leaderboard.empty" => { ru: "🏆 <b>Рейтинг пока пуст</b>", en: "🏆 <b>Leaderboard is empty</b>" },
    "leaderboard.header" => { ru: "🏆 <b>Топ пригласивших</b>", en: "🏆 <b>Top referrers</b>" },
    "leaderboard.refs_suffix" => { ru: "приглашений", en: "refs" },
    "leaderboard.footer" => { ru: "<i>Приглашайте друзей и поднимайтесь в рейтинге!</i>", en: "<i>Invite friends to climb the ranks!</i>" },

    // =========================================================================
    // Перенос подписки
    // =========================================================================
    "transfer.marker" => { ru: "Перенос подписки", en: "Transfer subscription" },
    // {0} = маркер, {1} = id подписки
    "transfer.prompt" => {
        ru: "➡️ <b>{0}</b>\n\nОтветьте на это сообщение именем пользователя (например, @username), которому нужно передать подписку #{1}.",
        en: "➡️ <b>{0}</b>\n\nReply to this message with the username (e.g. @username) you want to transfer subscription #{1} to."
    },
    // {0} = id подписки, {1} = получатель
    "transfer.ok" => { ru: "✅ Подписка #{0} передана пользователю {1}!", en: "✅ Subscription #{0} transferred to {1}!" },
    // {0} = техническая причина
    "transfer.failed" => { ru: "❌ Не удалось передать подписку: {0}", en: "❌ Transfer failed: {0}" },

    // =========================================================================
    // Поддержка и вход в приложение
    // =========================================================================
    "support.not_configured" => { ru: "❌ Контакт поддержки ещё не настроен.", en: "❌ Support contact is not configured yet." },
    "support.prompt" => { ru: "Нужна помощь? Нажмите кнопку ниже:", en: "Need help? Click the button below to contact support:" },
    "support.contact_btn" => { ru: "💬 Написать в поддержку", en: "💬 Contact support" },
    "login.code_failed" => { ru: "⚠️ Не удалось создать код. Попробуйте чуть позже.", en: "⚠️ Could not generate a code. Please try again later." },
    // {0} = шестизначный код
    "login.code" => {
        ru: "🔑 <b>Ваш код для входа</b>\n\n<code>{0}</code>\n\nВведите его в приложении. Код действует 5 минут и срабатывает один раз.",
        en: "🔑 <b>Your login code</b>\n\n<code>{0}</code>\n\nEnter it in the app. The code is valid for 5 minutes and works once."
    },
    "login.get_code_btn" => { ru: "🔑 Получить код для входа", en: "🔑 Get login code" },

    // =========================================================================
    // Уведомления: подписка, трафик, баланс
    // =========================================================================
    // {0} = название тарифа
    "notify.expired" => {
        ru: "⏰ Подписка «{0}» закончилась. Продлите её, чтобы VPN снова заработал.",
        en: "⏰ Your subscription «{0}» has expired. Renew to keep your VPN access."
    },
    // {0} = тариф, {1} = дата, {2} = сумма
    "notify.renewed" => {
        ru: "✅ <b>Подписка продлена автоматически</b>\n\n💎 Тариф: <b>{0}</b>\n📅 Действует до: <b>{1}</b>\n💳 Списано: <b>${2}</b>",
        en: "✅ <b>Subscription auto-renewed</b>\n\n💎 Plan: <b>{0}</b>\n📅 Valid until: <b>{1}</b>\n💳 Charged: <b>${2}</b>"
    },
    // Карточка в приложении для "notify.expired". Раньше её текст был зашит
    // по-английски прямо в monitoring.rs — единственное уведомление, которое
    // игнорировало язык пользователя. {0} = тариф, как и у самого notify.expired.
    "notify.expired_title" => { ru: "Подписка закончилась", en: "Subscription expired" },
    "notify.expired_body" => {
        ru: "Тариф «{0}» больше не действует. Продлите его, чтобы VPN снова заработал.",
        en: "Your «{0}» subscription has expired. Renew to keep your VPN access."
    },
    // Карточка для "notify.sni_rotation". Само сообщение уходит отдельным путём
    // (notification_service), но инбокс приложения строится по общей форме
    // <base>/_title/_body, и её нужно соблюсти. {0}/{1} = домены, {2} = id.
    "notify.sni_rotation_title" => { ru: "Нужно переподключиться", en: "Reconnection required" },
    "notify.sni_rotation_body" => {
        ru: "Настройки VPN обновлены: {0} → {1}. Переподключитесь в приложении.",
        en: "Your VPN settings changed: {0} → {1}. Reconnect in the app."
    },
    "notify.renewed_title" => { ru: "Подписка автопродлена", en: "Subscription auto-renewed" },
    // {0} = тариф, {1} = сумма, {2} = дата
    // Порядок подстановок намеренно тот же, что и у "notify.renewed" выше:
    // {0} тариф, {1} дата, {2} сумма. Раньше здесь были {1} сумма и {2} дата, и
    // это работало ровно до тех пор, пока обе строки заполнялись разными
    // вызовами. Теперь текст, заголовок и тело собираются из одного шаблона с
    // одним набором аргументов, а редактор в панели показывает оператору одну
    // подпись на каждый индекс — разный смысл {1} в двух половинах одного
    // уведомления был бы ловушкой для того, кто его правит.
    "notify.renewed_body" => { ru: "Тариф «{0}» — ${2} · до {1}", en: "Plan «{0}» — ${2} · until {1}" },
    // {0} = тариф, {1} = баланс, {2} = требуется
    // Просить пополнить баланс нельзя: пополнить его в этом продукте нечем —
    // экран баланса только показывает сумму, формы и API пополнения не
    // существует. Единственное действие, которое человек реально может
    // совершить, — оплатить тариф напрямую, туда и ведём словами и кнопкой.
    "notify.renew_failed" => {
        ru: "⚠️ <b>Автопродление не выполнено</b>\n\n💎 Тариф: <b>{0}</b>\n💰 На счету: <b>${1}</b>\n💳 Требовалось: <b>${2}</b>\n\nПродлите подписку, чтобы не потерять доступ.",
        en: "⚠️ <b>Auto-renewal failed</b>\n\n💎 Plan: <b>{0}</b>\n💰 On your account: <b>${1}</b>\n💳 Needed: <b>${2}</b>\n\nRenew your subscription to keep your access."
    },
    "notify.renew_failed_title" => { ru: "Автопродление не выполнено", en: "Auto-renewal failed" },
    // {0} = тариф, {1} = баланс, {2} = требуется
    "notify.renew_failed_body" => {
        ru: "Тариф «{0}»: на счету ${1}, требовалось ${2}. Продлите вручную.",
        en: "Plan «{0}»: ${1} on account, ${2} needed. Renew manually."
    },
    // {0} = баланс, {1} = тариф
    "notify.low_balance" => {
        ru: "⚠️ <b>Автопродление не сработает</b>\n\nНа счету: <b>${0}</b> — этого не хватит, чтобы продлить «{1}» автоматически.\n\nПродлите подписку вручную, чтобы не остаться без доступа.",
        en: "⚠️ <b>Auto-renewal won't go through</b>\n\nOn your account: <b>${0}</b> — not enough to renew «{1}» automatically.\n\nRenew manually so you don't lose access."
    },
    "notify.low_balance_title" => { ru: "Автопродление не сработает", en: "Auto-renewal won't go through" },
    // {0} = баланс, {1} = тариф
    "notify.low_balance_body" => {
        ru: "На счету ${0} — не хватит на «{1}». Продлите вручную.",
        en: "${0} on account — not enough for «{1}». Renew manually."
    },
    "notify.traffic80" => {
        ru: "⚠️ <b>Трафик на исходе</b>\n\nИзрасходовано <b>80%</b> месячного трафика.\nПодумайте о переходе на тариф побольше, чтобы не остаться без связи.",
        en: "⚠️ <b>Traffic warning</b>\n\nYou've used <b>80%</b> of your monthly traffic.\nConsider upgrading your plan to avoid interruption."
    },
    "notify.traffic80_title" => { ru: "Трафик на исходе", en: "Traffic warning" },
    "notify.traffic80_body" => {
        ru: "Израсходовано 80% месячного трафика. Возможно, стоит перейти на тариф побольше.",
        en: "You've used 80% of your monthly traffic. Consider upgrading your plan."
    },
    "notify.traffic90" => {
        ru: "🔶 <b>Трафик почти закончился</b>\n\nИзрасходовано <b>90%</b> трафика.\n<i>При достижении лимита доступ приостановится.</i>",
        en: "🔶 <b>Traffic critical</b>\n\nYou've used <b>90%</b> of your traffic.\n<i>Access will be paused when the limit is reached.</i>"
    },
    "notify.traffic90_title" => { ru: "Трафик почти закончился", en: "Traffic critical" },
    "notify.traffic90_body" => {
        ru: "Израсходовано 90% трафика. На 100% доступ приостановится.",
        en: "You've used 90% of your traffic. Access will be paused at 100%."
    },
    "notify.traffic_exceeded" => {
        ru: "🔴 <b>Трафик закончился</b>\n\nЛимит исчерпан, доступ приостановлен.\nПерейдите на тариф побольше или дождитесь суточного пополнения.",
        en: "🔴 <b>Traffic limit reached</b>\n\nYour traffic quota is exhausted and access has been paused.\nUpgrade or wait for the daily top-up to resume."
    },
    "notify.traffic_exceeded_title" => { ru: "Трафик закончился", en: "Traffic limit reached" },
    "notify.traffic_exceeded_body" => {
        ru: "Лимит трафика исчерпан, доступ приостановлен. Перейдите на другой тариф или дождитесь суточного пополнения.",
        en: "Your traffic quota is exhausted and access has been paused. Upgrade or wait for the daily top-up."
    },
    "notify.expiry3" => {
        ru: "⏰ <b>Подписка скоро закончится</b>\n\nОсталось <b>3 дня</b>.\nПродлите сейчас, чтобы не остаться без VPN.",
        en: "⏰ <b>Expiry alert</b>\n\nYour subscription expires in <b>3 days</b>.\nRenew now to avoid interruption."
    },
    "notify.expiry3_title" => { ru: "Подписка закончится через 3 дня", en: "Subscription expires in 3 days" },
    "notify.expiry3_body" => { ru: "Продлите сейчас, чтобы не остаться без VPN.", en: "Renew now to avoid interruption." },

    // Ротация SNI. {0} = старый домен, {1} = новый домен, {2} = id ротации
    "notify.sni_rotation" => {
        ru: "⚠️ <b>Нужно переподключиться</b>\n\n\
             Мы автоматически обновили настройки вашего VPN, чтобы соединение было стабильнее.\n\n\
             <b>Старый домен:</b> <code>{0}</code>\n\
             <b>Новый домен:</b> <code>{1}</code>\n\n\
             <b>📱 Что сделать:</b>\n\
             1️⃣ Отключите VPN\n\
             2️⃣ Подождите 10 секунд\n\
             3️⃣ Подключитесь снова\n\n\
             Новая конфигурация уже готова — перекачивать ничего не нужно.\n\n\
             <i>ID ротации: #{2}</i>",
        en: "⚠️ <b>Connection update required</b>\n\n\
             Your VPN configuration has been automatically updated for improved stability.\n\n\
             <b>Previous domain:</b> <code>{0}</code>\n\
             <b>New domain:</b> <code>{1}</code>\n\n\
             <b>📱 Action required:</b>\n\
             1️⃣ Disconnect from VPN\n\
             2️⃣ Wait 10 seconds\n\
             3️⃣ Reconnect to VPN\n\n\
             Your new configuration is ready. No need to re-download.\n\n\
             <i>Rotation ID: #{2}</i>"
    },
}

/// Подстановка `{0}`, `{1}`, … в произвольный шаблон.
///
/// Вынесена из [`tf`] отдельной чистой функцией, потому что тот же шаблон может
/// прийти не из таблицы переводов, а из БД — редактируемые шаблоны уведомлений
/// (`services::notification_templates`). Обе дороги обязаны подставлять
/// одинаково: разъехавшись, они дали бы уведомление, которое в предпросмотре
/// выглядит правильно, а пользователю уходит с дословным `{0}`.
///
/// Индексы, для которых аргумента нет, остаются в тексте как есть. Для
/// встроенных строк это невозможно по построению, а для отредактированных
/// защита стоит на сохранении — см. `NotifyEvent::validate_placeholders`.
pub fn substitute(template: &str, args: &[&str]) -> String {
    let mut result = template.to_string();
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{}}}", i), arg);
    }
    result
}

/// Форматирование строки перевода с подстановкой `{0}`, `{1}`, … .
pub fn tf(lang: Lang, key: &str, args: &[&str]) -> String {
    substitute(t(lang, key), args)
}

/// Совпадает ли `text` (ровно) со значением ключа в любом поддерживаемом языке.
///
/// Нужно для reply-клавиатуры: нажатие кнопки приходит боту обычным текстом, и
/// пользователь мог получить клавиатуру на одном языке, а переключиться на
/// другой — значит распознавать надо оба варианта.
pub fn matches_any_lang(text: &str, key: &str) -> bool {
    [Lang::Ru, Lang::En]
        .iter()
        .any(|&lang| t(lang, key) == text)
}

/// Содержит ли `text` значение ключа в любом поддерживаемом языке.
///
/// Используется для распознавания ответов (`reply_to_message`) на наши же
/// подсказки: в тексте подсказки сидит короткий маркер без разметки, и по нему
/// определяется, что именно пользователь заполняет.
pub fn contains_any_lang(text: &str, key: &str) -> bool {
    [Lang::Ru, Lang::En].iter().any(|&lang| {
        let marker = t(lang, key);
        marker != MISSING && text.contains(marker)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- разрешение языка -------------------------------------------------

    #[test]
    fn explicit_user_language_wins_over_setting() {
        assert_eq!(resolve_lang(Some("en"), Some("ru")), Lang::En);
        assert_eq!(resolve_lang(Some("ru"), Some("en")), Lang::Ru);
    }

    #[test]
    fn full_locale_tags_are_understood() {
        assert_eq!(resolve_lang(Some("ru-RU"), None), Lang::Ru);
        assert_eq!(resolve_lang(Some("en_US"), None), Lang::En);
        assert_eq!(resolve_lang(Some("EN-GB"), None), Lang::En);
    }

    #[test]
    fn unknown_or_garbage_user_language_falls_back_to_setting() {
        assert_eq!(resolve_lang(Some("de"), Some("en")), Lang::En);
        assert_eq!(resolve_lang(Some(""), Some("en")), Lang::En);
        assert_eq!(resolve_lang(Some("!!!"), Some("en")), Lang::En);
        assert_eq!(resolve_lang(None, Some("en")), Lang::En);
    }

    #[test]
    fn missing_setting_falls_back_to_russian() {
        assert_eq!(resolve_lang(None, None), Lang::Ru);
        assert_eq!(resolve_lang(Some("de"), None), Lang::Ru);
        // Настройка тоже может содержать мусор — тогда всё равно русский.
        assert_eq!(resolve_lang(Some("de"), Some("klingon")), Lang::Ru);
        assert_eq!(resolve_lang(None, Some("")), Lang::Ru);
    }

    #[test]
    fn default_is_russian() {
        assert_eq!(Lang::DEFAULT, Lang::Ru);
        assert_eq!(Lang::Ru.as_str(), "ru");
        assert_eq!(Lang::En.as_str(), "en");
    }

    // ----- полнота таблицы --------------------------------------------------

    /// Ключ невозможно добавить только в один язык: [`KEYS`] выводится из той же
    /// таблицы, что и [`t`], поэтому список здесь не поддерживается руками.
    #[test]
    fn every_key_has_both_languages() {
        assert!(!KEYS.is_empty(), "translation table is empty");
        for key in KEYS {
            for lang in [Lang::Ru, Lang::En] {
                let value = t(lang, key);
                assert_ne!(value, MISSING, "key `{key}` is missing for {lang:?}");
                assert!(
                    !value.trim().is_empty(),
                    "key `{key}` is empty for {lang:?}"
                );
            }
        }
    }

    #[test]
    fn keys_are_unique() {
        let mut sorted: Vec<&str> = KEYS.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(
            before,
            sorted.len(),
            "duplicate key in the translation table"
        );
    }

    #[test]
    fn unknown_key_yields_missing_marker() {
        assert_eq!(t(Lang::Ru, "no.such.key"), MISSING);
        assert_eq!(t(Lang::En, "no.such.key"), MISSING);
    }

    /// Русский текст обязан отличаться от английского — иначе перевод забыли.
    /// Исключения — строки, одинаковые в обоих языках по существу (бренды,
    /// чистая пунктуация с плейсхолдерами).
    #[test]
    fn russian_differs_from_english() {
        const SAME_BY_DESIGN: &[&str] = &[
            "topup.method_stars", // «⭐️ Telegram Stars» — бренд
            "plans.header",       // «💎 <b>{0}</b> ({1}/{2})» — только плейсхолдеры
            "promo.redeem_prompt_short",
        ];
        for key in KEYS {
            if SAME_BY_DESIGN.contains(key) {
                continue;
            }
            assert_ne!(
                t(Lang::Ru, key),
                t(Lang::En, key),
                "key `{key}` has identical RU and EN text — untranslated?"
            );
        }
    }

    // ----- корректность разметки --------------------------------------------

    /// Строки уходят в Telegram с `parse_mode=HTML`, а Telegram отклоняет
    /// СООБЩЕНИЕ ЦЕЛИКОМ при неизвестном или незакрытом теге. Непоказанное
    /// сообщение хуже английского, поэтому разметку проверяем тестом:
    /// каждый `<...>` — тег из белого списка, все теги закрыты и вложены.
    #[test]
    fn html_markup_is_well_formed() {
        // Теги, которые Telegram понимает в HTML parse mode и которые мы
        // реально используем.
        const ALLOWED: &[&str] = &["b", "i", "u", "s", "code", "pre", "a"];

        for key in KEYS {
            for lang in [Lang::Ru, Lang::En] {
                let text = t(lang, key);
                let mut stack: Vec<String> = Vec::new();
                let mut rest = text;

                while let Some(open_at) = rest.find('<') {
                    let after = &rest[open_at + 1..];
                    let close_at = after.find('>').unwrap_or_else(|| {
                        panic!("key `{key}` ({lang:?}): unterminated `<` in: {text}")
                    });
                    let raw = &after[..close_at];
                    rest = &after[close_at + 1..];

                    let (is_closing, body) = match raw.strip_prefix('/') {
                        Some(b) => (true, b),
                        None => (false, raw),
                    };
                    // `<a href="...">` — имя тега до первого пробела.
                    let name = body.split_whitespace().next().unwrap_or("").to_string();

                    assert!(
                        ALLOWED.contains(&name.as_str()),
                        "key `{key}` ({lang:?}): tag `<{name}>` is not allowed by Telegram HTML: {text}"
                    );

                    if is_closing {
                        let top = stack.pop().unwrap_or_else(|| {
                            panic!(
                                "key `{key}` ({lang:?}): `</{name}>` with no opening tag: {text}"
                            )
                        });
                        assert_eq!(
                            top, name,
                            "key `{key}` ({lang:?}): `</{name}>` closes `<{top}>`: {text}"
                        );
                    } else {
                        stack.push(name);
                    }
                }

                assert!(
                    stack.is_empty(),
                    "key `{key}` ({lang:?}): unclosed tag(s) {stack:?} in: {text}"
                );
            }
        }
    }

    /// Тексты для `answer_callback_query` и pre-checkout Telegram показывает
    /// во всплывающем окне БЕЗ parse mode — разметка там протекла бы как есть.
    #[test]
    fn plain_text_keys_carry_no_markup() {
        const PLAIN: &[&str] = &[
            "precheckout.bad_amount",
            "precheckout.bad_payload",
            "precheckout.session_missing",
            "precheckout.session_foreign",
            "precheckout.session_closed",
            "precheckout.not_stars",
            "precheckout.amount_mismatch",
            "precheckout.restricted",
            "precheckout.no_account",
            "precheckout.unavailable",
            "checkout.failed",
            "checkout.fulfillment_failed",
            "checkout.insufficient_balance",
            "checkout.charge_failed",
            "checkout.paid_toast",
            "checkout.invoice_toast",
            "checkout.choose_method_toast",
            "checkout.invalid_plan",
            "checkout.invalid_duration",
            "checkout.stars_unavailable",
            "services.activated_toast",
            "services.extended_toast",
            "services.not_found",
            "services.note_updated",
            "gift.created_toast",
            "cart.added_toast",
            "cart.cleared_toast",
            "cart.checkout_ok_toast",
            "store.purchase_ok_toast",
            "devices.reset_toast",
            "terms.must_accept",
            "error.not_implemented",
            "referral.new_referral_dm",
        ];
        for key in PLAIN {
            for lang in [Lang::Ru, Lang::En] {
                let text = t(lang, key);
                assert_ne!(text, MISSING, "key `{key}` is missing for {lang:?}");
                assert!(
                    !text.contains('<') && !text.contains('>'),
                    "key `{key}` ({lang:?}) is sent as plain text but contains markup: {text}"
                );
            }
        }
    }

    // ----- форматирование ---------------------------------------------------

    #[test]
    fn tf_substitutes_positional_args() {
        let ru = tf(Lang::Ru, "label_order", &["42"]);
        assert_eq!(ru, "заказ №42");
        let en = tf(Lang::En, "label_order", &["42"]);
        assert_eq!(en, "order #42");
    }

    #[test]
    fn tf_leaves_unused_placeholders_alone() {
        // Недостающий аргумент не должен паниковать.
        let out = tf(Lang::Ru, "invoice_created", &["Тариф"]);
        assert!(out.contains("Тариф"));
        assert!(out.contains("{1}"));
    }

    // ----- распознавание кнопок и ответов -----------------------------------

    #[test]
    fn menu_labels_match_in_both_languages() {
        assert!(matches_any_lang("🛍 Купить подписку", "menu.buy"));
        assert!(matches_any_lang("🛍 Buy Subscription", "menu.buy"));
        assert!(!matches_any_lang("🛍 Buy Something Else", "menu.buy"));
    }

    #[test]
    fn reply_markers_are_found_in_both_languages() {
        let ru_prompt = tf(
            Lang::Ru,
            "promo.redeem_prompt",
            &[t(Lang::Ru, "promo.redeem_marker")],
        );
        let en_prompt = tf(
            Lang::En,
            "promo.redeem_prompt",
            &[t(Lang::En, "promo.redeem_marker")],
        );
        assert!(contains_any_lang(&ru_prompt, "promo.redeem_marker"));
        assert!(contains_any_lang(&en_prompt, "promo.redeem_marker"));
        assert!(!contains_any_lang("что-то другое", "promo.redeem_marker"));
    }

    /// Маркеры распознаются в тексте, который Telegram уже отрендерил, поэтому
    /// в них не должно быть символов разметки HTML.
    #[test]
    fn markers_contain_no_markup() {
        const MARKERS: &[&str] = &[
            "promo.redeem_marker",
            "referral.alias_marker",
            "referral.referrer_marker",
            "transfer.marker",
            "services.note_prompt_marker",
        ];
        for key in MARKERS {
            for lang in [Lang::Ru, Lang::En] {
                let marker = t(lang, key);
                assert!(
                    !marker.contains('<') && !marker.contains('>') && !marker.contains('&'),
                    "marker `{key}` ({lang:?}) contains markup: {marker}"
                );
            }
        }
    }
}

//! Переводы DM-сообщений панельного бота (платёжный поток Mini App).
//!
//! Портирует паттерн `t()`/`tf()` из apps/caramba-bot/src/bot/translations.rs:
//! русский — язык по умолчанию (lang = None или "ru*"), для остальных кодов —
//! английский. Ключи — только для НОВЫХ сообщений платёжного потока; уже
//! существующие захардкоженные строки панельного бота намеренно не трогаем.

/// Get translated string. `lang` is user's language_code (e.g. Some("ru"), Some("en"), None).
/// Returns Russian for "ru*" (and None — default), English for everything else.
pub fn t(lang: Option<&str>, key: &str) -> &'static str {
    let is_ru = lang.is_none_or(|l| l.starts_with("ru")); // Default to Russian
    match (key, is_ru) {
        // =====================================================================
        // Payment flow (Mini App checkout DMs)
        // =====================================================================
        // {0} = plan/product label, {1} = amount, {2} = currency
        ("invoice_created", true) => {
            "Счёт на оплату — {0}, {1} {2}. Оплатите по кнопке; после оплаты вернитесь в приложение."
        }
        ("invoice_created", false) => {
            "Payment invoice — {0}, {1} {2}. Pay via the button below; return to the app once you're done."
        }

        ("pay_button", true) => "💳 Оплатить",
        ("pay_button", false) => "💳 Pay",

        ("payment_success_sub", true) => "✅ Оплата получена — подписка активирована.",
        ("payment_success_sub", false) => "✅ Payment received — your subscription is active.",

        ("payment_success_order", true) => "✅ Оплата получена — заказ оплачен.",
        ("payment_success_order", false) => "✅ Payment received — your order is paid.",

        ("open_app_button", true) => "📱 Открыть приложение",
        ("open_app_button", false) => "📱 Open App",

        // Метки продуктов для invoice_created, когда имя недоступно.
        // {0} = numeric id
        ("label_order", true) => "заказ №{0}",
        ("label_order", false) => "order #{0}",

        ("label_subscription", true) => "подписка",
        ("label_subscription", false) => "subscription",

        // Fallback - unknown key
        (_, _) => "???",
    }
}

/// Format version for strings with placeholders - returns String
pub fn tf(lang: Option<&str>, key: &str, args: &[&str]) -> String {
    let template = t(lang, key);
    let mut result = template.to_string();
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{}}}", i), arg);
    }
    result
}

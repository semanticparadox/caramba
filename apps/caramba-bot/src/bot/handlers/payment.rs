use crate::models::payment::PaymentType;
use crate::AppState;
use teloxide::prelude::*;
use teloxide::types::PreCheckoutQuery;
use tracing::{error, info, warn};

/// Максимально допустимая сумма в Stars (XTR).
/// 10 000 XTR ≈ $200 — разумный верхний предел для пополнения баланса.
const MAX_ALLOWED_XTR: u32 = 10_000;

pub async fn pre_checkout_handler(
    bot: Bot,
    q: PreCheckoutQuery,
    state: AppState,
) -> Result<(), teloxide::RequestError> {
    let query_id = q.id.clone();
    let tg_id = q.from.id.0 as i64;

    info!(
        tg_id,
        payload = %q.invoice_payload,
        amount = q.total_amount,
        currency = %q.currency,
        "Pre-checkout validation started"
    );

    // Хелпер: отклоняем инвойс с сообщением и пишем причину в лог
    let reject = |reason: &str| {
        warn!(tg_id, payload = %q.invoice_payload, reason, "Pre-checkout rejected");
    };

    // 1. Разбираем payload — должен соответствовать формату, заданному при создании инвойса
    let parsed = match PaymentType::from_payload(&q.invoice_payload) {
        Some(p) => p,
        None => {
            reject("invalid payload format");
            return bot
                .answer_pre_checkout_query(query_id, false)
                .error_message("Invalid payment data. Please try again.")
                .await
                .map(|_| ());
        }
    };

    // 2. User_id в payload должен совпадать с тем, кто нажал «Оплатить»
    if parsed.user_id != tg_id {
        reject("payload user_id mismatch");
        return bot
            .answer_pre_checkout_query(query_id, false)
            .error_message("Payment session mismatch. Please restart the payment.")
            .await
            .map(|_| ());
    }

    // 3. Пользователь должен существовать и не быть заблокирован
    match state.store_service.get_user_by_tg_id(tg_id).await {
        Ok(Some(user)) if user.is_banned => {
            reject("user is banned");
            return bot
                .answer_pre_checkout_query(query_id, false)
                .error_message("Your account has been suspended.")
                .await
                .map(|_| ());
        }
        Ok(None) => {
            reject("user not found");
            return bot
                .answer_pre_checkout_query(query_id, false)
                .error_message("User account not found. Please send /start and try again.")
                .await
                .map(|_| ());
        }
        Err(e) => {
            error!(tg_id, error = %e, "DB error during pre-checkout user lookup");
            return bot
                .answer_pre_checkout_query(query_id, false)
                .error_message("Service temporarily unavailable. Please try again later.")
                .await
                .map(|_| ());
        }
        Ok(Some(_)) => {} // пользователь найден и не заблокирован — продолжаем
    }

    // 4. Сумма должна быть положительной и не превышать максимум
    let amount = q.total_amount;
    if amount == 0 {
        reject("zero amount");
        return bot
            .answer_pre_checkout_query(query_id, false)
            .error_message("Payment amount must be greater than zero.")
            .await
            .map(|_| ());
    }
    if amount > MAX_ALLOWED_XTR {
        reject("amount exceeds maximum");
        return bot
            .answer_pre_checkout_query(query_id, false)
            .error_message("Payment amount exceeds the allowed limit.")
            .await
            .map(|_| ());
    }

    // Все проверки пройдены — подтверждаем
    info!(tg_id, amount, "Pre-checkout approved");
    bot.answer_pre_checkout_query(query_id, true).await?;
    Ok(())
}

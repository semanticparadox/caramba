use teloxide::prelude::*;
use teloxide::types::{PreCheckoutQuery, PreCheckoutQueryId};

use crate::AppState;

/// Telegram Stars (XTR) per-invoice safety cap. Normal top-ups/plans are far
/// below this; the cap only blocks absurd or forged amounts.
const MAX_XTR_PER_INVOICE: u32 = 1_000_000;

async fn reject(
    bot: &Bot,
    id: PreCheckoutQueryId,
    reason: &str,
) -> Result<(), teloxide::RequestError> {
    bot.answer_pre_checkout_query(id, false)
        .error_message(reason.to_string())
        .await?;
    Ok(())
}

/// Pre-checkout gate for Telegram Stars payments.
///
/// Telegram requires an answer within ~10s, after which the charge proceeds.
/// The previous handler blindly accepted EVERY invoice, so a banned user could
/// still pay, a malformed/forged payload would be accepted, and there was no
/// amount sanity check. We now validate the payer, payload ownership, ban state
/// and amount, rejecting with a user-visible reason so we never take a charge we
/// cannot (or must not) fulfill.
pub async fn pre_checkout_handler(
    bot: Bot,
    q: PreCheckoutQuery,
    state: AppState,
) -> Result<(), teloxide::RequestError> {
    // 1. Amount sanity.
    if q.total_amount == 0 || q.total_amount > MAX_XTR_PER_INVOICE {
        return reject(
            &bot,
            q.id.clone(),
            "Invalid payment amount. Please try again.",
        )
        .await;
    }

    // 2. Payload must be "{user_id}:{type}:{target}" or a bare numeric user_id.
    let parts: Vec<&str> = q.invoice_payload.split(':').collect();
    let raw_uid = if parts.len() >= 3 {
        parts[0]
    } else {
        q.invoice_payload.as_str()
    };
    let payload_user_id = match raw_uid.parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => {
            return reject(
                &bot,
                q.id.clone(),
                "Malformed payment session. Please start over.",
            )
            .await;
        }
    };

    // 3. Payer must exist, own the payload, and not be banned. The payload carries
    //    the internal DB user id; compare it against the user resolved from the
    //    Telegram id of whoever is actually paying.
    let tg_id = q.from.id.0 as i64;
    match state.store_service.get_user_by_tg_id(tg_id).await {
        Ok(Some(user)) => {
            if user.id != payload_user_id {
                return reject(
                    &bot,
                    q.id.clone(),
                    "Payment session does not match your account.",
                )
                .await;
            }
            if user.is_banned {
                return reject(&bot, q.id.clone(), "Your account is restricted.").await;
            }
        }
        Ok(None) => {
            return reject(
                &bot,
                q.id.clone(),
                "Account not found. Send /start and try again.",
            )
            .await;
        }
        Err(_) => {
            return reject(
                &bot,
                q.id.clone(),
                "Service temporarily unavailable. Please try again.",
            )
            .await;
        }
    }

    bot.answer_pre_checkout_query(q.id, true).await?;
    Ok(())
}

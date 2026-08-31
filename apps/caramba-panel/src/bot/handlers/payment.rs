use teloxide::prelude::*;
use teloxide::types::{PreCheckoutQuery, PreCheckoutQueryId};

use crate::AppState;
use crate::services::payment::stars::{
    MAX_STARS_PER_INVOICE, parse_session_invoice_payload, session_star_amount,
};

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
    if q.total_amount == 0 || i64::from(q.total_amount) > MAX_STARS_PER_INVOICE {
        return reject(
            &bot,
            q.id.clone(),
            "Invalid payment amount. Please try again.",
        )
        .await;
    }

    // 1b. Mini App / marketplace invoices carry `sess:{uuid}` (StarsProvider).
    //     They are validated against the stored payment session rather than the
    //     legacy "{user_id}:{type}:{target}" payload shape — without this branch
    //     every Mini App Stars checkout would be rejected as malformed.
    if let Some(session_id) = parse_session_invoice_payload(&q.invoice_payload) {
        return pre_checkout_session(&bot, q, state, session_id).await;
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

/// Pre-checkout validation for a Mini App Stars invoice (`sess:{uuid}`).
///
/// Same guarantees as the legacy branch — the payer must exist, own the
/// purchase and not be banned — but resolved through the payment session, plus
/// two extra checks the legacy payload cannot express: the session must still
/// be open, and the amount must match what the invoice was priced at (computed
/// with the same helper `StarsProvider` used, so no rounding drift).
async fn pre_checkout_session(
    bot: &Bot,
    q: PreCheckoutQuery,
    state: AppState,
    session_id: uuid::Uuid,
) -> Result<(), teloxide::RequestError> {
    let session = match state
        .marketplace_service
        .session_repo
        .get_by_id(session_id)
        .await
    {
        Ok(Some(session)) => session,
        Ok(None) => {
            return reject(
                bot,
                q.id.clone(),
                "Payment session not found. Please start over.",
            )
            .await;
        }
        Err(_) => {
            return reject(
                bot,
                q.id.clone(),
                "Service temporarily unavailable. Please try again.",
            )
            .await;
        }
    };

    if session.provider != "stars" {
        return reject(bot, q.id.clone(), "This invoice is not payable with Stars.").await;
    }

    // Already paid / expired / failed: refuse rather than take a charge we would
    // have to refund (fulfill_payment's atomic claim would drop it anyway).
    if session.status != "pending" {
        return reject(
            bot,
            q.id.clone(),
            "This invoice is no longer payable. Please start over.",
        )
        .await;
    }

    let tg_id = q.from.id.0 as i64;
    match state.store_service.get_user_by_tg_id(tg_id).await {
        Ok(Some(user)) => {
            if user.id != session.user_id {
                return reject(
                    bot,
                    q.id.clone(),
                    "Payment session does not match your account.",
                )
                .await;
            }
            if user.is_banned {
                return reject(bot, q.id.clone(), "Your account is restricted.").await;
            }
        }
        Ok(None) => {
            return reject(
                bot,
                q.id.clone(),
                "Account not found. Send /start and try again.",
            )
            .await;
        }
        Err(_) => {
            return reject(
                bot,
                q.id.clone(),
                "Service temporarily unavailable. Please try again.",
            )
            .await;
        }
    }

    match session_star_amount(&session) {
        Ok(expected) if i64::from(q.total_amount) >= expected => {}
        _ => {
            return reject(
                bot,
                q.id.clone(),
                "Invalid payment amount. Please start over.",
            )
            .await;
        }
    }

    bot.answer_pre_checkout_query(q.id, true).await?;
    Ok(())
}

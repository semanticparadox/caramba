// Эндпоинты управления платёжными провайдерами для администратора.
// Сейчас содержит только POST /admin/payments/{provider}/test — проверку подключения.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::Utc;
use serde_json::json;
use tracing::{info, warn};
use uuid::Uuid;

use crate::AppState;
use crate::handlers::admin::auth::is_authenticated;
use crate::services::payment::{
    aaio::AaioProvider, btcpay::BtcPayProvider, coinbase_commerce::CoinbaseCommerceProvider,
    cryptobot::CryptoBotProvider, cryptomus::CryptomusProvider, crystalpay::CrystalPayProvider,
    lava::LavaProvider, nowpayments::NowPaymentsProvider, oxapay::OxaPayProvider,
    paypalych::PaypalychProvider, plisio::PlisioProvider, provider::PaymentProvider,
    tribute::TributeProvider, wata::WataProvider,
};
use caramba_db::models::store::{PaymentSession, User};

// ── Тестовые параметры ────────────────────────────────────────────────────────
// Сумма 1 цент (100 = $1.00, поэтому 1 = $0.01) — минимальная, чтобы API
// принял запрос, но не создавала реального обязательства.
const TEST_AMOUNT_CENTS: i64 = 1;
const TEST_CURRENCY: &str = "USD";

/// Строит минимально заполненный PaymentSession для тестового вызова create_invoice.
/// Сессия НЕ сохраняется в БД — используется только как аргумент провайдеру.
fn test_session(provider: &str) -> PaymentSession {
    PaymentSession {
        id: Uuid::new_v4(),
        user_id: 0,
        product_id: 0,
        provider: provider.to_string(),
        external_id: None,
        amount: TEST_AMOUNT_CENTS,
        currency: TEST_CURRENCY.to_string(),
        status: "test".to_string(),
        metadata: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// Минимальный User-заглушка для передачи в create_invoice.
fn test_user() -> User {
    User {
        id: 0,
        tg_id: 0,
        username: None,
        full_name: None,
        balance: 0,
        referral_code: None,
        referrer_id: None,
        referred_by: None,
        is_banned: false,
        language_code: None,
        terms_accepted_at: None,
        warning_count: 0,
        trial_used: None,
        trial_used_at: None,
        last_bot_msg_id: None,
        created_at: Utc::now(),
        parent_id: None,
    }
}

// ── Вспомогательная функция: вызвать create_invoice провайдера в тестовом режиме ──
//
// Для провайдеров без sandbox-среды выполняем реальный вызов API.
// Если инвойс создан — сразу его никуда не сохраняем; единственный эффект —
// создание черновика/инвойса на стороне провайдера.
// Для провайдеров, у которых API возвращает pay_url, это означает появление
// «висящего» инвойса на $0.01 — он истечёт автоматически согласно настройкам TTL.
// Manual / Stars / balance — возвращаем ok без вызова API.
async fn invoke_provider_test(provider_name: &str, state: &AppState) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client init failed: {}", e))?;

    let session = test_session(provider_name);
    let user = test_user();
    let s = &state.settings;

    match provider_name {
        // ── Провайдеры без удалённых вызовов (всегда ok) ─────────────────────
        "manual" | "balance" => Ok("no remote check applicable — manual approval flow".to_string()),

        "stars" => {
            // Stars инвойсы создаются через Bot API; без tg_id пользователя
            // полноценная проверка невозможна. Проверяем наличие bot_token.
            let bot_token = s.get_or_default("bot_token", "").await;
            if bot_token.trim().is_empty() {
                return Err("Bot token not configured — required for Telegram Stars".to_string());
            }
            Ok("bot_token present — Stars should work when bot is running".to_string())
        }

        // ── CryptoBot ────────────────────────────────────────────────────────
        "cryptobot" => {
            let token = s.get_or_default("payment_api_key", "").await;
            if token.trim().is_empty() {
                return Err("CryptoBot API token is not configured".to_string());
            }
            let bot_username = s.get_or_default("bot_username", "testbot").await;
            let provider = CryptoBotProvider {
                token,
                bot_username,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("CryptoBot API error: {}", e))
        }

        // ── NOWPayments ──────────────────────────────────────────────────────
        "nowpayments" => {
            let api_key = s.get_or_default("nowpayments_api_key", "").await;
            if api_key.trim().is_empty() {
                return Err("NOWPayments API key is not configured".to_string());
            }
            let ipn_secret = s.get_or_default("nowpayments_ipn_secret", "").await;
            let panel_url = s.get_or_default("panel_url", "").await;
            let bot_username = s.get_or_default("bot_username", "").await;
            let provider = NowPaymentsProvider {
                api_key,
                ipn_secret,
                api_domain: panel_url,
                bot_username,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("NOWPayments API error: {}", e))
        }

        // ── Cryptomus ────────────────────────────────────────────────────────
        // Struct field is `api_key`, not `payment_api_key`.
        "cryptomus" => {
            let merchant_id = s.get_or_default("cryptomus_merchant_id", "").await;
            let api_key = s.get_or_default("cryptomus_payment_api_key", "").await;
            if merchant_id.trim().is_empty() || api_key.trim().is_empty() {
                return Err(
                    "Cryptomus merchant_id or payment_api_key is not configured".to_string()
                );
            }
            let panel_url = s.get_or_default("panel_url", "").await;
            let bot_username = s.get_or_default("bot_username", "").await;
            let provider = CryptomusProvider {
                merchant_id,
                api_key,
                api_domain: panel_url,
                bot_username,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("Cryptomus API error: {}", e))
        }

        // ── Lava ─────────────────────────────────────────────────────────────
        "lava" => {
            let project_id = s.get_or_default("lava_project_id", "").await;
            let secret_key = s.get_or_default("lava_secret_key", "").await;
            if project_id.trim().is_empty() || secret_key.trim().is_empty() {
                return Err("Lava project_id or secret_key is not configured".to_string());
            }
            let panel_url = s.get_or_default("panel_url", "").await;
            let provider = LavaProvider {
                project_id,
                secret_key,
                api_domain: panel_url,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("Lava API error: {}", e))
        }

        // ── AAIO ─────────────────────────────────────────────────────────────
        // AaioProvider has no api_domain field.
        "aaio" => {
            let merchant_id = s.get_or_default("aaio_merchant_id", "").await;
            let secret_1 = s.get_or_default("aaio_secret_1", "").await;
            let secret_2 = s.get_or_default("aaio_secret_2", "").await;
            if merchant_id.trim().is_empty() || secret_1.trim().is_empty() {
                return Err("AAIO merchant_id or secret_1 is not configured".to_string());
            }
            let provider = AaioProvider {
                merchant_id,
                secret_1,
                secret_2,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("AAIO API error: {}", e))
        }

        // ── WATA ─────────────────────────────────────────────────────────────
        "wata" => {
            let jwt_token = s.get_or_default("wata_jwt_token", "").await;
            let webhook_secret = s.get_or_default("wata_webhook_secret", "").await;
            if jwt_token.trim().is_empty() {
                return Err("WATA JWT token is not configured".to_string());
            }
            let panel_url = s.get_or_default("panel_url", "").await;
            let bot_username = s.get_or_default("bot_username", "").await;
            let provider = WataProvider {
                jwt_token,
                webhook_secret,
                api_domain: panel_url,
                bot_username,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("WATA API error: {}", e))
        }

        // ── CrystalPay ───────────────────────────────────────────────────────
        "crystalpay" => {
            let login = s.get_or_default("crystalpay_login", "").await;
            let secret = s.get_or_default("crystalpay_secret", "").await;
            let salt = s.get_or_default("crystalpay_salt", "").await;
            if login.trim().is_empty() || secret.trim().is_empty() {
                return Err("CrystalPay login or secret is not configured".to_string());
            }
            let panel_url = s.get_or_default("panel_url", "").await;
            let bot_username = s.get_or_default("bot_username", "").await;
            let provider = CrystalPayProvider {
                login,
                secret,
                salt,
                api_domain: panel_url,
                bot_username,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("CrystalPay API error: {}", e))
        }

        // ── Tribute ──────────────────────────────────────────────────────────
        "tribute" => {
            let api_key = s.get_or_default("tribute_api_key", "").await;
            let webhook_secret = s.get_or_default("tribute_webhook_secret", "").await;
            if api_key.trim().is_empty() {
                return Err("Tribute API key is not configured".to_string());
            }
            let bot_username = s.get_or_default("bot_username", "").await;
            let provider = TributeProvider {
                api_key,
                webhook_secret,
                bot_username,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("Tribute API error: {}", e))
        }

        // ── BTCPay Server ─────────────────────────────────────────────────────
        "btcpay" => {
            let btcpay_url = s.get_or_default("btcpay_url", "").await;
            let api_key = s.get_or_default("btcpay_api_key", "").await;
            let store_id = s.get_or_default("btcpay_store_id", "").await;
            let webhook_secret = s.get_or_default("btcpay_webhook_secret", "").await;
            if btcpay_url.trim().is_empty()
                || api_key.trim().is_empty()
                || store_id.trim().is_empty()
            {
                return Err("BTCPay Server url, api_key or store_id is not configured".to_string());
            }
            let bot_username = s.get_or_default("bot_username", "").await;
            let provider = BtcPayProvider {
                btcpay_url,
                api_key,
                store_id,
                webhook_secret,
                bot_username,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("BTCPay API error: {}", e))
        }

        // ── OxaPay ───────────────────────────────────────────────────────────
        "oxapay" => {
            let merchant_key = s.get_or_default("oxapay_merchant_key", "").await;
            if merchant_key.trim().is_empty() {
                return Err("OxaPay merchant key is not configured".to_string());
            }
            let panel_url = s.get_or_default("panel_url", "").await;
            let bot_username = s.get_or_default("bot_username", "").await;
            let provider = OxaPayProvider {
                merchant_key,
                api_domain: panel_url,
                bot_username,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("OxaPay API error: {}", e))
        }

        // ── Coinbase Commerce ─────────────────────────────────────────────────
        "coinbase" => {
            let api_key = s.get_or_default("coinbase_api_key", "").await;
            let webhook_secret = s.get_or_default("coinbase_webhook_secret", "").await;
            if api_key.trim().is_empty() {
                return Err("Coinbase Commerce API key is not configured".to_string());
            }
            let bot_username = s.get_or_default("bot_username", "").await;
            let provider = CoinbaseCommerceProvider {
                api_key,
                webhook_secret,
                bot_username,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("Coinbase Commerce API error: {}", e))
        }

        // ── Plisio ────────────────────────────────────────────────────────────
        "plisio" => {
            let api_key = s.get_or_default("plisio_api_key", "").await;
            if api_key.trim().is_empty() {
                return Err("Plisio API key is not configured".to_string());
            }
            let panel_url = s.get_or_default("panel_url", "").await;
            let bot_username = s.get_or_default("bot_username", "").await;
            let provider = PlisioProvider {
                api_key,
                api_domain: panel_url,
                bot_username,
            };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("Plisio API error: {}", e))
        }

        // ── Paypalych (pal24.pro) — SBP + USDT TRC20 ─────────────────────────
        "paypalych" => {
            let api_token = s.get_or_default("paypalych_api_token", "").await;
            if api_token.trim().is_empty() {
                return Err("Paypalych API token is not configured".to_string());
            }
            let shop_id = s.get_or_default("paypalych_shop_id", "").await;
            // The provider no longer reads `paypalych_webhook_secret` (the API
            // token itself signs the webhook); the setting is left in the DB
            // for back-compat with v0.9.52 installations.
            let provider = PaypalychProvider { api_token, shop_id };
            provider
                .create_invoice(&session, &user, &client)
                .await
                .map_err(|e| format!("Paypalych API error: {}", e))
        }

        _ => Err(format!("Unknown provider: {}", provider_name)),
    }
}

// ── Эндпоинт POST /admin/payments/{provider}/test ────────────────────────────
/// Проверяет подключение к платёжному провайдеру.
///
/// Выполняет реальный вызов create_invoice с тестовыми данными ($0.01).
/// Сессия НЕ сохраняется в БД — обязательства перед пользователем не создаются.
/// Для провайдеров без outbound API (manual, balance, stars) возвращает ok сразу.
///
/// Ответ: JSON `{"ok": true, "message": "..."}` или `{"ok": false, "error": "..."}`.
pub async fn test_provider_connection(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(provider): Path<String>,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"ok": false, "error": "not authenticated"})),
        )
            .into_response();
    }

    info!(provider = %provider, "Admin requested test connection for payment provider");

    let provider = provider.to_lowercase();
    let provider = provider.trim().to_string();

    match invoke_provider_test(&provider, &state).await {
        Ok(msg) => {
            info!(provider = %provider, message = %msg, "Payment provider test: OK");
            (StatusCode::OK, Json(json!({"ok": true, "message": msg}))).into_response()
        }
        Err(err) => {
            warn!(provider = %provider, error = %err, "Payment provider test: FAILED");
            (
                StatusCode::OK, // 200 с ok:false — клиент разбирает JSON
                Json(json!({"ok": false, "error": err})),
            )
                .into_response()
        }
    }
}

//! Клиент активации лицензии (контракт D).
//!
//! На старте и периодически: читает CARAMBA_* из env, POST'ит на
//! `{server}/v1/activate`, проверяет ed25519-подпись pubkey'ем из env,
//! апсертит проверенное состояние в `license_state` и кладёт его в кэш AppState.
//!
//! Best-effort: любой сбой (нет ключа, сервер недоступен, плохая подпись,
//! плохой pubkey) НЕ прерывает старт и НЕ паникует. При сбое сети, если есть
//! валидный кэш, действует офлайн-грейс (см. `license::effective_limits`).
//!
//! Анти-реплей: в `verify_activation` мы передаём СВОЙ `instance_id` из env, а
//! не эхо из тела ответа, поэтому ответ, подписанный для другого инстанса,
//! не пройдёт проверку.

use crate::AppState;
use caramba_db::repositories::license_repo::LicenseRepository;
use caramba_shared::license::{
    ActivationRequest, ActivationResponse, LicenseTier as SharedTier, verify_activation,
};

use super::{LicenseState, LicenseTier};

/// Имена env-переменных (контракт D). Читаются из процесса (dotenvy уже
/// загрузил .env на старте). Пустой/отсутствующий ключ => тир Free без вызова.
const ENV_LICENSE_KEY: &str = "CARAMBA_LICENSE_KEY";
const ENV_INSTANCE_ID: &str = "CARAMBA_INSTANCE_ID";
const ENV_SERVER_URL: &str = "CARAMBA_LICENSE_SERVER_URL";
const ENV_PUBKEY: &str = "CARAMBA_LICENSE_PUBKEY";

fn env_trimmed(key: &str) -> String {
    std::env::var(key).unwrap_or_default().trim().to_string()
}

fn shared_tier_to_local(t: SharedTier) -> LicenseTier {
    match t {
        SharedTier::Free => LicenseTier::Free,
        SharedTier::Pro => LicenseTier::Pro,
    }
}

/// Декодирует ed25519-pubkey из env. Принимает base64 или hex (32 байта).
/// Возвращает `None` при пустом/битом значении (-> трактуется как Free, не паника).
fn decode_pubkey(raw: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    if raw.is_empty() {
        return None;
    }
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(raw) {
        if bytes.len() == 32 {
            return Some(bytes);
        }
    }
    if let Ok(bytes) = hex::decode(raw) {
        if bytes.len() == 32 {
            return Some(bytes);
        }
    }
    None
}

/// Грузит кэшированное состояние из БД в AppState (без обращения к серверу).
/// Зовётся на старте до сетевой активации, чтобы грейс работал сразу.
pub async fn load_cached_into_state(state: &AppState) {
    let repo = LicenseRepository::new(state.pool.clone());
    match repo.get().await {
        Ok(Some(row)) => {
            let tier = match row.tier.trim().to_ascii_lowercase().as_str() {
                "pro" => LicenseTier::Pro,
                _ => LicenseTier::Free,
            };
            let limits = match serde_json::from_value(row.limits_json) {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(err = %e, "license: cached limits_json unparsable, treating as Free");
                    super::free_limits()
                }
            };
            let cached = LicenseState {
                tier,
                limits,
                expires_at: row.expires_at,
                last_verified_at: row.last_verified_at,
            };
            *state.license.write().await = Some(cached);
            tracing::info!("license: loaded cached state from license_state");
        }
        Ok(None) => {
            tracing::debug!("license: no cached state (Free instance or never activated)");
        }
        Err(e) => {
            tracing::warn!(err = %e, "license: failed to read cached state");
        }
    }
}

/// Лучшая попытка активации: env -> POST /v1/activate -> verify -> cache.
///
/// Never panics, never aborts startup. На успехе обновляет кэш и БД. На сбое
/// сети оставляет существующий кэш как есть (грейс решает в effective_*). При
/// отсутствии ключа выставляет кэш в `None` (чистый Free).
pub async fn activate_and_cache(state: &AppState) {
    let license_key = env_trimmed(ENV_LICENSE_KEY);
    let instance_id = env_trimmed(ENV_INSTANCE_ID);
    let server_url = env_trimmed(ENV_SERVER_URL);
    let pubkey_raw = env_trimmed(ENV_PUBKEY);

    // Нет ключа -> Free инстанс, серверу не звоним. Чистим кэш, чтобы тир
    // честно был Free.
    if license_key.is_empty() {
        *state.license.write().await = None;
        tracing::info!("license: no CARAMBA_LICENSE_KEY set, running Free tier");
        return;
    }

    if instance_id.is_empty() || server_url.is_empty() {
        tracing::warn!(
            "license: CARAMBA_LICENSE_KEY set but instance_id/server_url missing; keeping cached state (grace applies)"
        );
        return;
    }

    let pubkey = match decode_pubkey(&pubkey_raw) {
        Some(pk) => pk,
        None => {
            tracing::warn!(
                "license: CARAMBA_LICENSE_PUBKEY missing or malformed; cannot verify, keeping cached state (grace applies)"
            );
            return;
        }
    };

    let version = env!("CARGO_PKG_VERSION").to_string();
    let req = ActivationRequest {
        license_key: license_key.clone(),
        instance_id: instance_id.clone(),
        version,
    };

    let endpoint = format!("{}/v1/activate", server_url.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(err = %e, "license: failed to build http client; keeping cached state");
            return;
        }
    };

    let resp: ActivationResponse = match client.post(&endpoint).json(&req).send().await {
        Ok(r) => {
            if !r.status().is_success() {
                tracing::warn!(
                    status = %r.status(),
                    "license: activation server returned non-success; keeping cached state (grace applies)"
                );
                return;
            }
            match r.json::<ActivationResponse>().await {
                Ok(parsed) => parsed,
                Err(e) => {
                    tracing::warn!(err = %e, "license: activation response unparsable; keeping cached state");
                    return;
                }
            }
        }
        Err(e) => {
            tracing::warn!(err = %e, "license: activation server unreachable; keeping cached state (grace applies)");
            return;
        }
    };

    // Подпись проверяем СВОИМ instance_id из env (анти-реплей), не эхом из тела.
    if !verify_activation(&pubkey, &instance_id, &license_key, &resp) {
        tracing::warn!(
            "license: activation signature did NOT verify; ignoring response, keeping cached state. New privileged actions degrade to Free."
        );
        return;
    }

    // Подпись валидна -> кэшируем как проверенное состояние.
    let now = chrono::Utc::now();
    let local_tier = shared_tier_to_local(resp.tier);

    let verified = LicenseState {
        tier: local_tier,
        limits: resp.limits,
        expires_at: resp.expires_at,
        last_verified_at: now,
    };

    // Пишем в БД (idempotent upsert single row).
    let repo = LicenseRepository::new(state.pool.clone());
    let limits_json = serde_json::to_value(&resp.limits).unwrap_or(serde_json::Value::Null);
    let raw_payload = serde_json::to_value(&resp).unwrap_or(serde_json::Value::Null);
    let tier_str = match resp.tier {
        SharedTier::Free => "free",
        SharedTier::Pro => "pro",
    };
    if let Err(e) = repo
        .upsert(
            tier_str,
            &limits_json,
            resp.expires_at,
            &resp.signature,
            now,
            &raw_payload,
        )
        .await
    {
        // Кэш в памяти всё равно обновим — БД-сбой не должен ронять активацию.
        tracing::warn!(err = %e, "license: verified activation but failed to persist to license_state");
    }

    *state.license.write().await = Some(verified);

    if resp.expires_at <= now {
        tracing::warn!(
            "license: activation verified but license is EXPIRED; new privileged actions degrade to Free"
        );
    } else {
        tracing::info!(tier = %tier_str, "license: activation verified and cached");
    }
}

/// Фоновый цикл пере-верификации. Зовёт `activate_and_cache` и спит случайно
/// 12-24 ч (контракт D). Копирует паттерн фоновых задач из main.rs.
pub async fn reverify_loop(state: AppState) {
    use rand::Rng;
    loop {
        // Случайный интервал в [12ч, 24ч), чтобы инстансы не били сервер синхронно.
        let secs: u64 = {
            let mut rng = rand::rng();
            rng.random_range(12 * 60 * 60..24 * 60 * 60)
        };
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        activate_and_cache(&state).await;
    }
}

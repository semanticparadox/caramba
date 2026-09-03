//! Хранение и загрузка ключей CSM/1.
//!
//! Приватная часть корня в панель не попадает никогда. Онлайн-ключ приходит из
//! переменной окружения `CARAMBA_CSM_ONLINE_KEY` (32 байта семени в hex) и
//! намеренно НЕ переиспользует `SESSION_SECRET`: у того нет ни ротации, ни
//! `kid`, и связывать с ним подпись протокола значило бы утащить старую слабость
//! в новый слой доверия.

use anyhow::{Context, Result, anyhow};
use caramba_shared::csm;
use ed25519_dalek::SigningKey;
use sqlx::PgPool;

use super::{TenantIdentity, from_hex, hex};

/// Роль корня в таблице `csm_keys`.
pub const ROLE_ROOT: i16 = 1;
/// Роль онлайн-ключа.
pub const ROLE_ONLINE: i16 = 2;

/// Переменная окружения с семенем онлайн-ключа.
pub const ENV_ONLINE_KEY: &str = "CARAMBA_CSM_ONLINE_KEY";

/// Читает активный корневой ключ и выводит из него личность тенанта.
///
/// `None` означает, что оператор не включал протокол: это штатное состояние, а
/// не сбой.
pub async fn load_identity(pool: &PgPool) -> Result<Option<TenantIdentity>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT kid, public_key FROM csm_keys \
         WHERE role = $1 AND revoked_at IS NULL \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(ROLE_ROOT)
    .fetch_optional(pool)
    .await
    .context("csm: чтение корневого ключа")?;

    let Some((kid, public)) = row else {
        return Ok(None);
    };

    let root_public = from_hex::<32>(&public)?;
    let root_kid = from_hex::<12>(&kid)?;

    // kid обязан быть производным от ключа. Расхождение означает испорченную
    // строку в базе, и подписывать под ней нельзя: клиент отвергнет документ.
    let derived = csm::keyid_trunc(&root_public);
    if derived != root_kid {
        return Err(anyhow!(
            "csm: kid корневого ключа в базе ({}) не совпадает с производным от \
             публичного ключа ({})",
            kid,
            hex(&derived)
        ));
    }

    Ok(Some(TenantIdentity {
        pid: csm::pid_of(&root_public),
        root_public,
        root_kid,
    }))
}

/// Есть ли такой ключ в базе и не отозван ли он.
pub async fn key_exists(pool: &PgPool, kid_hex: &str) -> Result<bool> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT id FROM csm_keys WHERE kid = $1 AND revoked_at IS NULL LIMIT 1")
            .bind(kid_hex)
            .fetch_optional(pool)
            .await
            .context("csm: поиск ключа")?;
    Ok(found.is_some())
}

/// Регистрирует публичную часть ключа. Приватная часть сюда не передаётся и
/// здесь не хранится.
pub async fn register_public_key(
    pool: &PgPool,
    role: i16,
    public_key: &[u8; 32],
    label: Option<&str>,
) -> Result<String> {
    let kid = hex(&csm::keyid_trunc(public_key));
    sqlx::query(
        "INSERT INTO csm_keys (role, kid, public_key, label) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (kid) DO UPDATE SET revoked_at = NULL, label = EXCLUDED.label",
    )
    .bind(role)
    .bind(&kid)
    .bind(hex(public_key))
    .bind(label)
    .execute(pool)
    .await
    .context("csm: регистрация публичного ключа")?;
    Ok(kid)
}

/// Онлайн-ключ из окружения. `None`, если переменная не задана.
pub fn online_key_from_env() -> Result<Option<SigningKey>> {
    let raw = match std::env::var(ENV_ONLINE_KEY) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return Ok(None),
    };
    let seed = from_hex::<32>(&raw).with_context(|| {
        format!("csm: {ENV_ONLINE_KEY} должен быть 64 hex-символа (32 байта семени)")
    })?;
    Ok(Some(SigningKey::from_bytes(&seed)))
}

/// Все действующие ключи тенанта для ключевого документа: `(role, kid, pk)`.
pub async fn active_keys(pool: &PgPool) -> Result<Vec<(i16, [u8; 12], [u8; 32])>> {
    let rows: Vec<(i16, String, String)> = sqlx::query_as(
        "SELECT role, kid, public_key FROM csm_keys \
         WHERE revoked_at IS NULL ORDER BY role, created_at",
    )
    .fetch_all(pool)
    .await
    .context("csm: чтение действующих ключей")?;

    rows.into_iter()
        .map(|(role, kid, pk)| Ok((role, from_hex::<12>(&kid)?, from_hex::<32>(&pk)?)))
        .collect()
}

/// Отозванные идентификаторы ключей: попадают в список `rev` ключевого документа.
pub async fn revoked_kids(pool: &PgPool) -> Result<Vec<[u8; 12]>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT kid FROM csm_keys WHERE revoked_at IS NOT NULL ORDER BY revoked_at")
            .fetch_all(pool)
            .await
            .context("csm: чтение отозванных ключей")?;
    rows.into_iter()
        .map(|(kid,)| from_hex::<12>(&kid))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_key_from_env_requires_a_full_seed() {
        // SAFETY: тест однопоточный по этой переменной.
        unsafe { std::env::set_var(ENV_ONLINE_KEY, "deadbeef") };
        assert!(online_key_from_env().is_err());

        unsafe {
            std::env::set_var(
                ENV_ONLINE_KEY,
                "3e395bd70b7b39edf135a4610ed77446cf6b964e13daa8a9eae29402de45ff57",
            )
        };
        let key = online_key_from_env().unwrap().expect("ключ разобран");
        // Тот же ключ, что в корпусе векторов: проверяем производный kid.
        assert_eq!(
            hex(&csm::keyid_trunc(&key.verifying_key().to_bytes())),
            "21e3e2cc0a3ba777e69ce14c"
        );

        unsafe { std::env::remove_var(ENV_ONLINE_KEY) };
        assert!(online_key_from_env().unwrap().is_none());
    }
}

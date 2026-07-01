use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Cached, signature-verified license state (single row, id = 1).
///
/// Mirrors the `license_state` table from migration
/// `20260625000000_license_state.sql`. The panel writes this after a verified
/// activation and reads it for `effective_tier`/`effective_limits` plus the
/// offline grace window (anchored on `last_verified_at`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LicenseStateRow {
    pub tier: String,
    pub limits_json: serde_json::Value,
    pub expires_at: DateTime<Utc>,
    pub signature: String,
    pub last_verified_at: DateTime<Utc>,
    pub raw_payload: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct LicenseRepository {
    pool: PgPool,
}

impl LicenseRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Reads the cached license row, or `None` when no activation has happened
    /// yet (a Free instance with no key never writes a row).
    pub async fn get(&self) -> Result<Option<LicenseStateRow>> {
        let row = sqlx::query_as::<_, LicenseStateRow>(
            "SELECT tier, limits_json, expires_at, signature, last_verified_at, raw_payload \
             FROM license_state WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Upserts the single license row after a successful, signature-verified
    /// activation. `last_verified_at` is stamped to now by the caller and is
    /// the anchor for the offline grace window.
    pub async fn upsert(
        &self,
        tier: &str,
        limits_json: &serde_json::Value,
        expires_at: DateTime<Utc>,
        signature: &str,
        last_verified_at: DateTime<Utc>,
        raw_payload: &serde_json::Value,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO license_state
                (id, tier, limits_json, expires_at, signature, last_verified_at, raw_payload)
            VALUES (1, $1, $2, $3, $4, $5, $6)
            ON CONFLICT (id) DO UPDATE SET
                tier = EXCLUDED.tier,
                limits_json = EXCLUDED.limits_json,
                expires_at = EXCLUDED.expires_at,
                signature = EXCLUDED.signature,
                last_verified_at = EXCLUDED.last_verified_at,
                raw_payload = EXCLUDED.raw_payload
            "#,
        )
        .bind(tier)
        .bind(limits_json)
        .bind(expires_at)
        .bind(signature)
        .bind(last_verified_at)
        .bind(raw_payload)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

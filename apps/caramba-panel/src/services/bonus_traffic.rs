//! Bonus traffic — a one-off traffic allowance granted on top of whatever a
//! user's plan gives.
//!
//! # The one grant primitive
//!
//! Everything that hands out bonus traffic goes through [`grant_tx`] (or its
//! own-transaction wrapper [`grant`]). There are three sources today:
//!
//!   * **registration** — `signup_bonus_traffic_mb`, granted in the same
//!     transaction as the enrollment onboarding grant;
//!   * **promo codes of type `traffic`** — `promo_codes.traffic_gb`, granted
//!     inside `PromoService::redeem_code`'s transaction;
//!   * **referral** — `referral_bonus_traffic_mb_referrer` (granted when the
//!     referee's first purchase is fulfilled, alongside the existing MONEY
//!     reward) and `referral_bonus_traffic_mb_referee` (granted at enrollment
//!     to the invited user).
//!
//! Each grant is idempotent on `(user_id, source, reference)`: the unique index
//! on `traffic_bonuses` turns a retry, a duplicate webhook or a double-submitted
//! form into a no-op, and the denormalized `users.bonus_traffic_mb` is only
//! bumped when the ledger insert actually produced a row — so the cached total
//! can never drift upward.
//!
//! # How enforcement uses it
//!
//! The balance is an ALLOWANCE BUMP, not a consumable. Every quota gate
//! compares `used_traffic >= plan_limit_bytes + bonus_bytes` instead of
//! `used_traffic >= plan_limit_bytes`, so a user with bonus traffic is neither
//! expired (paid plans) nor throttled (free plans) until the plan allowance AND
//! the bonus are consumed. The gates live in
//! `subscription_service::{ensure_subscription_within_quota,
//! expire_over_quota_subscriptions, throttle_free_quota_subscriptions,
//! expire_over_quota_candidates}` and in `monitoring::daily_traffic_topup`; the
//! heartbeat ingestion in `api/v2/node.rs` enforces through
//! `expire_over_quota_candidates`.
//!
//! The SQL side of that comparison is spelled once in [`QUOTA_LIMIT_BYTES_SQL`]
//! so the gates cannot drift apart; the same arithmetic is available as a pure
//! function in [`quota_limit_bytes`] for the tests and for the display paths.

use anyhow::{Context, Result};
use sqlx::{PgPool, Postgres, Transaction};

// ---------------------------------------------------------------------------
// Settings keys
// ---------------------------------------------------------------------------

/// MB of bonus traffic granted to every newly enrolled user. `0` = off.
pub const SETTING_SIGNUP_BONUS_MB: &str = "signup_bonus_traffic_mb";

/// MB of bonus traffic granted to the REFERRER when an invited user's first
/// purchase is fulfilled. `0` = off. Independent of the money reward
/// (`referral_reward_percent`), which keeps working untouched.
pub const SETTING_REFERRAL_BONUS_MB_REFERRER: &str = "referral_bonus_traffic_mb_referrer";

/// MB of bonus traffic granted to the REFEREE (the invited user) at enrollment.
/// `0` = off. Independent of the first-purchase discount.
pub const SETTING_REFERRAL_BONUS_MB_REFEREE: &str = "referral_bonus_traffic_mb_referee";

// ---------------------------------------------------------------------------
// Grant sources
// ---------------------------------------------------------------------------

/// Registration bonus. Once per user, so the reference is a constant.
pub const SOURCE_SIGNUP: &str = "signup";
/// Promo code of type `traffic`. Reference = the promo code's id.
pub const SOURCE_PROMO: &str = "promo";
/// Referral reward for the inviter. Reference = the referred user's id.
pub const SOURCE_REFERRAL_REFERRER: &str = "referral_referrer";
/// Referral reward for the invited user. Reference = the inviter's id.
pub const SOURCE_REFERRAL_REFEREE: &str = "referral_referee";

/// The `reference` used by once-per-user sources.
///
/// `traffic_bonuses.reference` is NOT NULL precisely so the unique index bites:
/// a NULL reference would make every row distinct in Postgres and silently
/// disable idempotency.
pub const REFERENCE_ONCE_PER_USER: &str = "once";

/// Upper bound on a single grant, in MB (1 TB). Guards against an operator
/// typing an extra three zeros into a setting or a promo code; a grant that
/// large is always a mistake, never an intent.
pub const MAX_GRANT_MB: i64 = 1024 * 1024;

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Parse an MB-valued setting. Anything missing, unparseable, negative or
/// beyond [`MAX_GRANT_MB`] reads as `0` — i.e. "feature off" — because the
/// safe failure mode for a giveaway is to give nothing.
pub fn parse_bonus_mb(raw: Option<&str>) -> i64 {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|mb| (0..=MAX_GRANT_MB).contains(mb))
        .unwrap_or(0)
}

/// The effective traffic ceiling for a subscription, in bytes.
///
/// `None` means unlimited: a plan with `traffic_limit_gb <= 0` has no ceiling,
/// and bonus traffic on top of "unlimited" is still unlimited.
///
/// This is the Rust twin of [`QUOTA_LIMIT_BYTES_SQL`]; the two must agree.
pub fn quota_limit_bytes(traffic_limit_gb: i64, bonus_traffic_mb: i64) -> Option<i64> {
    if traffic_limit_gb <= 0 {
        return None;
    }
    let plan_bytes = traffic_limit_gb.saturating_mul(1024 * 1024 * 1024);
    let bonus_bytes = bonus_traffic_mb.max(0).saturating_mul(1024 * 1024);
    Some(plan_bytes.saturating_add(bonus_bytes))
}

/// Whether a subscription has consumed its plan allowance AND its bonus.
///
/// The single predicate behind every expire/throttle decision; `false` for
/// unlimited plans.
pub fn is_over_quota(used_traffic: i64, traffic_limit_gb: i64, bonus_traffic_mb: i64) -> bool {
    match quota_limit_bytes(traffic_limit_gb, bonus_traffic_mb) {
        None => false,
        Some(limit) => used_traffic >= limit,
    }
}

/// The quota ceiling as a SQL expression, for the sweeps that must evaluate it
/// set-at-a-time.
///
/// Assumes the query has `p` (plans), `s` (subscriptions) and `u` (users) in
/// scope. Kept as one shared constant so an added gate cannot forget the bonus
/// term — the failure mode of forgetting is throttling a user who still has
/// traffic left, which is invisible until they complain.
pub const QUOTA_LIMIT_BYTES_SQL: &str = "(CAST(p.traffic_limit_gb AS BIGINT) * 1073741824 \
     + COALESCE(u.bonus_traffic_mb, 0) * 1048576)";

/// Clamp a requested grant into the allowed range.
///
/// Non-positive means "nothing to do"; anything above [`MAX_GRANT_MB`] is
/// clamped rather than rejected, so a mistyped setting cannot abort the
/// enrollment or payment fulfillment the grant is embedded in.
pub fn normalize_grant_mb(amount_mb: i64) -> i64 {
    amount_mb.clamp(0, MAX_GRANT_MB)
}

/// THE idempotency decision, extracted from the SQL so it can be tested.
///
/// `ledger_row_inserted` is what `INSERT ... ON CONFLICT (user_id, source,
/// reference) DO NOTHING RETURNING id` reports: `true` only for the call that
/// actually created the ledger row. The denormalized balance must move exactly
/// when that is true — never on a retry, a duplicate webhook, or the loser of a
/// concurrent race, all of which insert nothing.
pub fn should_credit_balance(amount_mb: i64, ledger_row_inserted: bool) -> bool {
    normalize_grant_mb(amount_mb) > 0 && ledger_row_inserted
}

// ---------------------------------------------------------------------------
// The grant primitive
// ---------------------------------------------------------------------------

/// Grant `amount_mb` of bonus traffic to `user_id` inside the caller's
/// transaction.
///
/// Idempotent on `(user_id, source, reference)`. Returns `true` when this call
/// actually granted (a new ledger row), `false` when the grant already existed
/// or was a no-op (`amount_mb <= 0`).
///
/// Both statements run in the caller's transaction, and the balance update is
/// gated on the ledger insert having returned a row, so a concurrent duplicate
/// grant credits the user exactly once no matter how the two transactions
/// interleave: the loser of the unique-index race inserts nothing and therefore
/// updates nothing.
pub async fn grant_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    source: &str,
    reference: &str,
    amount_mb: i64,
    note: Option<&str>,
) -> Result<bool> {
    let amount_mb = normalize_grant_mb(amount_mb);
    if amount_mb == 0 {
        return Ok(false);
    }

    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO traffic_bonuses (user_id, source, reference, amount_mb, note) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (user_id, source, reference) DO NOTHING RETURNING id",
    )
    .bind(user_id)
    .bind(source)
    .bind(reference)
    .bind(amount_mb)
    .bind(note)
    .fetch_optional(&mut **tx)
    .await
    .context("Failed to record traffic bonus")?;

    if !should_credit_balance(amount_mb, inserted.is_some()) {
        tracing::debug!(
            user_id,
            source,
            reference,
            "bonus traffic already granted, skipping (idempotent)"
        );
        return Ok(false);
    }

    sqlx::query(
        "UPDATE users SET bonus_traffic_mb = COALESCE(bonus_traffic_mb, 0) + $1 WHERE id = $2",
    )
    .bind(amount_mb)
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .context("Failed to update bonus traffic balance")?;

    tracing::info!(
        user_id,
        source,
        reference,
        amount_mb,
        "bonus traffic granted"
    );
    Ok(true)
}

/// [`grant_tx`] in its own transaction, for callers that have no transaction of
/// their own to join.
pub async fn grant(
    pool: &PgPool,
    user_id: i64,
    source: &str,
    reference: &str,
    amount_mb: i64,
    note: Option<&str>,
) -> Result<bool> {
    if amount_mb <= 0 {
        return Ok(false);
    }
    let mut tx = pool.begin().await?;
    let granted = grant_tx(&mut tx, user_id, source, reference, amount_mb, note).await?;
    tx.commit().await?;
    Ok(granted)
}

/// Read an MB-valued setting straight from the `settings` table inside a
/// transaction.
///
/// The `SettingsService` cache is not reachable from every grant site (the
/// referral reward runs as a free function inside `fulfill_payment`'s
/// transaction), and this mirrors how `ReferralService::reward_percent` already
/// reads `referral_reward_percent`.
pub async fn setting_mb_tx(tx: &mut Transaction<'_, Postgres>, key: &str) -> Result<i64> {
    let raw: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = $1")
        .bind(key)
        .fetch_optional(&mut **tx)
        .await
        .with_context(|| format!("Failed to read setting {key}"))?;
    Ok(parse_bonus_mb(raw.as_deref()))
}

/// A user's current bonus balance in MB (0 when the user is gone).
pub async fn balance_mb(pool: &PgPool, user_id: i64) -> Result<i64> {
    let mb: Option<i64> =
        sqlx::query_scalar("SELECT COALESCE(bonus_traffic_mb, 0) FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .context("Failed to read bonus traffic balance")?;
    Ok(mb.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// In-memory stand-in for `traffic_bonuses` + `users.bonus_traffic_mb`,
    /// wired through the SAME decision function the real `grant_tx` uses.
    ///
    /// `insert` models `INSERT ... ON CONFLICT (user_id, source, reference) DO
    /// NOTHING RETURNING id`: it returns `true` only for the caller that
    /// actually created the row — which is exactly what Postgres guarantees,
    /// including for the loser of a concurrent race.
    #[derive(Default)]
    struct FakeLedger {
        rows: HashSet<(i64, String, String)>,
        balance_mb: i64,
    }

    impl FakeLedger {
        fn grant(&mut self, user_id: i64, source: &str, reference: &str, amount_mb: i64) -> bool {
            let amount_mb = normalize_grant_mb(amount_mb);
            if amount_mb == 0 {
                return false;
            }
            let inserted = self
                .rows
                .insert((user_id, source.to_string(), reference.to_string()));
            if !should_credit_balance(amount_mb, inserted) {
                return false;
            }
            self.balance_mb += amount_mb;
            true
        }
    }

    // ---- grant primitive idempotency ---------------------------------------

    #[test]
    fn a_repeated_grant_credits_exactly_once() {
        let mut ledger = FakeLedger::default();

        assert!(ledger.grant(1, SOURCE_SIGNUP, REFERENCE_ONCE_PER_USER, 200));
        assert_eq!(ledger.balance_mb, 200);

        // Retry / duplicate webhook / double-submitted form.
        for _ in 0..5 {
            assert!(
                !ledger.grant(1, SOURCE_SIGNUP, REFERENCE_ONCE_PER_USER, 200),
                "a repeat must not grant again"
            );
        }
        assert_eq!(ledger.balance_mb, 200, "balance must not drift on retries");
    }

    #[test]
    fn distinct_references_are_distinct_grants() {
        let mut ledger = FakeLedger::default();

        // Two different promo codes: both count.
        assert!(ledger.grant(1, SOURCE_PROMO, "10", 1024));
        assert!(ledger.grant(1, SOURCE_PROMO, "11", 512));
        assert_eq!(ledger.balance_mb, 1536);
        // …but replaying either one does not.
        assert!(!ledger.grant(1, SOURCE_PROMO, "10", 1024));
        assert!(!ledger.grant(1, SOURCE_PROMO, "11", 512));
        assert_eq!(ledger.balance_mb, 1536);
    }

    #[test]
    fn the_same_reference_under_different_sources_does_not_collide() {
        let mut ledger = FakeLedger::default();
        // Referrer reward for referee #7 and referee reward from inviter #7 are
        // different grants that happen to share the reference string.
        assert!(ledger.grant(1, SOURCE_REFERRAL_REFERRER, "7", 100));
        assert!(ledger.grant(1, SOURCE_REFERRAL_REFEREE, "7", 100));
        assert_eq!(ledger.balance_mb, 200);
    }

    #[test]
    fn a_concurrent_duplicate_credits_once() {
        // Both transactions attempt the same key; Postgres lets exactly one
        // INSERT return a row, and only that one is allowed to move the balance.
        let winner_inserted = true;
        let loser_inserted = false;
        assert!(should_credit_balance(500, winner_inserted));
        assert!(!should_credit_balance(500, loser_inserted));
    }

    #[test]
    fn a_disabled_source_grants_nothing_even_with_a_fresh_key() {
        let mut ledger = FakeLedger::default();
        // settings = 0 => "feature off": no ledger row, no balance change.
        assert!(!ledger.grant(1, SOURCE_SIGNUP, REFERENCE_ONCE_PER_USER, 0));
        assert!(!ledger.grant(1, SOURCE_SIGNUP, REFERENCE_ONCE_PER_USER, -100));
        assert_eq!(ledger.balance_mb, 0);
        assert!(ledger.rows.is_empty());
        // Turning the setting on later still works — the key was never claimed.
        assert!(ledger.grant(1, SOURCE_SIGNUP, REFERENCE_ONCE_PER_USER, 200));
        assert_eq!(ledger.balance_mb, 200);
    }

    #[test]
    fn an_absurd_amount_is_clamped_not_rejected() {
        assert_eq!(normalize_grant_mb(-1), 0);
        assert_eq!(normalize_grant_mb(0), 0);
        assert_eq!(normalize_grant_mb(200), 200);
        assert_eq!(normalize_grant_mb(MAX_GRANT_MB), MAX_GRANT_MB);
        assert_eq!(normalize_grant_mb(MAX_GRANT_MB * 10), MAX_GRANT_MB);

        let mut ledger = FakeLedger::default();
        assert!(ledger.grant(1, SOURCE_PROMO, "1", i64::MAX));
        assert_eq!(ledger.balance_mb, MAX_GRANT_MB);
    }

    // ---- settings parsing ("0 = off") --------------------------------------

    #[test]
    fn unset_or_zero_settings_mean_the_feature_is_off() {
        for raw in [None, Some(""), Some("  "), Some("0"), Some(" 0 ")] {
            assert_eq!(
                parse_bonus_mb(raw),
                0,
                "raw {raw:?} must read as 'feature off'"
            );
        }
    }

    #[test]
    fn garbage_and_out_of_range_settings_also_mean_off() {
        // The safe failure mode for a giveaway is to give nothing.
        for raw in [
            Some("abc"),
            Some("-100"),
            Some("1.5"),
            Some("1000 MB"),
            // Beyond MAX_GRANT_MB (1 TB) — an extra three zeros, not an intent.
            Some("2097152"),
        ] {
            assert_eq!(parse_bonus_mb(raw), 0, "raw {raw:?} must read as 0");
        }
    }

    #[test]
    fn positive_settings_are_honoured() {
        assert_eq!(parse_bonus_mb(Some("200")), 200);
        assert_eq!(parse_bonus_mb(Some(" 5120 ")), 5120);
        assert_eq!(parse_bonus_mb(Some("1048576")), MAX_GRANT_MB);
    }

    // ---- quota arithmetic --------------------------------------------------

    #[test]
    fn unlimited_plans_stay_unlimited_with_or_without_bonus() {
        assert_eq!(quota_limit_bytes(0, 0), None);
        assert_eq!(quota_limit_bytes(0, 5000), None);
        assert_eq!(quota_limit_bytes(-1, 5000), None);
        assert!(!is_over_quota(i64::MAX / 2, 0, 0));
        assert!(!is_over_quota(i64::MAX / 2, 0, 1024));
    }

    #[test]
    fn bonus_is_added_on_top_of_the_plan_allowance() {
        // 1 GB plan, no bonus.
        assert_eq!(quota_limit_bytes(1, 0), Some(1024 * 1024 * 1024));
        // 1 GB plan + 200 MB bonus.
        assert_eq!(
            quota_limit_bytes(1, 200),
            Some(1024 * 1024 * 1024 + 200 * 1024 * 1024)
        );
        // A negative cached balance can never SHRINK the plan allowance.
        assert_eq!(quota_limit_bytes(1, -500), Some(1024 * 1024 * 1024));
    }

    #[test]
    fn a_user_with_bonus_is_not_over_quota_below_plan_plus_bonus() {
        let gb = 1024 * 1024 * 1024;
        let mb = 1024 * 1024;

        // 1 GB plan, 200 MB bonus => ceiling is 1 GB + 200 MB.
        // Exactly at the plan allowance: NOT over quota any more (this is the
        // regression bonus traffic exists to prevent).
        assert!(!is_over_quota(gb, 1, 200));
        // One byte below the bonus ceiling: still fine.
        assert!(!is_over_quota(gb + 200 * mb - 1, 1, 200));
        // Exactly at the bonus ceiling: over quota (the gates use `>=`).
        assert!(is_over_quota(gb + 200 * mb, 1, 200));
        // Beyond it: over quota.
        assert!(is_over_quota(gb + 500 * mb, 1, 200));
    }

    #[test]
    fn without_bonus_the_gate_behaves_exactly_as_before() {
        let gb = 1024 * 1024 * 1024;
        assert!(!is_over_quota(gb - 1, 1, 0));
        assert!(is_over_quota(gb, 1, 0));
        assert!(is_over_quota(gb + 1, 1, 0));
    }

    #[test]
    fn negative_used_traffic_headroom_is_never_over_quota() {
        // `grant_onboarding_traffic_tx` seeds used_traffic negative; that must
        // keep reading as "plenty left".
        assert!(!is_over_quota(-500 * 1024 * 1024, 1, 0));
        assert!(!is_over_quota(-500 * 1024 * 1024, 1, 200));
    }

    #[test]
    fn quota_sql_and_rust_use_the_same_terms() {
        // Cheap guard against the two drifting: the SQL must reference both the
        // plan allowance and the bonus, with the same byte multipliers the Rust
        // side uses (1 GiB = 1073741824, 1 MiB = 1048576).
        assert!(QUOTA_LIMIT_BYTES_SQL.contains("p.traffic_limit_gb"));
        assert!(QUOTA_LIMIT_BYTES_SQL.contains("u.bonus_traffic_mb"));
        assert!(QUOTA_LIMIT_BYTES_SQL.contains("1073741824"));
        assert!(QUOTA_LIMIT_BYTES_SQL.contains("1048576"));
        assert_eq!(1024i64 * 1024 * 1024, 1_073_741_824);
        assert_eq!(1024i64 * 1024, 1_048_576);
    }
}

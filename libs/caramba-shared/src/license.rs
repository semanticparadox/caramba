//! Shared license types and ed25519 activation crypto (P4 contract B).
//!
//! These types are the FROZEN wire contract between the license control plane
//! (`apps/caramba-license`, the signer) and the panel (`apps/caramba-panel`,
//! the verifier). The canonical message layout below is a stable byte encoding:
//! once shipped it must never be reordered or re-encoded, or every signature
//! already issued to a live instance would stop verifying.
//!
//! Trust model:
//! - The license server holds the ed25519 private (signing) key.
//! - The panel holds only the ed25519 public (verifying) key
//!   (`CARAMBA_LICENSE_PUBKEY`) and can therefore confirm a response was issued
//!   by the real server, but cannot forge one.
//! - The signed message binds `instance_id + license_key + tier + expires_at +
//!   limits`. The verifier passes its OWN `instance_id` (from its env) into
//!   [`verify_activation`], not the value echoed by the server, so a response
//!   signed for instance A cannot be replayed onto instance B, and tier/limits/
//!   expiry cannot be edited without breaking the signature.

use serde::{Deserialize, Serialize};

#[cfg(feature = "license")]
use base64::Engine as _;
#[cfg(feature = "license")]
use chrono::{DateTime, Utc};
#[cfg(feature = "license")]
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// License tier. Serializes as `"free"` / `"pro"` on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseTier {
    Free,
    Pro,
}

impl LicenseTier {
    /// Stable string form used inside the canonical signed message and DB rows.
    pub fn as_str(&self) -> &'static str {
        match self {
            LicenseTier::Free => "free",
            LicenseTier::Pro => "pro",
        }
    }

    /// Parse from the canonical string form. Unknown values fall back to Free
    /// so a malformed cached row degrades safely rather than panicking.
    pub fn from_str_lenient(s: &str) -> LicenseTier {
        match s {
            "pro" => LicenseTier::Pro,
            _ => LicenseTier::Free,
        }
    }
}

/// Effective limits for an instance. Authoritative type (i64).
///
/// Convention: a `max_*` value of `0` means UNLIMITED (used by Pro). Free uses
/// real positive caps. `end_user_billing` / `branding` / `upstream_ads` /
/// `manual_approval` are feature gates applied by the panel enforcement matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseLimits {
    pub max_nodes: i64,
    pub max_users: i64,
    pub end_user_billing: bool,
    pub branding: bool,
    pub upstream_ads: bool,
    pub manual_approval: bool,
}

impl LicenseLimits {
    /// Canonical limits for a tier (single source of truth for both server and
    /// panel soft-degrade). Free: 2 nodes / 100 users, no billing, no branding,
    /// upstream ads on, manual approval on. Pro: unlimited, billing + branding
    /// on, no upstream ads, no manual approval.
    pub fn for_tier(tier: LicenseTier) -> LicenseLimits {
        match tier {
            LicenseTier::Free => LicenseLimits {
                max_nodes: 2,
                max_users: 100,
                end_user_billing: false,
                branding: false,
                upstream_ads: true,
                manual_approval: true,
            },
            LicenseTier::Pro => LicenseLimits {
                max_nodes: 1000,
                max_users: 0,
                end_user_billing: true,
                branding: true,
                upstream_ads: false,
                manual_approval: false,
            },
        }
    }
}

/// Request body for `POST {LICENSE_SERVER}/v1/activate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationRequest {
    pub license_key: String,
    pub instance_id: String,
    pub version: String,
}

/// Response body for `POST {LICENSE_SERVER}/v1/activate`.
///
/// `signature` is base64 (standard alphabet) over [`canonical_message`].
#[cfg(feature = "license")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationResponse {
    pub tier: LicenseTier,
    pub expires_at: DateTime<Utc>,
    pub limits: LicenseLimits,
    pub signature: String,
}

/// Build the deterministic, instance-bound canonical message that the ed25519
/// signature covers.
///
/// FROZEN LAYOUT — do not reorder or change encoding. Each logical field is
/// written as a length prefix (u32 big-endian byte count) followed by the raw
/// field bytes, so no field can bleed into the next and no separator can be
/// spoofed by field contents. Field order:
///
///   1. domain tag `b"caramba-license-v1"`
///   2. instance_id (utf-8)
///   3. license_key (utf-8)
///   4. tier (`"free"` / `"pro"`)
///   5. expires_at as RFC3339 in UTC, fixed format
///   6. max_nodes (i64 big-endian, 8 bytes)
///   7. max_users (i64 big-endian, 8 bytes)
///   8. end_user_billing (1 byte 0/1)
///   9. branding (1 byte 0/1)
///  10. upstream_ads (1 byte 0/1)
///  11. manual_approval (1 byte 0/1)
#[cfg(feature = "license")]
pub fn canonical_message(
    instance_id: &str,
    license_key: &str,
    tier: LicenseTier,
    expires_at: DateTime<Utc>,
    limits: &LicenseLimits,
) -> Vec<u8> {
    fn put_bytes(buf: &mut Vec<u8>, bytes: &[u8]) {
        // u32 big-endian length prefix, then the raw bytes.
        buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        buf.extend_from_slice(bytes);
    }

    let mut buf = Vec::new();
    put_bytes(&mut buf, b"caramba-license-v1");
    put_bytes(&mut buf, instance_id.as_bytes());
    put_bytes(&mut buf, license_key.as_bytes());
    put_bytes(&mut buf, tier.as_str().as_bytes());
    // Fixed RFC3339 / UTC rendering on both signer and verifier.
    let expires = expires_at
        .to_utc()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    put_bytes(&mut buf, expires.as_bytes());
    buf.extend_from_slice(&limits.max_nodes.to_be_bytes());
    buf.extend_from_slice(&limits.max_users.to_be_bytes());
    buf.push(limits.end_user_billing as u8);
    buf.push(limits.branding as u8);
    buf.push(limits.upstream_ads as u8);
    buf.push(limits.manual_approval as u8);
    buf
}

/// Sign a canonical activation message and return the base64 signature.
/// Server-side only (needs the private signing key).
#[cfg(feature = "license")]
pub fn sign_activation(
    signing_key: &SigningKey,
    instance_id: &str,
    license_key: &str,
    tier: LicenseTier,
    expires_at: DateTime<Utc>,
    limits: &LicenseLimits,
) -> String {
    let msg = canonical_message(instance_id, license_key, tier, expires_at, limits);
    let sig = signing_key.sign(&msg);
    base64::engine::general_purpose::STANDARD.encode(sig.to_bytes())
}

/// Verify an activation response against a known verifying (public) key.
///
/// `instance_id` and `license_key` MUST be the verifier's own values (from its
/// env), not values echoed by the server, so a signature minted for another
/// instance cannot be replayed here. Any malformed input (bad pubkey length,
/// bad base64, wrong signature length, bad signature) returns `false` — never a
/// panic — so a corrupt `CARAMBA_LICENSE_PUBKEY` degrades to Free rather than
/// crashing the panel.
#[cfg(feature = "license")]
pub fn verify_activation(
    verifying_key_bytes: &[u8],
    instance_id: &str,
    license_key: &str,
    resp: &ActivationResponse,
) -> bool {
    let key_arr: [u8; 32] = match verifying_key_bytes.try_into() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let verifying_key = match VerifyingKey::from_bytes(&key_arr) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let sig_bytes = match base64::engine::general_purpose::STANDARD.decode(resp.signature.as_bytes())
    {
        Ok(b) => b,
        Err(_) => return false,
    };
    let signature = match Signature::from_slice(&sig_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let msg = canonical_message(
        instance_id,
        license_key,
        resp.tier,
        resp.expires_at,
        &resp.limits,
    );
    verifying_key.verify(&msg, &signature).is_ok()
}

/// Decode a base64 ed25519 public key into raw bytes for [`verify_activation`].
/// Accepts both standard and url-safe (no-pad) base64. Returns `None` on any
/// malformed input.
#[cfg(feature = "license")]
pub fn decode_pubkey(b64: &str) -> Option<Vec<u8>> {
    let trimmed = b64.trim();
    if trimmed.is_empty() {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(trimmed))
        .ok()
}

#[cfg(all(test, feature = "license"))]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand_core::OsRng;

    fn fixed_expiry() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
            .unwrap()
            .to_utc()
    }

    #[test]
    fn canonical_message_is_deterministic() {
        let limits = LicenseLimits::for_tier(LicenseTier::Pro);
        let a = canonical_message("inst-1", "KEY-1", LicenseTier::Pro, fixed_expiry(), &limits);
        let b = canonical_message("inst-1", "KEY-1", LicenseTier::Pro, fixed_expiry(), &limits);
        assert_eq!(a, b);
    }

    #[test]
    fn sign_then_verify_roundtrips() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pub_bytes = signing_key.verifying_key().to_bytes();
        let limits = LicenseLimits::for_tier(LicenseTier::Pro);
        let expires = fixed_expiry();
        let signature =
            sign_activation(&signing_key, "inst-1", "KEY-1", LicenseTier::Pro, expires, &limits);
        let resp = ActivationResponse {
            tier: LicenseTier::Pro,
            expires_at: expires,
            limits,
            signature,
        };
        assert!(verify_activation(&pub_bytes, "inst-1", "KEY-1", &resp));
    }

    #[test]
    fn replay_onto_other_instance_fails() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pub_bytes = signing_key.verifying_key().to_bytes();
        let limits = LicenseLimits::for_tier(LicenseTier::Pro);
        let expires = fixed_expiry();
        let signature =
            sign_activation(&signing_key, "inst-1", "KEY-1", LicenseTier::Pro, expires, &limits);
        let resp = ActivationResponse {
            tier: LicenseTier::Pro,
            expires_at: expires,
            limits,
            signature,
        };
        // Same response, different instance id -> must fail.
        assert!(!verify_activation(&pub_bytes, "inst-2", "KEY-1", &resp));
    }

    #[test]
    fn tampering_limits_fails() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pub_bytes = signing_key.verifying_key().to_bytes();
        let expires = fixed_expiry();
        let pro_limits = LicenseLimits::for_tier(LicenseTier::Pro);
        let signature =
            sign_activation(&signing_key, "inst-1", "KEY-1", LicenseTier::Pro, expires, &pro_limits);
        // Attacker edits limits but keeps the signature.
        let mut tampered = pro_limits;
        tampered.max_nodes = 9999;
        let resp = ActivationResponse {
            tier: LicenseTier::Pro,
            expires_at: expires,
            limits: tampered,
            signature,
        };
        assert!(!verify_activation(&pub_bytes, "inst-1", "KEY-1", &resp));
    }

    #[test]
    fn malformed_pubkey_returns_false_not_panic() {
        let expires = fixed_expiry();
        let limits = LicenseLimits::for_tier(LicenseTier::Free);
        let resp = ActivationResponse {
            tier: LicenseTier::Free,
            expires_at: expires,
            limits,
            signature: "not-base64!!!".to_string(),
        };
        assert!(!verify_activation(&[0u8; 5], "inst-1", "KEY-1", &resp));
    }
}

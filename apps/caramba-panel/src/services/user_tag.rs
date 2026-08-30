//! Canonical sing-box auth-tag format shared by config generation and
//! connection enforcement.
//!
//! The orchestration layer injects every proxy user as `user_{tg_id}`
//! (Telegram id, NOT the subscription id). Traffic accounting
//! (`api/v2/node.rs::heartbeat`) and connection enforcement
//! (`ConnectionService`) must parse the tag with the exact same semantics —
//! interpreting it as a subscription id silently breaks enforcement (see the
//! NOTE in `traffic_service.rs` about the deleted `process_node_usage`).
//! Both sides go through these helpers so the format can never drift again.

/// Build the auth tag injected into node configs for a Telegram user.
pub fn user_tag(tg_id: i64) -> String {
    format!("user_{}", tg_id)
}

/// Parse an auth tag back into the Telegram id it encodes.
///
/// Returns `None` for anything that is not a `user_{i64}` tag (relay tags,
/// vless UUIDs, legacy garbage), so callers can fall back to other
/// identification strategies.
pub fn parse_user_tag(tag: &str) -> Option<i64> {
    tag.strip_prefix("user_")?.parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::{parse_user_tag, user_tag};

    /// Regression test: pins the tag format compatibility between config
    /// generation (orchestration_service) and connection matching
    /// (connection_service / heartbeat traffic accounting), in both
    /// directions. If either helper changes shape, enforcement silently
    /// dies — this test must fail first.
    #[test]
    fn generated_tag_round_trips_through_parser() {
        // Generation -> parsing: what orchestration writes, enforcement reads.
        assert_eq!(parse_user_tag(&user_tag(123456789)), Some(123456789));
        // Exact wire format sing-box configs carry today.
        assert_eq!(user_tag(123456789), "user_123456789");
        // Parsing -> generation: a parsed tag regenerates byte-identically.
        assert_eq!(user_tag(parse_user_tag("user_42").unwrap()), "user_42");
    }

    #[test]
    fn parser_rejects_non_user_tags() {
        // vless UUID in chains must not be mistaken for a user tag.
        assert_eq!(parse_user_tag("550e8400-e29b-41d4-a716-446655440000"), None);
        assert_eq!(parse_user_tag("relay_7_legacy"), None);
        assert_eq!(parse_user_tag("user_"), None);
        assert_eq!(parse_user_tag("user_abc"), None);
        assert_eq!(parse_user_tag(""), None);
    }

    #[test]
    fn parser_accepts_negative_and_large_ids() {
        // Telegram ids fit i64; keep the parser as wide as the generator.
        assert_eq!(parse_user_tag(&user_tag(i64::MAX)), Some(i64::MAX));
        assert_eq!(parse_user_tag(&user_tag(-1)), Some(-1));
    }
}

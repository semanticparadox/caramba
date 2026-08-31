//! Canonical form of the panel's public host.
//!
//! Payment providers build their IPN/webhook callbacks as
//! `https://{api_domain}/...`, so `api_domain` must be a BARE host. The value
//! falls back to the `panel_url` setting, which operators routinely store with
//! a scheme (`https://panel.example.com`) — concatenating that yields
//! `https://https://panel.example.com/...` and every payment callback is lost
//! silently, with only the pending-session poller as a backstop. Both the
//! startup path (`main.rs`) and the admin "test provider" path go through this.

/// Strip scheme and trailing slash so the value can be interpolated as a host.
pub fn normalize_api_domain(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize_api_domain;

    #[test]
    fn strips_scheme_and_trailing_slash() {
        for input in [
            "https://panel.exarobot.top",
            "http://panel.exarobot.top",
            "https://panel.exarobot.top/",
            "  panel.exarobot.top  ",
            "panel.exarobot.top",
        ] {
            assert_eq!(normalize_api_domain(input), "panel.exarobot.top");
        }
    }

    #[test]
    fn callback_url_never_doubles_the_scheme() {
        let domain = normalize_api_domain("https://panel.exarobot.top");
        let callback = format!("https://{}/api/webhooks/payment/nowpayments", domain);
        assert_eq!(
            callback,
            "https://panel.exarobot.top/api/webhooks/payment/nowpayments"
        );
        assert!(!callback.contains("https://https://"));
    }
}

# CARAMBA Release Timeline

This timeline is derived from the git tag/commit history in this repository and is intended for engineering continuity.

## 1. Pre-semver CI tags

| Tag | Commit | Notes |
|---|---|---|
| `v0.0.0-ci-20260218001259` | historical | Early CI release asset used by installer tests |
| `v0.0.0-ci-20260218002857` | `cdf8a11` | Installer dependency/service setup hardening |
| `v0.0.0-ci-20260218002936` | `24be451` | Hub mode node service path reinstall guard |

## 2. Semver tags (`v0.3.x`)

| Tag | Commit | Primary change summary |
|---|---|---|
| `v0.3.0` | `ef7f037` | Distributed installer roles + secure internal APIs |
| `v0.3.1` | `0180bab` | Installer bootstrap and panel startup stability |
| `v0.3.2` | `71cc26a` | Panel regressions, nodes UI, promo center, bot token flow |
| `v0.3.3` | `3296d5e` | Installer ETXTBSY fix with atomic swap/restart |
| `v0.3.4` | `23996ee` | Legacy node install/control flow restoration |
| `v0.3.5` | `caac43e` | Node creation fixes on legacy schema, port setup simplification |
| `v0.3.6` | `7d64583` | Embedded `modern.css` fallback route |
| `v0.3.7` | `3300466` | Pending node UX + schema compatibility migration |
| `v0.3.8` | `1522cb3` | Node persistence/read flow hardening for schema drift |
| `v0.3.9` | `ccfe960` | Node sync pipeline stabilization + legacy fallbacks |
| `v0.3.10` | `3ab9c27` | Node telemetry visibility and legacy schema hardening |
| `v0.3.11` | `0bd5ab1` | Embedded bot flow restore + bot log isolation |
| `v0.3.12` | `f524b8b` | Uninstall fixes, node/template stabilization, migration unification |
| `v0.3.13` | `6d73582` | Installer/node/template flow stabilization + formatting refresh |
| `v0.3.14` | `207db49` | Subscription/miniapp data path stabilization |
| `v0.3.15` | `e323a62` | Subscription links in admin + miniapp purchase/activation restore |
| `v0.3.16` | `adc3f41` | SNI/link generation fixes + miniapp cache busting + node location UI |
| `v0.3.17` | `abe0673` | Sub endpoint fallback + profile naming format |
| `v0.3.18` | `66c9647` | Active devices correction + miniapp sub/referral data |
| `v0.3.19` | `ddd0381` | Gift code lifecycle + promo management + referral reliability |
| `v0.3.20` | `ddd0381` | Same commit as `v0.3.19` (retagged release marker) |
| `v0.3.21` | `f05e2d2` | Miniapp auth hardening + loading/error handling |
| `v0.3.22` | `f309eb9` | Miniapp auth 502 path fix + home/referral UX |
| `v0.3.23` | `0d45c17` | Sub API proxy loop fix for intermittent 502 auth |
| `v0.3.24` | `5507d40` | jsonwebtoken crypto backend fix for panel auth crashes |
| `v0.3.25` | `075ac3c` | Miniapp auth stabilization + installer TUI + telemetry hardening |
| `v0.3.26` | `db1800d` | Multi-sub stats hardening + device lease tracking |
| `v0.3.27` | `d754a37` | System rollout UX refactor + panel/install routing hardening |
| `v0.3.28` | `eb1d27a` | Miniapp PIN lock UX + `/caramba-api` routing compatibility |
| `v0.3.29` | `3b5fed0` | Reality SNI mismatch fix in generated subscription links/configs |
| `v0.3.30-37` | `various` | macOS-style top bar, UI/UX polish, DOMTokenList JS fixes, unified layouts |
| `v0.3.38` | `latest` | Hiddify header fixes, referrers API implementation, Database NOT NULL constraint resiliency |

## 3. Non-tagged but important commits currently in main history

| Commit | Notes |
|---|---|
| `6fcc530` | Installer non-interactive upgrade preserving existing config |
| `35be744` | Device accounting hardening and traffic quota enforcement logic |
| `6f7657b` | Crypto provider hardening for miniapp auth path |
| `2d7f0a6` | Rich Telegram notifications (media + CTA support) |
| `a4a2951` | Markdown behavior compatibility in notifications |
| `e9f0daf` | Admin rollout UX and notification workflow stabilization |
| `296bf4a` | SNI discovery filters and control-plane blacklisting hardening |
| `c571957`, `b3398ed` | README rewrite and clearer product/install positioning |

## 4. Notes for future release hygiene

1. Avoid dual-tagging different semantic tags to same commit unless intentional and documented.
2. Keep release note content aligned with installer user-facing behavior.
3. For critical runtime fixes (auth, subscription generation), add explicit regression checks in CI where possible.


# Subscription edge: who serves `/sub/<uuid>` and why

Owner-facing complaint this answers:

> «подписочный сервис у нас обратным прокси к ноде в россии подключен … я просил
> делать так чтобы те кто не из россии подписочный сервис напрямую из польши
> брался»

## The topology, as measured (2026-09-04)

```
                                    ┌─────────────────── zeus (Poland, 57.128.240.245) ──┐
 client ── panel.exarobot.top:443 ─▶│ Caddy ─(@panel_routes /sub/*)─▶ caramba-panel :3000│
                                    │                                        ▲           │
 client ── app.exarobot.top:443 ─┐  │ Caddy ─(vhost app.…)──▶ caramba-sub :8080          │
                                 │  └────────────────────────────────────────┼───────────┘
                                 │                                           │
                                 └─▶ veles (RUSSIA, 141.98.191.214)          │
                                     Caddy: reverse_proxy panel.exarobot.top:443
                                                                             │
                    caramba-sub then calls PANEL_URL=http://127.0.0.1:3000 ──┘
                    with `Host: app.exarobot.top` (FRONTEND_DOMAIN)
```

Verified read-only on the live boxes:

| Fact | Command | Result |
|---|---|---|
| `panel.exarobot.top` is the panel, in Poland | `dig +short panel.exarobot.top` | `57.128.240.245` |
| `app.exarobot.top` is the **Russian relay** | `dig +short app.exarobot.top` | `141.98.191.214` |
| veles just proxies back to Poland | `ssh veles 'sudo cat /etc/caddy/Caddyfile'` | `reverse_proxy panel.exarobot.top:443` |
| zeus serves the sub vhost from caramba-sub | `ssh zeus 'sudo cat /etc/caddy/Caddyfile'` | `app.exarobot.top { reverse_proxy 127.0.0.1:8080 }` |
| `/sub/*` on the panel domain is **already routed to the panel** | same Caddyfile | `@panel_routes path … /sub/* → 127.0.0.1:3000` |
| caramba-sub is a byte-for-byte proxy of the panel's `/sub` | `apps/caramba-sub/src/handlers/subscription.rs` | `proxy_to_panel` → `resp.bytes()` forwarded verbatim |
| caramba-sub never trips the redirect | `ssh zeus 'sudo grep FRONTEND_DOMAIN …'` | `FRONTEND_DOMAIN=app.exarobot.top`, `PANEL_URL=http://127.0.0.1:3000` |
| exactly one active relay, and it is RU | `SELECT id,name,country_code,is_relay,status FROM nodes WHERE is_relay` | `2\|Russia\|RU\|t\|active` |

### The 308 was panel code, not Caddy

`settings.subscription_domain = app.exarobot.top`, and `subscription_handler`
redirected **any** request whose `Host` was not that domain:

```
$ curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' \
    https://panel.exarobot.top/sub/feb7e480-…
308 https://app.exarobot.top/sub/feb7e480-…
```

Confirmed to originate in the panel process, not the proxy:

```
$ ssh zeus "curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' \
    -H 'Host: panel.exarobot.top' http://127.0.0.1:3000/sub/<uuid>"
308 https://app.exarobot.top/sub/<uuid>
```

### What it cost, from a US egress (100.6.138.132), 3 samples each

| Path | total |
|---|---|
| **A** what a client actually walks today: `panel` → 308 → `app` (Russia) → Poland | **1.037 / 1.048 / 1.056 s** |
| **B** same body, same box, no Russia hop (`--resolve app.exarobot.top:443:57.128.240.245`) | **0.500 / 0.521 / 0.546 s** |
| C via Russia, no redirect (`https://app.exarobot.top/...`) | 0.537 / 0.619 / 0.673 s |

**A − B ≈ 0.52 s of pure round trip, on every subscription refresh, for every
non-Russian user.** Most of it is the redirect itself (a second TLS handshake to
a second host), not the Russian hop.

### Serving directly changes zero bytes

caramba-sub only proxies, so the "Poland direct" body and the "via Russia" body
are the same bytes out of the same generator:

```
$ ssh zeus "curl -sS -o /tmp/b_a.txt -H 'Host: app.exarobot.top' \
    -H 'X-Forwarded-For: 100.6.138.132' 'http://127.0.0.1:3000/sub/<uuid>?client=clash'; sha256sum /tmp/b_a.txt"
bf166a2913ba84a923af608ef90c7c952a0baaf74f0d3a1246ef9098ec945a57

$ curl -sS -o /tmp/b_b.txt 'https://app.exarobot.top/sub/<uuid>?client=clash'; shasum -a 256 /tmp/b_b.txt
bf166a2913ba84a923af608ef90c7c952a0baaf74f0d3a1246ef9098ec945a57
```

## What changed in code

`apps/caramba-panel/src/subscription.rs` and `apps/caramba-panel/src/api/v2/app.rs`.

The blanket 308 became a decision keyed on the country the panel already detects
for every subscription (the `Subscription geo` log line). Two halves:

1. **`subscription_handler`** redirects to `subscription_domain` only for clients
   that domain actually serves. Everyone else is served the body on the spot.
2. **`resolve_base_url`** (used by `GET /api/v2/app/subscription`) hands out
   `panel_url`-based links to those same clients, so the app's *first* fetch is
   already direct — no 308 to follow at all.

The reasoning that fixes the shape of the thing: **a redirect can never buy
reachability.** To see the 308 you must first have reached the panel. So it never
rescued a censored client — it only ever pinned a client to the mirror's domain,
which is worth something in Russia and is a pure detour everywhere else.

### The setting

`subscription_domain_countries` (panel `settings` table). Absent = `relays`.

| Value | Meaning |
|---|---|
| `relays` *(default)* | Countries where this install has an **active relay node**. Here that is exactly `RU`. |
| `*` | Everyone — the old unconditional behaviour. **Rollback lever.** |
| *(empty string)* | Nobody; the panel always serves the body itself. |
| `RU,BY` | Explicit ISO-2 list. |

Two deliberate edge rules, both covered by tests in `subscription.rs`:

- **Unknown country is never sent to the mirror** (except under `*`). Not
  caution — deduction: the request reached the panel, so the direct path
  demonstrably works for that client. Trading a proven path for a guessed one is
  a downgrade.
- **`relays` with no active relay degrades to `*`.** An install that has a
  subscription domain but no relay is using it as a plain front (CDN, vanity
  domain), and an upgrade must not silently take that away.

### What a Russian client gets — unchanged

- Client on `app.exarobot.top` → veles → zeus → caramba-sub → panel with
  `Host: app.exarobot.top`. Host equals `subscription_domain`, so the redirect
  branch is not even evaluated. Byte-identical to today.
- Client on `panel.exarobot.top` → geo says RU → RU is an active relay country →
  308 to `app.exarobot.top`, exactly as today.
- No redirect loop is possible: caramba-sub builds its request with
  `redirect::Policy::none()` and would return `502 "Panel subscription redirect
  loop"` rather than loop — and it never receives one, because of the Host above.

The redirect stays **308 (permanent)** on purpose: for the cohort the mirror
serves, "remember this address" is exactly right — the client rewrites its stored
URL to the domain that survives a block of the panel domain. Known cost: a
Russian user who moves abroad keeps a cached detour until the subscription URL is
re-issued.

## Caddy / installer: no change required

`apps/caramba-installer/src/setup.rs` already emits `/sub/*` into `@panel_routes`
on the panel domain whenever the sub domain is a *separate* host
(`if !same_domain_sub { main_path_rules.push("/sub/*") }`), which matches the live
Caddyfile. The direct path therefore works on a freshly installed box with no
edit to the installer and no edit to `/etc/caddy/Caddyfile`.

**Do not remove `/sub/*` from `@panel_routes`.** That line is what makes the
direct path exist; without it the panel domain answers 404 and the fix is inert.

## The one real operational dependency: GeoIP

The decision needs the client's country. Resolution order in
`libs/caramba-shared/src/geo_service.rs`: local MaxMind DB → `ip-api.com` →
`ipinfo.io`, cached 24 h in memory per client IP.

**On zeus there is currently no MaxMind database**:

```
$ ssh zeus 'sudo find /opt/caramba /var/lib/caramba /usr/share/GeoIP -maxdepth 3 -iname "*.mmdb"'
(no output)
```

so every cache miss is an outbound call to `ip-api.com` (free tier ≈ 45 req/min
per source IP). Consequences, stated plainly:

- If geo resolution fails, country is `unknown`, and an **RU client hitting the
  panel domain is served directly instead of being pinned to the mirror**. The
  config it receives is still correct; only the mirror pinning is lost, and only
  while geo is down.
- Fix properly: install a GeoLite2 Country DB at
  `/usr/share/GeoIP/GeoLite2-Country.mmdb` (or set `GEOIP_DB_PATH` in
  `/opt/caramba/.env`) and restart the panel. `main.rs` picks it up if the file
  exists and silently falls back to the HTTP services if it does not.
- Stopgap during a geo outage: set `subscription_domain_countries = *` to restore
  the old blanket redirect.
- Alternative that removes the dependency at the edge: have the reverse proxy set
  `X-Country-Code`; the panel prefers that header over GeoIP
  (`cf-ipcountry` is also accepted). Caddy needs a geo plugin for this, so the
  MaxMind DB is the simpler route.

## Where the country goes next

The panel now publishes what it detected, because nothing downstream can work it
out on its own (the client ships no geo database):

- `GET /api/v2/app/subscription` → `"client_country": "US" | null`
- the subscription body response → header `x-client-country: US | unknown`

Both are consumed by the *second* fix in this pass — the routing preset and the
domestic DNS resolver following the user instead of a hardcoded `ru-smart`. See
`libs/caramba-core/api/api.go` (`SetUserCountry`, `homeCountry`),
`libs/caramba-core/profile/profile.go` (`DomesticResolvers`) and
`apps/caramba-client/lib/state/core_config_state.dart`
(`defaultRouteIndexForCountry`, `adoptUserCountry`).

Known gap: caramba-sub forwards only `profile-title`,
`profile-update-interval` and `subscription-userinfo` from the panel response, so
`x-client-country` does **not** survive the `app.exarobot.top` path. Adding it to
that allowlist in `apps/caramba-sub/src/handlers/subscription.rs` is a one-line
follow-up for whoever owns that crate. The JSON field is unaffected — it travels
on the API path, not through caramba-sub.

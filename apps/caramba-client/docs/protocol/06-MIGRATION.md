# CSM/1 Migration

Status: normative, 2026-09-02. Companion to `01-DECISION.md` (rationale), `02-SPEC.md` (protocol behavior), `03-WIRE.md` (byte layout) and `04-THREAT-MODEL.md` (adversaries).

Scope: how CSM/1 reaches a fielded population without breaking it. The four migration phases and the defect each exists to catch; the panel release sequence with its flags, its byte-diff gates and its rollback per step; the signed kill switch; the mini app relay picker migration and its ordering constraint; the legacy compatibility matrix; the device and profile migration for the roughly twenty existing users; and the surfaces that are never removed.

Audience: the panel team (Rust), this session's client and core teams (Dart and Go), and whoever operates the live `exa_robot` tenant during the transition. Assume the reader has `03-WIRE.md` open and nothing else.

Key words MUST, MUST NOT, SHOULD, SHOULD NOT and MAY carry their RFC 2119 meanings. Rationale is not repeated here; it lives in `01-DECISION.md` and is cited by section number.

**Correction notes.** Where this document departs from `01-DECISION.md` or `00-DESIGN-BRIEF.md` because the code says otherwise or the sequencing does not hold, the departure is marked **Correction** and carries its evidence. Section 10 collects them.

---

## 0. The shape of the problem

Three things move at once and they move on different clocks.

1. **The panel** deploys on the operator's schedule. A bad deploy is reverted in minutes. It is the fastest of the three.
2. **The mini app** (`apps/caramba-app`) ships as static assets served by `caramba-sub` at `/app` (`apps/caramba-sub/src/main.rs:78-82`). It reloads on the user's next open. It is fast, but it shares server state with the panel through side effects that are not written down anywhere except in the code.
3. **The Flutter client** ships through two app stores with review latency and a per-user update decision. A fielded release cannot be recalled. `01-DECISION.md` A6 records that RU App Store removal is the expected steady state, so some fielded copies will never update again.

Every rule in this document falls out of that ordering. The panel may change first and must change reversibly. The client may not be relied on to change at all, so anything that must be revertible in the field is carried as signed data (section 4). The mini app sits between them and holds the one ordering constraint that is easy to get wrong (section 5).

A fourth actor is passive and never migrates: the Hiddify, v2rayNG, stock-Clash and sing-box population fetching `/sub/{uuid}`. It is invisible to the protocol and it must remain byte-for-byte unaffected until an operator deliberately turns a flag. Section 8 states what that population is owed, permanently.

---

## 1. The two release trains

The migration has two trains that interlock at four points. Neither train's step numbers imply calendar dates.

**Panel train** (`01-DECISION.md` section 7): P1 through P9 plus block 3. Owned by the panel team. Detailed in section 3.

**Client train** (this session): CR1 through CR4. Each CR is one app store release.

| Release | Contents | Enables |
|---|---|---|
| CR1 | The ladder behind `HTTPDoer` in Go; the CSM parser and verifier in Go and Dart; device key generation and enrollment; the diagnostics screen (B8); the transport rung screen (invariant 17); `http://` refusal and redirect hardening; the monotonic store | Phase 1, shadow |
| CR2 | The Go catalog renderer; both-ways build and local diff; the verification chrome (invariant 19) | Phase 2, verify and compare |
| CR3 | X3 (one fetcher, one verifier, one monotonic store, in the app process; the Network Extension receives a rendered config plus a validity window); the Keep or Revert card (invariant 22); the settings write with body-bound proofs | Phase 3, cutover |
| CR4 | Removal of the per-refresh legacy fetch from the hot path; the legacy render path stays compiled and reachable forever | Phase 4, steady state |

> The phase a client operates in is a property of the installed release, not of the data it receives. A client MUST NOT advance its phase because a document told it to. The only remote lever is the reverse one, section 4.

The reason is asymmetry of harm. Advancing a phase makes previously non-fatal conditions fatal (`03-WIRE.md` section 6.2: verification failures are logged and never fatal in phase 1, and fatal from phase 2 onward). An operator, or anyone who has compromised the online signing key, MUST NOT be able to make a fielded client stricter on demand, because "stricter" here means "refuses to connect". Retreating to the legacy path is safe in the other direction: it is the behavior the client had before CSM/1 existed, it is fully reviewed, and it is what a user in Moscow needs when the new path is broken.

Phase is persisted per profile as `csmPhase` (section 7.1) so that a client whose release supports phase 3 still operates a newly enrolled profile from phase 1 upward. Phase is `min(compiled_phase, profile_phase_reached + 1)` at each refresh: a profile advances one phase at a time and only when the exit criteria in section 2 are met locally for that profile.

---

## 2. The four phases

`01-DECISION.md` C7. Each phase exists to catch one class of defect on real traffic, at a point where that defect cannot yet hurt a paying user.

Common rules for all four:

- The high-water mark store (`03-WIRE.md` section 6.3) is live and persisted from phase 1. It is not switched on at cutover. A monotonic store that has never run in production is not a monotonic store, it is an untested assumption, and the failure mode it protects against (rollback) is silent.
- The `time_floor` (`03-WIRE.md` section 6.4) is established at enrollment in phase 1 and never decreases thereafter, across all phases and across app updates.
- No phase introduces telemetry. `01-DECISION.md` 5.4.6 forbids per-rung reporting and bounds everything else to opt-in, coarse, 24 hour aggregate, over an established tunnel, never on the request that carries device identity. With a population of roughly twenty users, every phase gate below is met by the operator's own devices plus direct contact with each user. **Telemetry MUST NOT be introduced to satisfy a phase gate.**
- Phase transitions are recorded in the diagnostics screen with the timestamp and the release that performed them, because the first support question after a bad cutover is "when did this change".

### 2.1 Minimum dwell

Every phase has a minimum dwell of **14 days** on the live tenant before its exit criteria may be evaluated.

Derivation, so the number is not arbitrary: the default refresh cadence is `ttl = 7200` seconds with `jit = 20` percent (`03-WIRE.md` section 11.6), which is roughly 12 directive fetches per device per day. Fourteen days is roughly 168 refreshes per device and roughly 3400 across a twenty device fleet, which is enough to observe a once-per-few-hundred-refresh defect at least once with margin. It also spans two weekends and at least one panel restart, which is what makes the catalog determinism check in 2.2 meaningful.

> `DWELL_DAYS = 14` is **provisional**. The measurement that changes it is the observed defect rate per thousand refreshes during phase 1: if phase 1 records zero anomalies in 3400 refreshes, the dwell for phases 2 and 3 MAY be reduced to 7 days; if it records more than one unexplained anomaly, the dwell for every subsequent phase doubles.

### 2.2 Phase 1: Shadow

**The defect this phase exists to catch:** three-language divergence in decode and signature verification, and encode nondeterminism in the panel signer, on real documents, before anything the user sees depends on either.

This is the phase that catches the failure `01-DECISION.md` X1 names: two independent verifiers that disagree produce a split brain between what the UI shows and what the tunnel dials, and a negative-fixture corpus alone cannot catch a divergence that only appears on a document shape the fixtures did not anticipate. Production traffic supplies the shapes.

**What runs.** On every refresh the client fetches `/sub/k1`, then the catalog chunks named by the directive, then `/sub/m1/{loc}`, over the full ladder. It parses and verifies each frame under `03-WIRE.md` sections 6.1 and 6.2. It advances the high-water mark. It then **discards the result** and builds its configuration exactly as it does today: `GET /sub/{uuid}?client=clash[&node_id=][&relay_country=]` through `FetchProfile` (`libs/caramba-core/subscription/subscription.go:122-171`), then `profile.AssembleMihomoConfigPinned`.

**What is fatal.** Nothing. A parse or verification failure is recorded with its error code from `03-WIRE.md` section 6.6, surfaced in the diagnostics screen, and otherwise ignored. This is stated normatively in `03-WIRE.md` section 6.2 and it applies only to this phase.

**Entry criteria.**

1. P1 through P6 deployed and stable on the live tenant, each having passed its byte-diff gate (section 3.5).
2. P4 deployed to `caramba-sub` **before** `csm_routes_enabled` is set for any tenant whose `subscription_domain` resolves to `caramba-sub`. Without it, `/sub/k1` matches `/sub/{uuid}` (`apps/caramba-sub/src/main.rs:73-76`) and is proxied as a subscription fetch for a subscription whose uuid is the literal string `k1`, which returns 404 with a text body instead of a key document.
3. The negative corpus in `05-TEST-VECTORS/` green in `cargo test`, `go test` and `flutter test`, with all three returning the **same error code** per fixture, not merely the same verdict.
4. CR1 shipped to both stores and installed by at least one device per platform under the operator's control.
5. `csm_routes_enabled = 1` on the tenant.

**Exit criteria**, all measured over the dwell:

1. Zero parse failures on documents the panel actually signed. Parse failures on documents from a hostile or broken mirror are expected and are counted separately.
2. Zero verification failures with codes `E_VERIFY_SIG`, `E_VERIFY_THRESHOLD`, `E_VERIFY_ROLE`, `E_VERIFY_PID`, `E_VERIFY_CATHASH` on documents the panel signed.
3. **Cross-implementation agreement, measured by replay.** The panel retains every `0x01` key document frame and every `0x02` and `0x04` catalog frame it emits during the dwell (these carry nothing personal; `01-DECISION.md` 5.7.3 and 5.2.4 make the catalog per-tier and byte-identical, and the key document is public). It MUST NOT retain `0x03` or `0x06` frames, which are per-device. The retained corpus is replayed through all three verifiers in CI, and all three MUST agree on the verdict and the code for every frame. Directive divergence is covered instead by synthetic fixtures generated against a test device key, because retaining real directives would be exactly the personal-data retention `01-DECISION.md` 5.7.1 forbids.
4. **Catalog determinism, stated against the content digest.** For an unchanged tier content model, `sha256(catalog frame)` is identical across at least three panel restarts, across two concurrently running panel processes, and across at least one deliberate re-serve. It is **not** required to be identical across a deliberate re-sign, because `iat` is mandatory and moves; a re-sign at a different wall-clock time necessarily produces a different `chash` and a different `cat_id`. What `03-WIRE.md` 1.5 requires, and what this criterion tests, is that the panel **persists** the signed frame keyed by `(tier, content_digest)` and re-signs only when the content digest changes, so a restart serves the same bytes rather than new ones. The test therefore has three parts: change nothing and restart, and assert byte identity; change nothing and run two processes, and assert byte identity; change one node's port, and assert the `chash` changes exactly once and the `ver` increments exactly once.
5. No `E_VERIFY_VERSION` caused by the panel signing a document with a version at or below one it already emitted. If this occurs, it is a panel defect and the fix is to sign a higher version, which is the same fix as in production; the high-water mark is never reset.

**Rollback.** Set `csm_cap_mask` to clear every bit (section 4). Clients keep fetching and verifying CSM documents, because `cap` is carried inside those documents and a client that stopped fetching could never observe the bit clearing, but they act on no capability and lose nothing, because nothing depended on them. Section 4.7 says the same thing and this sentence used to contradict it. Setting `csm_routes_enabled = 0` is also available in this phase only, and only before PNR-1 (section 3.6).

### 2.3 Phase 2: Verify and compare

**The defect this phase exists to catch:** generator divergence between the Rust Clash generator that serves Hiddify and v2rayNG and the Go renderer that will serve Connect, plus geo nondeterminism in the selection the panel signs. This is `01-DECISION.md` A8, the two-renderer tax, and it is the risk with the widest blast radius, because a divergence here means the app's server list and the tunnel's actual outbound disagree.

**What runs.** Everything from phase 1, plus: on every refresh the client renders a mihomo configuration from the verified catalog and the verified directive's `sel`, in Go, and diffs it against the legacy body it fetched. **It dials the legacy body.** The diff is recorded locally and rendered on the diagnostics screen as a per-class count. A verification failure is now fatal for that document (`03-WIRE.md` section 6.2), meaning the document is refused and the client falls back to the last verified document or to the legacy path, but a *diff* is never fatal.

**The comparison rule**, stated exactly, because "diff the configs" is not implementable as written:

> The client MUST parse both YAML documents and compare only the `proxies` sequence and the `proxy-groups` sequence. Every other top level key is owned by `profile.AssembleMihomoConfig` on the client side and is not the renderer's output.
>
> The `proxies` sequences MUST be compared as multisets keyed by the `name` field, and for each matched pair every scalar field MUST be equal, including absent versus empty (an absent `flow` and an empty `flow` are a diff, per `03-WIRE.md` section 5 `fl` value 0, which exists because `subscription_generator.rs:230-232` records that emitting an empty `flow` breaks Happ).
>
> The `proxy-groups` sequences MUST be compared in order, including the `CARAMBA` selector group and, when `csm_clash_relay_chains` is on, the Auto-Relay group and every `dialer-proxy` binding.
>
> A name present in one and absent in the other is diff class `missing` or `extra`. A matched name with unequal fields is diff class `field:<fieldname>`. A different ordering of `proxies` is **not** a diff, because it is a multiset; a different ordering of `proxy-groups` **is**.

**Entry criteria.**

1. Phase 1 exit criteria met.
2. P8 shipped with `csm_clash_relay_chains = 0`, its byte-diff gate green (with the flag off, output is byte-identical to today).
3. P9 shipped: the catalog is one node model and Clash, sing-box and V2Ray are renderers of it.
4. The identical-proxy-name fixture green: the Rust generator and the Go renderer emit the same proxy names for the same node set, including the uniquifier (`01-DECISION.md` 5.2.5). Until P8 lands the Clash path has no uniquifier at all, unlike the sing-box path's `unique_tag` closure at `subscription_generator.rs:1566`, so this fixture is meaningless before P8 and is a hard gate after it.
5. The cross-egress determinism test green (P6): sign a directive from an IP geolocating to one country, fetch the legacy config from an IP geolocating to another, assert the emitted node set and relay set are identical. Today `client_cc` comes from `x-country-code`, `cf-ipcountry` or a GeoIP lookup (`apps/caramba-panel/src/subscription.rs:147-152`) and is in the cache key (`:692`), and the relay filter falls back to it (`:753-758`), so without P6 this test fails by construction.
6. CR2 fielded.

**Exit criteria.**

1. Zero diffs of class `missing`, `extra` or `field:*` across every active subscription and every node shape the fleet emits, for the whole dwell.
2. Zero diffs after a node is added, a node is disabled and an inbound is toggled on the live tenant. These three operations MUST be performed deliberately during the dwell, because they are the operations that expose an ordering defect and they do not otherwise happen weekly.
3. `get_all_active_relay_infos` (`apps/caramba-panel/src/services/subscription_service.rs:1845-1850`) has an explicit `ORDER BY`, and `get_nodes_for_plan` (`libs/caramba-db/src/repositories/node_repo.rs:959-971`, `ORDER BY n.sort_order ASC` with `SELECT DISTINCT`) has a deterministic tiebreaker on `n.id`. Both are verified by a test that runs the same query ten times against a fleet containing at least two nodes with equal `sort_order` and asserts identical output.
4. The verification chrome (invariant 19) shows version, issued, expires, signer fingerprint, verification result and decoded fields for each document currently in use, on the operator's own device, correctly.

**Rollback.** Clear `csm_cap_mask` bit 0. The client stops rendering from the catalog and reverts to phase 1 behavior for as long as the bit is clear. No app release is needed and nothing the user sees changes, because the legacy body was authoritative throughout this phase anyway.

### 2.4 Phase 3: Cutover

**The defect this phase exists to catch:** the operational failure modes that only appear when the tunnel actually depends on the new path. There are four and none of them is visible in phases 1 and 2, because in both the legacy body was still authoritative:

1. Two cores with two work directories, therefore two high-water mark stores, which is a rollback hole rather than defence in depth. Verified: on iOS `packages/caramba_vpn/darwin/Extension/PacketTunnelProvider.swift:147` builds a core inside the Network Extension and `:166` calls `configure(panelUrl, subscriptionID:, accessToken:)` there, while `packages/caramba_vpn/darwin/Classes/CarambaVpnPlugin.swift:313-318` builds a second one under a separate `caramba-tools` work directory. `01-DECISION.md` X3.
2. The 50 MiB Network Extension ceiling, against a build that now carries a renderer as well as an engine.
3. The ladder under real blocking, where a rung that works in a lab fails in the market.
4. The settings write path, whose failure mode is silent: a write that does not land looks exactly like a write that landed and was overridden.

**What runs.** The client dials the configuration it built from the verified catalog and directive. The legacy fetch continues once per refresh and is still diffed, but it is no longer authoritative. The Network Extension receives a rendered configuration plus a validity window and holds no fetcher, no verifier and no high-water mark.

**Entry criteria.**

1. Phase 2 exit criteria met on every supported platform independently. iOS and Android exit separately; a platform that has not met them stays in phase 2.
2. X3 landed: exactly one fetcher, one verifier and one monotonic store, all in the app process.
3. Network Extension peak resident memory measured with mihomo plus the renderer on the oldest supported iOS device, at least 20 percent below the 50 MiB ceiling. `01-DECISION.md` A12 requires this measurement in week one of the whole project, not here; this criterion is the second reading of it, taken against the build that actually cuts over.
4. **The kill switch tested end to end**, twice: once on a staging tenant, and once on at least one real fielded device per platform, by clearing `csm_cap_mask` bit 0, observing the device return to the legacy render path within one refresh interval, restoring the mask, and observing the device return to catalog rendering within one further refresh interval. **Both halves of that are now testable**, which they were not while `02-SPEC.md` 6.5 used the `catalog.cap AND directive.cap` rule: under AND, the cached catalog signed while the mask was `FFFFFFFE` carried bit 0 clear for the whole of its 30-day life, so restoring the mask changed nothing until the catalog was re-signed. `02-SPEC.md` Correction 11 replaces that rule; section 4.3 below carries the precedence this criterion depends on. A kill switch that has never been pulled is not a kill switch, and one that has never been released is not a switch at all.
5. `exph` (the offline grace window, `03-WIRE.md` section 8.3 key 24) set deliberately by the operator, with its consequence rendered beside it in the panel UI (`01-DECISION.md` C5).
6. CR3 approved by both stores and fielded. Store review is on the critical path here and nowhere else in this document.

**Exit criteria.**

1. The full dwell at cutover with no kill switch activation.
2. Every active user reachable by the operator confirms the app connects, on the platform they use. With roughly twenty users this is a list, not a statistic.
3. Zero `E_VERIFY_*` outcomes on paths that are not deliberately hostile.
4. At least one deliberate blackout drill: the operator origin is made unreachable for one hour and every operator device continues to connect on cached documents, per invariant 16, with the configuration age and its source rendered (invariant 21).

**Rollback.** The kill switch, section 4. This is the phase for which the kill switch exists, and after CR3 is fielded it is the only rollback that exists (PNR-2, section 3.6).

### 2.5 Phase 4: Steady state

**The defect this phase exists to catch:** none. It is where scaffolding is removed, and its risk is that removal is done too early. Every item below is gated on a condition, not on a date.

**What runs.** The client stops fetching the legacy body on every refresh. It keeps the legacy fetch and the legacy render compiled, reachable and exercised by a launch-time self test, forever, because the kill switch depends on them.

**Entry criteria.**

1. Phase 3 exit criteria met on every supported platform.
2. **Connect clients are the majority of `/sub/{uuid}` fetches on the tenant**, measured from the panel's own access logs by User-Agent over 7 consecutive days. This is the condition `01-DECISION.md` 4.13 attaches to removing the auto-pin, and it is a property of the tenant, not of the client.
3. The mini app relay picker migration complete through step M5 (section 5.3).
4. The deterministic node tiebreaker from phase 2 exit criterion 3 in place. Removing the auto-pin without it lets a pasted-URL user's exit change silently between fetches, which the pin made sticky.

**What is removed, in this order.**

| Order | Removal | Flag | Precondition |
|---|---|---|---|
| 1 | Relay write on GET (`subscription.rs:744-751`) | `csm_relay_write_on_get = 0` | M5 complete |
| 2 | Explicit node persist on GET (`subscription.rs:636-644`) | `csm_node_persist_explicit = 0` | The mini app reads `last_node_id` from the row that `POST /api/client/subscription/{id}/server` writes, which it already does (`apps/caramba-app/src/exa/lib/useServers.ts:65-72`, `apps/caramba-app/src/exa/pages/Connect.tsx:34`) |
| 3 | Auto-pin on GET (`subscription.rs:617-634`) | `csm_autopin = 0` | Entry criteria 2 and 4 |
| 4 | Device counting by IP | `csm_device_count_mode = thumbprint` | Every active subscription has at least one enrolled device key and no IP-only client has fetched for 30 days (section 7.2) |

Both query parameters keep filtering exactly as they do today after every one of these removals. `01-DECISION.md` 5.4.5: the writes go, the filters stay, so a pasted URL sees no change.

**Exit criteria.** None. This is the terminal state. The migration project is complete when all four removals above have landed and the byte-diff gate for `/sub/{uuid}` has been retained (not retired) as a permanent regression gate, per section 8.

### 2.6 Phase summary

| | Phase 1 Shadow | Phase 2 Compare | Phase 3 Cutover | Phase 4 Steady |
|---|---|---|---|---|
| Config the tunnel dials | legacy | legacy | **catalog** | catalog |
| CSM documents fetched | yes | yes | yes | yes |
| Verification failure | logged | fatal for that document | fatal for that document | fatal for that document |
| Both-ways render and diff | no | yes | yes | no |
| Legacy fetch per refresh | yes | yes | yes | no |
| Client release | CR1 | CR2 | CR3 | CR4 |
| Rollback | clear `csm_cap_mask` | clear bit 0 | clear bit 0 | clear bit 0 |
| Catches | decode and signature divergence | renderer and geo divergence | operational dependence | nothing; removes scaffolding |

---

## 3. The panel release sequence

`01-DECISION.md` section 7. Each step below carries its flag, its byte-diff gate and its rollback. Steps within a block may ship in any order unless an explicit ordering constraint is stated.

### 3.1 Flag registry

All flags live in the panel `settings` table and are read through `state.settings.get_or_default(key, default)`, the same mechanism `subscription_domain` uses at `apps/caramba-panel/src/subscription.rs:113-117`. All are per-tenant.

| Key | Type | Default (existing panel) | Default (new install) | Read by |
|---|---|---|---|---|
| `csm_routes_enabled` | bool | `0` | `1` | route guard on `/sub/k1`, `/sub/c1/*`, `/sub/m1/*`, `/sub/b1/*`, `/sub/r1/*` |
| `csm_cap_mask` | u32 hex | `FFFFFFFF` | `FFFFFFFF` | the catalog and directive signers, section 4 |
| `csm_hpk_generation` | uint | `0` meaning absent | `1` | the catalog builder, `hpkv` (`02-SPEC.md` 10.2) |
| `csm_branding_signed` | bool | `0` | `1` | catalog and directive builders |
| `csm_geo_pinned` | bool | `0` | `1` | directive builder (`sel.rcc`, `sel.nid`) |
| `csm_clash_relay_chains` | bool | `0` | `1` | `generate_clash_config`; sets capability bit 2 |
| `csm_autopin` | bool | `1` | `1` | `subscription.rs:617-634` |
| `csm_node_persist_explicit` | bool | `1` | `1` | `subscription.rs:636-644` |
| `csm_relay_write_on_get` | bool | `1` | `1` | `subscription.rs:744-751` |
| `csm_device_count_mode` | enum `ip` / `union` / `thumbprint` | `ip` | `union` | `subscription.rs:203-263` |
| `csm_first_k1_served_at` | timestamp, write-once | null | null | PNR-1 guard, section 3.6 |

> A flag MUST NOT be read inside a hot path without caching. `csm_cap_mask` in particular is read on every directive signature; it is cached for 60 seconds, which bounds kill switch propagation delay at the panel by 60 seconds on top of the client refresh interval.

> **That 60-second cache does not exist today and building it is a named P3 deliverable, not an assumption.** `SettingsService` is a process-local `HashMap` behind an `RwLock`, filled once by `reload_cache()` in the constructor and mutated only by this process's own `set()` (`apps/caramba-panel/src/settings.rs:14-49`, `:70-85`); `reload_cache` is called nowhere else for this service. There is no TTL and no invalidation path, so a `csm_cap_mask` change written by a second panel instance, by a bot process, or by direct SQL is **never observed at all** by a process that started before it. The kill switch is the only rollback that exists after PNR-2, so its propagation bound cannot rest on a mechanism that is not there.
>
> P3 MUST add a timestamped, per-key TTL read for the CSM flags: a value older than 60 seconds is re-read from the database on next access, and the read failure path keeps the last known value rather than falling back to the compiled default, so a database blip does not silently un-kill a fleet. Until that lands, the panel MUST read `csm_cap_mask` from the database on **every** directive signature, and the 60 in section 4.3's arithmetic becomes 0.

> **The split defaults in the table above are seeded, not compiled.** `get_or_default(key, default)` takes one compiled default and cannot produce `0` on an existing panel and `1` on a new one. The install path MUST write the row: a fresh install seeds `csm_routes_enabled = 1`, `csm_branding_signed = 1`, `csm_geo_pinned = 1`, `csm_clash_relay_chains = 1` and `csm_device_count_mode = union`, and a migration on an existing panel seeds none of them, leaving `get_or_default` to return the existing-panel value. The compiled default for every flag is therefore the existing-panel column.

### 3.2 Block 1: additive, zero observable change

Every step in this block MUST pass the byte-diff gate of section 3.5 with zero differences. That is the definition of "zero observable change" and it is checked, not asserted.

**P1. Enrollment code issuance.** Three issuers writing one row: a scoped admin API token with its own middleware and rate limit mounted beside the bot router (there is no `/api/v2/admin` router and no admin API auth surface today; the admin surface is a server-rendered HTMX UI behind a Redis session cookie plus CSRF middleware), a bot `/invite` command, and a `caramba-panel enroll issue` subcommand. Extend `enrollment_codes` with `label`, `plan_id`, `ephemeral`, `revoked_at`.

- Flag: none. Purely additive schema plus new routes.
- Gate: byte-diff green; plus a migration test asserting existing rows redeem exactly as before.
- Rollback: revert the deploy. Issued codes become unredeemable, which is why no code may be issued to a real user before P3 is stable.
- Blocker for the Apple 2.1(a) demo code: `max_uses: 0` can never redeem, because the validity SELECT requires `used_count < max_uses` (`apps/caramba-panel/src/services/store_service.rs:396-402`). The demo code MUST be `max_uses NULL` meaning unlimited (schema plus predicate change) or a large finite value.

**P2. Panel identity and signing keys.** `caramba-panel csm keygen root`; a `csm_keys` table holding public keys only; the online signing key under its own secret with its own rotation, never `SESSION_SECRET`.

- Flag: none.
- Gate: byte-diff green (nothing on the request path changes); plus a test asserting the tool refuses to write the private half into the panel working directory.
- Rollback: revert the deploy, before PNR-1 only.

**P3. The `csm` module, the read routes, the enrollment routes and the four stores.** Frame encode and sign, payload builders, the locator HMAC with a per-subscription generation column and its index, and `/sub/k1`, `/sub/c1/{cat_id}/{i}`, `/sub/m1/{loc}`, `/sub/b1/{code}`, `/sub/r1/{loc}` on the root router beside `/sub/{uuid}` (`apps/caramba-panel/src/main.rs:1584-1595`), constructed as `03-WIRE.md` 13.1 requires so the compression and header layers do not wrap them.

Four stores and two routes that no earlier draft of this document named, each a deliverable of this step:

| Deliverable | Why |
|---|---|
| `csm_catalogs`, keyed by `(tier, content_digest)`, holding `ver`, `iat`, `chash`, the signed frame and its chunk frames | `03-WIRE.md` 1.5: the panel persists and re-serves rather than re-signing, or `iat` moves and the published tier hash dies at the next restart. Phase 1 exit criterion 4 tests exactly this. |
| `csm_root_docs`, holding imported root-signed `0x01`, `0x05` and `0x08` frames with their versions | the root key is offline (section 3.7), so these frames arrive by import and are served from a table |
| `csm_devices`, holding `dtp`, the signing SPKI, the agreement public key, `rkv`, the hardware tier, the subscription id, the account id and the timestamps | `03-WIRE.md` 13.8 and `02-SPEC.md` 9.6; it is also the store the device list must join against |
| `CSM_LOC_SECRET`, 32 bytes, generated by `caramba-panel csm keygen`, stored with the online signing key secret | `03-WIRE.md` section 4: the locator HMAC key has to come from somewhere, and it MUST NOT be `SESSION_SECRET` or `APP_JWT_SECRET` |
| `POST /api/v2/app/csm/enroll/code` in the **public** group and `POST /api/v2/app/csm/enroll/device` in the **protected** group | `03-WIRE.md` 13.2 and 13.8; one path cannot be in both auth groups, and a first enrollment has no JWT |
| the revised `GET /api/v2/app/devices` query that surfaces a lease-less CSM device | `02-SPEC.md` 9.6; it is an ordering constraint on `csm_device_count_mode` reaching `union` |
| the TTL read for the CSM flags | section 3.1 |
| the content-addressed rule-set and geo store at `/rulesets/{name}/{sha256}` and `/geo/{name}/{sha256}` | `02-SPEC.md` 4.4: a signed hash and a twelve-hourly overwrite cannot both be right. Until it exists, `rs` and `geo` stay empty and capability bit 6 stays clear. |

- Flag: `csm_routes_enabled`. With the flag off the routes are registered but return 404 with an empty body, which is what `03-WIRE.md` section 13.5 requires for an unknown locator anyway, so an early probe learns nothing.
- Ordering constraint: **P4 MUST be deployed before `csm_routes_enabled` is set to 1.**
- Gate: byte-diff green; plus the header-set assertion from `03-WIRE.md` section 13.4 (no `Content-Encoding`, none of the five constant security headers the root router stamps, none of the three legacy subscription headers); plus a test asserting `/sub/m1/{loc}` never calls `track_access` and never enforces the device limit; plus a test asserting the rate limiter on `/sub/m1` and `/sub/r1` fails **closed** on a Redis error, deliberately opposite to the existing limiter at `subscription.rs:155-167`, which fails open.
- Rollback: `csm_routes_enabled = 0`, before PNR-1 only. After PNR-1 the routes MUST keep serving, forever (section 3.6).

**P4. `caramba-sub` passthrough.** Explicit routes for all five CSM paths, cache bypass, verbatim query string forwarding, `X-CSM-Loc` and `X-CSM-Proof` forwarding, and the `subscription_domain` redirect reimplemented or extracted. `03-WIRE.md` section 13.7 is the requirement list and it is exhaustive.

- Flag: none; the routes 404 upstream until `csm_routes_enabled` is on.
- Gate: a `caramba-sub` specific byte-diff gate (section 3.5, second corpus) proving `/sub/{uuid}` through the proxy is unchanged; plus an integration test that fetches each CSM path through `caramba-sub` and asserts the response bytes equal the panel's response bytes exactly.
- Rollback: revert the deploy. Safe at any time before `csm_routes_enabled` is set, and after that only if no tenant's `subscription_domain` points at `caramba-sub`.
- Note on the cache: `caramba-sub` caches successful subscription responses for 300 seconds under `sub:config:{uuid}:{client}:{relay}:{node}` (`apps/caramba-sub/src/handlers/subscription.rs:124-130` and `:224-236`), a key that omits country and variant, while the panel's own key is `sub_config_v5:{uuid}:{client}:{node}:{variant}:{cc}:{relay}` (`subscription.rs:692`) with a 60 second TTL (`:825`). Two subscribers in different countries share one `caramba-sub` entry. CSM routes MUST bypass this cache entirely; the documents are small and content addressing already gives the chunk route its own `Cache-Control: immutable`.

**P5. Branding under the signed surface.**

> **Correction 4 (section 10).** `GET /api/v2/app/branding` MUST NOT be removed or moved. It is registered in the public group before `route_layer(require_app_jwt)` (`apps/caramba-panel/src/api/v2/mod.rs:155-157` and `:207`) and every fielded client calls it. What changes is authority, not existence: the same fields are additionally carried in signed data, a CSM client uses only the signed copy, and the CSM client never opens an operator-supplied URL (`lib/features/branding/powered_by.dart:126` opens `support_url` and `bot_url` with `LaunchMode.externalApplication` today; invariant 10 forbids it).

- Flag: `csm_branding_signed`, controlling only whether the signed copy is emitted.
- Gate: byte-diff green on `/sub/{uuid}`; plus a test asserting the JSON body of `GET /api/v2/app/branding` is byte-identical before and after; plus a test asserting no key, seed or bootstrap material appears on that endpoint.
- Rollback: `csm_branding_signed = 0`.

**P6. Determinism and geo resolution.** Resolve geo server side at directive-signing time and emit explicit `sel.rcc` and `sel.nid` so the legacy config fetch never consults GeoIP. Add `ORDER BY` to the relay query and a deterministic tiebreaker to node ordering.

- Flag: `csm_geo_pinned` for the directive fields. The `ORDER BY` and tiebreaker changes are **unconditional** and are the one place in block 1 where the byte-diff gate is expected to show a difference.
- Gate: run the gate twice. First, record the one-time delta caused by the ordering change and have it reviewed field by field: it may only ever be a reordering of `proxies` entries, never a change of set membership or of any field value. Second, prove stability by running the corpus ten consecutive times against a fleet containing at least two nodes with equal `sort_order` and asserting all ten outputs are byte-identical. Before this change that assertion fails, which is the whole point.
- Rollback: `csm_geo_pinned = 0` for the directive half. The ordering half is not rolled back; a revert would reintroduce nondeterminism.

> **Correction 3 (section 10).** `01-DECISION.md` P6 states that the Clash body is deterministic with respect to randomness and that determinism "can never extend to the v2ray body". The second half overstates it. The two randomization sites are `subscription_generator.rs:884-889` and `:906-910`, both inside `generate_v2ray_config`, and both are reached only when `si.network` is `xhttp`, `splithttp` or `httpupgrade` **and** `si.x_padding_bytes` is unset. A fleet with no such inbound produces a byte-deterministic v2ray body, and a fleet with one produces a body that differs only in the value of `xPaddingBytes=`. That is why the gate in section 3.5 permits exactly one normalization and no others.

### 3.3 Block 2: behavior change behind flags

**P7. Preferences as real state.** New columns, the single write endpoint with body-bound proofs and nonces (`03-WIRE.md` section 13.6), and the removal of the write-on-GET side effects with both query parameters retained as filters.

- Flags: `csm_relay_write_on_get`, `csm_node_persist_explicit`, `csm_autopin`, all defaulting to the current behavior.
- Ordering constraint: see section 5. `csm_relay_write_on_get` MUST NOT be set to 0 until M5 is complete.
- Ordering constraint: `csm_autopin` MUST NOT be set to 0 until phase 4 entry criteria 2 and 4 are met.
- Gate: byte-diff green with every flag at its default; plus a second gate run with each flag flipped individually, whose delta is reviewed and expected (flipping `csm_autopin` changes the body for a subscription with no pin from one proxy to the whole fleet, which is precisely the change `01-DECISION.md` 4.13 refuses to make early).
- Rollback: set the flag back. All three are pure behavior switches with no schema dependency in the reverse direction.

> **Correction 2 (section 10).** `01-DECISION.md` P7 says the fix is to "point it at the new write endpoint, or keep the write behind a flag until it is migrated", which reads as two steps. It is three, because **no relay write endpoint exists anywhere in the panel today.** The node equivalent does exist (`POST /api/client/subscription/{id}/server`, `apps/caramba-panel/src/api/client.rs:187-193`), and `subscriptions.relay_country` is readable (`GET /api/v2/app/subscriptions` returns it, `apps/caramba-panel/src/api/v2/app_account.rs:609` and `:701-704`), and `GET /api/v2/app/relays` lists the countries (`app_account.rs:732-745`), but the **only writer of `subscriptions.relay_country` in the entire codebase is the GET side effect at `subscription.rs:744-751`.** A relay write endpoint is therefore a named deliverable that must be built before the mini app can be pointed anywhere.

**P8. The relay gap.** `generate_clash_config` consumes `relay_nodes` and emits mihomo `dialer-proxy` bindings plus an Auto-Relay group, additively (N exits plus M relays), not as the sing-box path's cartesian product. The missing proxy-name uniquifier is fixed in the same pass.

- Flag: `csm_clash_relay_chains`, default 0 on existing panels and 1 on new installs. With the flag off, output MUST be byte-identical to today, including the absence of the uniquifier.
- The capability bit that unhides the relay picker (bit 2, `03-WIRE.md` section 5.1) is set by this flag and only by this flag. The picker stays dark until the generator is real; `00-DESIGN-BRIEF.md` R13.
- Gate: byte-diff green with the flag off. With the flag on, a separate expected-delta review, plus the Rust/Go identical-proxy-name fixture, plus a proxy count assertion: for a 40 exit, 3 relay fleet the output MUST contain 43 proxies, not 120.
- Rollback: `csm_clash_relay_chains = 0`. The capability bit clears at the next catalog signature and the picker goes dark again. A client that had selected a relay keeps the selection in `sel.relay` but MUST NOT render the control; the selection is honored by the panel until the user changes it.

**P9. The catalog as one node model with several renderers.** The item most likely to overrun (`01-DECISION.md` A8).

- Flag: none; it is a refactor whose observable output must not change.
- Gate: byte-diff green across all four generators. This is the single most important gate in the whole sequence, because P9 touches the code path that serves every existing paying user of every client.
- Rollback: revert the deploy.

### 3.4 Block 3: cleanup

Plan-scope `GET /relays`; put `GET /api/v2/client/recommended` behind auth or fold it into the signed catalog; extend rate limiting across the app router mirroring the bot router's two-layer design and decide fail-open versus fail-closed deliberately; emit both the legacy `Profile-Update-Interval` header and the signed refresh value; enforce mirror ASN diversity at save time by resolution, not by label.

- Gate: byte-diff green on `/sub/{uuid}`; the `/relays` and `/recommended` changes are API-surface changes and get their own contract tests against the fielded client's expectations.
- Rollback: per-item deploy revert. None of these is a point of no return.
- Ordering constraint: plan-scoping `GET /relays` changes what a fielded client's relay picker shows. It MUST land in the same panel deploy as, or after, a client release that tolerates an empty relay list, which CR1 does.

### 3.5 The byte-diff gate, defined exactly

"Nothing any existing client sees changes" is only a claim if it is executable. It is executable as follows.

**Harness.** `apps/caramba-panel/tests/legacy_bytediff.rs`, corpus `apps/caramba-panel/tests/fixtures/legacy_corpus.json`, baselines under `target/legacy-baseline/`.

**Procedure.** On the merge base, run with `--record` to write one baseline file per request key. On the branch, run without it. Any difference fails the build and prints a unified diff of the first 200 differing bytes with 40 bytes of context on each side.

**Request key.** `sha256` of the canonical request description: the method, the path, the query string with parameters sorted by name, then the `User-Agent`, then the `X-Country-Code` header, each on its own line separated by `\n`.

**Corpus dimensions.**

| Dimension | Values | Count |
|---|---|---|
| subscription uuid | every active subscription on the tenant | 20 today |
| client selection | `?client=clash`, `?client=v2ray`, `?client=singbox`, `?client=hiddify`, no `?client=` with no User-Agent (detects `html`, `services/subscription_service.rs:1656-1659`), no `?client=` with a `Mozilla/5.0` User-Agent (detects `html`, `:1672-1673`) | 6 |
| `node_id` | absent, and the subscription's stored `node_id` | 2 |
| `relay_country` | absent, `none`, and one live relay country | 3 |
| `X-Country-Code` | absent, `RU`, `DE` | 3 |
| `variant` | absent for all, plus one live variant for the **two** explicit sing-box selections (`?client=singbox`, `?client=hiddify`) | +1 case each |

That is 20 x 6 x 2 x 3 x 3 = 2160 responses, plus 20 x 2 x 2 x 3 x 3 = 720 sing-box variant cases, for 2880 baselines. The variant dimension multiplies by **2**, not 3: only the two explicit sing-box client selections take a variant, and the User-Agent-detected sing-box case is not in the corpus as a separate variant row. The prose previously said three and the arithmetic said two; two is right and 2880 is self-consistent with it.

**What is compared.** The HTTP status code, the complete ordered list of response headers with their exact names and values, and the body bytes. Not a parsed representation of any of them.

**The one permitted normalization.** In `v2ray` bodies only, every occurrence of the regular expression `xPaddingBytes=[0-9]+` is replaced by `xPaddingBytes=P` before comparison. This is the only normalization the gate performs.

> Adding a second normalization requires an explicit entry in section 10 of this document with its evidence. A normalization is a place where a real regression hides, and each one must cost something to add.

**Two gotchas that make the gate pass vacuously if ignored.**

1. The panel caches generated bodies for 60 seconds under `sub_config_v5:...` (`subscription.rs:692` and `:825`). The harness MUST run against a flushed Redis or against a distinct key prefix, or the branch run returns the merge base's cached bytes and every assertion passes for the wrong reason.
2. `caramba-sub` caches for 300 seconds (`apps/caramba-sub/src/handlers/subscription.rs:229-233`). The main corpus runs against the panel directly. A second, smaller corpus (one client selection, all 20 uuids) runs through `caramba-sub` with its cache disabled, to catch header-forwarding regressions in the proxy; today it forwards only `profile-title`, `profile-update-interval` and `subscription-userinfo` on the response side (`handlers/subscription.rs:196-203`).

### 3.6 Points of no return

Two moments after which a rollback stops being a deploy revert.

**PNR-1: the first key document served.** The panel writes `csm_first_k1_served_at` on the first 200 response from `/sub/k1` after `csm_routes_enabled` was set to 1, once, and never clears it.

> After PNR-1 the tenant MUST keep serving a valid, verifiable key document at `/sub/k1` forever. `csm_routes_enabled` MUST NOT be set back to 0. The panel MUST refuse the setting change and say why.

The reason is invariant 13 and section 6.5: a client that has pinned a root key never falls back to unverified mode, so withdrawing the key document turns every enrolled client into a hard error rather than a graceful downgrade. That is deliberate (it is what closes the one-field downgrade attack) and it means route withdrawal is not a rollback mechanism. After PNR-1 the rollback mechanism is the capability mask.

**PNR-2: the first phase 3 client fielded.** Once CR3 is installed on a device the operator does not control, that device's tunnel depends on the catalog path. From that moment the only rollback is the kill switch. There is no panel deploy that returns that device to phase 2, because phase is a property of the release (section 1).

### 3.7 Root signing is offline, so root-signed artifacts arrive by import

`01-DECISION.md` 5.1.4 and `02-SPEC.md` 10.5 put the root private key off the panel host, and the tool refuses to write it into the panel working directory. Three artifacts are nevertheless root-signed and served by the panel: the key document (7 day lifetime), the reserve pool (7 days), and the bootstrap blob. There is no pipeline for any of them in `01-DECISION.md` section 7 and none in an earlier draft of this document. This is that pipeline, and it is a P2 and P3 deliverable.

| Step | Where | What |
|---|---|---|
| generate | operator's own machine | `caramba-panel csm keygen root`, mnemonic printed once |
| build | panel | `caramba-panel csm build k1 --out k1-N.bin` assembles the **unsigned** payload from `csm_keys`, the revocation list and the current `csm_catalogs` tier hashes, and writes it out |
| sign | operator's own machine | `caramba-panel csm sign --key <mnemonic> k1-N.bin` produces the frame; the subcommand refuses to run when it detects it is on the panel host |
| import | panel | `caramba-panel csm import k1-N.bin` verifies the frame against the pinned root public key in `csm_keys`, checks `ver == current + 1`, and writes it to `csm_root_docs` |

> **Cadence.** The key document MUST be re-signed and imported at least weekly, and MUST be re-signed whenever any served tier's catalog `chash` changes (`02-SPEC.md` 4.3). The reserve pool MUST be re-signed at least weekly. An operator who cannot meet the weekly cadence has chosen a fleet that cannot change between signings, and the panel MUST warn on the admin dashboard when the newest imported key document is within 48 hours of `exp`.

> **What `/sub/k1` serves when the newest imported key document has expired.** It serves it anyway, unchanged, with a 200. It MUST NOT return 404, 503 or an empty body, because PNR-1 forbids withdrawing the key document and `02-SPEC.md` 2.2 makes an expired anchor still a valid anchor. The panel logs and alerts; the client renders the anchor's age under invariant 21. A 503 is reserved for the case where **no** key document has ever been imported.

> **Bootstrap blobs are pre-signed in batches, because a blob cannot be signed after its code is minted.** A blob's payload contains a per-code value, so it cannot be produced before the code exists and it cannot be produced on the panel after it does. The resolution is that codes are generated **offline**, in a batch, signed into blobs on the same machine, and imported together: `caramba-panel csm blobs --count N` on the operator's machine emits N codes and N signed `0x05` frames, and `caramba-panel csm import blobs.tar` writes the codes into `enrollment_codes` with `kind = 'user'` and the frames into `csm_root_docs`. `GET /sub/b1/{code}` returns **404** for a code with no pre-signed blob, which is the same response as an unknown code and leaks nothing. An operator who issues a code through the admin endpoint or the bot gets a code with no blob, which is legitimate: the blob is the offline escape kit, and the online issuers serve the online path.

---

## 4. The signed kill switch

### 4.1 What it must do

Revert a fielded client from building its own configuration to fetching the legacy one, on the operator's command, without an app store release, without the client trusting anything unsigned, and without violating invariant 13.

`01-DECISION.md` section 9 names the specific failure it must survive: "a client on rung R0 with a valid cached document will otherwise stop asking whether a rollback is available".

### 4.2 The mechanism

> **The kill switch is capability bit 0 cleared.**
>
> Bit 0 (`0x00000001`, `03-WIRE.md` section 5.1) means "per-node connection material present in the catalog". When it is clear, the client MUST NOT build a mihomo configuration from the catalog and MUST source its configuration from `GET /sub/{uuid}` instead.
>
> The panel emits `cap = computed_cap AND csm_cap_mask` in both the catalog (key 14) and the directive (key 17). `csm_cap_mask` defaults to `FFFFFFFF`. Setting it to `FFFFFFFE` clears bit 0 fleet wide for that tenant.

**Why a capability bit and not a new field.** `03-WIRE.md` defines no kill switch field, and its reserved capability bits 12 through 31 are specified as "a signer MUST emit zero, a v1 verifier MUST ignore", so a new bit cannot carry meaning to a fielded v1 client. Bit 0 already means exactly the thing that must be revoked, it is already mandatory in both documents, and the client already has the intersection rule (`effective = operator_cap AND client_cap`) and the rule that a gated control is hidden rather than inert. Reusing it costs zero wire bytes and zero new code paths. This is recorded as Correction 1.

**Why this is not a downgrade attack.** Invariant 13 forbids falling back to *unverified* legacy mode. Clearing bit 0 does not do that. The client continues to fetch, parse and verify the key document, the catalog and the directive under the full profile; it continues to enforce the high-water mark, the time floor and the nonce; it continues to refuse an unauthorized signer. What changes is only the **source of the node list and connection parameters**, and the parameters it then uses are the ones named by the signed directive's `sel.nid` and `sel.rcc`. A missing `cap` field remains a hard, non-dismissible error; a present `cap` field with bit 0 clear is signed data and is honored. The distinction is exactly the one invariant 13 draws.

### 4.3 Precedence and propagation

`cap` appears in two documents with different lifetimes: the catalog (30 days) and the directive (1 hour), and during a kill switch they disagree for as long as the cached catalog lives. This document previously observed that `03-WIRE.md` 5.1 did not state which wins and then filled the gap here, while `02-SPEC.md` 6.5 filled it differently with an AND rule. The disagreement is resolved and the rule below is now stated in all three documents: `03-WIRE.md` 5.1 carries it for a reader who has only that document, `02-SPEC.md` 6.5 carries it with its Correction 11, and this section is the operational consequence.

> The `cap` carried in the freshest **verified and unexpired** directive is authoritative. The catalog's `cap` is used only when no unexpired verified directive is held, which is the first-run case and the deep-offline case. The one exception is the four content-presence bits, 0, 4, 5 and 6: a directive cannot set one whose backing array is absent from the bound catalog, because the data it promises is not there.

The AND rule the specification previously carried was safe for clearing a bit and broken for restoring one: a catalog signed while the mask was `FFFFFFFE` would have carried bit 0 clear for the whole of its 30-day life, so section 4.6 below and phase 3 entry criterion 4 were both false under it. Nothing is lost by taking the fresher copy, because both documents are signed by the same `online` role key.

This is what makes the kill switch fast. Propagation is bounded by the directive refresh interval:

```
worst case adoption = ttl * (1 + jit/100) + panel flag cache
                    = 7200 * 1.20 + 60
                    = 8700 seconds, roughly 2 hours 25 minutes
```

at the default `ttl` of 7200 and `jit` of 20 percent (`03-WIRE.md` section 11.6, section 17), and with the 60-second panel flag cache that section 3.1 now requires P3 to build. An operator who needs it faster lowers `ttl` in the directive first; because the new `ttl` only takes effect at the next fetch, going from 7200 down costs one full interval first, so the bound for a two-step reduction to the floor is `8700 + 900 * 1.20 = 9780` seconds, the flag cache being charged once. An operator anticipating a risky cutover SHOULD lower `ttl` to **900** for the cutover window in advance, which puts the bound at `900 * 1.20 + 60 = 1140` seconds, and restore it afterward.

> 900 is the floor, not 300. `02-SPEC.md` 8.6.1 sets `TTL_FLOOR = 900` and requires at least 10 percent jitter regardless of `jit`, so a client will not poll faster than that however low the signed `ttl` goes. The floor was chosen at 900 rather than 1800 precisely so that this acceleration works; an earlier draft of `04-THREAT-MODEL.md` section 4 carried 1800, which would have made the paragraph above silently false.

### 4.4 The R0 problem

A client whose network rungs all fail operates on cached documents (rung R0). Its cached catalog is valid for 30 days and its cached directive, while expired, still connects (invariant 16). Nothing in that state pulls a kill switch.

> A client operating on cached documents MUST continue to attempt a full ladder refresh on the normal `ttl` schedule with jitter. It MUST NOT enter any state in which it stops attempting. There is no "give up" transition, and there is no backoff that exceeds `ttl * (1 + jit/100)`.

A per-attempt exponential backoff is permitted **within** one refresh interval across rungs, and is forbidden **across** intervals. The visible consequence is the configuration age and source chrome that invariant 21 already requires, which now doubles as the user-visible signal that the kill switch cannot reach this device.

The residual, stated rather than hidden: **a client that can reach no rung at all cannot be killed.** Nothing can reach it. It is bounded only by `exph`, the operator-set offline grace window, after which the client stops offering to connect at all (`03-WIRE.md` section 6.5). An operator who wants a tighter bound on unreachable clients sets a shorter `exph` and accepts the corresponding loss of blackout tolerance; `01-DECISION.md` C5 requires that trade to be printed beside the dial.

### 4.5 What the client does with bit 0 clear

1. It MUST NOT render a configuration from the catalog.
2. It MUST fetch `GET /sub/{uuid}?client=clash` through the ladder, appending `&node_id=<sel.nid>` when the directive carries `sel.nid` and `&relay_country=<sel.rcc>` when the directive carries `sel.rcc`, mapping the sentinel `--` to the URL literal `none` (`03-WIRE.md` section 8.3, `02-SPEC.md` 7.3 and its Correction 4; the sentinel is `--`, not `"NO"`, because `NO` is Norway). It MUST NOT append `&variant=` regardless of `sel.variant`, because the Go core does not send that parameter today (`libs/caramba-core/subscription/subscription.go:131-138` sets only `client`, `node_id` and `relay_country`) and `caramba-sub` drops it (`apps/caramba-sub/src/handlers/subscription.rs:11-19`).
3. It MUST continue to verify every CSM document it fetches, at full strictness.
4. It MUST render the server list from the fetched body via `parseServers` (`libs/caramba-core/subscription/subscription.go:288-305`), whose `Server.ID` is the mihomo proxy name.
5. It MUST translate its stored catalog-id exit pin into a proxy name using the cached catalog's `pn` field, by the reverse of the algorithm in section 7.4, and MUST apply the same visible fallback of section 7.5 when the translation is ambiguous or empty.
6. It MUST show the state on the diagnostics screen as "configuration source: provider (legacy)", with the timestamp at which the switch was observed. The user is entitled to know their app changed how it works.
7. It MUST NOT persist bit 0 as a permanent property of the profile. The bit is re-read from every directive.

Settings continue to apply locally through `profile.AssembleMihomoConfigPinned`, exactly as in phase 1, so the kill switch does not cost the user their kill switch, split mode, DNS or routing preset.

### 4.6 The reverse operation, and its constraint

Restoring `csm_cap_mask` to `FFFFFFFF` returns clients to catalog rendering within the same bound as 4.3. There is no separate un-kill mechanism and no state to clear.

> An operator MUST NOT toggle `csm_cap_mask` bit 0 more than once per `ttl` interval. Two toggles inside one interval are not observable as two events by a client, and the resulting behavior depends on which directive that device happened to fetch, which is a race the operator cannot see.

### 4.7 Other bits, and what they do not do

Clearing bit 2 hides the relay picker; that is P8's rollback and is not a kill switch. Clearing bit 3 disables the settings write endpoint from the client's point of view and makes settings local-only, which is the rollback for P7's write path. Clearing bit 1 disables sealing, which MUST NOT be done: an unsealed directive on a mirror exposes `dtp` and the full selection to that mirror, and `01-DECISION.md` BC2 is the reason sealing exists. If sealing must be disabled, the correct operation is to stop serving directives through mirrors, not to unseal them.

Clearing every bit at once (`csm_cap_mask = 00000000`) is the phase 1 and phase 2 rollback of section 2: it stops the client acting on any CSM capability while leaving verification running.

---

## 5. The mini app relay picker migration

### 5.1 What exists today, verified

**Client side.** `apps/caramba-app/src/exa/lib/subscription.ts:31-53`. On first read for a subscription, `subscriptionUrl` derives a relay country from the browser timezone: if `Intl.DateTimeFormat().resolvedOptions().timeZone` is one of twelve Russian zone names it stores `"RU"`, otherwise `"none"`, into `localStorage` under the key `relay_${sub.id}`. It then appends `?relay_country=<value>` to the subscription URL for any value that is neither `auto` nor `none`. That URL is what the user copies (`exa/pages/Connect.tsx:41,296`, `exa/pages/Guide.tsx:44`, `exa/sheets/ClientPickerSheet.tsx:52`).

**Server side.** `apps/caramba-panel/src/subscription.rs:744-751`. When `?relay_country=` is present on a config GET, the handler executes `UPDATE subscriptions SET relay_country = $1 WHERE id = $2`. The comment above it (`:744`) states the purpose. This UPDATE is the **only** writer of that column anywhere in the codebase. The column is read back by `GET /api/v2/app/subscriptions` (`api/v2/app_account.rs:609`, `:701-704`) and used as the second priority in relay selection (`subscription.rs:739-743`, `:753-758`).

So the mini app's picker is not a picker in the ordinary sense. It is a client-side choice that becomes server state as a side effect of the user's VPN client fetching a config, at an unpredictable later time, possibly from a different device.

**The node picker, by contrast, is already correct.** `exa/lib/useServers.ts:65-72` calls `POST /api/client/subscription/{subId}/server` with `{node_id}` (`apps/caramba-panel/src/api/client.rs:187-193`), and `exa/pages/Connect.tsx:34` and `exa/pages/Servers.tsx:32` read `sub.last_node_id` back. Only the relay half is missing.

### 5.2 The ordering constraint

> `csm_relay_write_on_get` MUST NOT be set to 0 before step M5 below is complete.

If the UPDATE is removed first, `subscriptionUrl` keeps writing to `localStorage` and keeps appending the query parameter, the parameter keeps filtering the response (which is correct and stays), but the choice stops persisting. The user's next fetch from a different client, or from the VPN app, which does not carry that query parameter, silently reverts to GeoIP-derived relay selection. The user sees the relay change by itself with no action, on a control that appeared to work.

The reverse ordering, migrating the mini app first, is safe at every intermediate point: the mini app writes through the new endpoint, the old UPDATE still fires when the parameter is present, and both write the same column with the same value.

### 5.3 The sequence

**M1. Build the relay write endpoint.** `POST /api/client/subscription/{id}/relay`, body `{"relay_country": "<ISO-2 uppercase>" | "none"}`, on the same router and behind the same `auth_middleware` as the node endpoint (`api/client.rs:187-193`), validating against the set returned by `GET /api/v2/app/relays` plus the literal `none`, and writing `subscriptions.relay_country`. Closed vocabulary, per invariant 11.

- Gate: byte-diff green (the endpoint is new and the GET path is untouched).
- Rollback: revert the deploy; nothing depends on it yet.

**M2. Point the mini app at it.** `subscriptionUrl` stops writing `localStorage` and stops deriving from timezone. The relay value comes from `sub.relay_country`, which the mini app already receives. A relay picker UI calls M1's endpoint and refetches. The query parameter is still appended to the copied URL when a relay is selected, because it is still a filter and pasted URLs must keep working.

- Migration of existing `localStorage` values: on first load after M2, for each subscription whose `sub.relay_country` is null and whose `localStorage` key `relay_${sub.id}` holds a value other than `none`, the mini app calls M1 once with that value and then deletes the key. If `sub.relay_country` is already set, the server value wins and the key is deleted without a call. This is the only place a timezone-derived value is ever promoted to server state deliberately, and it runs once.
- Gate: a manual check that a fresh browser profile with a Russian timezone and an existing subscription ends with `relay_country = 'RU'` on the server and no `relay_${sub.id}` key in `localStorage`.
- Rollback: redeploy the previous asset bundle. The values already promoted stay, which is correct and idempotent.

**M3. Remove the timezone heuristic entirely.** A new subscription gets no relay until the user picks one, and until then the panel's GeoIP fallback applies, which is the same behavior the heuristic was approximating. This is separated from M2 so that M2 is a pure re-plumbing whose delta is easy to review.

**M4. Wait for the asset cache to drain.** `caramba-sub` serves the mini app from `/app` (`apps/caramba-sub/src/main.rs:78-82`). Every open of the Telegram mini app fetches fresh assets, so the drain is bounded by the longest interval between opens across the user population, not by an HTTP cache. Seven days is sufficient for a twenty user tenant and MUST be confirmed by checking that every active subscription has either a non-null `relay_country` or a recorded call to M1's endpoint.

**M5. Remove the write on GET.** `csm_relay_write_on_get = 0`. The `?relay_country=` parameter keeps filtering, exactly as before.

- Gate: byte-diff green with the flag at its default; a reviewed expected-delta run with the flag flipped, which must show **no body difference at all** (the UPDATE has no effect on the response body, only on the database), and a database assertion that the column is unchanged after a GET carrying the parameter.
- Rollback: `csm_relay_write_on_get = 1`.

### 5.4 The Connect client's relation to this

The Flutter client sends `?relay_country=` through the same path (`libs/caramba-core/subscription/subscription.go:136-138`, driven by `SetRelayCountry` at `libs/caramba-core/api/api.go:575`), so it too has been writing server state as a side effect. After M5 the Connect client's relay choice is carried by the signed settings write (`03-WIRE.md` section 13.6, `pol` key 3) in phase 3 and later, and by M1's endpoint in phases 1 and 2. The important consequence for section 7.6: **`subscriptions.relay_country` already holds every relay choice that has ever taken effect**, from either client, which is why the directive's `sel.rcc` is a better migration source than the client's own stored index.

---

## 6. The legacy compatibility matrix

### 6.1 Byte-identical, permanently

Nothing in CSM/1 changes any of the following, at any phase, with any flag setting. Each is covered by the gate in section 3.5.

| Surface | Anchor |
|---|---|
| `GET /sub/{uuid}` request contract: `client`, `node_id`, `variant`, `relay_country` | `apps/caramba-panel/src/subscription.rs:38-44` |
| The 308 redirect to `subscription_domain` when the `Host` header does not match | `subscription.rs:113-137` |
| The four generators and their content types | section 8 |
| `Subscription-Userinfo`, `Profile-Title`, `Profile-Update-Interval` (value `"2"`, forever) | `subscription.rs:826-845`; invariant 7 |
| The rate limit `rate:sub:{uuid}`, 30 per 60 seconds, failing open | `subscription.rs:155-167` |
| The 403 text bodies on inactive subscription, quota and device limit | `subscription.rs:183`, `:193-197`, `:263` |
| The 404 text body `Requested server not found` | `subscription.rs:614` |
| The HTML landing page for browsers and unknown agents | `subscription.rs:326-557` |
| `caramba-sub`'s `/sub/{uuid}` proxy, its header forwarding and its 300 second cache | `apps/caramba-sub/src/handlers/subscription.rs` |
| `GET /api/v2/app/branding` as a public, unauthenticated JSON endpoint | `api/v2/mod.rs:155-157`; Correction 4 |

The CSM refusal semantics of `03-WIRE.md` section 13.5 (empty non-200 bodies, refusals carried as signed `st` and `rc` in a 200 directive) apply **only to the CSM routes**. They do not change `/sub/{uuid}`, which keeps its text bodies for the clients that display them.

### 6.2 Changes behind a flag

| Change | Flag | With the flag at its default |
|---|---|---|
| Clash relay chains and the proxy-name uniquifier | `csm_clash_relay_chains` | byte-identical to today |
| Auto-pin on first fetch | `csm_autopin` | byte-identical to today |
| Explicit node persist on GET | `csm_node_persist_explicit` | byte-identical to today |
| Relay persist on GET | `csm_relay_write_on_get` | byte-identical to today |
| Device counting by thumbprint | `csm_device_count_mode` | byte-identical to today |
| Signed branding copy | `csm_branding_signed` | not emitted; the JSON endpoint is unchanged either way |
| Explicit `sel.rcc` and `sel.nid` in directives | `csm_geo_pinned` | not emitted; the legacy GET still uses GeoIP |
| The CSM routes themselves | `csm_routes_enabled` | 404 with an empty body |

### 6.3 Changes unconditionally, once

Two, both in P6, both one-time and both in the direction of determinism:

1. `ORDER BY` added to `get_all_active_relay_infos` (`services/subscription_service.rs:1845-1850`, which has none today).
2. A deterministic tiebreaker on `n.id` added to `get_nodes_for_plan` (`libs/caramba-db/src/repositories/node_repo.rs:959-971`, which orders by `sort_order` alone and selects `DISTINCT`, so ties resolve arbitrarily).

Both may change the order of `proxies` entries in a body exactly once, for fleets with ties. Neither may change set membership or any field value, and the gate procedure of section 3.2 P6 is what proves it.

### 6.4 What an un-upgraded operator's client does

Three cases, and they are not symmetric.

**A CSM-capable client enrolling against a panel with no CSM.** `/sub/k1` returns 404 (or, through an un-migrated `caramba-sub`, is proxied as a subscription fetch for the literal uuid `k1` and returns a 404 with a text body). The profile has **not** pinned a root key, so invariant 13 does not apply.

> The client MUST refuse to complete a CSM enrollment against such a panel, MUST NOT pin anything, and MUST offer the legacy paths (account login, pasted subscription URL) with a visible, permanent state on the profile reading that this provider does not support verified configuration. It MUST NOT retry the CSM path silently in the background, because a retry loop against a panel that will never answer is a battery and traffic-shape cost with no upside.

Re-checking is a user action, one button, on the profile screen.

**A CSM-enrolled client whose operator rolls back.** Hard error, per section 6.5. This is why route withdrawal after PNR-1 is forbidden (section 3.6).

**An un-upgraded client against a CSM panel.** Nothing changes. It fetches `/sub/{uuid}`, gets the same bytes it got last week, and never learns CSM exists. This is the entire point of block 1 and it is what the gate in section 3.5 protects.

### 6.5 The sticky-CSM rule

Invariant 13, stated as an implementable rule.

> A `ConnectionProfile` carries a boolean `csmPinned`, persisted in secure storage. It is set to `true` at the instant the first key document for that profile verifies successfully under `03-WIRE.md` section 6.2, together with the `pid` and the `link_pin` that were verified. It is **never** cleared. The only way to clear it is to delete the profile.
>
> Once `csmPinned` is true for a profile, all of the following are hard, non-dismissible errors for that profile, and none of them is a downgrade path:
>
> 1. `/sub/k1` returning anything other than a verifiable key document for the pinned `pid`.
> 2. A directive or catalog whose `cap` field is absent.
> 3. A document whose `pid` does not byte-equal the pinned `pid`.
> 4. Any document **fetched from the pinned origin (rung R1)** that fails `03-WIRE.md` section 6.2 at V4 through V8, **after the whole enabled ladder has been walked in one cycle without any rung producing a document that verifies**.
>
> A hard error means the profile shows an error state naming the failure, offers a retry and offers profile deletion, and does not connect using an unverified configuration. It does **not** mean the app is unusable: other profiles are unaffected, and an already-established tunnel is not torn down (invariant 16 governs the expiry case; this rule governs the authenticity case, and an authenticity failure on a *refresh* MUST NOT disconnect a running tunnel either, it only refuses the new document).

The attack this closes: an adversary who can drop or rewrite one response makes `/sub/k1` return 404, or strips one CBOR key from the directive, and a client that treated either as "this operator does not do CSM" would silently return to an unauthenticated configuration path. That is a one-field downgrade and it is exactly what `01-DECISION.md` invariant 13 exists to forbid.

> **Item 4 is scoped to the pinned origin and to a completed ladder cycle, and it was not before.** An earlier form made *any* V1-through-V8 failure a hard, non-dismissible error for the profile. `03-WIRE.md` 6.2 and `02-SPEC.md` 2.5 say the opposite for exactly those failures: a verification failure does not stop the ladder, the client MAY continue to the next rung, and a hostile mirror is the case the ladder exists for. Under the unscoped rule, one mirror in the R2 pool returning a well-formed frame for another tenant's `pid` would put the profile into a non-dismissible error state offering only retry and profile deletion, which is a remote denial of service available to any party in the mirror pool. Failures on R2, R3, R4 and R5 advance the ladder and are recorded in the attempt history; they do not change profile state. V1 through V3 are dropped from the item because V1 cannot fail (`03-WIRE.md` 6.2) and V2 and V3 are anchor-side conditions rather than statements about the fetched bytes.

The operational cost, stated plainly: an operator who enables CSM has made a one-way commitment for every client that enrolls afterward. The rollback lever from that point is the capability mask, which is a signed instruction and not an absence.

---

## 7. Device and profile migration

Population: roughly twenty subscriptions on the live `exa_robot` tenant. Small enough that every case below can be checked by hand, and it MUST be, because a twenty user fleet with one broken user is a five percent failure rate.

### 7.1 What a client stores today, verified

| Store | Backend | Key | Contents |
|---|---|---|---|
| `ConnectionProfilesStore` | `flutter_secure_storage` (`AndroidOptions(encryptedSharedPreferences: true)`, `IOSOptions(first_unlock)`) | `caramba.connection_profiles` | the whole profile list as one JSON string |
| | | `caramba.active_profile_id` | the active profile id |
| `TokenStore` | same secure storage | `caramba.access_token`, `caramba.refresh_token`, `caramba.user_id` | the JWT pair and user id |
| `PrefsStore` | `shared_preferences` | `caramba.core_config` | the user's core selections as JSON |
| | | `caramba.app_settings`, `caramba.first_run`, `caramba.tunnel_mode`, `caramba.guest_mode` | app-level settings |

Anchors: `apps/caramba-client/lib/data/connection_profiles_store.dart:14-15`, `lib/data/token_store.dart:9-11`, `lib/data/prefs_store.dart:20-27`.

Per-profile fields that matter here (`lib/data/models/connection_profile.dart:40-90`): `id`, `type` (`rawSub` or `panelAccount`), `source`, `panelUrl`, `subscriptionUuid`, `accessToken`, `rawConfig`, `format`, `servers` (a cached `List<ImportedServer>`), `selectedServerId`, `lastProbe`, `serversUpdatedMs`, `brandingCache`, `lastActiveMs`.

Both stores serialize to JSON with per-field defaults on read (`connection_profile.dart` documents the contract explicitly; `prefs_store.dart:17-19` states it for the prefs side), so **every field CSM/1 adds is additive and needs no schema migration.** New fields, all in `ConnectionProfile`:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `csmPid` | hex string, 16 chars | `""` | the pinned tenant identity |
| `csmLinkPin` | string, 20 chars | `""` | the pinned root key fingerprint |
| `csmPinned` | bool | `false` | sticky, section 6.5 |
| `csmPhase` | int | `0` | highest phase reached, section 1 |
| `csmMigratedV1` | bool | `false` | section 7.8 |
| `csmTimeFloor` | int, Unix seconds | `0` | `03-WIRE.md` section 6.4 |
| `csmDtp` | string, 26 chars base32 Crockford | `""` | this device's thumbprint for this profile |

Two stores are added:

- **The monotonic high-water mark store**, one JSON map in secure storage under `caramba.csm_hwm`, keyed by `(pid, doc_type, scope)` per `03-WIRE.md` section 6.3. It MUST live in exactly one place and that place is the app process (X3). The Network Extension MUST NOT hold one. Residual, stated: secure storage is not a hardware rollback guarantee. A local attacker with the device unlocked and the keychain accessible can roll the mark back. The mitigations against that adversary are the time floor and the nonce, not this store, and `04-THREAT-MODEL.md` owns that boundary.
- **The document cache** for rung R0, in the app support directory alongside `config.yaml`, at `csm/<pid>/`: `k1.bin`, `c1-<cat_id>-<i>.bin`, `m1.bin`. Frames, verbatim, as received, because the signature covers the transmitted bytes (invariant 1) and a re-encoded cache is a cache that cannot be verified again.

### 7.2 The device key

Generated at first run of CR1, per profile, non-exportable, P-256, in Secure Enclave or StrongBox where available, with the software tier explicit and recorded at enrollment and visible to both the user and the operator below Android 12 (`PURPOSE_AGREE_KEY` is API 31). `dtp = sha256(device_signing_SPKI_DER)[0..16]` (`03-WIRE.md` section 4).

Enrollment for the existing twenty is the account JWT bridge, not an enrollment code: `POST /api/v2/app/csm/enroll/device`, the protected-group route of `03-WIRE.md` 13.2, authenticated by the account JWT the client already holds in `TokenStore` (`01-DECISION.md` 5.5.5, and its 4.7 is why this bridge must exist). The body and the `X-CSM-Proof` are `03-WIRE.md` 13.8. No user action, no operator action, no support ticket. A user who has been logged out re-logs in and enrolls on the way through.

**The device-limit transition.** The limit is enforced today on the config fetch by apparent source IP (`subscription.rs:203-263`, via `get_active_ips`), and the lease rows carry `last_ip` with a 15 minute window (`api/v2/app_account.rs:632-638`). Switching to thumbprint counting in one step breaks in both directions: an un-upgraded client presents no thumbprint and would count as zero devices, and a mixed household counts twice.

> While `csm_device_count_mode = union`, the device count is the cardinality of the union of two sets over the counting window: the distinct thumbprints presented by requests that carry one, and the distinct source IPs of requests that carry none. It degrades exactly to today's behavior when no client presents a thumbprint, and exactly to thumbprint counting when every client does.

`csm_device_count_mode` moves `ip` to `union` in P7 and `union` to `thumbprint` in phase 4, gated as in section 2.5. `00-DESIGN-BRIEF.md` R9 is retired only when the manifest path stops counting (already required by `03-WIRE.md` section 13.3) **and** the config path counts by thumbprint.

### 7.3 The exit pin lives in two different namespaces

> **Correction 5 (section 10).** `01-DECISION.md` section 9 describes "the server pin mapping from Clash proxy name to a stable node id", which covers only one of the two cases. Verified in the client:
>
> - For a **`rawSub`** profile the pin is `ConnectionProfile.selectedServerId`, which is the **mihomo proxy name**: it is chosen from `profile.servers` (`lib/features/servers/servers_screen.dart:118-132` and `lib/features/autotune/autotune_screen.dart:139`), whose entries are `ImportedServer` with `id` set from the proxy name by the core (`libs/caramba-core/subscription/subscription.go:288-305` sets `Server.ID = p.Name`), and it is persisted in secure storage.
> - For a **`panelAccount`** profile the user's selection is a `Server` from `GET /api/v2/app/servers` whose `id` is the numeric `nodes.id` (`lib/data/models/server.dart:14`, `apps/caramba-panel/src/api/v2/app.rs:248-250`), it is held only in memory by `SelectedServerNotifier` (`lib/state/servers_state.dart:22-32`), and it is **not persisted on the device at all**. The durable pin for that profile is server side, in `subscriptions.node_id`, written by the auto-pin at `subscription.rs:617-634` and the explicit persist at `:636-644`.

Both mappings are specified below and both are needed. On today's tenant the second case is the common one, because the auto-pin has been setting `subscriptions.node_id` for every new subscription since it was introduced.

### 7.4 The mapping algorithm

Catalog node entry ids have the form `n<node_id>i<inbound_id>` (`03-WIRE.md` section 8.2.1, key 1) and the display name `pn` is the verbatim Clash proxy name (key 2).

**Case A, `panelAccount`: numeric node id to catalog id.** The authoritative input is the signed directive's `sel.nid` (`03-WIRE.md` section 8.3, `sel` key 7), which the panel fills from `subscriptions.node_id` under `csm_geo_pinned`.

1. Collect every catalog entry in `ex` whose `id` matches the regular expression `^n<nid>i[0-9]+$` for the value of `sel.nid`, comparing `<nid>` as a decimal string with no leading zeros.
2. Exactly one match: that is the pin. Done.
3. More than one match (the node has several enabled inbounds): choose the entry with the numerically smallest `<inbound_id>`, mark the resolution `provisional`, and record it. A provisional resolution is honored silently for the current session and is re-evaluated at the next catalog change; it does not raise the card of section 7.5, because the node is right and only the inbound is a guess, and the panel's own auto-pin made an equally arbitrary choice.
4. Zero matches (the node was deleted, disabled or moved out of the plan's group): fall back, section 7.5.

**Case B, `rawSub`: proxy name to catalog id.**

1. Collect every catalog entry in `ex` whose `pn` **byte-equals** the stored `selectedServerId`. The comparison is over raw UTF-8 bytes, with no Unicode normalization, no trimming and no case folding. The name is emitted by `format!("{} {}{}", node_label, proto_label, relay_suffix)` at `subscription_generator.rs:1165`, where `format_node_label` returns the country flag emoji alone (`:156-158`), producing strings like `🇩🇪 Stealth`, and the flag is a fixed pair of Regional Indicator code points chosen by a `match` on the country code (`:120-152`). Normalizing would be a place for Rust and Dart to disagree.
2. Exactly one match: that is the pin. Done.
3. More than one match: **unresolvable.** Fall back, section 7.5. This case exists because `generate_clash_config` has no proxy-name uniquifier, unlike the sing-box path's `unique_tag` at `subscription_generator.rs:1566`, so two enabled inbounds of the same protocol shape on two nodes in the same country produce the same name. After P8 lands the uniquifier the names become unique and new pins are resolvable, but a pin captured **before** P8 may still be ambiguous, and it MUST NOT be resolved by guessing.
4. Zero matches: fall back, section 7.5.

**The reverse mapping**, used when the kill switch is active (section 4.5) or in phases 1 and 2 when the legacy body is authoritative: take the stored catalog id, find its entry in the cached catalog, and use that entry's `pn` as the proxy name to pin in the `CARAMBA` selector. When two entries share a `pn`, the pin is ambiguous in the same way and gets the same card.

### 7.5 The visible fallback

> When a pin cannot be resolved, the client MUST set the profile's exit selection to unpinned (automatic) and MUST raise a non-dismissible card on the Servers screen. The card MUST NOT block connecting: the tunnel comes up on automatic selection, which is what the user gets today when they have never picked a server.

The card states, in this order: that the previously pinned server could not be matched to the provider's current server list; the old pin rendered **verbatim as inert text**, with URL-shaped substrings stripped at render and never rendered on the same surface as the verification chrome (invariants 10 and 11, `01-DECISION.md` B3); and a single action, "Choose a server", that opens the picker.

Persistence rules for the old pin value, because it is operator-supplied text and invariant 11 forbids persisting operator-supplied values outside a closed vocabulary:

1. The old pin was already persisted before CSM/1 existed. It MAY be read and rendered once by this card.
2. It MUST be deleted from storage as soon as the user chooses a server, or after 30 days, whichever comes first.
3. It MUST NOT be echoed to the operator in any request, ever, in any phase.

Silently choosing a different node is forbidden. The user pinned a server for a reason the client does not know, and a silent substitution is indistinguishable from the operator moving them.

### 7.6 The relay setting

`CoreConfig.relay` is stored in `shared_preferences` under `caramba.core_config` as an **integer index into a runtime list**, not as a country code (`lib/state/core_config_state.dart:24`, `:101`, `:129`). The list is `Relay.defaults` when the panel list has not loaded and the panel list when it has (`lib/state/core_config_state.dart:226-228`). `Relay.defaults` is `[Выкл, Авто, TR, KZ, FI]` (`lib/data/models/relay.dart:61-76`), so index 2 means Turkey under the defaults and means whatever country sorts first alphabetically under the live list, since `GET /api/v2/app/relays` groups into a `BTreeMap` keyed by country code (`api/v2/app_account.rs:740-745`).

A naive migration that reads the stored index against whichever list happens to be loaded therefore silently changes a user's relay country. It MUST NOT be done.

> **The migration source for the relay is the signed directive's `sel.rcc`, not the stored index.** For a `panelAccount` profile the client takes `sel.rcc`, maps the sentinel `--` to "off" and any two-letter code to that country, writes the result into `pol[3]` as the user's value only when `subscriptions.relay_country` was already set, and discards the stored index. For a `rawSub` profile the relay setting is discarded outright, because the raw path ignores relay entirely (`libs/caramba-core/api/api.go:656-659`: on the raw path the panel is never consulted and `relay` is not applied).

The argument that makes this lossless rather than merely convenient: a local relay choice only ever took effect by being sent as `?relay_country=` on a config fetch, and every such fetch wrote the value into `subscriptions.relay_country` at `subscription.rs:744-751`. A choice that never reached the panel never changed the user's traffic. So the server's value is exactly the set of choices that ever mattered, and the stored index adds nothing except a way to be wrong.

> **What the migration writes for a user with no stored relay, which is nearly all of them.** `subscriptions.relay_country` is null for any subscription whose config was never fetched with `?relay_country=`, and the sole writer of that column is the GET side effect at `subscription.rs:744-751`. For such a user the migration MUST write `pol[3] = ""`, the **unset** state of `02-SPEC.md` 7.3, and MUST NOT write `--`. The two are not the same: unset means the operator resolves, which reproduces today's behavior of falling back to `client_cc` and including same-country relay chains, while `--` means the user chose no relay and would remove relay chaining from every one of those users at cutover. The panel then emits a concrete `sel.rcc` with `pol[3].src = 3`.

Residual, stated: a user who moved the relay picker in the app and then never connected loses that pending choice at migration. It had never taken effect, and the relay picker is capability-gated dark until P8 anyway (bit 2), so on today's tenant the number of affected users is expected to be zero and MUST be confirmed by inspecting `subscriptions.relay_country` against the operator's own device before CR1 ships.

### 7.7 The rest of `caramba.core_config`

`protocol`, `preset`, `stack`, `dns`, `mtu`, `ipv6`, `fakeIp`, `killSwitch` and `split` stay exactly where they are and keep their current semantics. CSM/1 adds provenance (`src`, `03-WIRE.md` section 5) on top, and the settings vocabulary is the `CorePolicy` string set, never `CoreConfig` indices (`01-DECISION.md` 5.4.1). Two migration rules:

1. **`corePolicyFrom` needs its inverse.** `lib/state/core_policy_mapping.dart:45` maps config to policy; the reverse is required in the same file so a fetched `pol` repopulates the pickers. Without it settings sync is write-only and a second device shows stale UI over correct behavior, which reads as a bug forever.
2. **The first directive establishes the baseline silently.** At the first directive a profile receives after migration, every locally stored setting that differs from the operator value is marked `src = user` and every setting that matches is marked `src = default`. **No Keep or Revert card fires on that first directive.** Cards fire only on changes observed after the baseline. Without this rule every existing user gets a wall of cards on upgrade for settings they set months ago, which trains them to dismiss the card that matters.

`split.apps` is not migrated because it never moves: invariant 15, enforced by the client's own serializer in both directions.

### 7.8 Ordering and idempotency

The per-profile migration runs at most once, guarded by `csmMigratedV1`, in this order:

1. Generate the device key and enroll it (7.2). Failure here aborts the migration for this profile and it retries at the next refresh; nothing is written.
2. Fetch and verify the key document. On success set `csmPid`, `csmLinkPin`, `csmPinned = true` and `csmTimeFloor`. **This is the point of no return for the profile** (section 6.5).
3. Fetch and verify the catalog and the directive.
4. Resolve the exit pin (7.4) and the relay (7.6).
5. Establish the settings baseline (7.7 rule 2).
6. Set `csmMigratedV1 = true` and `csmPhase = 1`.

Steps 1 through 3 are re-runnable and write nothing until they succeed. Steps 4 and 5 are idempotent. If step 4 falls back, the fallback state is re-evaluated at every catalog change until the user chooses a server, and `csmMigratedV1` is still set, so the migration does not re-run from step 1.

An `http://` source is handled before any of this. `normalizePanelUrl` accepts plain http today (`lib/data/models/enrollment.dart:60-71`) and `fetchSubscriptionBody` sets `followRedirects: true` with no size cap and no per-hop validation (`lib/data/subscription_fetch.dart:22-49`). CR1 refuses `http://` for every manifest, config, rule-set and geo fetch, with the `.onion` exception (invariant 8).

> An existing profile whose stored `source` or `panelUrl` uses `http://` MUST NOT be auto-upgraded to `https://` and MUST NOT be probed. It shows a blocking card on that profile naming the problem and offering re-entry. Probing the http origin to see whether https works would send the very request the rule exists to prevent.

Before CR1 ships, the operator MUST check the tenant's `subscription_domain` and the twenty stored profile sources for `http://`. If the tenant's own subscription domain is http-only, CR1 is blocked until that is fixed, because otherwise CR1 breaks every user at once.

### 7.9 The twenty, concretely

The migration is small enough to be a checklist rather than a statistic. Before phase 3 entry, the operator MUST have, for each active subscription:

1. The profile type on each of the user's devices (`rawSub` or `panelAccount`), which decides which mapping case applies.
2. Whether `subscriptions.node_id` is set, and whether the node it names is still active and still in the subscription's plan group.
3. Whether `subscriptions.relay_country` is set, and whether it names a country that still has an active relay node.
4. Whether the account has a valid refresh token, because a logged-out account cannot enroll a device key without the user logging in.
5. Whether the stored source uses `http://`.

Items 2 and 3 are one SQL query. Item 4 is one query against the refresh token table. Items 1 and 5 require the device, so they are answered by the user's first CR1 launch, and the migration is designed so that a wrong answer to either produces a visible card rather than a silent failure.

---

## 8. What is never removed

> `GET /sub/{uuid}` and the four generators are permanent surfaces of the panel. They are not deprecated, they are not scheduled for removal, and no `dep` entry naming them may ever be signed.

The four generators, with their anchors:

| Generator | Selected by | Function | Content type |
|---|---|---|---|
| HTML landing page | no `?client=` and a browser or absent User-Agent (`services/subscription_service.rs:1656-1659`, `:1672-1673`) | inline at `subscription.rs:326-557` | `text/html` |
| Clash / mihomo | `?client=clash`, or a `clash` or `stash` User-Agent (`:1663-1664`) | `generate_clash_config`, `singbox/subscription_generator.rs:1138-1547` | `text/yaml; charset=utf-8` |
| V2Ray / Xray URI list | `?client=v2ray`, or `v2ray`, `xray`, `fair`, `shadowrocket` or `happ` (`:1665-1671`) | `generate_v2ray_config`, `subscription_generator.rs:810-1137` | `text/plain; charset=utf-8` |
| sing-box | `?client=singbox`, `?client=hiddify` (aliased at `subscription.rs:659-660`), a `hiddify` or `sing-box` User-Agent (`:1661-1662`), or anything unrecognized (`:1674-1675`) | `generate_singbox_config`, `subscription_generator.rs:1548-1924` | `application/json; charset=utf-8` |

Why this is absolute, not merely a current intention:

1. **The population is real and it is the paying one.** Hiddify, v2rayNG, stock Clash and sing-box users in Russia receive an 18 to 25 KB YAML or JSON in one connection today. CSM/1 does not reach them and was never designed to.
2. **The kill switch depends on it.** Section 4.5 requires a fielded Connect client to be able to fall back to `GET /sub/{uuid}?client=clash` at any moment, forever. A removed endpoint is a removed rollback.
3. **`01-DECISION.md` 4.13** refuses to remove the auto-pin before Connect clients are the majority for exactly this population's sake, and that reasoning does not expire when they become the majority; it only stops binding on the auto-pin.
4. **The deprecation channel does not apply.** `01-DECISION.md` B7 requires a signed, dated `dep` entry with a minimum 180 day notice for any withdrawn surface (`03-WIRE.md` section 8.1). This document does not place `/sub/{uuid}` or any generator on that path. A future decision to withdraw one would be a `spec_version` change and a re-review, not a `dep` entry.

Also never removed, for the same reasons: the three legacy response headers with `Profile-Update-Interval` fixed at `"2"` (invariant 7); the `subscription_domain` 308 redirect; `caramba-sub`'s `/sub/{uuid}` route; and the `?client=`, `?node_id=`, `?variant=` and `?relay_country=` query parameters as **filters**, which survive every write-removal in section 2.5.

The byte-diff gate of section 3.5 is not retired at the end of the migration. It becomes the permanent regression gate for this section.

---

## 9. Rollback catalogue

One table, for the person deciding at 3am.

| Situation | Lever | Effect visible in | Available until |
|---|---|---|---|
| A block 1 deploy is bad | revert the deploy | immediately | PNR-1 (P2, P3 only) |
| CSM routes are misbehaving before anyone enrolled | `csm_routes_enabled = 0` | immediately | PNR-1 |
| Clash relay chains are wrong | `csm_clash_relay_chains = 0` | next config fetch, 60 s cache | always |
| The settings write path is wrong | clear `csm_cap_mask` bit 3 | one `ttl` | always |
| The relay picker is a placebo | clear `csm_cap_mask` bit 2 | one `ttl` | always |
| Client-built configs are wrong or unsafe | clear `csm_cap_mask` bit 0 (the kill switch) | one `ttl`, 8700 s at defaults | always |
| Everything CSM is wrong, nobody has cut over | `csm_cap_mask = 00000000` | one `ttl` | always |
| The mini app relay migration went wrong | redeploy the previous asset bundle; `csm_relay_write_on_get = 1` | next mini app open | always |
| The auto-pin removal broke pasted-URL users | `csm_autopin = 1` | next config fetch | always |
| Device counting broke a household | `csm_device_count_mode = ip` | next config fetch | always |
| An online signing key is compromised | root-signed key document with the `kid` in `rev` | one key document refresh, worst case 7 days (`01-DECISION.md` A2) | always |
| A node is seized | node id in `rev` in the key document | one key document refresh; honored against the cached catalog even offline | always |
| A locator leaked | increment that subscription's `gen` column | next directive fetch | always |

Not in the table, because they are not levers: withdrawing the key document after PNR-1 (forbidden, section 3.6); returning a fielded CR3 client to phase 2 (impossible, section 1); reaching a client that can contact no rung at all (impossible, section 4.4).

---

## 10. Corrections to the inputs

**Correction 1: the kill switch has no wire field, and must not get one.** `01-DECISION.md` section 9 requires "the signed kill switch that reverts a fielded client to the legacy path without an app-store release". `03-WIRE.md` defines no such field in any document type, and its capability bits 12 through 31 are specified as "a signer MUST emit zero, a v1 verifier MUST ignore", so a newly allocated bit is invisible to every already-fielded v1 client, which is precisely the population the kill switch exists for. Implemented instead as capability **bit 0 cleared**, through a panel-side AND mask (`csm_cap_mask`) applied at signing time to both the catalog and the directive. Bit 0 already carries exactly the meaning being revoked, it is mandatory in both documents, and the client already has the intersection rule and the hide-the-control rule. Zero wire bytes, zero new code paths, and it works on a client shipped before anyone thought of it. Section 4.

**Correction 2: the mini app relay migration is three steps, not two.** `01-DECISION.md` P7 says "Point it at the new write endpoint, or keep the write behind a flag until it is migrated". Verified: **no relay write endpoint exists**. The node equivalent does (`POST /api/client/subscription/{id}/server`, `apps/caramba-panel/src/api/client.rs:187-193`), `subscriptions.relay_country` is readable through `GET /api/v2/app/subscriptions` (`api/v2/app_account.rs:609`, `:701-704`) and the countries are listed by `GET /api/v2/app/relays` (`app_account.rs:732-745`), but the sole writer of the column in the whole codebase is the GET side effect at `subscription.rs:744-751`. Building the endpoint is a named deliverable ordered before the mini app change, which is ordered before the write removal. Section 5.3, steps M1, M2, M5.

**Correction 3: the v2ray body is nearly deterministic, and the gate can exploit that.** `01-DECISION.md` P6 states that determinism "can never extend to the v2ray body without removing that padding or seeding it deterministically". The two randomization sites are `subscription_generator.rs:884-889` and `:906-910`, and both are reached only when `si.network` is `xhttp`, `splithttp` or `httpupgrade` **and** `si.x_padding_bytes` is unset. A fleet with no such inbound produces a byte-deterministic v2ray body; a fleet with one produces a body differing only in the value of `xPaddingBytes=`. The byte-diff gate therefore covers all four generators, with exactly one permitted normalization. Section 3.5.

**Correction 4: `GET /api/v2/app/branding` must not be moved or removed.** `01-DECISION.md` X2 says "Move `GET /api/v2/app/branding` under the signed surface" and P5 says "Move the fields under signed data before anything else ships". Read as a move, it breaks every fielded client, because the endpoint is public, unauthenticated and called at startup (`api/v2/mod.rs:155-157`, registered before `route_layer(require_app_jwt)` at `:207`). The security problem X2 identifies is real but it is one of **authority**, not existence: the client opens operator-supplied `support_url` and `bot_url` externally (`lib/features/branding/powered_by.dart:126`), which invariant 10 forbids. The endpoint stays and keeps returning identical JSON; the same fields are additionally carried in signed data; a CSM client uses only the signed copy and opens nothing. Sections 3.2 P5 and 6.1.

**Correction 5: the exit pin lives in two namespaces, and the decision record names one.** `01-DECISION.md` section 9 asks for "the server-pin mapping from Clash proxy name to stable node id". That covers `rawSub` profiles, where `selectedServerId` is the mihomo proxy name (`lib/data/models/connection_profile.dart:75-77`; `Server.ID = p.Name` at `libs/caramba-core/subscription/subscription.go:288-305`). It does not cover `panelAccount` profiles, where the user's selection is a numeric `nodes.id` from `GET /api/v2/app/servers` (`lib/data/models/server.dart:14`, `api/v2/app.rs:248-250`), is held only in memory (`lib/state/servers_state.dart:22-32`), and is durably stored **server side** in `subscriptions.node_id`. On today's tenant the second case is the common one, because the auto-pin has been writing that column for every new subscription. Both mappings are specified. Section 7.3 and 7.4.

**Correction 6: the relay setting must be migrated from the server, not from the device.** `CoreConfig.relay` is an integer index into a runtime list whose contents depend on whether `GET /api/v2/app/relays` had loaded (`lib/state/core_config_state.dart:24,101,129`, `:226-228`; `Relay.defaults` at `lib/data/models/relay.dart:61-76`). Index 2 means Turkey under the defaults and means the alphabetically first live relay country under the panel list, since the panel groups into a `BTreeMap` (`api/v2/app_account.rs:740-745`). Migrating from the stored index silently changes a user's relay country. Migrating from the signed directive's `sel.rcc` is exact, because a local relay choice only ever took effect by being sent as `?relay_country=`, and every such fetch wrote the value into `subscriptions.relay_country`. Section 7.6.

**Correction 7: the node ordering anchor is in `caramba-db`, not the panel.** `01-DECISION.md` P6 cites "`node_repo.rs:959-967`". The file is `libs/caramba-db/src/repositories/node_repo.rs` and the query is `get_nodes_for_plan` at `:959-971`: `SELECT DISTINCT n.* ... ORDER BY n.sort_order ASC`, so equal `sort_order` values resolve in arbitrary order and `filtered_nodes.first()` in the auto-pin (`subscription.rs:617-634`) therefore has no defined answer for a fleet with ties. The relay query `get_all_active_relay_infos` at `apps/caramba-panel/src/services/subscription_service.rs:1845-1850` has no `ORDER BY` at all, as the decision record says. Section 6.3.

**Correction 8: `variant` is not a legacy-path blocker.** `03-WIRE.md` section 13.7 point 4 and capability bit 10 track `caramba-sub` silently dropping `variant`. That matters for a CSM directive carrying a non-default `sel.variant`, but it does not affect the legacy Connect path in any phase, because the Go core never sends the parameter: `FetchProfile` sets only `client`, `node_id` and `relay_country` (`libs/caramba-core/subscription/subscription.go:131-138`). The kill switch path in section 4.5 therefore explicitly does not append `&variant=`.

**Correction 9: two client-side hardening items change behavior for existing users and are listed, not assumed.** `FetchProfile` reads the response body with an unbounded `io.ReadAll` (`libs/caramba-core/subscription/subscription.go:161`), with no `io.LimitReader`, and the Dart `fetchSubscriptionBody` sets `followRedirects: true` with no size cap and no per-hop scheme or origin validation (`lib/data/subscription_fetch.dart:22-49`). `01-DECISION.md` 5.3.4 and 5.1.8 require both to change. They land in CR1 and they can break an existing profile that relies on a redirect chain or an `http://` origin, so they are migration items with a visible failure mode (section 7.8), not silent prerequisites.

**Correction 10: `01-DECISION.md` C7 does not say who advances a phase.** The four phases are described as a process without naming the authority that moves a client between them. Assigned here to the installed app release, with remote phase advance explicitly forbidden and remote retreat (the kill switch) explicitly allowed, because advancing makes previously non-fatal conditions fatal and that lever must not be reachable by a compromised online signing key. Section 1.

**Correction 11: `csm_cap_mask` is now the freshest verified directive's copy, and the AND rule is withdrawn.** Section 4.3 previously observed that `03-WIRE.md` 5.1 stated no precedence and filled the gap here, while `02-SPEC.md` 6.5 filled it with `catalog.cap AND directive.cap`. Two documents cannot both be right about the only rollback that exists after PNR-2. The AND rule is withdrawn: it made clearing a bit work and restoring one impossible for a catalog lifetime, so section 4.6 and phase 3 entry criterion 4 were both false under it. The rule is now stated in `03-WIRE.md` 5.1, `02-SPEC.md` 6.5 and section 4.3.

**Correction 12: the panel flag cache the kill-switch arithmetic rests on does not exist.** `SettingsService` is a process-local `HashMap` filled once at construction with no TTL and no invalidation path (`apps/caramba-panel/src/settings.rs:15-23`, `:25-40`, `:70-85`), so a `csm_cap_mask` change written by another process is never observed. Section 3.1 makes the 60-second TTL read a P3 deliverable and states what the panel does until it lands. The same paragraph fixes the split install defaults, which `get_or_default` cannot express with one compiled default and which are seeded at install instead.

**Correction 13: root-signed artifacts need a pipeline and there was none.** The root key is offline by `01-DECISION.md` 5.1.4, and three served documents are root-signed, one of which cannot be signed before its enrollment code exists. Section 3.7 specifies the build, sign and import cycle, the `csm_root_docs` table, the weekly cadence, batch blob pre-signing, and what `/sub/k1` serves when the newest imported key document has expired.

**Correction 14: the phase 1 catalog determinism criterion was unpassable as written.** "Identical across at least one deliberate re-sign" cannot hold while `iat` is mandatory and moves. Phase 1 exit criterion 4 is restated against the content digest of `03-WIRE.md` 1.5, and the persisted-frame storage is a P3 deliverable.

**Correction 15: the no-relay sentinel is `--`.** Sections 4.5 and 7.6 said `"NO"`, `02-SPEC.md` 7.3 Correction 4 says `--`, and the shipped corpus carries `--`. `NO` is Norway, so a panel emitting it would silently disable relaying for exactly the operator who has a Norwegian relay. Both sites now say `--`. Section 7.6 additionally states what the migration writes for a user with no stored relay, which is the unset empty string and not the sentinel.

**Correction 16: the sticky-CSM rule made a hostile mirror a fleet-visible brick.** Section 6.5 item 4 previously made any V1-through-V8 failure a non-dismissible profile error, against `03-WIRE.md` 6.2 and `02-SPEC.md` 2.5, which both say a verification failure advances the ladder. It is now scoped to the pinned origin, to V4 through V8, and to a completed ladder cycle.

---

## 11. Constant and flag summary

| Name | Value | Where | Provisional |
|---|---|---|---|
| `DWELL_DAYS` | 14 | section 2.1 | **yes**: observed defect rate per thousand refreshes in phase 1 |
| kill switch adoption bound, default `ttl` | 8700 s | section 4.3 | no (derived from `ttl` 7200, `jit` 20, flag cache 60) |
| kill switch adoption bound, `ttl` 900 | 1140 s | section 4.3 | no |
| two-step `ttl` reduction bound, to the floor | 9780 s | section 4.3 | no |
| client-side `ttl` floor | 900 s (`02-SPEC.md` 8.6.1) | section 4.3 | no |
| panel flag cache | 60 s, **once P3 builds it**; 0 until then | section 3.1 | no |
| root key document re-sign cadence | 7 days, and on any served tier `chash` change | section 3.7 | no |
| key document expiry warning threshold | 48 hours | section 3.7 | no |
| byte-diff corpus size | 2880 baselines | section 3.5 | no (20 subscriptions today; scales with the tenant) |
| permitted gate normalizations | 1 (`xPaddingBytes=`) | section 3.5 | no |
| mini app asset drain | 7 days | section 5.3 M4 | no |
| old-pin retention after a failed mapping | 30 days | section 7.5 | no |
| IP-only client silence before `thumbprint` mode | 30 days | section 2.5 | no |
| ordering-stability repetitions | 10 runs | section 3.2 P6 | no |
| Network Extension headroom target | 20 percent below 50 MiB | section 2.4 | **yes**: the measurement itself, `01-DECISION.md` A12 |

**Flags:** `csm_routes_enabled`, `csm_cap_mask`, `csm_branding_signed`, `csm_geo_pinned`, `csm_clash_relay_chains`, `csm_autopin`, `csm_node_persist_explicit`, `csm_relay_write_on_get`, `csm_device_count_mode`, `csm_first_k1_served_at`. Defaults and readers in section 3.1.

**Client fields added:** `csmPid`, `csmLinkPin`, `csmPinned`, `csmPhase`, `csmMigratedV1`, `csmTimeFloor`, `csmDtp` on `ConnectionProfile`; the stores `caramba.csm_hwm` (secure storage) and `csm/<pid>/` (app support directory). Section 7.1, with the per-item integrity requirements in `02-SPEC.md` 8.8.3.

**Panel stores added:** `csm_catalogs`, `csm_root_docs`, `csm_devices`, `csm_keys`, the `CSM_LOC_SECRET`, the `kind` column on `enrollment_codes`, the `dtp` column on `subscription_device_leases`, and the content-addressed rule-set and geo store. Sections 3.2 P2 and P3, and 3.7.

---

## Changelog

One review pass, 2026-09-02, by three reviewers reading the whole set for cross-document consistency, panel implementability and client implementability. What it changed in this document:

**Blocking**

- Sections 4.5 and 7.6 now use `--` as the no-relay sentinel. They were the last two sites in the set still carrying the retracted `"NO"`, which is Norway, and the panel is the writer of that field.
- Section 4.3 resolves the `cap` precedence disagreement with `02-SPEC.md` 6.5 rather than filling a gap the other document had already filled differently. Under the withdrawn AND rule the un-kill in 4.6 and phase 3 entry criterion 4 were both false.
- Section 2.2 exit criterion 4 is restated against a content digest, because "identical across a re-sign" cannot hold while `iat` moves, and the persisted-frame storage is now a named P3 deliverable.
- Section 3.1 states that the 60-second panel flag cache the kill-switch arithmetic depends on does not exist, and makes building it a P3 deliverable; the split install defaults are seeded rather than compiled.
- Section 3.7 is new: the offline root-signing pipeline, its cadence, batch blob pre-signing, and what `/sub/k1` serves when the newest key document has expired.
- Section 6.5 item 4 is scoped to the pinned origin and a completed ladder cycle, closing a remote denial of service available to any party in the mirror pool.

**Serious**

- Section 3.2 P3 enumerates the four panel stores and the two enrollment routes that no earlier draft named, including the enrollment split across the public and protected auth groups.
- Section 4.3's two-step reduction bound is corrected to 9780 seconds and tied to the 900-second client-side `ttl` floor.
- Section 7.6 states what the relay migration writes for a user with no stored relay, which is the unset empty string and not the sentinel.
- Section 2.2's rollback sentence no longer says clients "stop fetching CSM documents entirely", which contradicted section 4.7 and would have made the mask unobservable.
- Section 3.2 P3 carries the content-addressed rule-set store, without which a signed resource hash goes stale within twelve hours of every signature.

**Minor**

- Section 3.5's corpus dimension prose said three sing-box variant cases where the arithmetic and the 2880 total say two.
- Phase 3 entry criterion 4 now requires the kill switch to be released as well as pulled.
- Section 11 adds the panel-side store list and the new constants.

Six new corrections, 11 through 16, are recorded in section 10.

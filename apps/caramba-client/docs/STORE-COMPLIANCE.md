# Store compliance for Caramba Connect

This is the operator runbook for passing the Apple App Store and Google Play
review with the Caramba Connect client. It pairs with the VPN entitlement and
build steps in `apps/caramba-client/INTEGRATION.md`. Nothing here was submitted
in this run; this is the checklist and the source-of-truth copy to use.

Reference peers that ship this kind of config-and-subscription VPN client:
Hiddify, Streisand, v2rayNG, Shadowrocket, FoXray. The rules below keep the app
in the same lane they occupy.

## 1. Positioning

The app is a privacy and anti-censorship client. The user supplies their own
subscription or panel account; the app connects to servers the user or their
operator already runs. State this plainly in the listing and the review notes.
Do not promise anonymity, and do not market any use for illegal activity. The
listing prose must follow `apps/caramba-client/ANTI-SLOP.md` (no em-dash, no
slop words, plain functional language).

## 2. No dynamic executable code

Apple guideline 2.5.2 and Play policy both forbid loading executable code at
runtime. The client is clean on this point and must stay clean:

- A subscription is config DATA, never code. The Go import path
  (`libs/caramba-core/subimport`) parses clash YAML, sing-box JSON, v2ray base64
  lists, and single URIs into a mihomo config. It does not eval, exec, load a
  plugin, or fetch executable logic. Verified by source read: no `os/exec`,
  `plugin`, `unsafe`, `syscall`, or `reflect`-based dispatch in that package.
- There is no remote logic patching, no JavaScript bridge that runs downloaded
  code, no over-the-air rule that changes app behavior beyond declared config.
- Routing rule sets pulled from a panel (`/rulesets/*`) are domain and IP lists
  in text form, consumed as data by mihomo. They are not code.

Keep any future feature on the same rule: remote content is data, the app
binary is the only logic.

## 3. iOS privacy manifest (PrivacyInfo.xcprivacy)

Add `PrivacyInfo.xcprivacy` to the Runner and to the Network Extension target.
The client collects nothing by default and runs no third-party trackers, so:

- `NSPrivacyTracking`: `false`.
- `NSPrivacyTrackingDomains`: empty array.
- `NSPrivacyCollectedDataTypes`: empty array (the app does not collect data for
  Apple's definitions; account email and credentials go only to the user's own
  panel, not to the developer).
- `NSPrivacyAccessedAPITypes`: declare a reason for each required-reason API the
  app actually uses:
  - File timestamp APIs: reason `C617.1` if used for app-owned files only.
  - User defaults: reason `CA92.1` (app and its extension share state through
    the App Group; access is the app's own data).
  - Keychain access from secure token storage (flutter_secure_storage): declare
    if the SDK triggers a required-reason API; confirm against the plugin's own
    manifest and merge.

Confirm the final set against what the shipped build links. If a dependency
adds a required-reason API, its reason must appear here too.

## 4. Google Play Data Safety

Fill the Data Safety form as no collection and no sharing, which matches the
build:

- Data collected: none by the developer. The account email and password the
  user enters are sent only to the operator panel the user chose, over HTTPS,
  and are stored on device in secure storage. The developer receives nothing.
- Data shared: none. No analytics SDK, no ad SDK, no attribution SDK.
- Security practices: data encrypted in transit; the user can request account
  deletion through their operator panel.
- VPN traffic is not logged or inspected by the client.

If the operator runs their own analytics on their own panel, that is the
operator's disclosure on their own terms of service, not the app's.

## 5. No third-party trackers in the Free upsell

The Free tier upsell is first-party. The `upstream_ads` flag from
`GET /api/v2/app/branding` only shows the app's own "Powered by Caramba Connect"
card with one quiet link. It is not a third-party ad network, has no tracker
SDK, and collects nothing. Do not swap it for an ad SDK; that would change the
Data Safety answers and pull in network-policy review.

## 6. VPN entitlements and permissions

The platform VPN capabilities are in `apps/caramba-client/INTEGRATION.md`. The
review-relevant ones:

- iOS and macOS: Network Extension (Packet Tunnel) with the Personal VPN /
  packet-tunnel-provider entitlement. Explain in the review notes that the
  tunnel routes the user's traffic to servers the user configured.
- Android: `VpnService` with a user consent dialog on first connect, plus
  `FOREGROUND_SERVICE_SPECIAL_USE` with `foregroundServiceType=specialUse`. Play
  requires a written justification for the special-use foreground service: state
  that it keeps the VPN tunnel alive while connected. The `POST_NOTIFICATIONS`
  permission is for the connection status notification.
- Desktop: Windows needs administrator for WinTun; Linux needs root or
  `CAP_NET_ADMIN`. Not store-review items, but document them for users.

## 7. Future permissions to declare when added

The QR and file import features are stubs today and request no camera or storage
permission, so they raise nothing at review now. When implemented:

- Camera (QR scan): add the iOS `NSCameraUsageDescription` and the Android
  camera permission, and update the iOS privacy manifest and the Play Data
  Safety form to reflect that the camera is used only to read a subscription QR
  on device, with no image stored or transmitted.
- File picker: declare the document-picker usage; on iOS this uses the system
  picker and needs no broad storage permission.

Until those land, do not declare permissions the app does not use.

## 8. Fallback distribution

A universal client that can connect to any panel can draw extra review scrutiny.
If a store rejects the universal framing, narrow the listing to the operator's
own service for that submission, or distribute through sideload and AltStore as
a fallback. Keep the rejection reason and the resubmission notes with this file.

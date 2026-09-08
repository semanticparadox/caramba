# Desktop recovery verification — 2026-09-08

Recovered the interrupted desktop work on `feat/connect-protocol` from base
`c1a9dac`, preserving the existing tracked and untracked client changes. No
reset, stash, migration-patch reapplication, release publication, or panel edits.
Original brief: a platform-specific desktop shell, toolbar, tray lifecycle and
ship-inspired Caramba Connect identity. Historical D0–D13 work was retained.

## Corrections reviewed in this session

- Autostart distinguishes unsupported macOS from an unavailable native channel.
  Pending Login Items approval preserves the request across restart and can be
  cancelled. System writes are serialized; stale errors cannot undo newer input.
- Native window/autostart channels register with the concrete Flutter controller
  in `MainFlutterWindow.awakeFromNib`. A live macOS 27 run initially exposed
  unavailable capability detection; after this change the unsupported-version
  message disappeared. Dock presentation retains an explicit weak window reference.
- Sidebar/Home/server actions fit their content; server summaries wrap and machine
  names have more room and full-name tooltips. Desktop import uses a close icon.
- Settings form really caps at 720 px; the final index section activates at the
  bottom. Action buttons grow beyond their 120 px minimum for larger text.
- Existing shared Keychain options and keyboard menu fixes were reviewed and
  preserved. No plaintext storage fallback was added.

Separate implementation and read-only review agents examined these changes.

## Current evidence

| Check | Result |
| --- | --- |
| Full Flutter suite before final native-registration correction | 1163 passed, 8 skipped |
| Desktop suite after final native-registration correction | 191 passed |
| Final `flutter analyze` | No issues |
| Final `bash scripts/build.sh macos --release` | Passed, native core enabled, app 174.1 MB |
| `git diff --check` | Passed |
| Live Home labels, settings action labels | Full labels visible in rebuilt app |
| Live Cmd+1 / Cmd+2 / Cmd+3 | Switch sections |
| Live Cmd+Shift+S and Escape | Server panel opens and closes |
| Live Cmd+W, Cmd+Q | Window hides; app quits respectively (disconnected) |
| Launching hidden existing app via app launcher | Window returns; not proof of Dock/tray click path |
| Live autostart capability on macOS 27 | Unsupported-version message absent after native correction |

## Verification limits

Dock automation timed out. Flutter accessibility value writes did not reliably
reach the actual input, clipboard pasting timed out, and synthetic typing dropped
characters. Therefore native profile import → quit → relaunch, connected shutdown,
tray menu actions, Dock clicking, Login Items registration/approval, start-in-tray,
and file-dialog Dock duplication are **not certified** by this session. The local
test subscription pointed only at 127.0.0.1 and was not successfully imported.
Windows/Linux/iOS were not run on this Mac. The skipped full-suite cases include
environment-dependent native integration tests.

Pending approval text refreshes on restart or a subsequent toggle, not immediately
after approval in System Settings. Review also noted pre-existing shutdown error
handling limits around a throwing tray cleanup or a stalled disconnect future;
normal disconnected Cmd+Q succeeded, but those exceptional paths were not exercised.

The R2 ship icon remains a proposal from the historical icon review, not owner
approval of a launcher icon. Existing tray assets were retained; no new icon was
silently rolled out across platforms.

Beads and TaskWing executables were unavailable in PATH and checked local install
directories. This is a verification record, not a replacement task tracker. HQ
`docs-sync` succeeded after the sandbox network failure was retried with approval.

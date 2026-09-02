# Atmosphere layer: where the design came from

The connection-state background behind Caramba Connect is called **Open Chart**.
The shipped spec lives in `DESIGN.md` section 4.2 and the code lives in
`lib/atmosphere/`. This folder is the record of how that spec was chosen, kept
so a future change can be argued against the original reasoning instead of
re-litigated from scratch.

## What is here

| Path | What it is |
|---|---|
| `JUDGE-VERDICT.md` | Three judges (product designer, Flutter engineer, product owner) scored three concepts on nine criteria. Open Chart won on every card. Section 4 is the binding list: six must-fix items, six worth borrowing from the losing concepts, two explicitly not to borrow. |
| `concepts/C-open-chart/` | The winner: `concept.md` (metaphor, per-state composition, tokens, choreography, risks), `painter-notes.md` (the Flutter implementation plan) and `demo.html` (a runnable canvas prototype that is the reference for the geometry). |
| `concepts/A-masonry/`, `concepts/B-carrier/` | The two losing concepts. Kept because the verdict borrows real work from both: the type-band invariant and the platform low-power read come from B, the battery contract as a test and the closing asymmetry come from A. |
| `shots/C-*.png` | The concept's own renders of all four states in both themes. These are the visual target the Flutter port was checked against. |

## What changed on the way into Flutter

The verdict's must-fix items are all in, and the places where the shipped layer
deviates from `concept.md` are listed in `DESIGN.md` section 4.2 rather than
here, so there is one current source of truth. The short version:

- Light `success` is `#177A41` (already the value in `lib/theme/colors.dart`), so
  the connect label clears AA at normal text size.
- Country codes are cut from five to three, and connected is the state with the
  fewest marks on screen rather than the most.
- The boundary's top and bottom edges and the quiet lens are all derived from
  the laid-out header row and connect block, not from constants.
- The status bar inset and the connect block are protected bands, asserted at
  2/255 of `bgBase` by `test/atmosphere/atmosphere_paint_test.dart`.
- The ticker is stopped, not idled, in every settled state and whenever the app
  is not resumed, asserted by `test/atmosphere/atmosphere_layer_test.dart`.

## Regenerating the Flutter renders

```
ATMO_SHOTS_DIR=/tmp/atmo flutter test test/atmosphere/render_shots_test.dart
```

Writes one PNG per state per theme, the atmosphere plus a plain mock of Home's
chrome. Without the environment variable the test skips.

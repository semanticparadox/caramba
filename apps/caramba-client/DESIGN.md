# Caramba Connect Design System

> **Positioning (one line):** Caramba Connect is a calm, fast VPN client. Neutral surface, no ornament, color only where it means something.

Canonical design system for the **exarobot VPN super-app** (Flutter: Android, iOS, macOS, Windows, Linux). Dark-first, neutral, quiet. Every value is concrete and droppable into Flutter; the Dart token files live under `lib/theme/`. The proven reference implementation is `demo/caramba-demo.html`; this document and the Dart tokens match it. The hard rules are in `ANTI-SLOP.md`.

---

## 0. Principles

1. **Neutral base, no brand hue.** The app is monochrome. There is no purple, no indigo, no teal, no gradient brand color. (The old "Electric Iris" purple is gone.)
2. **Color means status.** The only saturated colors in the product are the three connection states: green = connected, amber = connecting, red = error. If a pixel is colored, it is reporting state.
3. **One high-contrast neutral does the work of an accent.** The primary button is white-on-black in dark, black-on-white in light. Selection and focus use the same near-white / near-black neutral. We call this token `accent` in code, but it is a neutral, not a hue.
4. **Flat surfaces, one ambient layer.** Solid fills that step up in lightness with elevation. No glassmorphism (one subtle nav blur is allowed), no colored glows, no breathing auras. The single exception is the atmosphere layer in section 4.2: a flat line drawing behind the app that opens and closes with the connection state. It is capped at 8 percent field alpha, it never blurs or glows, and it holds completely still in every settled state. Nothing else in the product gets an ambient background.
5. **Real icons, not emoji.** One Lucide set, consistent stroke. Countries are mono 2-letter codes (NL, DE, RU), never flag emoji.
6. **System type, mono for data.** SF Pro on Apple, the platform UI font elsewhere. Monospace only for latency, bytes, codes, prices.
7. **Plain copy.** No marketing language, no em-dash anywhere in UI text. See `ANTI-SLOP.md`.

---

## 1. Color System

No brand accent hue. The palette is neutral; status colors are the only saturated values. Token names are kept stable so screens keep compiling; the `accent*` group is now a **neutral high-contrast** group, not a violet.

### 1.1 Dark theme (default)

| Token | Hex | Use |
|---|---|---|
| `bgBase` | `#0A0A0A` | App background, deepest plane. |
| `bgCanvas` | `#0C0C0C` | Scaffold / scroll background. |
| `surface1` | `#161616` | Cards, list rows, nav bar. |
| `surface2` | `#1E1E1E` | Elevated cards, sheets, inputs. |
| `surface3` | `#272727` | Popovers, menus, switch-off track. |
| `surfaceInset` | `#0E0E0E` | Inset wells (chart, code/mono blocks). |
| `borderSubtle` | `#2A2A2A` | Hairline dividers, card borders. |
| `borderStrong` | `#3D3D3D` | Input borders, signal-bar off, selected row edge base. |
| `accent` | `#FAFAFA` | Primary button fill, selection, focus ring. **Neutral, not a hue.** |
| `accentVariant` | `#EDEDED` | Hover/pressed neutral. |
| `accentDeep` | `#D4D4D4` | Pressed neutral. |
| `accentSubtle` | `#FAFAFA14` (8%) | Flat neutral tint (selected chips). |
| `success` | `#30D158` | Connected. Online dot, secure badge, good ping. |
| `successDeep` | `#248A3D` | Connected pressed / deep. |
| `successSubtle` | `#30D1581A` | Connected tint backgrounds. |
| `warning` | `#FF9F0A` | Connecting, reconnecting, mild attention, quota low. |
| `warningSubtle` | `#FF9F0A1A` | Warning tint. |
| `danger` | `#FF453A` | Error, fault, destructive. |
| `dangerDeep` | `#D70015` | Danger pressed. |
| `dangerSubtle` | `#FF453A1A` | Danger tint. |
| `info` | `#A0A0A0` | Neutral-informational. **Kept neutral, no second hue.** |
| `textHi` | `#FAFAFA` | Primary text, headings. |
| `textMed` | `#A0A0A0` | Secondary text, labels, idle glyphs. |
| `textLow` | `#7C7C7C` | Captions, placeholders, chevrons. |
| `textOnAccent` | `#0A0A0A` | Text on the near-white accent (dark text). |
| `textOnSuccess` | `#062A12` | Text on solid success fills. |
| `overlayScrim` | `#00000099` (60%) | Modal/sheet backdrop. |
| `shimmerBase` | `#161616` | Skeleton base. |
| `shimmerHi` | `#232323` | Skeleton highlight sweep. |

### 1.2 Light theme

Warm-neutral, not stark white. Status colors are deepened to hold WCAG AA (≥4.5:1) on white. The accent inverts to near-black.

| Token | Hex | Use |
|---|---|---|
| `bgBase` | `#FAFAFA` | App background. |
| `bgCanvas` | `#FFFFFF` | Scaffold. |
| `surface1` | `#FFFFFF` | Cards, rows, nav. |
| `surface2` | `#F4F4F4` | Elevated, inputs. |
| `surface3` | `#ECECEC` | Popovers, switch-off track. |
| `surfaceInset` | `#F1F1F1` | Chart wells, mono blocks. |
| `borderSubtle` | `#E5E5E5` | Hairlines. |
| `borderStrong` | `#D2D2D2` | Input borders. |
| `accent` | `#0A0A0A` | Primary button fill (inverted). **Neutral.** |
| `accentVariant` | `#1F1F1F` | Hover. |
| `accentDeep` | `#000000` | Pressed. |
| `accentSubtle` | `#0A0A0A0F` (~6%) | Flat neutral tint. |
| `success` | `#1E9E54` | Connected (deepened for white). |
| `successDeep` | `#177A41` | Pressed. |
| `successSubtle` | `#1E9E5414` | Tint. |
| `warning` | `#A85D00` | Connecting / attention (deepened). |
| `warningSubtle` | `#A85D0014` | Tint. |
| `danger` | `#C8102E` | Error (deepened). |
| `dangerDeep` | `#A00B24` | Pressed. |
| `dangerSubtle` | `#C8102E14` | Tint. |
| `info` | `#585858` | Neutral-informational. |
| `textHi` | `#0A0A0A` | Primary. |
| `textMed` | `#585858` | Secondary. |
| `textLow` | `#6E6E6E` | Caption. |
| `textOnAccent` | `#FFFFFF` | Text on the near-black accent (white). |
| `textOnSuccess` | `#FFFFFF` | Text on solid success. |
| `overlayScrim` | `#00000073` (45%) | Backdrop. |
| `shimmerBase` | `#ECECEC` | Skeleton base. |
| `shimmerHi` | `#F6F6F6` | Skeleton highlight. |

> **Theming rule:** dark is the default, light is a faithful sibling. The accent is the inverted neutral (`#FAFAFA` dark, `#0A0A0A` light). Status colors deepen in light to keep AA. Never introduce a hue outside the three status families.

### 1.3 Gradients and glows (retained names, de-slopped)

The token names `accentGradient`, `connectedGradient`, `cyanKiss`, `glowAccent`, `glowConnected` are kept because screens reference them, but their meaning changed:

- `accentGradient` is now a **flat** single-color "gradient" (solid `accent`). No purple to lilac.
- `connectedGradient` is solid `success` green. There is **no cyan kiss**; `cyanKiss` aliases `success` so any leftover reference stays on-palette.
- `glowAccent` / `glowConnected` are a **faint neutral lift** (low-opacity black shadow), not a colored halo. The connected dial reports state through the green ring and check glyph, not a glow.

---

## 2. Typography

**System stack, no webfont.** Sans is the platform UI font (SF Pro on Apple) via `fontFamilyFallback`. Mono is the system monospace (SF Mono / Menlo / Consolas) with JetBrains Mono as an optional bundled fallback. No `google_fonts`, no Plus Jakarta Sans.

`--sans: -apple-system, BlinkMacSystemFont, 'SF Pro Text', 'Segoe UI', Roboto, system-ui`
`--mono: ui-monospace, 'SF Mono', 'JetBrains Mono', Menlo, monospace`

Tabular figures (`FontFeature.tabularFigures()`) are on for `display` and every `mono*` style so changing numbers (ping, bytes, timer) do not jitter.

### Type scale (px; mirrors the demo t1 to t5)

| Role | Family | Size / Line-height | Weight | Tracking | Notes |
|---|---|---|---|---|---|
| `display` | sans | 34 / 40 | 700 | -0.5 | Hero numbers (timer), splash. Tabular. |
| `headline` | sans | 27 / 33 | 700 | -0.5 | Screen titles (t1). |
| `titleLg` | sans | 20 / 26 | 600 | -0.2 | Section / sheet titles (t2). |
| `titleMd` | sans | 17 / 23 | 600 | -0.1 | Card titles, server name. |
| `bodyLg` | sans | 16 / 24 | 500 | 0 | Primary body. |
| `bodyMd` | sans | 15 / 22 | 500 | 0 | Default UI text (t3). |
| `bodySm` | sans | 13 / 18 | 500 | 0 | Dense secondary (t4). |
| `label` | sans | 15 / 18 | 600 | 0 | Buttons, tabs. |
| `caption` | sans | 11 / 14 | 600 | 0.6 (UPPERCASE) | Overlines, meta (t5). |
| `monoMd` | mono | 13 / 18 | 600 | 0 | Ping, bytes, codes (t4). |
| `monoSm` | mono | 11 / 15 | 600 | 0 | Inline stats (t5). |

Weights: 500 body, 600 labels/titles, 700 display/headline. No 800/900.

---

## 3. Spacing, Radius, Elevation, Borders

### 3.1 Spacing (4px base, 8px rhythm)

`s0 0` · `s1 4` · `s2 8` · `s3 12` · `s4 16` · `s5 20` · `s6 24` · `s8 32` · `s10 40` · `s12 48` · `s16 64` · `s20 80`.

- Screen edge padding: **20** (mobile), **32** (desktop).
- Stacked card gap: **8 to 12**. Section spacing: **24**.

### 3.2 Radius

`xs 8` (code/mono boxes) · `sm 12` (icon buttons, toasts) · `button 14` (buttons, list items) · `md 16` (default card) · `lg 22` (sheets, large cards) · `xl 28` (hero) · `pill 999`.

Buttons & list rows: **14**. Cards: **16**. Sheets: top **22**.

### 3.3 Elevation (flat, neutral, no colored glow)

| Token | Dark | Light |
|---|---|---|
| `card` | `0 2 8 rgba(0,0,0,0.25)` | `0 1 3 rgba(0,0,0,0.07)` |
| `raised` | `0 8 24 rgba(0,0,0,0.35)` | `0 6 18 rgba(0,0,0,0.10)` |
| `sheet` | `0 16 48 rgba(0,0,0,0.45)` | `0 16 40 rgba(0,0,0,0.14)` |
| `glowAccent` | faint neutral `0 2 12 rgba(0,0,0,0.08)` | `0 2 10 rgba(0,0,0,0.06)` |
| `glowConnected` | same faint neutral | same |

In dark, prefer a 1px `borderSubtle` over a shadow for the flattest look. Cards in the demo use border, not shadow.

### 3.4 Borders

- Hairline: **1px** `borderSubtle`.
- Input / focus base: **1.5px** `borderStrong`.
- Focus ring (a11y): 2px `accent` (the neutral high-contrast) on focusable controls. The demo input focus uses `border-color: textHi` plus a soft 14% halo of the same neutral.
- Selected list row: a 3px `textHi` left edge bar + `surface2` fill + `borderStrong` border (no colored selection).

---

## 4. Connection-State Visual Language

Two things carry the connection state: the **connect dial**, which reports it in the foreground, and the **atmosphere layer**, a chart of the network the client can reach that opens and closes behind the whole screen. The dial answers "what is happening"; the chart answers "what does that mean for me". The dial changes first and the chart follows about 150ms later, so the world reads as a consequence of the state change rather than as decoration on top of it.

### 4.1 The connect dial

One hero control. A circular tappable control (mobile **196px**, desktop **232px**), centered on Home, with a flat face (`surface1`, `146px`) and a 2px ring whose **color is the status**. No glow, no breathing aura, no rotating comet decoration beyond a single indeterminate arc while connecting.

| State | Ring | Face | Glyph | Label | Motion |
|---|---|---|---|---|---|
| **Disconnected** | `borderStrong`, static | `surface1` | power, `textMed` | "Tap to connect" (`textMed`) | Idle. |
| **Connecting** | `warning` indeterminate arc, rotating | `surface1` | dots, `warning` | "Connecting" | One rotating arc (`transform` only). |
| **Connected** | `success`, solid | `surface1`, ring + glyph `success` | shield-check, `success` | "Protected" | Static. Connect timer counts up below in tabular mono. |
| **Error / fault** | `danger`, static | `surface1`, glyph `danger` | alert, `danger` | "Couldn't connect" | One brief shake on entry, then static. |
| **Reconnecting** | `warning` rotating arc | `surface1` | dots, `warning` | "Reconnecting" | As connecting. Kill-switch banner if traffic is blocked. |

**Tap:** toggles connect/disconnect with haptic (medium on mobile). Transitions cross-fade glyph/label over ~200ms; ring color morphs over ~350ms. State color drains back to neutral `borderStrong` on disconnect.

**Kill-switch banner:** a thin `warningSubtle` bar under the app bar: "Traffic paused while reconnecting." Amber, not red, unless it is a true failure.

The state label plus the sub-line form the **connect block**. It is wrapped in one `Semantics(liveRegion: true)` container, so a state change is announced once instead of twice.

### 4.2 The atmosphere layer: Open Chart

Code: `lib/atmosphere/`. Tokens: `AtmosphereTokens` (a second `ThemeExtension`, deliberately separate from `AppTokens` so no component can reach it). Reference concept and judging record: `docs/atmosphere/`.

**The metaphor.** The background is a survey chart of the network the client can reach, and the dial is the chart's home station. Off, the chart is closed: an irregular boundary octagon is drawn around you, the world outside it is hatched at 45 degrees (the cartographic mark for a restricted area), and every route out of the dial stops at a perpendicular dead-end cap. On, the chart is open: the boundary is gone, the graticule is visible, and every route runs unbroken to its station.

The inversion is the whole idea. Normally hatching marks a small forbidden patch inside an open sea. Here the hatching is everything **outside** a small boundary drawn around you: disconnected, you are the restricted area.

Three structural rules keep this from collapsing into the generic network-graph background:

1. **Square station marks, never circles.** Circles plus glowing lines is the VPN landing page cliche.
2. **Orthogonal routing with 45 degree elbows, never radial spokes.** Radial spokes read as a sunburst. Elbows read as something a person laid out.
3. **Zero blur, zero shadow, zero halo.** This is a line drawing. Nothing both moves and glows.

**Geometry is fixed across every state.** Nothing ever changes position. What changes is which parts are drawn, how bright they are, and whether the boundary is closed. That constraint is what keeps the layer calm.

#### Per-state composition

| State | Boundary | Hatch / shade | Graticule | Routes | Stations | Marks on screen |
|---|---|---|---|---|---|---|
| **Disconnected** | closed, 1.2px | full | 0.18 | flat grey to the boundary, dead-end cap, then a 2/4 dotted ghost to a hollow station | hollow 5px squares | ~62 |
| **Connecting** | split into 8 arcs, 26px gaps | 0.35 / 0.45 | 0.55 | probe amber, extending and retracting on a staggered loop | hollow | ~66 |
| **Connected** | none | none | 1.0 | live green, unbroken, dial ring to station | filled 5px square + hairline ring, three carry a mono country code | ~48 |
| **Error** | closed | full | 0.18 | retracted to the caps, one route keeps a 9px fault cross at its boundary crossing | hollow | ~63 |
| **Reconnecting** | ajar, 55 percent gap closure | 0.50 / 0.60 | 0.40 | probe amber at 60 percent amplitude, slower loop | hollow | ~64 |

**Connected is not the busiest state.** It drops the hatch, the shade, the boundary and every dead-end cap, and it carries three country codes rather than the concept's five. The mark counts above are the budget: if a change pushes connected above connecting, the change is wrong. `test/atmosphere/atmosphere_paint_test.dart` asserts the code cap and the three dropped elements.

**Color independence.** Every state is distinguishable with all color removed, because the structure differs: closed boundary plus hatch plus caps (disconnected), broken boundary plus partial routes (connecting), no boundary plus graticule plus full routes (connected), closed boundary plus an X (error), half-closed boundary (reconnecting). Hue only confirms what lightness and structure already said.

#### Tokens

Every value is an alpha over the base plane, so the composite is deterministic. The light column was chosen against white, not inverted from dark: field alphas are lower (dark hairlines on paper read heavier than light hairlines on black) and structural marks are higher, so light carries the state through structure instead of through a grey wash.

| Token | Dark | Light | Purpose |
|---|---|---|---|
| `grid` | `#FAFAFA` @ 3.0% | `#0A0A0A` @ 3.8% | Graticule minor line, 1px. |
| `gridMajor` | `#FAFAFA` @ 5.2% | `#0A0A0A` @ 6.0% | Every fourth graticule line. |
| `hatch` | `#FAFAFA` @ 2.6% | `#0A0A0A` @ 3.2% | Restricted hatch, 1px at 9px pitch, 45 degrees. |
| `shade` | `#000000` @ 45% | `#0A0A0A` @ 5.0% | Radial ramp outside the boundary: alpha 0 at the dial, this value at the frame edge. |
| `lift` | `#FAFAFA` @ 2.5% | none | Full field lift on connected. In a dark theme, light means lift. |
| `routeIdle` | `#FAFAFA` @ 10% | `#0A0A0A` @ 16% | Route inside the boundary while closed. 1.5px. |
| `routeGhost` | `#FAFAFA` @ 4.5% | `#0A0A0A` @ 7% | Route beyond the boundary, dotted 2 on / 4 off. |
| `routeProbe` | `#FF9F0A` @ 24% | `#A85D00` @ 34% | Route while connecting. Derived from `warning`. |
| `routeLive` | `#30D158` @ 22% | `#177A41` @ 34% | Route when connected. Derived from `success`. |
| `barrier` | `#FAFAFA` @ 14% | `#0A0A0A` @ 22% | Boundary stroke, 1.2px. |
| `nodeIdle` | `#FAFAFA` @ 16% | `#0A0A0A` @ 24% | Hollow station mark. |
| `nodeLive` | `#30D158` @ 50% | `#177A41` @ 66% | Filled station mark plus hairline ring. |
| `fault` | `#FF453A` @ 55% | `#C8102E` @ 65% | The single dead route cross on error. |
| `label` | `#FAFAFA` @ 20% | `#0A0A0A` @ 24% | Mono country codes, connected only. |
| `quietLens` | `#0A0A0A` | `#FAFAFA` | The base plane, painted back over the layer. Equals `bgBase`. |

Structural tokens: `barrierHw 134` · `barrierRise 132` · `barrierDrop 12` · `barrierCut 36` · `gridPitch 44` · `gridMajorEvery 4` · `hatchPitch 9` · `gapHalfWidth 13` · `lensPad 16` · `lensFeather 40` · `topFeather 18` · route ring offset `104` (dial radius 98 plus a 6px gap) · gutter inset `14`.

Three rules that stop the palette drifting:

1. **Field alpha ceiling 8 percent.** `grid`, `gridMajor`, `hatch` and `lift` are fields and stay at or under 8 percent. Point marks and hairlines may go higher because they cover about 3 percent of the layer. Asserted in `atmosphere_paint_test.dart`.
2. **Only three hues exist.** Probe is `warning`, live is `success`, fault is `danger`. No fourth family, no gradient between hues, no per-route color ramp along a route's length.
3. **No glow.** Nothing is blurred, nothing has a shadow, nothing has a halo.

#### Motion

All durations belong to the atmosphere layer and are longer than the dial's own, on purpose.

| Transition | Total | What moves | Curve |
|---|---|---|---|
| to **connecting** | 620ms | boundary splits at the 8 crossings first (0 to 260ms), caps rotate out and fade (60 to 300ms), hatch and shade drop (140 to 560ms), graticule rises (300 to 620ms), probe loop starts at 180ms | `easeOutCubic` on the gaps, `easeInOutCubic` on the fields |
| to **connected** | 1200ms | routes extend to full length staggered 70ms **inside-out** (nearest station first), tint crosses amber to green along each route's own extension, station ignites at its own 0.92, boundary and hatch fade out, graticule and lift rise, country codes land last | `easeOutCubic` extension, `easeInOutCubic` fields |
| to **disconnected** | 700ms | the reverse, **outside-in**: the furthest station goes dark first and the routes retract toward the dial, so a disconnect visibly draws back to your thumb | `easeOutCubic` retraction |
| to **error** | 420ms + 260ms shake | as disconnect but compressed and accelerating, so the closing reads as a slam; the fault cross draws in over the last 120ms; the chart layer shakes once horizontally, 5px damped to 0 | `easeInCubic` |
| to **reconnecting** | 480ms | boundary returns to 55 percent closure and stays visibly ajar, graticule holds at 0.40, routes drop to the slow probe. No shake: reconnecting is not a failure. | `easeInOutCubic` |
| **probe loop** | 2200ms (3000ms reconnecting) | each route extends to 0.78, holds, retracts to 0.15, repeats, staggered per route so they are never in sync | `easeInOutSine` per probe |

The asymmetry is intentional: opening a world takes longer than losing it. The gaps opening **before** the routes move is also intentional, because it says the wall gave way and then traffic went through, rather than the routes breaking the wall.

#### Frame budget

| State | Resting fps | Ticker |
|---|---|---|
| Disconnected | 0 | stopped |
| Connected | 0 | stopped |
| Error | 0 | stopped after the shake |
| Connecting | 30 (20 under low power) | running, bounded by the real connect timeout |
| Reconnecting | 30 (20 under low power) | running |
| Any state, app not resumed | 0 | stopped |
| Any state, reduce motion | 0 | no ticker is ever created |

The ticker is **stopped, not idled**, so the two states a user spends essentially all their time in cost zero frames. The gate is a per-frame timestamp check against `AtmosphereBudget.minFrameGap`, and the stamp is reset on every state change so the first frame after a transition always paints. `setState` runs twice per transition, at the start and at the end, to flip `willChange`; every frame in between is a `ChangeNotifier` bump that repaints only the `CustomPaint`. `test/atmosphere/atmosphere_layer_test.dart` asserts all of it.

**Low power.** `AtmosphereBudget.lowPower` drops the cap from 30 to 20fps. It is plumbed, not sniffed: the app sets it from `PowerManager.isPowerSaveMode` (Android) or `ProcessInfo.isLowPowerModeEnabled` (iOS) and pushes it down through `AtmosphereBudgetScope`. Until that channel exists the default is `false`.

**Reduce motion.** `MediaQuery.disableAnimations` switches the layer to a genuinely different path, not a slower one: three static compositions cross-faded on opacity over 200ms, no geometry in flight, no ticker of the layer's own. The probe states resolve to a deterministic mid-range extension, never "wherever the loop happened to be", so the frozen composition is the same every time.

#### Safe areas and the type-band invariant

The layer paints the base plane under everything, then protects two bands from itself.

- **Status bar inset.** No line work, no shade, no lift is painted above `viewPadding.top`. The system draws the clock and the battery there in a color we do not control. Below the inset, an 18px linear ramp of the base plane feathers the layer in, so the clip leaves no seam.
- **The connect block.** The state label is the only text on the screen painted in a status color, and status greens and ambers have far less contrast headroom than the neutral text does. The layer paints the base plane back over itself in a feathered clearing around the measured connect block, so the composite under those words is exactly `bgBase`. The boundary is drawn **after** the lens and skipped where it would cross the words, the same treatment the route crossings get, so the "you are enclosed" mark survives at full strength right under the label.

**The invariant, asserted:** every pixel inside the status bar band and inside the connect block rect is within 2/255 of `bgBase`, in both themes, in all five states, and mid-transition while the lift is ramping. See `test/atmosphere/atmosphere_paint_test.dart`. A change to any atmosphere alpha that breaks this fails the build.

**Derived, not constant.** The boundary top edge clears the measured header row, the boundary bottom edge encloses the measured connect block, and the quiet lens follows that same rect. A text scale change therefore moves the chart with the layout instead of cropping the sub-line.

#### Measured composites

Sampled from the rendered layer above the cards, not estimated. Field range is the 3rd to 97th percentile; the trimmed 6 percent is hairline and point-mark coverage, which no text sits on.

| Theme / state | Field range | `textHi` | `textMed` |
|---|---|---|---|
| Dark, disconnected | `#0A0A0A` to `#101010` | 18.2:1 to 19.0:1 | 7.3:1 to 7.6:1 |
| Dark, connecting | `#0A0A0A` to `#111111` | 18.1:1 to 19.0:1 | 7.2:1 to 7.6:1 |
| Dark, connected | `#0A0A0A` to `#171717` | 17.2:1 to 19.0:1 | 6.9:1 to 7.6:1 |
| Dark, error | `#0A0A0A` to `#101010` | 18.2:1 to 19.0:1 | 7.3:1 to 7.6:1 |
| Light, disconnected | `#F3F3F3` to `#FAFAFA` | 17.8:1 to 19.0:1 | 6.4:1 to 6.8:1 |
| Light, connecting | `#F3F3F3` to `#FAFAFA` | 17.8:1 to 19.0:1 | 6.4:1 to 6.8:1 |
| Light, connected | `#F1F1F1` to `#FAFAFA` | 17.5:1 to 19.0:1 | 6.3:1 to 6.8:1 |
| Light, error | `#F3F3F3` to `#FAFAFA` | 17.8:1 to 19.0:1 | 6.4:1 to 6.8:1 |

The worst single point in the light field is a major graticule line crossing the hatch inside full shade, at the frame corners: `#DBDBDB`, a 12 percent composite deviation, where `textMed` still reads 5.1:1 and `textHi` 14.3:1. The 8 percent ceiling is per element, not per composite.

Status text sits on the quiet lens, which is exactly `bgBase`, so those ratios are the palette's own and the atmosphere costs them nothing: dark `success` 9.8:1, `warning` 9.6:1, `danger` 5.8:1; light `success` 5.2:1, `warning` 4.8:1, `danger` 5.6:1. Light `success` is `#177A41`, which clears AA at normal text size; the older `#1E9E54` did not.

The wordmark and the plan chip are the only other strings on bare atmosphere. They sit above the boundary in the left third of the header band, where the chart's only marks are the graticule and the top tie route (which leaves the dial on the centre line). Measured worst case there: `#232323` in dark and `#E3E3E3` in light, both 15.2:1 against `textHi`.

Everything else on Home is structurally safe: cards and rows are opaque `surface1`, the stats grid is `surface1` over a `borderSubtle` grid, and the nav is `bgCanvas` at 92 percent over a blur. The config and stats block additionally sits on a `bgBase` plane at 82 percent, ramped in over the first 24px, so the chart slides under the content instead of competing with a dense grid of numbers.

#### Strength: how other screens use the layer

`AtmosphereLayer.strength` multiplies every alpha in the layer. Geometry is unchanged, so the chart stays true wherever it appears.

| Surface | Strength | Behavior |
|---|---|---|
| **Home** | 1.0 | Full composition. The dial is the home station; the boundary encloses the dial and the connect block. |
| **Servers** | 0.45 | Same geometry, translated up and snapped to whole device pixels so the 9px hatch cannot alias behind a scrolling list. Rows stay opaque `surface1`, so the layer is only ever seen in the gutters. |
| **Settings, Profile** | 0.30 | Texture, not a diagram. |
| **Sheets and modals** | 0.0 | The scrim covers it anyway, and strength 0 stops the ticker. |
| **Splash, Login, Autotune** | 0.0 | These screens have no connection state to report. A closed chart during login would imply a failure that has not happened. |

#### Accessibility

The whole layer is wrapped in `ExcludeSemantics`. It is never the only carrier of information: the dial ring, the glyph and the connect block's live region already report the state, and the screen reader announcement is unchanged by the layer's presence.

#### What would make this look cheap

Named so a future change can be argued against, not just felt to be wrong:

1. Station marks becoming circles, routes becoming straight radial spokes, or anything gaining a glow. That is the generic tech background every VPN landing page already has.
2. Doubling the alphas. Then it is a wireframe grid over a terminal, which is a cheaper product. If a stakeholder says "I can barely see it", that is the design working. If they ask for the state to be more visible, the answer is more structure, never more wash.
3. A status hue turned into a full-bleed field. Section 1.3 and the LILA rule in `ANTI-SLOP.md` exist to prevent exactly that. Hue lives in hairlines and point marks only.
4. More country codes. Three is the target and five is the hard ceiling, connected only.

---

## 5. Component Specs

### 5.1 Buttons

| Variant | Fill | Text | Border | Height | Radius |
|---|---|---|---|---|---|
| **Primary** | solid `accent` (neutral: white on dark, black on light) | `textOnAccent`, label 600 | none | 50 | 14 |
| **Secondary / ghost-box** | `surface2` | `textHi` | 1px `borderSubtle` | 50 | 14 |
| **Quiet / text** | transparent | `textHi` (or `danger` for destructive) | none | 44 | 12 |
| **Destructive confirm** | `dangerSubtle` or solid `danger` | `danger` / `textHi` | optional 1px `danger` | 50 | 14 |
| **Icon button** | `surface1` (or transparent) | icon `textMed` to `textHi` on press | 1px `borderSubtle` for boxed | 44×44 (icon 24) | 12 |

Pressed: `scale 0.98` + slight brightness. One primary per screen region. Loading: inline spinner, label hidden.

### 5.2 Server list row

Min-height **60**, radius 14, `surface1`, 1px `borderSubtle`, padding 16, 14px gap. Selected: `surface2` fill, `borderStrong` border, 3px `textHi` left edge bar.

```
┌────────────────────────────────────────────────────┐
│  NL   Netherlands · Amsterdam      ▌▌▌▎▎  42 ms   ✓ │
│       Stealth · Speed              load: low         │
└────────────────────────────────────────────────────┘
```
- **Country**: mono 2-letter code in a small box, NOT a flag emoji.
- **Name**: `titleMd` `textHi`; **city/protocol** sub: `bodySm` `textMed`.
- **Ping**: `monoMd`, bucketed color. `<60ms` `success`, `60 to 150` `warning`, `>150` `textMed`, timeout `danger` shown as a centered dot. A small 5-bar signal glyph mirrors the bucket (`borderStrong` off, `textMed` on).
- **Tag**: mono uppercase pill, `textMed` text + `borderStrong` border; `tag.ok` uses `success` border + text.
- **Relay marker**: a mono "via relay" tag when the proxy name carries the relay suffix. A glyph/text marker, never color-only.
- Trailing: chevron `textLow`, or a `textHi` check on the selected row.

### 5.3 Bottom navigation (mobile)

Height **80** including safe area, `surface1` at ~92% over a `blur(12px)` backdrop (the single allowed blur), top 1px `borderSubtle`. Items: **Home · Servers · Market · Browser · Settings**. Active = icon + label `textHi`; inactive = `textLow`. **No pill, no colored indicator** behind the active icon. Labels always visible, `caption`. On desktop this becomes a left rail (72 collapsed / 240 expanded) with the same tokens.

### 5.4 Cards

`surface1` (or `surface2` raised), radius 16, padding 16 to 20, 1px `borderSubtle` (preferred over shadow in dark). Section header above a card: `caption` UPPERCASE `textLow`. Stat values use `display`/`titleLg` with `mono` units.

### 5.5 Sheets / modals

Bottom sheet: `surface1`, top radius **22**, 1px top `borderStrong`, drag handle 36×5 `surface3` centered, `sheet` elevation, backdrop `overlayScrim`. Enter: slide-up ~260ms `easeOutCubic`. Desktop: centered dialog, max-width 520. Title `titleLg`, close top-right.

### 5.6 Toggles & controls

- **Switch**: track 46×28, knob 22. Off: track `surface3` + 1px `borderStrong`, knob `textMed`. On: track `accent` (neutral), knob `textOnAccent`. ~150ms.
- **Segmented / tabs**: pill container `surface2`, active segment `surface1` + `card` elevation, `label` text. Sliding indicator ~220ms.
- **Checkbox/radio**: 20px, `borderStrong` to `accent` fill with `textOnAccent` check.
- **Slider**: track `surface3`, active `accent`, thumb `accent`.

### 5.7 Code / OTP input

Per-digit boxes, `surface1`, 1px `borderSubtle`, radius 12, mono tabular 22px. Focus: `borderStrong` to `textHi` + soft 14% `textHi` halo. Error: `danger` border + `field-err` line in `danger` `bodySm`.

### 5.8 Traffic / usage

- **Throughput**: dual line/sparkline in an `surfaceInset` well, radius 16. Lines are **neutral** (`textHi` down, `textMed` up), not colored. Current value pinned in `display`-grade + `mono` units. Calm, ambient, no gridlines except a faint baseline.
- **Quota bar**: height 6, radius 3, fill `textHi` (neutral) on `surfaceInset`. Turns `warning` past ~85%, `danger` past ~97%. Label `bodySm` "42.1 of 100 GB" + reset date `caption`.

### 5.9 Empty / loading / error

- **Loading**: skeleton shimmer (`shimmerBase` blocks, `shimmerHi` sweep, ~1.2s). No bare spinner on full screens.
- **Empty**: centered Lucide line glyph (`textMed`), `titleMd` headline, `bodyMd` `textMed` one-liner, one primary CTA. e.g. "No servers yet. Pull your subscription." + "Refresh".
- **Error**: `danger` glyph, plain cause, "Try again" primary + "Details" quiet (raw error in `monoSm` inside a `surfaceInset` block).
- **Offline / panel down**: a `warning` inline banner "Showing cached config".

### 5.10 Chips, tags, banners

- **Chip**: pill, `surface2`, `label`; selected = `accentSubtle` (neutral) + 1px `borderStrong`.
- **Tag**: mono uppercase, `textMed` + `borderStrong` border; `ok` variant uses `success`.
- **Status badge**: tiny pill. Connected `successSubtle`/`success`; Expiring `warningSubtle`/`warning`.
- **Inline banner**: full-width, radius 12, tinted bg + matching Lucide icon + `bodySm`; variants info(neutral)/warning/danger.

---

## 6. Screens

See `demo/caramba-demo.html` for the worked, anti-slop reference of every screen: Splash, Login (Telegram + Email, OTP), Autotune, Home/Connect (dial + stats + quota), Servers (mono country codes, ping buckets, signal bars, relay tags), Protocol picker, Profile (devices, referral code in mono), Settings (grouped rows, switches), generic bottom sheet, toast. Mobile portrait, 20px edge padding; desktop reflows to left rail + centered content column (≤960) with the same components.

---

## 7. Motion & Interaction

1. **Calm.** micro 120ms, standard 220ms, sheets ~260 to 320ms. Curves: `easeOutCubic` enter, `easeInOutCubic` state morph. The atmosphere layer runs longer on purpose (620 to 1200ms, table in section 4.2) so the world reads as a consequence of the state change.
2. **Animate `transform`/`opacity` only, plus exactly one ambient layer.** No infinite breathing glows. The connecting arc rotates. The atmosphere layer of section 4.2 is the only ambient motion in the product, it only runs while connecting or reconnecting, it is capped at 30fps (20 under low power), and its ticker is stopped in every settled state and whenever the app is not resumed. Anything else that wants to move ambiently is out of spec.
3. **State changes are narrated.** Connect/disconnect cross-fades label + glyph and morphs the ring color.
4. **Haptics** (mobile): medium on connect/disconnect, light on toggle/selection, error pattern on fault.
5. **Numbers glide**, tabular figures prevent shift.
6. **Pressed feedback** everywhere: scale 0.98 + brightness within 80ms.

---

## 8. Accessibility

- **Contrast:** `textHi` on `surface1` ≈ 15:1 (dark) / ≈ 16:1 (light); `textMed` ≥ 4.5:1; status colors deepened in light to hold ≥4.5:1. Never color-alone: ping/load pair color with bars/icons; connected pairs green with a check glyph + "Protected".
- **Tap targets:** ≥ 44×44; icon buttons padded even when the glyph is 24.
- **Focus:** visible 2px `accent` (neutral) ring on all controls; logical tab order; the dial is a labeled toggle ("Connect" / "Disconnect, connected").
- **Reduced motion:** honor `MediaQuery.disableAnimations`, replacing the connecting arc and number tweens with static/opacity cross-fades.
- **Dynamic type:** respect platform text scaling; rows wrap, critical state text never truncates.
- **Screen readers:** announce state changes via a live region; decorative elements `excludeSemantics`.
- **Color-blind safety:** the three status families differ in lightness and are always paired with a shape/icon; relay is a text tag.

---

## 9. Dart Design Tokens

The tokens live in `lib/theme/` (not at the end of this doc). Names exported, for the screens stage:

**`lib/theme/colors.dart`**
- `class AppColors` with instances `AppColors.dark` and `AppColors.light`. Fields (all `Color`): `bgBase, bgCanvas, surface1, surface2, surface3, surfaceInset, borderSubtle, borderStrong, accent, accentVariant, accentDeep, accentSubtle, success, successDeep, successSubtle, warning, warningSubtle, danger, dangerDeep, dangerSubtle, info, textHi, textMed, textLow, textOnAccent, textOnSuccess, overlayScrim, shimmerBase, shimmerHi`. Getters: `accentGradient`, `connectedGradient` (both flat now), `cyanKiss` (aliases `success`).
- `abstract final class AppShadows`: `card, raised, sheet, glowAccent, glowConnected` (dark) and `cardLight, raisedLight, sheetLight, glowAccentLight, glowConnectedLight` (light). All neutral, no colored glow.

**`lib/theme/typography.dart`**
- `abstract final class AppType`: `display, headline, titleLg, titleMd, bodyLg, bodyMd, bodySm, label, caption, monoMd, monoSm` (all `TextStyle` getters, color applied at call site). Plus `sansFallback` / `monoFallback` `List<String>`.

**`lib/theme/spacing.dart`**
- `AppSpace`: `s0..s20`, `screenPadMobile (20)`, `screenPadDesktop (32)`.
- `AppRadius`: doubles `xs(8), sm(12), button(14), md(16), lg(22), xl(28), pill(999)`; `BorderRadius` consts `r8, r12, r14, r16, r22, r28, sheetTop`.
- `AppBorders`: `hairline(1), input(1.5), focus(2)`.
- `AppMotion`: `micro, standard, large, ringMorph` durations; `enter`, `morph` curves.
- `AppOrb`: `diameterMobile(196), diameterDesktop(232), ringStroke(2), faceMobile(146)`.
- `AppBreakpoints`: `desktop(840), contentMaxWidth(960), dialogMaxWidth(520)`.

**`lib/theme/tokens.dart`**
- `class AppTokens extends ThemeExtension<AppTokens>` carrying `colors` (`AppColors`) + resolved `elevCard, elevRaised, elevSheet, glowAccent, glowConnected`. Factories `AppTokens.dark()` / `AppTokens.light()`.
- Extension `AppTokensX on BuildContext`: `context.tokens` (the extension) and `context.c` (the active `AppColors`).

**`lib/theme/app_theme.dart`**
- `class AppTheme` with `AppTheme.dark()` / `AppTheme.light()` returning `ThemeData` (Material 3, neutral `ColorScheme`, full component themes, `AppTokens` registered as an extension).

---

## 10. Notes for the Flutter Team

- **No hue, ever.** `accent` is a neutral. If you reach for a color that is not green/amber/red status, you are off-spec. The `info` token is intentionally neutral gray.
- **The dial** is a `CustomPainter` (ring arc + flat face); only the connecting arc animates, gated behind a `reduceMotion` flag.
- **Tabular figures** are baked into `display` and `mono*`; apply `FontFeature.tabularFigures()` on any ad-hoc number style.
- **Single blur** allowed: the bottom nav backdrop. No other glass.
- **Glows** (`glowAccent`/`glowConnected`) are faint neutral lifts; do not turn them into colored halos.
- **Desktop** swaps bottom nav to a left rail, sheets to centered dialogs (max 520), bumps the orb to 232px; everything else is shared.

*End of exarobot Design System v2.0 (de-slopped, neutral).*

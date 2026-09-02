# Concept B: CARRIER

**One line:** The Home screen is a weather report on your own signal. Disconnected, a low grey ceiling presses down onto a fenced horizon and the carrier trace is torn by interference. Connected, the ceiling lifts, the horizon runs unbroken to both edges, and the distant lights come up all the way out.

---

## 1. The metaphor

Three physical facts do all the work. None of them is decoration; each one is a reading.

1. **The ceiling.** Horizontal strata of ink stacked from the top of the frame down. When you are exposed, the deck is low, thick, and layered: eight strata compressed into the top quarter of the screen, with a hard lit edge under each one. When you are protected, the deck has lifted out of frame and left two thin strata at the very top. Height and layer count are the primary signal. A user glancing at the phone from across a table reads the state from the amount of open space above the dial, before any color or text resolves.

2. **The horizon.** A single hairline at the exact vertical center of the connect dial, so the dial sits on it like an aperture. Disconnected, that line exists only as two short stubs either side of the dial, broken into dashes and studded with short vertical ticks: a fence, a wall, the edge of what you can reach. Connected, the ticks retract, the dashes close, and the line runs continuously to both edges of the screen. This is the single strongest image in the concept: **the wall becomes a horizon.**

3. **The distant lights.** Twenty-six one-pixel beacons sitting on the horizon at irregular intervals. They never move and they never twinkle. They are gated by one scalar, `reach`, which is how far out you can see. Disconnected, `reach` is short enough that only the three or four sitting right at the dial's edge resolve at all, and faintly. You see nothing past your feet. Connected, they ignite outward from the dial in a distance-ordered stagger and stay lit, the far ones dimmer than the near ones so the row reads as depth rather than as a dotted line. That is the network being visibly reachable.

Underneath those three, two supporting registers:

4. **The carrier trace.** One 1px polyline in the 52px band below the state label. Disconnected it is interference: value noise, hard amplitude, three fixed dropout gaps where the trace simply is not there. Connecting, a lock cursor sweeps left to right and the noise resolves behind it. Connected it is a slow clean two-harmonic wave, amplitude 3.2px. It is never bars, never mirrored, never filled. It is a scope, not an equalizer.

5. **Grain.** A 96px tiled noise texture at 1.4% to 6.2% alpha, masked to the ceiling zone only. Interference lives in the murk; clear air has no grain. This is why the grain mask is the ceiling and not the screen.

**Color is almost absent.** Everything above is neutral. Each state adds one radial wash, centered on the dial, capped at 8% alpha, which reads as light coming through the opening rather than as a color filter. Lightness and structure carry the meaning; hue only confirms it.

---

## 2. Per-state

Geometry below is for a 390x844 viewport. Ceiling heights are fractions of viewport height. `reach` drives horizon visibility and beacon ignition through one shared envelope: a horizon segment or beacon at normalized distance `d` from screen center (0 at the dial, 1.0 at the left/right edge) is visible when `0.66 + 0.50 * reach > d`, with a 0.20-wide soft edge.

### Disconnected
- **Composition:** ceiling at `0.245` (207px), eight strata, thickest at the top. Sky compressed into a 13px sliver above the horizon. Horizon at y=220 reduced to two 60px stubs poking out from behind the dial, dashed `[10, 16]`, with 9px fence ticks every 26px. Three or four beacons resolve just past the dial's edge and no more. Carrier is torn noise with three dropout gaps.
- **Light:** flat. No lift band at the horizon, no wash. The brightest pixel on screen is the dial ring.
- **Motion:** 8fps. Deliberately low. The grain offset jumps between four precomputed positions and the noise trace advances in coarse steps, so the field visibly stutters. The low frame rate is the aesthetic and the battery budget agreeing with each other: the state that should look jammed is also the state that must cost nothing while the phone sits in a pocket.
- **Reads as:** boxed in. Low lid, short horizon, nothing past the fence.

### Connecting
- **Composition:** ceiling lifts to `0.150` (127px), strata thin from eight to five. Fence ticks retract to zero. Dashes open to `[19, 10]`. `reach` 0.15, so the horizon pushes about 20px further out on each side and a couple more beacons come up faint.
- **Light:** `atmoCastConnecting` at 6%, an amber wash centered on the dial.
- **Motion:** 30fps for the duration. The carrier hunts: a lock cursor travels left to right at 0.85 screens/second and behind it the noise partially resolves, then loses lock at the wrap. Nothing else loops.
- **Reads as:** the lid is moving. Something is being negotiated.

### Connected
- **Composition:** ceiling at `0.064` (54px), two strata, a thin cap at the very top. The sky gradient's horizon stop brightens from `atmoSkyHorizonClosed` to `atmoSkyHorizonOpen`, so there is a real luminous band where the dial sits. A 52px neutral lift band sits on the horizon. Horizon solid, full width, edge to edge. Every beacon the dial does not occlude is lit, twelve of the twenty-six on a 390pt frame, far ones at 45% of near brightness. Carrier is the clean wave.
- **Light:** `atmoCastConnected` at 7%, green, radial from the dial.
- **Motion:** 6fps, and only the carrier phase advances. Everything else is a static frame. Under OS low-power mode or battery saver, drops to 0fps and the ticker stops.
- **Reads as:** open. You can see all the way out.

### Error
- **Composition:** ceiling slams to `0.265` (224px), which is 4px *below* the horizon line, so the lid physically crosses it. Nine strata. `reach` 0, so only the few beacons at the dial's edge survive. The horizon fractures at a fixed x derived from the error code (stable across rebuilds, so the same failure always breaks in the same place, `0.765` of width in the reference build, chosen to clear the dial so the break is never hidden behind it): a 14px gap with a 2px vertical step between the two sides, plus a 10px `danger` notch at the break. The carrier flatlines with one spike at the break's x.
- **Light:** `atmoCastError` at 8%, red, radial.
- **Motion:** one 6px horizontal displacement of the whole field over 120ms, matching the dial's existing shake, then the composition is fully static and the ticker stops. No looping alarm. An error that keeps moving is an error you learn to ignore.
- **Reads as:** the thing you were standing on does not line up any more.

### Reconnecting
- **Composition:** ceiling at `0.190` (160px), six strata. Fence ticks at 40% height, half-raised. Dashes present but sliding inward at 12px/s toward the dial, so the horizon looks like it is trying to close itself. `reach` 0.22.
- **Light:** `atmoCastConnecting` at 5%, one notch below Connecting, because this is a degraded state, not a fresh attempt.
- **Motion:** 8fps. The six nearest beacons run a deterministic two-state duty cycle (on for 3 frames, off for 2, phase offset per index from a fixed table). Deterministic, never `Random()`, so it is reproducible in golden tests and never looks like noise. The carrier hunts at 60% of the connecting sweep rate.
- **Reads as:** still walled off, but working on it. Pairs with the existing amber kill-switch banner.

---

## 3. Transition choreography

Durations are in ms from the state change. Curves match the existing `AppMotion` vocabulary where possible.

### Disconnected to Connecting (on tap)
| t | What | Duration | Curve |
|---|---|---|---|
| 0 | Haptic medium, dial ring morphs to amber (existing 350ms) | 350 | easeInOutCubic |
| 0 | Fence ticks retract into the horizon, staggered 12ms per tick outward from center | 240 | easeOutCubic |
| 0 | Ceiling lifts 207px to 127px, strata 8 to 5 | 900 | easeInOutCubic |
| 0 | Carrier switches to hunt mode, lock cursor starts its first pass | continuous | linear |
| 0 | Cast crossfades neutral to amber 6% | 400 | linear |
| 0 | Grain 5.5% to 3.5% | 400 | easeOutCubic |
| 0 | `reach` 0 to 0.15 | 300 | easeOutCubic |

### Connecting to Connected (handshake completes)
This is the one moment in the app that gets to feel like something.

| t | What | Duration | Curve |
|---|---|---|---|
| 0 | **The lock.** Carrier snaps from noise to the clean wave with a slight amplitude overshoot | 180 | easeOutBack (tension 1.3) |
| 60 | Ceiling lifts 127px to 54px, strata 5 to 2 | 700 | easeOutQuint |
| 0 | Cast crossfades amber to green 7% | 500 | linear |
| 0 | Grain 3.5% to 1.4% | 600 | easeOutCubic |
| 120 | Horizon dashes close to solid, and the sky's horizon stop brightens | 780 | easeOutCubic |
| 200 | `reach` 0.15 to 1.0. Beacons ignite outward, each fading in over 220ms as the reach envelope crosses it, so the ignition front travels from the dial to both edges | 900 | easeOutCubic |
| 380 | Horizon lift band fades in | 520 | easeOutCubic |

Settle at 1100ms. Then the ticker drops to 6fps.

The `easeOutQuint` on the ceiling is chosen over `easeOutCubic` on purpose: a very fast start with a long tail reads as a lid being pulled away, and the long tail keeps the eye on the opening while the beacons arrive.

### Connected to Disconnected
Deliberately asymmetric. Opening is earned and takes 1.1s. Closing is loss and takes 420ms.

| t | What | Duration | Curve |
|---|---|---|---|
| 0 | Carrier breaks to noise. No easing, it just goes, over 60ms | 60 | linear |
| 0 | Ceiling drops 54px to 207px, strata 2 to 8 | 420 | easeInCubic (accelerating: it falls) |
| 0 | `reach` 1.0 to 0. Beacons extinguish inward toward the dial | 260 | easeInCubic |
| 0 | Cast drains to neutral | 300 | linear |
| 0 | Grain to 5.5% | 200 | linear |
| 160 | Fence ticks rise back out of the horizon | 260 | easeOutCubic |

### Any state to Error
| t | What | Duration | Curve |
|---|---|---|---|
| 0 | Whole field displaces 6px horizontally and back, once | 120 | easeInOutCubic |
| 0 | Ceiling slams to 224px, strata to 9 | 90 | easeInQuart |
| 0 | Horizon fracture opens at the seeded x, 2px step appears | 150 | easeOutCubic |
| 0 | Carrier flattens, spike appears at the break x | 150 | easeOutCubic |
| 0 | `reach` to 0 | 120 | easeInCubic |
| 0 | Cast to red 8% | 200 | linear |

Static from 400ms. Ticker stops.

### Connected to Reconnecting
Ceiling drops 54px to 160px over 300ms `easeInCubic`, `reach` 1.0 to 0.22 over 240ms, fence ticks rise to 40% over 200ms, cast crossfades green to amber 5% over 350ms. Fast, because losing the tunnel is not a gentle event, but it does not slam the way an error does.

---

## 4. Atmosphere tokens

These layer **under** the existing `AppColors` and are only consumed by the atmosphere painter. Nothing in the component layer may read them. They live in `lib/theme/atmosphere.dart` as `AppAtmosphere.dark` / `AppAtmosphere.light`, carried on `AppTokens` as `atmosphere`.

### 4.1 Color tokens

| Token | Dark | Light | Purpose |
|---|---|---|---|
| `atmoSkyTop` | `#0A0A0A` | `#F5F5F5` | Top stop of the field gradient. |
| `atmoSkyHorizonClosed` | `#0D0D0D` | `#F0F0F0` | Horizon stop of the field when the lid is down. Nearly flat against the top. |
| `atmoSkyHorizonOpen` | `#1E1E1E` | `#FDFDFD` | Horizon stop when the sky is clear. The open sky is the brightest region in dark, the cleanest in light. |
| `atmoSkyBottom` | `#0B0B0B` | `#FAFAFA` | Bottom stop, matched to `bgBase` so cards sit on it without a seam. |
| `atmoCeilingInk` | `#000000` | `#CFCFCF` | Stratum fill. Alpha per stratum, cumulative cap `atmoCeilingMaxAlpha`. |
| `atmoCeilingEdge` | `#FFFFFF` at 5.0% | `#000000` at 4.8% | 1px lit edge under each stratum. This is what makes the deck legible in dark, where fill differences of a few values are invisible on LCD. |
| `atmoHorizonClosed` | `#3D3D3D` | `#D2D2D2` | Horizon hairline while restricted. Same value as `borderStrong`. |
| `atmoHorizonOpen` | `#525252` | `#ADADAD` | Horizon hairline when clear. One step of lightness, no hue. |
| `atmoBeacon` | `#A0A0A0` | `#6E6E6E` | Distant lights. Same value as `textMed`, so they can never out-shout text. |
| `atmoCarrier` | `#8A8A8A` | `#7A7A7A` | Locked carrier trace. |
| `atmoCarrierBroken` | `#6A6A6A` | `#9A9A9A` | Interference trace. Dimmer in dark, lighter in light: broken signal is always the lower-contrast of the pair. |
| `atmoHorizonLift` | `#FFFFFF` at 5.0% | `#000000` at 3.0% | 52px neutral lift band on the open horizon. A lightness lift, not a colored halo. Sits inside the `glowAccent` rule in DESIGN.md section 3.3. |
| `atmoCastConnecting` | `#FF9F0A` | `#A85D00` | Radial wash. Alpha capped at `atmoCastMaxConnecting`. |
| `atmoCastConnected` | `#30D158` | `#1E9E54` | Radial wash. |
| `atmoCastError` | `#FF453A` | `#C8102E` | Radial wash. |
| `atmoFractureNotch` | `#FF453A` at 45% | `#C8102E` at 45% | The 10px vertical notch at the horizon break. The only saturated line work in the whole layer. |

### 4.2 Scalar tokens

| Token | Value | Purpose |
|---|---|---|
| `atmoCeiling` per state | `.245 / .150 / .064 / .265 / .190` | Ceiling bottom as a fraction of viewport height. Disconnected / connecting / connected / error / reconnecting. |
| `atmoStrata` per state | `8 / 5 / 2 / 9 / 6` | Visible stratum count. Fractional values are allowed during a tween; stratum `k` gets `alpha *= clamp(strata - k, 0, 1)`. |
| `atmoGrain` per state | `.055 / .035 / .014 / .062 / .048` | Grain alpha. |
| `atmoReach` per state | `0 / .15 / 1.0 / 0 / .22` | Horizon extent and beacon ignition. |
| `atmoFence` per state | `1 / 0 / 0 / 1 / .4` | Fence tick height multiplier. |
| `atmoCastMax` per state | `0 / .06 / .07 / .08 / .05` | Wash alpha at the radial center. |
| `atmoCeilingMaxAlpha` | `.75` dark, `.50` light | Cumulative stratum alpha cap. In light this is what keeps the heaviest lid at `#E2E2E2` and no darker. |
| `atmoVisibilityBase` | `0.66` | Horizon visible out to `d = base + 0.50 * reach`, soft edge width `0.20`. |
| `atmoBeaconCount` | `26` | Irregular x, fixed seed. |
| `atmoFractureX` | `0.765` | Horizon break position. Seeded from the error code in production, always clamped clear of the dial. |
| `atmoBeaconFalloff` | `0.55` | Far beacons at `1 - falloff * d` of near brightness. |
| `atmoCarrierAmp` | `3.2` locked, `7.5` broken | Trace amplitude in logical px. Hard cap 9. |
| `atmoStrengthHome` | `1.00` | Layer opacity on Home. |
| `atmoStrengthList` | `0.35` | Servers, Market, Browser. |
| `atmoStrengthSettings` | `0.22` | Settings, Profile. Ticker off. |
| `atmoStrengthAuth` | `0.60` | Splash, Login, Autotune. Static. |
| `atmoFps` per state | `8 / 30 / 6 / 0 / 8` | Resting frame rate. Transitions always run at 30 for their duration, then fall back. |
| `atmoTransitionFps` | `30` | |
| `atmoGrainTile` | `96` | Noise tile edge in device pixels, drawn 1:1. |
| `atmoTypeBandTop` | `0.395` | Fraction of height above which no ceiling or grain may render. See section 8. |

Everything above is a token because every one of these numbers is a taste decision someone will want to argue about, and none of them should end up as a literal in a painter.

---

## 5. Rendering

### Painter, not shader

`CustomPainter`, and specifically not a `FragmentProgram`.

- The layer is line work: hairlines, dashes, 26 one-pixel dots, one 130-point polyline, nine rects. Canvas draws antialiased thin geometry well and cheaply. Antialiased hairlines in GLSL are a per-pixel derivative problem and cost far more than they are worth here.
- The one per-pixel element, grain, is solved without a shader by a precomputed tile drawn through an `ImageShader` with `TileMode.repeated`. One draw call.
- A `FragmentProgram` adds a `.frag` asset, a first-paint shader-compile jank risk, and a full-viewport per-pixel cost at 3x device pixel ratio, roughly 1.2M invocations per frame, to render what is currently about 40 draw ops.

### What is precomputed

| Thing | When | Cost |
|---|---|---|
| Grain tile, 96x96 `ui.Image` from a seeded LCG through `decodeImageFromPixels` | Once per theme at app start | ~36KB, ~2ms |
| Sky `LinearGradient` shader | On state settle, cached in the painter's state object | Negligible, but rebuilding it every frame is the classic mistake |
| Beacon x positions, 26 doubles with fixed jitter | Once, `const`-adjacent static | Zero |
| Value-noise table, 512 `Float32List` entries | Once | Zero. Deterministic, so golden tests are stable |
| Stratum geometry table, 9 `[offset, height, alpha]` triples | Compile time `const` | Zero |
| Settled backdrop (sky + strata + grain + cast) as a `ui.Image` | Optional tier-2, captured on state settle | Turns the resting frame into one blit plus the line work |

### Repaint budget

- The atmosphere sits under a `RepaintBoundary` so it never dirties anything above it, and the content column sits under its own boundary so the timer ticking does not dirty the atmosphere.
- `shouldRepaint` returns true only when `frame`, `phase`, `state`, `strength`, `reduceMotion` or `colors` differ. `frame` is an `int` bumped by a gated ticker, never a raw `Animation<double>`, which is how the effective frame rate is controlled: the ticker callback only bumps `frame` when `elapsed - lastEmit >= 1000 / targetFps`.
- Draw ops per frame: 1 gradient rect, up to 9 stratum rects plus 9 edge lines, 1 tiled-image rect, 1 radial rect, 1 lift rect, ~60 horizon segments (batched into one `drawPoints` / `drawRawPoints` call), 26 dots (one `drawRawPoints` with `PointMode.points`), 1 polyline. Under 25 real calls after batching.

### Expected cost

On a mid-range Android (Snapdragon 6-series class, 1080x2400): the frame is dominated by the two full-viewport fills, the sky gradient and the grain tile. Budget ~1.2ms raster, ~0.4ms UI.

- Disconnected at 8fps: about 1.3% of one core. Under a milliwatt against the tunnel's own draw.
- Connecting at 30fps for a typical 2 to 4 second handshake: about 5% of one core, bounded.
- Connected at 6fps: about 1%. Under low-power mode, 0fps and the ticker is stopped, so the cost is exactly zero.
- Error and reduce-motion: 0fps, one frame ever.

If a device is slower than that, the tier-2 backdrop cache above removes both full-viewport fills from the resting frame and leaves only the carrier polyline redrawing, which is a few hundred microseconds.

### How it pauses

- `AppLifecycleState.paused`, `.inactive`, `.hidden`: `ticker.stop()`, unconditionally.
- Route not current (Servers pushed over Home): `TickerMode` already muffles it; the painter also drops to its screen strength.
- Settings and Profile: strength 0.22 and the ticker never starts. A settings screen has no business animating.
- `MediaQuery.disableAnimations`: reduce-motion path, no ticker.
- `MediaQuery.highContrast`: strength 0 everywhere, flat cast at 3% only.
- Resting fps of 0 after a transition settles: `ticker.stop()` outright, not a ticker running at 0.

---

## 6. Reduce-motion fallback

Every state already has a settled composition. In reduce-motion the painter renders only settled compositions and the ticker never starts.

- State change renders the outgoing settled composition and the incoming settled composition into two layers and cross-fades opacity over 200ms, `easeInOutCubic`. That is five frames, then the ticker stops again. Nothing translates, nothing morphs, nothing sweeps.
- Disconnected loses its 8fps stutter and becomes a still frame with the fence up, the horizon stubbed and the carrier drawn in its worst-case torn pose. Composition alone still reads restricted, which is the test any of this has to pass.
- Grain uses offset 0 and never jitters.
- Reconnecting loses the beacon duty cycle: the six near beacons render at their mean duty (60% alpha) instead.
- The error shake is dropped entirely; the error composition appears through the 200ms cross-fade like every other state.

The concept degrades to a set of five still images, and those five images are distinguishable from each other at a glance. If they were not, the motion would be doing work the composition should have been doing.

---

## 7. Screen integration

The atmosphere is one widget, `AtmosphereLayer(state, strength)`, mounted once in the app shell behind the `Navigator`, not per screen. Screens declare a strength; the shell tweens between strengths over 220ms when the route changes.

| Surface | Strength | Elements drawn | Ticker |
|---|---|---|---|
| **Home** | 1.00 | All. The horizon is pinned to the dial's vertical center via the dial's known geometry, so the dial sits on the line as an aperture. | Per state |
| **Servers, Market, Browser** | 0.35 | Sky and ceiling and cast only. No horizon, no beacons, no carrier. | Per state, but line work is skipped so the frame is two fills |
| **Settings, Profile** | 0.22 | Sky and cast only. | Off. Static frame at the current state's settled composition |
| **Splash, Login, Autotune** | 0.60 | All, disconnected composition, static | Off |
| **Sheets and dialogs** | unchanged | The atmosphere is behind `overlayScrim` at 60%, so it is effectively gone. No special case needed. | unchanged |

The reasoning behind dropping the line work on list screens: a moving trace behind scrolling rows is noise, and beacons behind a list read as dust on the screen. Ceiling and cast survive because they are large, slow and out of the way, and they keep the app feeling like one continuous place rather than "the screen with the effect" plus five plain screens.

The Splash and Login choice is a small narrative payoff: the app opens under a low ceiling, and the first successful connect is the first time the user sees the sky open. Nobody will name it, and everybody will feel it.

---

## 8. Accessibility and contrast

**The type-band invariant.** No ceiling stratum, grain, lift band or fracture may render below `atmoTypeBandTop` (0.395 of height, y=334 on a 390x844 viewport). The lowest the ceiling ever reaches is the error state at y=224, which leaves 110px of margin. The invariant is asserted in a widget test, not just documented, because it is the thing that guarantees text contrast and it is the thing a future change will quietly break.

Below that line the only atmosphere elements are the carrier trace, which sits in its own 52px band between the sub-label and the first card, and the cast, whose radial falloff puts it at 1.5% to 2% alpha by the time it reaches the labels.

**Measured worst-case pairs** (brightest atmosphere pixel in dark, darkest in light, under each piece of text that sits directly on the layer):

| Pair | Ratio | Requirement | Result |
|---|---|---|---|
| `textHi` `#FAFAFA` on dark open sky `#1E1E1E` | 16.4:1 | 4.5 | pass |
| `textMed` `#A0A0A0` on dark open sky `#1E1E1E` | 6.4:1 | 4.5 | pass |
| `warning` `#FF9F0A` on dark sky + amber cast `#2C261D` | 7.3:1 | 3.0 (20px/600 is large text) | pass |
| `success` `#30D158` on dark sky + green cast `#1F2B22` | 7.3:1 | 3.0 | pass |
| `textMed` `#585858` on light sky `#F0F0F0` | 5.6:1 | 4.5 | pass |
| `warning` `#A85D00` on light sky `#F2F2F2` | 5.7:1 | 3.0 | pass |
| `danger` `#C8102E` on light sky + red cast `#F5EBEC` | 6.1:1 | 3.0 | pass |
| `textLow` `#7C7C7C` on dark open sky `#1E1E1E` | 4.0:1 | 3.0 (caption at 11/600 is not large; used only inside opaque cards) | n/a on the layer |

Cards are opaque `surface1`, so every ratio inside them is unchanged from the current system.

**Other notes:**

- The whole layer is wrapped in `ExcludeSemantics`. It announces nothing. State changes are announced by the dial's existing live region, unchanged.
- The atmosphere is never the sole carrier of state. Ring color, glyph, label and timer already carry it, and the atmosphere is redundant confirmation. A user who cannot perceive the layer at all loses nothing.
- Color-blind safety: state is fully legible from ceiling height, horizon continuity and beacon count, with zero hue. That is the whole point of putting the meaning in structure.
- `MediaQuery.highContrast` sets strength to 0 and leaves a flat 3% cast, so the layer collapses to almost nothing without a separate code path.
- Dynamic type: the layer's geometry is anchored to the dial's laid-out rect and the carrier band's laid-out rect, both read from the render tree, so growing text pushes the carrier band down and the atmosphere follows. Nothing is hard-coded to a pixel y.

---

## 9. Risks, and what would make this look cheap

1. **Beacons that twinkle become a starfield, and a starfield is a screensaver.** They must never move, never randomly flicker, never vary in size. They sit on one line at fixed x and only change opacity on state transitions. The only flicker in the entire concept is the reconnecting duty cycle on six specific dots, and it is a fixed on-3-off-2 pattern from a table, not `Random()`.

2. **The cast turning the app into "green mode / red mode."** Capped at 8%, applied as a radial from the dial and only to the sky and strata, never to surfaces, text, borders or the grain. If someone asks for it stronger, the answer is to raise the ceiling further, not to raise the alpha. A code review rule: any `atmoCast*` alpha above 0.08 is rejected.

3. **Grain that reads as a JPEG artifact or a film filter.** The tile is drawn at 1:1 device pixels, never scaled, and it is masked to the ceiling zone. Grain over the open sky would be both cheap-looking and semantically wrong: clear air has no interference.

4. **Evenly spaced, smoothly blended strata become a striped gradient, which is exactly the mesh-background look the anti-slop rules exist to prevent.** The stratum table is non-uniform by construction (heights 0.34, 0.13, 0.09, 0.07, 0.055, 0.045, 0.038, 0.030), each has a hard 1px lit edge, and there are never more than nine. Structure, not blur.

5. **The carrier turning into a music visualizer.** One polyline. Never bars, never a mirrored pair, never filled, amplitude hard-capped at 9px. If anyone adds a fill under the curve, the concept is dead.

6. **Running the layer at 60fps and shipping a battery regression.** This is the real risk, and it is the one that will actually happen if the fps gating is treated as an optimization instead of a requirement. `atmoFps` is a token and the ticker gate is the first thing to check in review. A frame-rate assertion in the debug build (log a warning if the atmosphere paints more than `atmoFps + 2` times per second) is cheap insurance.

7. **In light theme, an overcast lid on white reading as a dirty screen.** The light lid is a clean neutral `#CFCFCF` with visible stratum edges. It is layered, not smeared. Any blur on it and it becomes a smudge.

8. **The horizon not landing on the dial's center.** If the line is even 2px off, the whole image falls apart and reads as an accidental divider. It is anchored to the dial's actual laid-out rect, and there is a golden test for it.

9. **Doing this on every screen at full strength.** The strength ladder exists because a Settings screen with weather behind it is a toy. Home is where the atmosphere earns its place; everywhere else it is a tint that keeps the app coherent.

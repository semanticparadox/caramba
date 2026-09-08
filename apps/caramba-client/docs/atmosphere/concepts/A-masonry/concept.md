# Concept A: Masonry

**One line:** The background is a wall. Disconnected it is closed masonry pressed flat against the glass; connected the same courses retreat into their corners and become an open lattice with depth and light behind it.

---

## 1. The metaphor

The brief's family is walls and lattice. The version I chose commits to a single structural fact:

> **The wall and the lattice are the same geometry at two values of one parameter.**

A running-bond grid of cells covers the frame. Every cell has an openness `o ∈ [0,1]`.

- At `o = 0` the cell is a filled panel with a dark mortar seam on every edge. Edge to edge, this is an unbroken wall. There is no route through it and nothing behind it.
- At `o = 1` the seam has withdrawn into four corner brackets and the fill has drained to the "look through" value. What is left is a node with struts leaving it. Edge to edge, this is a lattice.

Nothing is added between the two states and nothing is swapped out. The wall *becomes* the lattice by letting go of its middles. That is the whole idea, and it is why the transition can be a single wavefront rather than a cross-fade between two artworks.

Two supporting moves carry the rest of the meaning:

**Depth.** Disconnected the wall is one plane, flat on the glass, zero parallax, corners darkened by a vignette. There is no room in the composition. Connected, two more copies of the same lattice appear at scale 1.34 and 1.78 about the dial centre at 34% and 12% strength. The wall now has thickness and the frame reads as something you are passing through rather than facing.

**Light.** The vignette (dark corners, boxed in) and the centre lift (light arriving from beyond the openings) are the same axis run in opposite directions. Disconnected: vignette 1.0, centre light 0. Connected: vignette 0, centre light 1.0. Lightness alone, no hue, tells you whether the world is closed or open.

Hue is confirmation only. Status colour enters as a colour **mix of at most 16%** on strut and node strokes, scaled by openness, so a closed wall is always fully neutral and only an opening can carry a status hue. Fills are never tinted.

---

## 2. Per state

Frame is 390×844. The vanishing point and every wavefront origin is the dial centre, a fixed point at (195, 230). It never moves.

### Disconnected
- **Composition.** Full-frame running-bond wall, `o = 0` everywhere. Panels alternate by course (`atmoPanel` / `atmoPanelAlt`) so the bond reads as brickwork rather than graph paper. Seams are the darkest value on screen. One plane only. Corners pulled down by `atmoVignette` at full strength.
- **Motion.** None. The canvas paints one frame and the ticker stops. 0 fps for as long as the state lasts.
- **Light.** No centre lift. The brightest thing on the frame is the dial face.
- **At a glance.** Closed, static, boxed in, pressed close. The eye has nowhere to go except the dial, which is the only affordance.

### Connecting
- **Composition.** A shell of open wall travels outward from the dial and is sealed shut behind it. Two wavefronts on the same easing curve, the second trailing the first by 0.22 of the period, so the open shell starts wide near the dial and thins as it reaches the frame edge. Peak openness inside the shell is 0.52, never full: the wall gives, then closes again. Depth planes rise to 50%, vignette drops to 0.30.
- **Motion.** 1.9 s loop, the only state that animates continuously. 30 fps.
- **Light.** Centre lift at 14%, just enough to say something is happening behind the wall.
- **At a glance.** The wall is being tried and it will not stay open. This is the handshake, not the connection. Amber tint sits only on the shell, mixed at about 12%, so the still frame is close to neutral and the meaning is in the movement.

### Connected
- **Composition.** `o = 1` everywhere. Struts and nodes only. The lattice **dissolves toward the frame edge** (a radial fade of up to 82% back into the base), so density is concentrated where the light is and the corners are left clean. All three depth planes at full. Vignette at 0, centre lift at full.
- **Motion.** Static frame, 0 fps. Every 4.8 s a **reach pulse** leaves the dial and crosses the lattice in 1.6 s: a narrow Gaussian band of raised strut luminance travelling outward along the existing structure, peaking mid-travel and dying before the edge. It is a reachability probe fanning out over the mesh, not a decoration and not a scan line. Duty cycle is about 33%; between pulses the canvas does not repaint at all.
- **Light.** Full centre lift. The composition is brightest in the middle, where the openings are.
- **At a glance.** Open, calm, lit, with depth. Critically, **connected is quieter than disconnected**: fewer marks on screen, no dark corners, and the ink concentrated in the middle. Protection should feel like relief, not like more UI.

### Error
- **Composition.** The wall is closed, `o = 0`, but it is a **damaged** wall: six cells around the dial are gone. Each void is drawn at the "look through" value with a `atmoTintError` outline at 34% and a faint node-coloured lip on its bottom and right edge, the light catching a broken brick. Vignette at full, no depth, no centre light.
- **Motion.** On entry only: the sweep halts at whatever radius it had reached, holds 90 ms, then a hard close over 230 ms on `easeInQuad` (the wall slams), a single 190 ms lateral shake of ±3.2 px on the whole lattice with three half-cycles and a linear decay, then the voids fade in over 280 ms. After that, static at 0 fps. Nothing blinks and nothing loops.
- **At a glance.** It tried, it broke, it is shut, and the damage is at the connection point. The bricks are missing near the dial and nowhere else, so the fault is spatially attributed.

### Reconnecting
- **Composition.** Same sweep mechanism as connecting, but the wall does not start from closed. Peak openness 0.62 and depth at 70% are held from the connected state, so the ground you gained is still visible. Period stretches to 2.6 s and the shell moves more slowly. Centre lift held at 22%.
- **Motion.** Continuous, but noticeably calmer than a cold connect. 30 fps.
- **At a glance.** Still holding, working slower. Distinguishable from a cold connect without reading a word, because the wall behind the shell is half open rather than solid. The amber kill-switch banner appears above the dial in the content layer.

---

## 3. Transition choreography

Every transition is driven by one of four kinds: `flat`, `sweep` (two chasing wavefronts), `openRun` (one wavefront, no chaser), `closeRun` (one wavefront running inward). There is no cross-fade between rendered states anywhere.

| From → to | What moves | Duration | Curve |
|---|---|---|---|
| any → **connecting** | openness → 0.52 | 340 ms | easeOutCubic |
| | depth → 0.50, centre light → 0.14 | 430 ms | easeOutCubic |
| | vignette → 0.30 | 420 ms | easeOutCubic |
| | tint → amber | 320 ms | linear |
| | then the 1.9 s sweep loop begins | | |
| **connecting → connected** | the sweep stops chasing itself: the open wavefront runs 0 → 1.30 and never seals | 620 ms | easeOutQuint |
| | openness → 1.0 | 300 ms | easeOutCubic |
| | depth → 1.0 | 700 ms | easeOutCubic |
| | centre light → 1.0 | 800 ms | easeOutCubic |
| | vignette → 0 | 620 ms | easeOutCubic |
| | tint amber → green | 520 ms | linear |
| | dial ring morphs to `success` (existing spec) | 350 ms | easeInOutCubic |
| | settles to a static frame, first reach pulse fires | at 660 ms | |
| **connected → disconnected** | closing wavefront 1.30 → −0.30, inward | 520 ms | easeInOutCubic |
| | depth → 0, centre light → 0 | 600 ms | easeOutCubic |
| | vignette → 1.0 | 640 ms | easeOutCubic |
| | tint drains to neutral | 420 ms | linear |
| **connecting → error** | hold at current radius | 90 ms | n/a |
| | closing wavefront → −0.30 (slam) | 230 ms | easeInQuad |
| | lateral shake ±3.2 px, 3 half-cycles, decaying | 190 ms from +300 ms | sine × linear decay |
| | depth → 0, centre light → 0 | 300 / 260 ms | easeOutCubic |
| | vignette → 1.0 | 420 ms | easeOutCubic |
| | voids fade in | 280 ms from +330 ms | easeOutCubic |
| **error → connecting** (retry) | voids fade out, tint amber, normal connecting entry | 180 / 320 ms | linear |
| **connected → reconnecting** | openness 1.0 → 0.62, depth → 0.70, light → 0.22 | 340 / 430 ms | easeOutCubic |
| | tint green → amber | 320 ms | linear |

The signature: **opening is centrifugal, closing is centripetal.** Connect throws a wavefront out from under your thumb; disconnect draws one back to it, and the last cell to seal is the one at the dial. The asymmetry is what makes the two transitions feel like different events rather than one animation played backwards.

Total settle time on connect is under one second, matching the existing motion budget (micro 120, standard 220, large 260 to 320) at its large end plus the light settling behind it.

---

## 4. Atmosphere tokens

Layered strictly **under** the existing `AppColors` set. Nothing here replaces a token; nothing here is used for text, controls, borders, or icons. The atmosphere never paints above `z = 0` and never becomes a surface.

### Structure (theme independent)

| Token | Value | Purpose |
|---|---|---|
| `atmoCellW` | 46 dp | Cell width. Clamped `clamp(38, shortestSide / 8.5, 56)` so the bond stays coarse on small phones and does not moiré. |
| `atmoCellH` | 21 dp | Cell height. ~2.2:1 keeps it masonry, not graph paper. |
| `atmoSeam` | 1 physical px | Stroke width, `1 / devicePixelRatio` in logical units, coordinates snapped to physical pixels. |
| `atmoBand` | 0.22 | Wavefront softness in normalised radius. |
| `atmoOpenGapX` | 0.62 | Fraction of a horizontal edge that withdraws at full open. |
| `atmoOpenGapY` | 0.60 | Same for vertical edges. |
| `atmoDepth` | `[1.0, 1.34, 1.78]` | Depth plane scales about the dial centre. |
| `atmoDepthAlpha` | `[1.0, 0.34, 0.12]` | Strength of each plane at full depth. |
| `atmoDissolve` | 0.82 | How far an open strut fades back into the base at the frame edge. |
| `atmoTintMax` | 0.16 | Hard cap on the status colour mix. Strokes only. |

### Colour

| Token | Dark | Light | Purpose |
|---|---|---|---|
| `atmoBase` | `#0A0A0A` | `#FAFAFA` | Ground plane. Equals `bgBase`; the atmosphere never replaces the background, it draws on it. |
| `atmoPanel` | `#151515` | `#ECECEC` | Closed cell face, primary course. Lighter than base in dark (near, lit) and darker than base in light (a slab in front). |
| `atmoPanelAlt` | `#0E0E0E` | `#F4F4F4` | Alternate course. The lightness step is what makes the running bond legible. |
| `atmoVoid` | `#060606` | `#FFFFFF` | The look-through value at full open. Deeper than base in dark, brighter than base in light. Both directions read as an opening. |
| `atmoSeamClosed` | `#040404` | `#D6D6D6` | Mortar. The darkest value in the dark theme. |
| `atmoSeamOpen` | `#343434` | `#BEBEBE` | Lattice strut at full open. |
| `atmoNode` | `#454545` | `#A8A8A8` | 2 px node mark at open cell corners. Appears above `o = 0.55` and only where the dissolve is under 0.45. |
| `atmoLightCore` | `#FFFFFF` @ 9% | `#FFFFFF` @ 90% | Centre lift, radius 430, three stops. Full strength only when connected. |
| `atmoVignette` | `#000000` @ 40% | `#0A0A0A` @ 12% | Corner darkening, radius 600. Full strength only when the wall is closed. |
| `atmoGuard` | `#0A0A0A` @ 68% | `#FAFAFA` @ 70% | Text contrast floor. A radius-266 radial of the base colour centred 24 px below the dial, painted last, always on, never animated. |
| `atmoPulse` | `#FAFAFA` @ 26% | `#0A0A0A` @ 16% | Reach pulse luminance on struts. |
| `atmoTintConnecting` | `warning` `#FF9F0A` | `warning` `#A85D00` | Mixed into open struts at up to 16% × 1.5 weight. |
| `atmoTintConnected` | `success` `#30D158` | `success` `#1E9E54` | Mixed into open struts and nodes at up to 16%. |
| `atmoTintError` | `danger` `#FF453A` | `danger` `#C8102E` | Void outlines only, at 34% stroke alpha. Never on the wall. |

### Per-screen strength

| Token | Value | Purpose |
|---|---|---|
| `atmoStrengthHome` | 1.0 | Full. Motion enabled. |
| `atmoStrengthTab` | 0.35 | Servers, Profile, Settings. Depth planes off, motion always off, static composition for the current state. State stays glanceable in the gutters without competing with list rows. |
| `atmoStrengthAuth` | 0.45 | Splash, Login, Autotune. Disconnected composition, static. |

Sheets and dialogs sit above `overlayScrim`, so no rule is needed there.

---

## 5. Rendering approach

**CustomPainter, not a fragment shader.** Reasoning:

- The composition is line and rect geometry with per-cell parameters, which is exactly what Skia and Impeller batch well. Expressing a running-bond lattice as an SDF costs a full-screen fragment pass every frame for a picture that is 95% background.
- `FragmentProgram` has first-use shader compilation cost on some Android drivers, which would show up as a hitch on the first connect, the single most important moment in the app.
- Painter geometry is trivially inspectable and testable. A shader is not.

The two gradient layers *are* precomputed, which is where a shader would have earned its keep anyway.

### Layer split

Three siblings in a `Stack`, each its own `RepaintBoundary`, so a change in one never rasterises the others.

1. **`AtmosphereLight`** holds the precomputed centre-lift and vignette images (built once per theme and size at half resolution, drawn scaled with `FilterQuality.low`). Only their opacity animates, driven by `FadeTransition`, so this layer never re-rasterises: the compositor changes an opacity on an existing texture.
2. **`AtmosphereLattice`** is the `CustomPaint`. The only thing that ever repaints.
3. **`AtmosphereGuard`** is a static `DecoratedBox` with a `RadialGradient` of `atmoGuard`. Never animates, never repaints, painted above the lattice and below all content.

### What is precomputed

- The cell list (position, course parity, normalised radius, a stable per-cell hash), rebuilt only on size or orientation change. 468 cells at 390×844.
- The per-cell hash provides ~10% grain on openness so the wavefront edge is never a mathematically perfect ring.
- 33 pre-lerped panel fill colours per course, rebuilt on theme change.
- The two gradient images, rebuilt on theme or size change.

### Per-frame work

- Front plane: 468 rect fills as one `drawVertices` call, plus struts batched into 6 openness × 3 dissolve buckets and emitted as `drawRawPoints(PointMode.lines)`, one call per non-empty bucket.
- Two depth planes: struts only, no fills, culled by transformed bounds, same bucketing.
- Reach pulse when live: a second short pass over the ~60 cells inside the Gaussian band, 4 alpha buckets.
- Roughly 40 to 55 draw calls, about 4,400 line segments, one vertex buffer.

Estimated raster cost on a mid-range Android (Snapdragon 6-series class, 1080×2340) is 1.5 to 3 ms per animating frame against a 33 ms budget at 30 fps. The dominant real cost is not the draw, it is waking the GPU 30 times a second, which is exactly why the idle states genuinely idle. If a device profile misses the budget, the two dial-back levers in order are: drop the third depth plane, then halve the stroke buckets to 4 × 2.

### How it pauses

The ticker is created but not started. It runs only while `_needsFrames` is true, and that is true only when: a parameter tween is live, the state is `connecting` or `reconnecting`, or a reach pulse is inside its 1.6 s window. It is force-stopped when any of these hold:

- `MediaQuery.disableAnimations` is true (then it never starts at all)
- `AppLifecycleState` is `paused`, `inactive`, or `hidden`
- the Home route is not the top route, or the widget's `TickerMode` is disabled (Flutter already does this for off-screen tab pages)

Connected steady state schedules the next pulse with a `Timer`, not a ticker, so a protected phone sitting in a pocket runs a 4.8 s timer and paints nothing. Disconnected and error steady states hold no timer at all.

---

## 6. Reduce-motion fallback

`MediaQuery.disableAnimations` switches the whole system to a still-image model. No ticker is ever created, no timer is scheduled, the reach pulse does not exist.

Each state has a fixed static composition, chosen so the state is still readable from structure alone:

| State | openness | depth | light | vignette | wavefronts | voids |
|---|---|---|---|---|---|---|
| Disconnected | 0 | 0 | 0 | 1.0 | none | 0 |
| Connecting | 0.52 | 0.50 | 0.14 | 0.30 | frozen at 0.62 / 0.28 | 0 |
| Reconnecting | 0.62 | 0.70 | 0.22 | 0.30 | frozen at 0.50 / 0.16 | 0 |
| Connected | 1.0 | 1.0 | 1.0 | 0 | none | 0 |
| Error | 0 | 0 | 0 | 1.0 | none | 1 |

Freezing the connecting sweep at a mid radius matters: it leaves a visible open shell around the dial, so "mid-handshake" is still a distinct picture and not just "a wall with amber text on it".

Transitions between stills are a **200 ms opacity cross-fade between two painted canvases**. No translation, no scale, no shake. The error shake is dropped outright rather than substituted, since the voids appearing at 280 ms already carry the event. The demo implements this literally with two stacked canvases and a CSS opacity transition, so the fallback can be reviewed rather than promised.

---

## 7. Screen integration

- **Home** is the only screen at full strength with motion. The dial sits at the atmosphere's origin, so the wavefronts, the depth vanishing point, the centre light and the guard are all concentric with the control that causes them. Connect and the world opens from your thumb.
- **Servers, Profile, Settings, Market, Browser** get `atmoStrengthTab` 0.35, depth planes off, motion off, static composition for the current state. The atmosphere shows only in the 20 px gutters and above the first card. You can tell whether you are protected while scrolling a server list without reading anything, and list rows never fight a moving background. Cards are opaque `surface1`, so nothing behind them affects legibility.
- **Splash, Login, OTP, Autotune** get 0.45, disconnected composition, static. The first connect then becomes the first time the wall has ever opened, which is worth something.
- **Sheets and dialogs** need no rule: `overlayScrim` covers the atmosphere.
- **Desktop** keeps everything, with `atmoCellW` bumped to 56 and the origin placed at the dial inside the centred content column rather than at the window centre, so a wide window does not throw the vanishing point off to one side.

---

## 8. Accessibility and contrast

**The guard layer is the contrast contract.** `atmoGuard` is painted last, is never animated, and damps the atmosphere to at most a few percent lightness delta from `bgBase` across a radius-266 disc covering the dial, the state label and the sub-line, which is the only text in the app that sits directly on the atmosphere. Everything else sits on opaque `surface1`.

Measured worst cases (relative luminance, WCAG 2.1 contrast ratio):

| Case | Effective background | Text | Ratio |
|---|---|---|---|
| Dark, disconnected, wordmark on wall | `#151515` | `textHi` `#FAFAFA` | 16.8:1 |
| Dark, disconnected, sub-line on wall | `#151515` | `textMed` `#A0A0A0` | 6.9:1 |
| Dark, connected, sub-line over a lit opening | `#0F0F0F` | `textMed` `#A0A0A0` | 7.2:1 |
| Dark, connected, sub-line landing on a tinted strut | `#202A22` | `textMed` `#A0A0A0` | 5.6:1 |
| Light, disconnected, wordmark on wall | `#ECECEC` | `textHi` `#0A0A0A` | 15.1:1 |
| Light, connected, sub-line on a tinted strut | `#DFE5E1` | `textMed` `#585858` | 5.6:1 |

The floor across every state, both themes, is 5.6:1, above the 4.5:1 AA requirement for normal text, and the strut cases are 1 px lines that a glyph stroke only partially overlaps. The rule to hold in code review: **atmosphere luminance inside the guard radius stays within ±6 L\* of `bgBase`.**

Other notes:

- **Never colour alone.** State is legible from structure (closed wall vs open lattice), from the dial ring and glyph, and from the text label. A viewer with any form of colour blindness reads all three. The tint is confirmation of something already said twice.
- **Lightness ordering is preserved.** The three status families already differ in lightness and the atmosphere adds no new hue families.
- **Semantics.** The whole atmosphere `Stack` is wrapped in `ExcludeSemantics`. State changes are announced by the existing dial live region, unchanged.
- **Dynamic type.** The atmosphere is behind text and has no layout relationship to it. The guard radius is a fixed dp value sized for the largest supported text scale, not the default one.
- **Photosensitivity.** Nothing flashes. Peak temporal luminance change is the reach pulse at 26% alpha over 1.6 s, which is far under any flash threshold, and it happens at most once per 4.8 s.

---

## 9. Risks, and what would make it look cheap

**Density and moiré.** Too many cells and the wall becomes a circuit-board wallpaper that shimmers when scrolled past. Mitigation: cell width clamped to a floor of 38 dp, seams exactly one physical pixel, coordinates snapped to the physical pixel grid. If it shimmers on a real device, the cell gets bigger, never the seam thinner.

**The tunnel.** Depth planes scaling about a fixed point can slide into a sci-fi wormhole. Mitigation: three planes maximum, scale steps capped at 1.78, back planes at 34% and 12%, strut-only with no fills, and the vanishing point never moves or animates its position. If it ever reads as a flythrough, the third plane goes.

**Tint creep.** This is the one that would actually break the anti-slop rule. The first pass of this concept ran the mix at 30% with a floor applied to closed seams, and the connected state came out as neon green graph paper and the connecting state as an orange wall. The fix is structural, not a value tweak: the mix is multiplied by openness, so a closed wall is mathematically incapable of carrying a hue, and the cap is 16%. Anyone raising `atmoTintMax` should have to justify it against a screenshot.

**Connected looking busier than disconnected.** The failure mode where protection feels like more UI instead of less. This is why the radial dissolve exists: at full open the lattice concentrates in the middle and lets go of the corners, so connected has fewer marks on screen than closed masonry, not more. If a future change removes the dissolve, the concept inverts and stops working.

**The reach pulse turning into a scan line.** Make it faster, brighter or more frequent and it becomes a cliché. The guardrails are 3.2 s minimum gap, 1.6 s travel, 26% peak alpha, brightest mid-travel, dead before the frame edge, and it rides the existing struts rather than drawing a shape of its own.

**Light theme going graph paper.** In light, panel fill does the work and the seam is a low-contrast hairline. If someone "improves" light theme by darkening the seams to match dark theme's read, it becomes engineering paper. The wall in light is a slab, defined by its fill, not its lines.

**The guard reading as a smudge.** A radial of the base colour behind the dial can look like a dirty lens if it is too tight or too strong. Current settings (radius 266, four stops, peak 68%) keep it broad and gradual. It is worth checking on an OLED at low brightness, where near-black banding is most visible; the fix if it bands is to add a 1% dither, not to reduce the guard, because the guard is the contrast contract.

**The error voids reading as UI.** Six dark rectangles with red outlines could look like broken image placeholders. They work because they are cell-aligned, clustered near the dial, and carry a lit lip on the bottom and right edge. If they were randomly placed or unaligned to the bond, they would look like a bug.

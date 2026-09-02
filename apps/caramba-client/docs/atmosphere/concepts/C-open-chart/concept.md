# Concept C: Open Chart

**One line:** the background is a survey chart of the network the client can reach, and the connect dial is the chart's home station. Off, the chart is closed: a boundary is drawn around you and every route out is capped at a dead end. On, the chart is open: the boundary is gone, the graticule is visible, and every route runs unbroken to its station.

---

## 1. The metaphor

Not a constellation, not a globe, not floating nodes. A **chart**: the flat, engineered cartography of a nautical plotting sheet or a transit diagram. Three properties of real charts do the work here:

1. **Charts have a graticule.** A faint measured grid means the space is surveyed and known. A chart with no graticule is uncharted water. The grid appearing is the single strongest "you can see now" signal available, and it costs almost nothing to draw.
2. **Charts mark restricted areas.** Hatching inside a boundary is the universal cartographic mark for "you may not go here." That is a real convention, not an invented one, and users read it without being taught.
3. **Routes are drawn, not implied.** A plotted course is a polyline with corners, waypoints, and a terminator. It is engineered geometry, which is why it reads as infrastructure instead of decoration.

The inversion is the whole idea: normally hatching marks the small forbidden patch inside a large open sea. Here the hatching is **everything outside a small boundary drawn around you**. Disconnected, you are the restricted area. The world is the part you are shut out of.

The routing style is deliberately orthogonal with 45 degree elbows, the way a metro diagram or a PCB trace is routed. Straight radial spokes from a center point read as a sunburst and look generic. Elbowed routes read as something a person laid out.

### The load-bearing decision

The dial is not a control floating on a background. The dial **is** the home station of the chart. Every route leaves the dial's ring at a tangent point, and the boundary is an irregular octagon drawn around the dial, not a circle (a circle would read as a second status ring and fight the dial). When the app is disconnected, the user's eye reads a hub inside an enclosure. That is the emotional payload, and it is delivered by composition, not by color.

---

## 2. Per-state description

Geometry is fixed across all states. Nothing moves position, ever. What changes is which parts of the chart are drawn, how bright they are, and whether the boundary is closed. That constraint is what keeps the whole thing calm.

### Disconnected

**Composition.** The boundary octagon is closed and solid around the dial, enclosing the dial, the state label, and the sub-line. Outside it, the field is hatched at 45 degrees, 9px pitch, and darkened by a soft radial shade that deepens toward the frame edges. Eight routes leave the dial ring and are drawn in a flat grey up to the boundary, where each one ends in a **dead-end cap**: a short bar perpendicular to the route. Past the boundary the same routes continue as a 2/4 dotted ghost at roughly half the alpha, ending at hollow square station marks. No graticule.

**Motion.** None. Zero frames. The ticker is stopped and one cached picture is composited.

**Light.** The lowest-lightness state in the set. In dark theme the field sits at `bgBase` with no lift. In light theme the shade pulls the outside of the boundary down to about 94 percent lightness, so the enclosed area is the only clean paper on the screen.

**What the user reads at a glance.** I am boxed in. The lines out of me stop at a wall. The rest of the world is drawn but greyed and dotted, so it clearly exists and clearly is not reachable.

### Connecting

**Composition.** The boundary breaks. At each of the eight points where a route crosses it, a gap opens and widens to 26px, so the boundary becomes eight short arcs floating apart rather than a wall. The dead-end caps rotate out and fade. The hatch drops to about a third. The graticule fades in at roughly half strength. Routes fill outward from the boundary in a warm desaturated amber.

**Motion.** A **probe cycle**, not a spinner. Each route extends to about 78 percent of its length, holds briefly, retracts to about 15 percent, and goes again, on a 2.2s loop with a 110ms stagger between routes. Routes are therefore never all at the same length, which reads as parallel handshakes in flight rather than a progress bar. Nothing rotates. This is the only state that animates continuously, and it is bounded by how long a real connect takes (1 to 3 seconds).

**Light.** Mid. The shade is at 45 percent, the graticule is in, so the field is measurably brighter than disconnected without being at the connected level yet.

**What the user reads.** The wall is coming apart and the lines are trying to get through.

### Connected

**Composition.** No boundary, no hatch, no shade. The graticule is at full strength with major lines every fourth cell, one of which passes exactly through the dial center (the chart is registered to you). All eight routes run unbroken from the dial ring to their stations in a desaturated green hairline. Station marks are filled and each carries a hairline ring. Five of them carry a mono two letter country code at 20 to 26 percent alpha: NL, DE, US, JP, SE. Corner turns that fall outside the old boundary are drawn as small hollow circles, which are the relay hops. Two routes run down the left and right gutters and off the bottom edge with an edge tie mark, and one runs off the top: the chart is a window on something larger.

A soft elliptical **quiet lens** sits under the state label and sub-line, painting the base plane back over the graticule and over the two routes that would otherwise pass behind those words (section 4).

**Motion.** None, after arrival. Connected is a **still frame**. The ticker stops. The only thing moving on Home is the session timer text. This is deliberate and it is the concept's main battery argument.

**Light.** The brightest state. In dark theme the entire field is lifted by 2.5 percent white, so `bgBase` composites to roughly `#101010` and the app visibly opens up. In light theme the shade is simply removed and the paper returns to `#FAFAFA`.

**What the user reads.** One coherent, surveyed, fully connected map. Everything is reachable and nothing is agitating.

### Error

**Composition.** The boundary slams shut, the hatch and shade return, and the routes retract to their caps. One route, the one that was furthest along when it failed, keeps its ghost and gets a **fault cross**: a 9px X in desaturated red exactly at its boundary crossing. Everything else is the disconnected composition.

**Motion.** The retract runs at 420ms, roughly half the normal speed, so the closing reads as abrupt. The chart layer shakes once horizontally, 5px damped to 0 over 260ms, in the same gesture as the dial's error shake in the existing spec. Then static.

**What the user reads.** It closed, and here is the specific route that died.

### Reconnecting

**Composition.** Distinct from connecting on purpose, because the user has already been connected and should not be shown a cold start. The boundary comes back but only to 55 percent gap closure, so it stays visibly **ajar**. The hatch returns at half. The graticule stays at 40 percent rather than dropping to zero. Routes probe amber as in connecting, but with 60 percent amplitude and a slower 3.0s loop.

**What the user reads.** The chart is not closed, it is being re-drawn. Paired with the kill-switch banner from the existing spec, this says "paused, not lost."

---

## 3. Transition choreography

All durations are the atmosphere layer's own. They are longer than the dial's own transitions on purpose: the dial reports the state change immediately, the world catches up behind it. That lag of about 150ms is what makes the background feel like a consequence rather than a decoration.

### Disconnected to Connecting (total 620ms)

| t | What happens | Curve |
|---|---|---|
| 0 | Dial ring goes amber (existing 350ms morph), haptic fires | existing |
| 0 to 260ms | Boundary splits at the eight crossings, gaps widen 0 to 26px | `easeOutCubic` |
| 60 to 300ms | Dead end caps rotate 90 degrees about their route and fade to 0 | `easeInOutCubic` |
| 140 to 560ms | Hatch 1.0 to 0.35, shade 1.0 to 0.45 | `easeInOutCubic` |
| 300 to 620ms | Graticule 0.18 to 0.55 | `easeOutCubic` |
| 180ms onward | Probe loop starts, routes staggered 110ms apart | linear phase, `easeInOutSine` per probe |

The gaps opening **before** the routes move matters. The wall gives way first, then traffic goes through. Reversing that order makes it look like the routes broke the wall, which is the wrong story for a VPN that negotiates rather than forces.

### Connecting to Connected (total 1200ms)

| t | What happens | Curve |
|---|---|---|
| 0 to 240ms | Probe loop unwinds: every route eases from wherever it is to a common 0.35 | `easeOutCubic` |
| 200 to 1100ms | Routes extend to full length, staggered 70ms in **inside-out** order (nearest station first) | `easeOutCubic` |
| 200 to 900ms | Route tint crosses from probe amber to live green, per route, following its own extension | `linear` in premultiplied RGBA |
| Per route, at its own 0.92 | Station mark ignites: hollow to filled plus hairline ring, 180ms | `easeOutCubic` |
| 300 to 800ms | Boundary arcs fade to 0, hatch to 0, shade to 0 | `easeInOutCubic` |
| 400 to 1000ms | Graticule 0.55 to 1.0, field lift 0 to 2.5 percent (dark only) | `easeOutCubic` |
| 900 to 1200ms | Country code labels fade in, all together | `easeOutCubic` |
| 1200ms | Ticker stops. Static frame. | |

The staggered ignition is the moment the concept earns its keep: the map does not switch on, it **completes**, station by station, over about 900ms. It reads as reachability propagating.

### Connected to Disconnected (total 700ms)

Reverse, but **outside-in** and faster: the furthest station goes dark first, routes retract toward the dial, the boundary re-forms from the eight arcs sliding closed, hatch and shade return. Labels drop first at 0 to 200ms. The asymmetry (1200ms to open, 700ms to close) is intentional: opening a world should take longer than losing it.

### To Error (total 420ms plus 260ms shake)

Same as connected to disconnected but compressed to 420ms with `easeInCubic` (accelerating, so it reads as a slam), the fault cross draws in over the last 120ms, and the horizontal shake runs 0 to 260ms.

### Connected to Reconnecting (total 480ms)

Boundary arcs return to 55 percent closure, hatch to 0.5, graticule holds at 0.4, routes drop to the slow probe. No shake, because reconnecting is not a failure.

---

## 4. Atmosphere tokens

These layer **under** the existing `AppColors`. They never replace a status color, and no screen component is allowed to reference them. Everything is an alpha over the base plane so the composite is deterministic and the contrast math holds.

| Token | Dark | Light | Purpose |
|---|---|---|---|
| `atmoGrid` | `#FAFAFA` @ 3.0% | `#0A0A0A` @ 4.5% | Graticule minor line, 1px. |
| `atmoGridMajor` | `#FAFAFA` @ 5.2% | `#0A0A0A` @ 7.5% | Every fourth graticule line, 1px. |
| `atmoHatch` | `#FAFAFA` @ 2.6% | `#0A0A0A` @ 3.8% | 45 degree restricted hatch, 1px at 9px pitch. |
| `atmoShade` | `#000000` @ 45% | `#0A0A0A` @ 5.5% | Radial darkening outside the boundary. Carries "unlit" in light theme, edge falloff in dark. |
| `atmoLift` | `#FAFAFA` @ 2.5% | none (0%) | Full field lift on connected. In a dark theme, light means lift. |
| `atmoRouteIdle` | `#FAFAFA` @ 10% | `#0A0A0A` @ 14% | Route inside the boundary, disconnected. 1.5px. |
| `atmoRouteGhost` | `#FAFAFA` @ 4.5% | `#0A0A0A` @ 6% | Route beyond the boundary, dotted 2/4. |
| `atmoRouteProbe` | `#FF9F0A` @ 24% | `#A85D00` @ 30% | Route while connecting or reconnecting. Derived from `warning`. |
| `atmoRouteLive` | `#30D158` @ 22% | `#1E9E54` @ 32% | Route when connected. Derived from `success`. |
| `atmoNodeIdle` | `#FAFAFA` @ 16% | `#0A0A0A` @ 22% | Hollow 5px station mark. |
| `atmoNodeLive` | `#30D158` @ 50% | `#1E9E54` @ 62% | Filled 5px station mark plus hairline ring. |
| `atmoBarrier` | `#FAFAFA` @ 14% | `#0A0A0A` @ 20% | Boundary stroke, 1.2px. |
| `atmoFault` | `#FF453A` @ 55% | `#C8102E` @ 60% | The single dead route cross on error. |
| `atmoLabel` | `#FAFAFA` @ 20% | `#0A0A0A` @ 26% | Mono country codes on stations, connected only. |
| `atmoQuietLens` | `#0A0A0A` @ 90% | `#FAFAFA` @ 90% | The base plane painted back over the atmosphere behind the status label. See below. |

Three rules that keep this from drifting:

1. **Alpha ceiling.** No single atmosphere element exceeds 8 percent alpha as a *field* (grid, hatch, shade, lift). Point marks and hairlines (stations, fault, routes) may go higher because they cover a negligible pixel area. Measured on the built prototype, the maximum composite deviation of any large region from `bgBase` is 7.1 percent in dark (connected, where the lift is also in play) and 7.1 percent in light (disconnected, at a graticule crossing inside the hatch field).
2. **Only three hues exist.** Probe is `warning`, live is `success`, fault is `danger`. There is no fourth family, and no gradient between hues anywhere. The routes are flat hairlines at one alpha along their whole length.
3. **No glow.** Nothing is blurred, nothing has a shadow, nothing has a halo. This is a line drawing.

Structural tokens (non color):

`atmoBarrierHw 134` · `atmoBarrierHh 150` · `atmoBarrierCut 36` · `atmoGridPitch 44` · `atmoGridMajorEvery 4` · `atmoHatchPitch 9` · `atmoRingOffset 6` (gap between the dial edge and where routes begin) · `atmoGapWidth 26` · `atmoStrengthHome 1.0` · `atmoStrengthServers 0.45` · `atmoStrengthSettings 0.30` · `atmoLensRx 176` · `atmoLensRy 46`.

### The quiet lens

The one place where atmosphere and text genuinely collide is the connect state label, because that label is the only text on the screen painted in a status color, and status greens and ambers have far less contrast headroom than the neutral text does. So the layer paints the base plane back over itself in a soft ellipse centred under the label (`atmoLensRx` x `atmoLensRy`, 90 percent at the centre falling to 0 at the edge). Inside it the graticule and any route passing behind the words are suppressed to within two or three levels of pure `bgBase`, while the field lift is preserved so no dark patch appears in dark theme.

It costs one gradient fill. It is invisible: it is painted in the background color. And it reads, if anything, as a legend clearing on a chart, which is the right accident to have.

---

## 5. Rendering approach

**Pure `CustomPainter`. No `FragmentProgram`, no Rive.** The composition is a line drawing of roughly 60 stroke operations. A shader would add pipeline warm-up cost on Impeller, an extra asset, a platform matrix to test, and a fallback path, and it would buy nothing: there is no noise field, no blur, no per-pixel math here. Choosing the boring renderer is a deliberate part of the concept.

### What is precomputed

A `ChartGeometry` value object built once per `(Size, TextDirection)` and cached in an `InheritedWidget`:

- The dial-ring attachment point of each of the eight routes.
- Each route as `List<Offset>` plus a cumulative-length table, so any partial length maps to a point with one binary search and no `PathMetric` allocation.
- The boundary polygon, its cumulative-length table, and the arclength position of each of the eight route crossings (so gaps are just ranges to skip while stroking).
- Each route's crossing arclength, crossing point, and the unit normal there (used for the dead end cap and the fault cross).
- Which corner vertices fall outside the boundary (those become relay waypoint circles).
- The graticule line list and the hatch line list.

Everything above is integer-ish geometry that never changes at runtime.

### What is cached as a raster

- **Hatch field.** A 64x64 `ui.Image` tile containing the 45 degree hatch at device pixel ratio, painted once, then filled through `ImageShader(TileMode.repeated)` clipped to the region outside the boundary. One fill op instead of about 140 `drawLine` calls.
- **The two idle compositions.** `disconnected` and `connected` are each recorded once into a `ui.Picture` via `PictureRecorder` and replayed with `canvas.drawPicture`. Rebuilt only on size or theme change. In those states the painter does exactly one draw call.

### Per-frame cost during a transition

Roughly: 1 background fill, 1 lift fill, 28 graticule `drawLine`, 1 clip plus 1 gradient fill plus 1 shader fill for shade and hatch, 8 dotted ghost strokes, 8 partial-length strokes, about 10 boundary sub-range strokes, 8 caps, about 13 station and waypoint marks. Around 80 ops, all stroke or fill, no saveLayer, no blur, no clip beyond one even-odd clip.

**Expected cost on a mid range phone** (Snapdragon 6-series class, 1080x2400): raster in the low single-digit milliseconds per frame, well inside a 33ms budget at 30fps. This is an estimate to be confirmed with `flutter run --profile` and the timeline, watching the raster thread specifically; the acceptance gate is raster under 6ms at p95 during the connect transition. If it misses, the first lever is baking the graticule into the same cached tile as the hatch.

### Frame gating

- The atmosphere runs off a single `Ticker` in a `SingleTickerProviderStateMixin` state object that owns the whole layer.
- **30fps cap:** the ticker fires at display rate; the painter's repaint notifier is only bumped when `elapsed - lastFrame >= Duration(milliseconds: 33)`. On a 120Hz panel this drops three of every four frames.
- **The ticker is stopped, not just ignored, whenever the composition is static:** disconnected settled, connected settled, error settled, and any state under reduce-motion after its 200ms cross-fade. Connecting and reconnecting are the only states that hold the ticker, and they are bounded by the real connect timeout.
- **Backgrounding:** an `AppLifecycleListener` stops the ticker on `inactive` and `paused`. On `resumed`, if the state is still connecting, the probe phase is recomputed from `DateTime.now()` against the stored connect start, so the animation resumes where the real handshake actually is instead of rewinding.

### Reduce-motion fallback

When `MediaQuery.disableAnimations` is true (or the platform reduce-motion flag is set), the layer switches to three static compositions and opacity cross-fades between them:

- **Closed chart** (disconnected and error, error adding the fault cross and no shake).
- **Half open chart** (connecting and reconnecting): boundary gaps frozen at their state's width, every route frozen at the mid point of its probe range (a deterministic value, not wherever the loop happened to be), amber. Nothing loops. The dial's own indeterminate arc is frozen too, so the whole screen is genuinely still.
- **Open chart** (connected).

The cross-fade is a 200ms `AnimatedSwitcher`-equivalent on opacity only, implemented as two `drawPicture` calls with an alpha ramp. No geometry animates, no length changes, no stagger. The ticker stops as soon as the fade lands. This is a genuinely different rendering path, not the same animation slowed down, which is the point.

---

## 6. Screen integration

The atmosphere is one widget mounted **once**, above the `Scaffold` background and below the navigator, so it does not rebuild or restart when the user changes tabs. Its strength is read from an `AtmosphereScope` that the current route sets.

| Surface | Strength | Behavior |
|---|---|---|
| **Home** | 1.0 | Full composition. The dial sits exactly on the home station. The boundary encloses the dial, the state label, and the sub-line. |
| **Servers** | 0.45 | Same geometry, translated up 180px so the routes read as vertical rails flanking the list. All list rows are opaque `surface1`, so the atmosphere is only ever seen in the 22px gutters and above the first row. The station whose code matches the selected server is drawn live even here, so the map stays true. |
| **Settings, Profile** | 0.30 | Same treatment as Servers. It should register as texture, not as a diagram. |
| **Sheets and modals** | 0.0 | The `overlayScrim` covers it anyway. The layer is told to stop the ticker while a sheet is open. |
| **Splash, Login, Autotune** | 0.0 | These screens have no connection state to report. Showing a closed chart during login would imply a failure that has not happened. |

Two routes deliberately run down the left and right gutters (x=14 and x=376) and off the bottom edge. On Home they pass behind the config card and the stats card and are visible only as short hairline rails in the 22px gutters. That is what makes the atmosphere feel like it is under the whole app rather than parked behind the dial.

---

## 7. Accessibility and contrast

**Structural safety.** Every content surface on Home is an opaque fill: cards and rows are `surface1`, stats are `surface1` over a `borderSubtle` grid, the nav is `canvas` at 92 percent over a blur. Text inside those is unaffected by the atmosphere by construction. Only four strings sit on bare background: the wordmark, the plan chip, the state label (`cstate`), and the sub-line (`csub`).

**Measured composites.** These are sampled from the built prototype, not estimated: the numbers below come from reading the rendered canvas pixels in a clean strip of atmosphere (no text, no card) and computing WCAG ratios against them.

| Theme / state | Atmosphere field range | Text | Contrast | Result |
|---|---|---|---|---|
| Dark, disconnected | `#090909` to `#111111` | `textHi #FAFAFA` | 18.1:1 | AAA |
| Dark, disconnected | `#090909` to `#111111` | `textMed #A0A0A0` | 7.2:1 | AAA |
| Dark, connected | `#0F0F0F` to `#1B1B1B` | `textHi #FAFAFA` | 16.5:1 | AAA |
| Dark, connected | `#0F0F0F` to `#1B1B1B` | `textMed #A0A0A0` | 6.6:1 | AAA |
| Light, disconnected | `#E9E9E9` to `#F9F9F9` | `textHi #0A0A0A` | 16.3:1 | AAA |
| Light, disconnected | `#E9E9E9` to `#F9F9F9` | `textMed #585858` | 5.9:1 | AA |
| Dark, connected, under the lens | `#0F0F0F` | `success #30D158` | 8.5:1 | AAA |
| Light, connected, under the lens | `#F6F6F6` to `#FAFAFA` | `success #1E9E54` | 3.2:1 | AA large text |

The alpha ceiling in section 4 is what produces the four neutral-text rows, and the quiet lens is what produces the two status-text rows. Any change to an atmosphere alpha has to be re-measured against this table.

**One honest flag on the light theme.** `success #1E9E54` scores 3.31:1 on bare `#FAFAFA`, so it was already a large-text-only color before this concept existed; the connect label is 20px at weight 600, which clears the large-text bar, and the quiet lens keeps the atmosphere from eroding it (3.20:1 with the lens versus 2.72:1 without). The atmosphere therefore costs 0.11 of a ratio point rather than pushing a passing color into failure. Separately, and outside this concept's scope, light `success` should be deepened toward `#177A41` if the design system ever wants that label to pass AA at normal text size.

**Color independence.** Every state is distinguishable with all color removed, because the structure differs: closed boundary plus hatch plus caps (disconnected), broken boundary plus partial routes (connecting), no boundary plus graticule plus full routes (connected), closed boundary plus an X mark (error), half-closed boundary (reconnecting). A user who cannot separate the amber and green hairlines still sees a wall or no wall. This is the reason lightness and structure carry the meaning and hue only confirms it.

**Semantics.** The entire layer is wrapped in `ExcludeSemantics`. It is never the only carrier of information: the dial ring, the glyph, and the state label already report state, and the screen reader announcement is unchanged.

**Motion sensitivity.** Covered by the reduce-motion path above. There is additionally no motion at all in the two states the user spends 99 percent of their time in.

**Dynamic type.** The atmosphere geometry is anchored to the dial center and to the frame, not to text metrics, so it does not shift when text scales. At very large text scales the state label can grow past the boundary bottom edge, which is acceptable: the text stays on an opaque-enough plane and the contrast table above still holds.

---

## 8. Risks, and what would make this look cheap

1. **Drifting into "network constellation."** If the station marks become circles, the routes become straight radial spokes, and anything gets a glow, this collapses into the generic tech-background that every VPN landing page already has. The defenses are structural and must not be negotiated away: square station marks, elbowed orthogonal routing, zero blur, zero shadow, and no element that both moves and glows.
2. **Blueprint or hacker cosplay.** Push the alphas up by a factor of two and this becomes a wireframe grid over a terminal, which is a different and much cheaper product. The alpha ceiling is the guard. If a stakeholder says "I can barely see it," that is the design working.
3. **Hatch moire.** A 9px 45 degree hatch on a 2.75x screen can alias into visible banding when the layer is translated (Servers). Mitigations: pitch never below 8 logical px, the tile is rasterized at device pixel ratio, translation is snapped to whole device pixels, and the hatch is never animated (only cross-faded in alpha).
4. **The labels turning into clutter.** Five 9px mono codes at 20 percent alpha are a nice payoff. Ten would be noise, and putting them in any state other than connected would be noise in the state where the user is already anxious. Hard cap: five, connected only.
5. **The probe loop reading as a progress bar that lies.** Routes retracting could suggest failure to a user watching closely. It is mitigated by the stagger (they are never in sync, so it reads as several parallel attempts, not one failing attempt) and by the fact that connecting rarely lasts longer than one full cycle. If field feedback says otherwise, the fallback is monotone extension without retraction.
6. **Cost on old Android.** If profiling shows the transition missing budget on the floor device, the graduated fallbacks in order are: bake the graticule into the hatch tile, drop the transition to 20fps, then drop straight to the reduce-motion cross-fade path on devices below a measured threshold. The reduce-motion path already exists and is already designed, so the low-end fallback is not extra work.
7. **Layout coupling.** The boundary is sized to enclose the dial and both status lines. If Home's vertical rhythm changes, the boundary has to be re-tuned or it will crop the sub-line. Fix: derive `atmoBarrierHh` from the measured height of the connect block rather than hard-coding it, once the Flutter layout is final.

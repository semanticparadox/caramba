# Atmosphere layer, Caramba Connect: judge verdict

Panel: a product designer (PD), a Flutter engineer who owns the frame budget (FE), and the
product owner who wrote the brief (PO). Each judge scored all three concepts 1 to 10 on the
nine criteria, independently, after reading both documents per concept and all 24 screenshots.

**Winner: Concept C, Open Chart.** It wins on every judge's card and it wins on the criterion
the brief cares about most, which is reading the state without reading a word.

---

## 1. Scores

### Judge 1: product designer

| # | Criterion | A Masonry | B Carrier | C Open Chart |
|---|---|---|---|---|
| 1 | State legible at a glance | 6 | 6 | 9 |
| 2 | Emotional quality | 7 | 7 | 8 |
| 3 | Craft, taste, anti-slop | 6 | 6 | 8 |
| 4 | Text contrast over the layer | 7 | 7 | 8 |
| 5 | Battery plan credibility | 8 | 7 | 9 |
| 6 | Transition choreography | 9 | 8 | 8 |
| 7 | Flutter implementability | 7 | 7 | 9 |
| 8 | Light theme quality | 5 | 4 | 8 |
| 9 | Fit with the app and the dial | 7 | 6 | 9 |
| | **Total (max 90)** | **62** | **58** | **76** |

### Judge 2: Flutter engineer

| # | Criterion | A Masonry | B Carrier | C Open Chart |
|---|---|---|---|---|
| 1 | State legible at a glance | 6 | 5 | 8 |
| 2 | Emotional quality | 6 | 6 | 7 |
| 3 | Craft, taste, anti-slop | 7 | 6 | 8 |
| 4 | Text contrast over the layer | 7 | 8 | 8 |
| 5 | Battery plan credibility | 9 | 8 | 9 |
| 6 | Transition choreography | 8 | 8 | 8 |
| 7 | Flutter implementability | 8 | 7 | 9 |
| 8 | Light theme quality | 5 | 4 | 8 |
| 9 | Fit with the app and the dial | 7 | 7 | 9 |
| | **Total (max 90)** | **63** | **59** | **74** |

### Judge 3: product owner

| # | Criterion | A Masonry | B Carrier | C Open Chart |
|---|---|---|---|---|
| 1 | State legible at a glance | 6 | 6 | 9 |
| 2 | Emotional quality | 7 | 7 | 8 |
| 3 | Craft, taste, anti-slop | 6 | 6 | 8 |
| 4 | Text contrast over the layer | 8 | 7 | 8 |
| 5 | Battery plan credibility | 9 | 7 | 9 |
| 6 | Transition choreography | 9 | 8 | 9 |
| 7 | Flutter implementability | 8 | 7 | 9 |
| 8 | Light theme quality | 5 | 4 | 8 |
| 9 | Fit with the app and the dial | 7 | 6 | 9 |
| | **Total (max 90)** | **65** | **58** | **77** |

### Aggregate

| Concept | PD | FE | PO | Total (max 270) |
|---|---|---|---|---|
| A Masonry | 62 | 63 | 65 | **190** |
| B Carrier | 58 | 59 | 58 | **175** |
| C Open Chart | 76 | 74 | 77 | **227** |

---

## 2. Critique

### Concept A, Masonry

The idea is the best idea in the set on paper. "The wall and the lattice are the same geometry
at two values of one parameter" is a real design thesis, and the centrifugal-versus-centripetal
rule for connect and disconnect is the sharpest single sentence any of the three wrote. The
engineering is first rate: the `revision` counter for `shouldRepaint` over in-place mutated
params, the `Timer`-not-`Ticker` connected steady state, and naming the
`ticker.isTicking == false` test as "the battery contract" are the marks of someone who has
shipped this before. The render then fails to deliver the thesis. In `A-connected.png` the
lattice is not a lattice, it is a field of small green crosshair marks scattered edge to edge
including behind the status bar, and it is visibly busier than `A-disconnected.png`, which is
the exact inversion the concept says would break it. In `A-connecting.png` the atmosphere is
indistinguishable from disconnected in a still frame, so the entire connecting read depends on
motion, which the reduce-motion path cannot supply. The error state is the worst of it:
`A-error-light.png` renders five white rectangles with red outlines, one directly behind the
"Caramba Connect" wordmark row, one behind the sub-line, one clipping the top card, and they
read exactly like broken image placeholders rather than missing bricks. Light disconnected
(`A-disconnected-light.png`) is legible but reads as subway tile, which is literal and a
little dated for a product opened daily.

### Concept B, Carrier

The metaphor is the most emotionally ambitious of the three and it half works. "The wall
becomes a horizon" is a good line, and in `B-connected.png` the hairline running edge to edge
through the dial centre is the single most affecting mark any concept produced. The engineering
notes are the most operationally paranoid: the per-state `atmoFps` token table, the documented
`_lastEmit` first-frame bug, the debug frame-rate assertion, and the type-band invariant as an
asserted widget test are all things the other two should copy. Three things sink it. First, the
state casts render as full-bleed colored haze, not as light through an opening:
`B-connecting.png` has an amber glow filling the top half of the frame and `B-error.png` has a
red one, which is a colored field, and DESIGN.md section 1.3 plus the LILA rule exist
specifically to prevent that. Their own risk 2 predicted it and the render shows it happening
at the capped alpha. Second, the light theme collapses. In `B-disconnected-light.png` the
eight-stratum deck is essentially invisible on white, so the primary signal, deck height, is
gone; in `B-error-light.png` all that survives is a faint pink smear and a stray angular tick
under the sub-line that reads as a rendering glitch. Third, disconnected idles at 8fps and
connected at 6fps, so the two states a user lives in never reach zero, which is a real battery
cost the other two do not pay.

### Concept C, Open Chart

The inversion is the strongest concept move on the panel: normally hatching marks the small
forbidden patch inside an open sea, and here the hatch is everything outside a boundary drawn
around you, so you are the restricted area. `C-disconnected.png` delivers it without a word:
an octagon enclosing the dial and both status lines, hatch outside, eight routes stopping at
perpendicular dead-end caps, and the world beyond continuing as dotted ghosts to hollow square
stations, so it is visibly there and visibly unreachable. `C-connected.png` is the only
connected state on the panel that reads as reach rather than as texture, with routes running
unbroken to labeled NL, DE, SE, US, JP stations that reuse the mono two-letter convention
already in the server list. `C-connecting.png` is the best connecting frame in the set, because
the routes sit at visibly different lengths and read as several parallel handshakes rather than
a progress bar. `C-error-light.png` shows the restraint: one 9px red X at the boundary crossing
of the route that died, and nothing else changes. Two honest weaknesses. The connected state is
the busiest composition in the set, so "calm" is delivered less well than "open", and the
country codes plus graticule plus relay circles push it closest of the three to the network
graph cliche the brief warns about. And it self-reports the one contrast failure on the panel:
light `success #1E9E54` at 3.2:1, passing only because the state label is 20px at weight 600.

---

## 3. Shared weaknesses

These are not tiebreakers, they are notes for whoever implements the winner.

1. **Every light theme is a re-tint, not a redesign.** All three concepts define light by
   inverting alphas and hoping. B loses its primary signal entirely
   (`B-disconnected-light.png`), A loses most of its connected read
   (`A-connected-light.png`), and C is the least bad rather than actually good. Light needs a
   pass of its own, with its own alpha values chosen against white rather than derived from
   the dark ones.

2. **All three make connected busier than disconnected.** The brief asks for calm and relief
   on connect, and all three deliver more ink instead. A explicitly claims the opposite and
   the render contradicts it. C is the most honest about it. Whoever ships needs to count
   marks on screen per state and enforce that connected is not the maximum.

3. **Nobody measured anything on a device.** The millisecond and fps figures in all three
   documents are estimates written in the register of measurements. C is the only one that
   names a pass criterion (raster p95 under 6ms during the transition, zero raster work in the
   settled states). Treat every other number as a hypothesis.

4. **Nobody thought about the OS status bar.** The atmosphere runs to the top of the frame in
   all three. `A-connected.png` has green marks behind "9:41" and "5G 100%", `B-disconnected.png`
   puts the darkest strata exactly where the status bar text sits, and `C-connected.png` runs
   the graticule up behind it. The system draws that text and we do not control its color.

---

## 4. Conditions for implementation, Concept C

### Must fix before it ships

1. **Deepen light `success`.** C flags `#1E9E54` at 3.2:1 on `#FAFAFA` and passes only on the
   large-text exemption. Change light `success` to `#177A41` in DESIGN.md (C proposes this
   itself, outside its own scope) so the state label passes AA at normal size and does not
   become a failure the first time anyone changes the label to 17px.

2. **Reduce the connected mark count.** In `C-connected.png` the graticule, eight routes, five
   country codes, the relay circles and the edge ties together out-ink every other state. Cut
   the country codes from five to three, or draw only the major graticule lines, then re-check
   that connected reads calmer than connecting. Their hard cap of five codes is the ceiling,
   not the target.

3. **Derive the boundary height from layout, not a constant.** `atmoBarrierHh 150` is
   hard-coded and the boundary must enclose the dial, the state label and the sub-line. C names
   this as its own risk 7. Measure the connect block's laid-out height and derive the octagon
   from it, then add a golden at 100%, 150% and 200% text scale showing the sub-line still
   inside.

4. **Prove the quiet lens against the routes.** In `C-connected.png` a vertical route runs
   directly under "Защищено" toward the config card. The lens is specified to suppress it. Add
   the widget test that samples the rendered pixels under the label rect and asserts the
   composite stays within the section 7 contrast table at every text scale.

5. **Pixel-snap the Servers translation.** C moves the whole geometry up 180px at strength
   0.45 and the 9px 45-degree hatch will alias behind scrolling rows on a 2.75x panel. Snap the
   translation to whole device pixels as specified, rasterize the tile at dpr, and test it
   against an actually scrolling list, not a static frame.

6. **Handle the status bar.** Suppress atmosphere line work in the top safe-area inset, or
   guarantee the composite there stays within a few percent of `bgBase`, the way the quiet
   lens does under the label.

### Worth borrowing from the losers

7. **From B: the type-band invariant as an asserted test.** Paint each state to a `ui.Image`
   and assert that no pixel in the protected bands differs from the base plane by more than
   1/255. C has a measured contrast table but nothing that stops a future edit from breaking
   it. This is the single most valuable thing on either losing card.

8. **From B: read platform low-power mode.** Android `PowerManager.isPowerSaveMode`, iOS
   `ProcessInfo.processInfo.isLowPowerModeEnabled`, and drop the transition cap from 30 to 20fps
   when either is on. C only handles reduce-motion.

9. **From B: the first-frame fix and the debug frame counter.** Reset the last-emit timestamp
   on every state change so the first frame after a transition always paints, and log a warning
   in debug when the layer paints more than target fps plus two. C's 33ms gate has the same
   latent first-frame bug B documented.

10. **From A: the battery contract as a test.** Assert `ticker.isTicking == false` after
    settling into disconnected, connected and error, and after `AppLifecycleState.paused` in
    every state including connecting. C stops the ticker correctly in prose; make it a test.

11. **From A: the closing asymmetry, stated sharply.** C already opens inside-out and closes
    outside-in. Adopt A's articulation for the boundary specifically: the last segment to close
    is the crossing nearest the dial, so disconnect visibly draws back to your thumb.

12. **From A: a touch of grain in the field elements.** C's routes are deterministic geometry
    and should stay that way, but a sub-pixel irregularity in the hatch and graticule stops the
    chart looking machine-generated. Optional, and only if it survives the moire test in
    condition 5.

### Explicitly do not borrow

13. **Do not add B's radial state cast.** `B-connecting.png` and `B-error.png` show a status
    hue turned into a field, and that is the thing DESIGN.md and ANTI-SLOP.md forbid. C stays
    on the right side of the LILA rule precisely because its hue lives only in hairlines and
    point marks. If a stakeholder asks for the state to be "more visible", the answer is more
    structure, never more wash.

14. **Do not adopt A's error voids.** `A-error-light.png` is the cautionary render. C's single
    fault cross at the boundary crossing of the route that actually died is better information
    in a tenth of the ink.

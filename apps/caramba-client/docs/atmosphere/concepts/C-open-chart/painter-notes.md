# Concept C, Open Chart: Flutter implementation notes

Renderer decision: **`CustomPainter` only.** No `FragmentProgram`, no Rive, no `saveLayer`, no `ImageFilter`. The composition is about 80 stroke and fill operations over flat geometry. A shader would add an asset, a warm-up hitch on first paint under Impeller, and a fallback path, and would compute nothing a raster canvas cannot. The one raster trick used is an `ImageShader` tile for the 45 degree hatch, which replaces about 140 `drawLine` calls with a single fill.

---

## 1. Widget tree

The atmosphere is mounted once, above the scaffold background and below the navigator, so switching tabs never rebuilds it or restarts its ticker.

```
MaterialApp
└─ Builder                                  // theme + MediaQuery available
   └─ AtmosphereHost                        // StatefulWidget, owns the Ticker
      ├─ AtmosphereScope                    // InheritedWidget: geometry + strength
      └─ Stack
         ├─ Positioned.fill
         │   └─ RepaintBoundary             // isolates the layer's own raster
         │       └─ CustomPaint(
         │            painter: ChartPainter(...),
         │            isComplex: true,
         │            willChange: <true only while the ticker runs>,
         │          )
         └─ Navigator / Router              // Home, Servers, Settings, sheets
```

Notes on the tree:

- `RepaintBoundary` is required. Without it, one atmosphere frame dirties the whole screen's layer and the session timer's own repaints drag the chart along with them.
- `willChange` is wired to whether the ticker is currently running. When the composition is static it must be `false`, or the engine keeps the layer in a "may change" bucket.
- Screens push their strength with `AtmosphereScope.of(context).setStrength(...)` in `didChangeDependencies`, or, cleaner, a `RouteAware` observer maps route names to strength: Home 1.0, Servers 0.45, Settings and Profile 0.30, Splash / Login / Autotune 0.0. Opening a modal route sets 0.0 and stops the ticker.

---

## 2. Geometry, built once

```dart
@immutable
class ChartGeometry {
  final Size size;
  final Offset home;              // the dial centre, in layer-local coordinates
  final Path barrier;             // the octagon, for the even-odd clip
  final List<Offset> barrierPts;  // closed polyline, for stroking sub-ranges
  final List<double> barrierCum;  // cumulative arc lengths of barrierPts
  final List<RouteGeom> routes;
  final List<Offset> gridMinor, gridMajor;  // flattened line endpoints
  final Rect lensRect;            // the quiet lens ellipse bounds

  static ChartGeometry build(Size size, Offset dialCentre) { ... }
}

@immutable
class RouteGeom {
  final List<Offset> pts;         // 3 to 5 vertices, orthogonal plus 45 degree elbows
  final List<double> cum;         // cumulative lengths, cum.last == total
  final double crossLen;          // arc length at the first exit from the barrier
  final Offset crossPt;
  final Offset crossNormal;       // unit direction there, for the cap and the fault X
  final double barrierGapAt;      // arc length of that crossing projected onto the barrier
  final List<Offset> waypoints;   // elbow vertices outside the barrier: relay hops
  final String? code;             // 'NL', 'DE', 'US', 'JP', 'SE', or null
  final bool edgeTie;             // route leaves the frame instead of ending at a station
  final int openRank;             // ignition order, nearest station first
}
```

`build` runs on layout, is memoised on `(size, dialCentre)`, and does the 1px walk that finds each route's first exit from the barrier polygon. It is pure integer-ish geometry, well under a millisecond, and it never runs again unless the window resizes or Dynamic Type moves the dial.

The dial centre is passed in rather than assumed, so the chart stays registered to the real dial when the vertical rhythm or text scale shifts. In the prototype this is the `measure()` call that offsets the whole composition; in Flutter it is a `GlobalKey` on the dial plus `RenderBox.localToGlobal`, resolved once per layout.

Two cached rasters, keyed on `(size, devicePixelRatio, brightness)`:

```dart
ui.Image  _hatchTile;   // 64x64, one repeating 45 degree hatch cell
ui.Picture _idleClosed; // the settled disconnected composition
ui.Picture _idleOpen;   // the settled connected composition
```

`_hatchTile` is painted through `Paint()..shader = ImageShader(tile, TileMode.repeated, TileMode.repeated, Matrix4.identity().storage)` inside a `clipPath(barrier, ClipOp.difference)`.

---

## 3. The painter

```dart
class ChartPainter extends CustomPainter {
  ChartPainter({
    required this.geo,
    required this.tokens,      // AtmosphereTokens, resolved from brightness
    required this.frame,       // ChartFrame: everything the painter needs for THIS frame
    required this.strength,    // 1.0 Home, 0.45 Servers, 0.30 Settings
    required Listenable repaint,
  }) : super(repaint: repaint);

  @override
  bool shouldRepaint(ChartPainter old) =>
      old.frame   != frame    ||   // ChartFrame is an @immutable value type with ==
      old.tokens  != tokens   ||
      old.strength!= strength ||
      !identical(old.geo, geo);
}
```

`shouldRepaint` is a value comparison, never `true`. The animation is driven by the `repaint` `Listenable` (see the ticker section), so during a transition the painter is repainted by the notifier and `shouldRepaint` is not even consulted; when the state settles, the notifier stops firing and the layer goes quiet. `ChartFrame` is the whole per-frame parameter set as one immutable record:

```dart
@immutable
class ChartFrame {
  final double grid, hatch, shade, lift, barrier, gap, cap, label, fault;
  final List<double> reach;   // per route, 0..1
  final Color routeTint;      // lerped between idle / probe / live
  final double shakeDx;       // 0 except during the 260ms error shake
}
```

### paint() order

```
1  drawRect            bgBase                                (untranslated)
2  drawRect            atmoLift @ frame.lift                 (untranslated)
3  canvas.translate(frame.shakeDx, 0)
4  graticule           28 drawLine, minor / major paints
5  save + clipPath(barrier, ClipOp.difference)
     shade             one radial gradient fill
     hatch             one ImageShader fill
   restore
6  per route: ghost remainder (dashed, alpha 1 - reach)
             live part 0 .. crossLen + reach * (total - crossLen)
             dead end cap at crossPt, alpha cap * (1 - reach / 0.4)
7  barrier             stroke the closed polyline, skipping [gapAt - g, gapAt + g]
8  fault cross         two drawLine at the failed route's crossPt
9  stations            hollow drawRect, or filled drawRect + drawCircle ring
   waypoints           small drawCircle outlines
   edge ties           one drawLine each
   labels              TextPainter, connected only, 5 of them, cached
10 quiet lens          one radial gradient fill (scaled canvas for the ellipse)
```

Two implementation details that matter:

- **Partial-length strokes without `PathMetric`.** `ui.PathMetric.extractPath` allocates a fresh `Path` every frame. Since routes are short polylines with a cumulative-length table, walk `cum` directly and emit `moveTo` / `lineTo` into a reused `Path` (`path.reset()` per frame). Same output, no per-frame `Path` churn, and it makes the barrier gap stroking fall out of the same helper.
- **Dashed ghost.** Flutter has no dash effect. The ghost segment is short and straight, so emit the dashes in the same walk: step along the remainder at 2 on / 4 off. Do not reach for `path_drawing`.

### Labels

Five `TextPainter`s, laid out once in `ChartGeometry.build` and stored there, re-`paint`ed with an alpha-modulated color. Never lay out text inside `paint`.

---

## 4. Ticker gating

```dart
class _AtmosphereHostState extends State<AtmosphereHost>
    with SingleTickerProviderStateMixin {

  late final Ticker _ticker = createTicker(_onTick);
  final ValueNotifier<int> _repaint = ValueNotifier(0);  // the painter's Listenable

  Duration _lastFrame = Duration.zero;
  static const _minFrameGap = Duration(milliseconds: 33);  // 30fps cap

  void _onTick(Duration elapsed) {
    if (elapsed - _lastFrame < _minFrameGap) return;       // drop 3 of 4 on a 120Hz panel
    _lastFrame = elapsed;
    _frame = _solver.frameAt(elapsed);                     // pure function, see below
    _repaint.value++;                                      // repaints ONLY the CustomPaint

    if (_solver.isSettled(elapsed) && _solver.isStaticState) {
      _ticker.stop();                                      // stop, do not just idle
      setState(() {});                                     // flips willChange to false
    }
  }
}
```

- **Never `setState` per frame.** The `ValueNotifier` passed as `repaint` drives the painter directly and skips the build phase entirely. `setState` is called exactly twice per transition: when it starts (to set `willChange: true`) and when it settles.
- **Stopping matters more than the fps cap.** `disconnected`, `connected` and `error` stop the ticker outright after their transition, so those two states, which are where a user spends essentially all their time, cost zero frames. `connecting` and `reconnecting` hold the ticker, and they are bounded by the real connect timeout.
- **Backgrounding.**

```dart
_lifecycle = AppLifecycleListener(
  onInactive: _ticker.stop,
  onPause:    _ticker.stop,
  onResume:   _resumeFromWallClock,
);

void _resumeFromWallClock() {
  if (!_solver.isStaticState) {
    _solver.rebasePhase(DateTime.now().difference(_connectStartedAt));
    _ticker.start();
  }
}
```

The rebase is the point: the probe loop's phase is recomputed from how long the real handshake has actually been running, so returning to the app does not rewind the animation to where it was when the screen went off.

- **Modal routes** call `AtmosphereScope.setStrength(0)`, which stops the ticker: nothing is visible under an `overlayScrim` anyway.

---

## 5. State machine feed

The VPN connection state already exists as a stream. The atmosphere subscribes to it and does nothing else:

```dart
enum ConnState { disconnected, connecting, connected, error, reconnecting }

// in AtmosphereHost
StreamSubscription<ConnState> _sub = vpn.state.listen((next) {
  _solver.transitionTo(
    next,
    at: _ticker.isActive ? _lastFrame : Duration.zero,
    reduceMotion: _reduceMotion,
  );
  if (next == ConnState.error && !_reduceMotion) _solver.startShake();
  if (!_ticker.isActive) { _ticker.start(); setState(() {}); }
});
```

`ChartSolver` is a plain Dart class with no Flutter dependency, which makes it unit-testable:

```dart
class ChartSolver {
  ConnState from, to;
  Duration  startedAt;
  Duration  duration;       // 620 connecting, 1200 connected, 700 disconnected,
                            // 420 error, 480 reconnecting, 200 under reduce motion

  ChartFrame frameAt(Duration now);   // pure: (from, to, elapsed) -> ChartFrame
  bool isSettled(Duration now);
  bool get isStaticState;             // disconnected | connected | error, or reduceMotion
}
```

`frameAt` does three things:

1. Lerp the nine field scalars from the `from` state's table to the `to` state's table with `Curves.easeInOutCubic`.
2. Compute each route's `reach`, staggering the progress by `openRank` (inside-out when opening to connected, outside-in when closing) and evaluating the probe loop for `connecting` / `reconnecting`.
3. Lerp `routeTint` between the two states' tints, per route, so the amber-to-green crossover follows each route's own extension rather than happening globally.

Because it is pure, the transition tables are golden-testable: assert that `frameAt(startedAt + duration)` equals the target state's static frame exactly, for all twenty from/to pairs. That is the test that stops a future edit from leaving a state mid-transition forever.

### Reduce motion

`MediaQuery.disableAnimations` (plus the platform flag) sets `_reduceMotion`, and the solver switches to a genuinely different path rather than a slowed-down one:

- `duration` becomes 200ms and the only thing that animates is opacity.
- `frameAt` returns the **target state's static frame** immediately; the 200ms is spent cross-fading the previously recorded `ui.Picture` over the new composition (`canvas.drawPicture(old)` with `saveLayer` alpha, the one place `saveLayer` is used, for 200ms, once).
- Probe states resolve to a deterministic mid-range extension, not "wherever the loop was", so the frozen composition is the same every time.
- The dial's own indeterminate arc is frozen by the existing `reduceMotion` flag in the dial painter, so the screen is genuinely still and not merely calmer.

---

## 6. Tokens

`AtmosphereTokens` is a second `ThemeExtension`, kept separate from `AppTokens` so nothing in the component layer can reach it:

```dart
@immutable
class AtmosphereTokens extends ThemeExtension<AtmosphereTokens> {
  final Color grid, gridMajor, hatch, shade, lift,
              routeIdle, routeGhost, routeProbe, routeLive,
              nodeIdle, nodeLive, barrier, fault, label, quietLens;
  final double barrierHw, barrierHh, barrierCut,
               gridPitch, hatchPitch, ringOffset, gapWidth, lensRx, lensRy;

  factory AtmosphereTokens.dark();
  factory AtmosphereTokens.light();
}
```

Values are in section 4 of `concept.md`. Two lint-able rules to add to the review checklist:

1. No widget outside `lib/theme/atmosphere/` may import `AtmosphereTokens`. The atmosphere is a layer, not a palette.
2. Field colors (`grid`, `gridMajor`, `hatch`, `shade`, `lift`) stay at or under 8 percent alpha. That ceiling is what the contrast table in `concept.md` is built on, and it should be asserted in a widget test that samples the rendered layer and checks the composite range, the same way the prototype's numbers were measured.

---

## 7. Profiling gate before this ships

Run `flutter run --profile` on the floor device and record the timeline across a full disconnect to connect to disconnect cycle.

- **Pass:** raster thread p95 under 6ms during the transition, and **zero** raster work in the two settled states (this is the one that catches a ticker that was never actually stopped).
- **First lever if it misses:** bake the graticule into the hatch tile so steps 4 and 5 collapse into one shader fill.
- **Second lever:** drop the transition cap from 30 to 20fps. The choreography is length-based, not frame-based, so it degrades cleanly.
- **Last resort:** route devices below a measured threshold to the reduce-motion path. It already exists, it is already designed, and it still tells the whole story.

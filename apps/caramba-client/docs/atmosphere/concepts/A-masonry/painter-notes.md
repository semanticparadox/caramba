# Concept A: Flutter translation

Painter, not shader. Rationale is in `concept.md` §5. What follows is the shape of the code.

---

## 1. Widget tree

```
HomeScreen
└─ Stack
   ├─ Positioned.fill(
   │    child: RepaintBoundary(
   │      child: AtmosphereLight(state: state, strength: 1.0)))   // opacity only
   ├─ Positioned.fill(
   │    child: RepaintBoundary(
   │      child: AtmosphereLattice(controller: atmo, strength: 1.0)))
   ├─ Positioned.fill(
   │    child: IgnorePointer(child: AtmosphereGuard()))           // static, never repaints
   └─ SafeArea(child: HomeContent(...))                           // dial, rows, stats, quota
```

The whole atmosphere group is wrapped once in `ExcludeSemantics`.

- **`AtmosphereLight`** holds the two precomputed `ui.Image`s (centre lift, vignette) and drives each with its own `FadeTransition` from `atmo.lightAmount` / `atmo.vignetteAmount`. Because only opacity changes, the compositor reuses the existing texture and this layer never re-rasterises.
- **`AtmosphereLattice`** is the single `CustomPaint`. It is the only thing that repaints.
- **`AtmosphereGuard`** is a `DecoratedBox` with a `RadialGradient` of `atmoGuard`. Const, no animation.

On Servers / Profile / Settings the same three widgets are inserted with `strength: 0.35`, `depthEnabled: false`, `motionEnabled: false`. On Splash / Login / Autotune, `strength: 0.45` and the state pinned to disconnected.

---

## 2. The controller

`AtmosphereController extends ChangeNotifier` and owns a `Ticker` it did not start.

```dart
enum AtmoKind { flat, sweep, openRun, closeRun }

class AtmoParams {
  double open = 0, depth = 0, light = 0, vignette = 1, voids = 0;
  double front = -0.3, front2 = -0.3;   // wavefront positions, normalised radius
  double tint = 0;                      // 0..1.5 weight on the status mix
  Color  tintColor = const Color(0x00000000);
  double shake = 0;                     // px, x only
}

class AtmosphereController extends ChangeNotifier {
  final AtmoParams p = AtmoParams();
  AtmoKind kind = AtmoKind.flat;
  int revision = 0;                     // bumped on every applied frame

  late final Ticker _ticker;            // created with TickerProvider, never auto-started
  final List<_Tween> _tweens = [];
  Duration _sweepStart = Duration.zero;
  Duration _pulseStart = const Duration(days: -1);
  Timer? _pulseTimer;
  bool reduceMotion = false;
  bool _foreground = true;
}
```

**Ticker gating.** One predicate decides everything:

```dart
bool get _needsFrames =>
    !reduceMotion &&
    _foreground &&
    (_tweens.isNotEmpty ||
     kind == AtmoKind.sweep ||
     _pulseElapsed < const Duration(milliseconds: 1600));

void _sync() {
  if (_needsFrames && !_ticker.isTicking) _ticker.start();
  if (!_needsFrames && _ticker.isTicking) { _ticker.stop(); notifyListeners(); }
}
```

`_sync()` is called after every state change, after every tween completes, and at the end of each tick. When the last tween drains in a static state the ticker stops itself and one final frame is painted.

**Frame throttle.** The tick callback rejects frames closer than 32 ms apart, so the painter runs at ≤30 fps regardless of display refresh:

```dart
void _onTick(Duration elapsed) {
  _stepTweens(elapsed);
  if (kind == AtmoKind.sweep) _advanceSweep(elapsed);
  if (elapsed - _lastPaint < const Duration(milliseconds: 32)) return;
  _lastPaint = elapsed;
  revision++;
  notifyListeners();
  _sync();
}
```

**Lifecycle.** An `AppLifecycleListener` sets `_foreground` and calls `_sync()`; `paused`, `inactive` and `hidden` all stop the ticker and cancel `_pulseTimer`. `TickerMode` already suspends the ticker when the Home route is not current, so tab switches need no extra code.

**Connected steady state** holds no ticker. It schedules `_pulseTimer = Timer(const Duration(milliseconds: 4800), ...)`, which sets `_pulseStart` and calls `_sync()`. The ticker then runs for 1.6 s and stops again.

---

## 3. The sweep

Both wavefronts ride the same curve; the second is the first delayed by 0.22 of a period, which makes the open shell start wide near the dial and thin as it travels.

```dart
void _advanceSweep(Duration elapsed) {
  final period = state == ConnState.reconnecting ? 2600 : 1900;
  final t = ((elapsed - _sweepStart).inMilliseconds % period) / period;
  p.front  = -0.25 + 1.62 * Curves.easeOutCubic.transform((t / 0.70).clamp(0, 1));
  p.front2 = -0.25 + 1.62 * Curves.easeOutCubic
      .transform((((t - 0.22) / 0.70).clamp(0, 1)).toDouble());
}
```

---

## 4. Precomputed geometry

Built once per size in `didChangeDependencies` / on `LayoutBuilder` size change, stored on the controller, never per frame.

```dart
class AtmoCell {
  final double x, y;      // top-left, physical-pixel snapped
  final double rn;        // radius from origin / maxRadius, 0..1
  final int    course;    // 0 or 1, running bond parity
  final double grain;     // stable hash, 0..1
}
```

Packed as four parallel `Float32List`s plus one `Uint8List` for `course`, not a `List<AtmoCell>`, so the paint loop does no pointer chasing.

- Origin is the dial centre in local coordinates. `maxRadius = hypot(w/2 + cellW, max(originY, h - originY) + cellH)`.
- Odd courses are offset by `cellW / 2`.
- `cellW = (shortestSide / 8.5).clamp(38, 56)`, `cellH = cellW / 2.2`.
- Seam coordinates get `+ 0.5 / devicePixelRatio` and are snapped so a 1 px seam lands on a physical pixel boundary. Skipping this is what makes the lattice shimmer.
- 33 pre-lerped panel fill `Color`s per course, rebuilt on theme change only.
- The two gradient `ui.Image`s built at half resolution with `PictureRecorder`, rebuilt on theme or size change only.

---

## 5. CustomPainter

```dart
class LatticePainter extends CustomPainter {
  LatticePainter({
    required this.geo,        // precomputed cells
    required this.p,          // AtmoParams (mutated in place by the controller)
    required this.kind,
    required this.revision,   // monotonic, bumped once per applied frame
    required this.tokens,     // AtmoColors for the active theme
    required this.strength,
    required this.depthEnabled,
    required this.pulseT,     // -1 when no pulse is live
  }) : super(repaint: controller);   // controller is the Listenable

  @override
  bool shouldRepaint(LatticePainter old) =>
      old.revision != revision ||        // the controller applied a new frame
      old.tokens   != tokens   ||        // theme flipped
      old.geo      != geo      ||        // size or orientation changed
      old.strength != strength ||
      old.kind     != kind;
}
```

`revision` is the whole trick: `AtmoParams` is mutated in place for zero allocation per frame, so identity comparison on it would always return false. The monotonic counter is the honest signal, and it only advances on frames the controller decided to apply, which is already 30 fps capped. `shouldRepaint` returning false for everything else means a static state costs nothing even if an ancestor rebuilds.

### paint()

```dart
void paint(Canvas canvas, Size size) {
  canvas.drawRect(Offset.zero & size, basePaint);          // atmoBase
  if (p.shake != 0) { canvas.save(); canvas.translate(p.shake, 0); }

  for (var plane = planes.length - 1; plane >= 0; plane--) {
    final a = plane == 0 ? 1.0 : planeAlpha[plane] * p.depth;
    if (a < 0.015 || (plane > 0 && !depthEnabled)) continue;
    canvas.save();
    if (plane > 0) {
      canvas.translate(origin.dx, origin.dy);
      canvas.scale(planeScale[plane]);
      canvas.translate(-origin.dx, -origin.dy);
      canvas.saveLayer(null, Paint()..color = Color.fromRGBO(0, 0, 0, a));
    }
    _drawPlane(canvas, planeScale[plane], fills: plane == 0);
    if (plane > 0) canvas.restore();
    canvas.restore();
  }

  if (p.voids > 0.01) _drawVoids(canvas);
  if (p.shake != 0) canvas.restore();
}
```

The back planes use one `saveLayer` each for their group alpha. If that shows up in a profile, replace it by pre-multiplying alpha into each bucket's stroke colour, which is exactly what the HTML demo does with `globalAlpha` and costs nothing.

### Openness and bucketing

```dart
double _cellOpen(double rn) => switch (kind) {
  AtmoKind.sweep    => p.open * math.max(0, _wf(p.front, rn) - _wf(p.front2, rn)),
  AtmoKind.openRun  => p.open * _wf(p.front, rn),
  AtmoKind.closeRun => p.open * _wf(p.front, rn),
  AtmoKind.flat     => p.open,
};

double _wf(double f, double r) => ((f - r) / kBand).clamp(0.0, 1.0);
```

Per cell, `o = _cellOpen(rn) * (0.90 + 0.10 * grain)`. The grain is what stops the wavefront edge from being a mathematically perfect ring, and it is the difference between "designed" and "generated".

Two accumulators per plane:

- **Fills** (front plane only): append six vertices per cell into one reusable `Float32List`, colour taken from the 33-entry lerp table indexed by `(o * 32).toInt()` and the course parity, emitted as a single `canvas.drawVertices(VertexMode.triangles, ...)`.
- **Struts**: each cell contributes its **top and left** edges only, as two pairs of corner segments; neighbours supply the rest. That halves the geometry and running bond means the vertical seams already stagger correctly. Segment length is `cellW/2 * (1 - 0.62*o)` horizontally, `cellH/2 * (1 - 0.60*o)` vertically. Segments go into `6 open × 3 dissolve = 18` reusable `Float32List` buffers, each flushed with one `drawRawPoints(PointMode.lines, buf, paint)`.

Bucket stroke colour:

```dart
var c = Color.lerp(tokens.seamClosed, tokens.seamOpen, o)!;
final tf = p.tint * o * kTintMax;                   // kTintMax = 0.16
if (tf > 0.002) c = Color.lerp(c, p.tintColor, tf)!;
final fade = o * kDissolve * _smoothstep(0.20, 0.98, rn * planeScale);
if (fade > 0.01)  c = Color.lerp(c, tokens.base, fade)!;
```

Nodes: a 2 px square at the top-left corner of every cell with `o > 0.55 && fade < 0.45`, accumulated into one `Path` and filled once.

Reach pulse: a second pass over only the cells where `exp(-((rn - pulsePos)/0.085)^2) * (1 - dissolve)` exceeds 0.06 (about 60 cells), into 4 alpha buckets, stroked with `atmoPulse`.

**Per-frame budget:** 1 `drawVertices`, ~18 `drawRawPoints` for the front plane, ~12 more per visible back plane, 1 node `drawPath`, up to 4 pulse strokes. 40 to 55 draw calls, ~4,400 segments, zero allocations after warm-up because every buffer is reused and only `_length` is reset.

---

## 6. State machine feed

The existing connection bloc/notifier is the single source of truth. One listener translates its state into controller calls; the controller owns no connection logic and the connection layer knows nothing about the atmosphere.

```dart
void onConnState(ConnState next) {
  atmo.applyState(next);   // pushes tweens + sets kind, then calls _sync()
}
```

`applyState` is a switch that mirrors the transition table in `concept.md` §3. Two details that matter:

- **`connected`** sets `kind = openRun` with `front` tweening `-0.25 → 1.30` over 620 ms `easeOutQuint`, then a 660 ms timer flips `kind = flat`, pins `open = 1`, fires the first pulse and starts the pulse schedule. If the state changes during those 660 ms the timer is cancelled, so a fast connect-then-drop cannot strand the painter in `openRun`.
- **`disconnected` and `error`** set `kind = closeRun` and seed `p.open` from `max(previousOpen, 0.44)` before running `front` inward. Seeding from the previous value is what makes disconnect-from-connecting and disconnect-from-connected both look correct without a separate code path.

`applyState` is idempotent and safe to call with the state it is already in.

**Reduce motion.** `reduceMotion` is read from `MediaQuery.disableAnimations` and pushed into the controller. When true, `applyState` skips every tween and writes the static composition from the §6 table directly, then bumps `revision` once. The cross-fade is handled one level up by an `AnimatedSwitcher(duration: 200ms, transitionBuilder: FadeTransition)` around the `CustomPaint`, keyed on the state, so two painted stills cross-fade and nothing translates or scales.

---

## 7. Test hooks

- `AtmosphereController` is pure Dart with an injectable `TickerProvider` and clock, so the whole transition table is unit-testable without a widget.
- A golden test per state per theme (10 goldens) using a fixed size and a pinned clock, with `kind = flat` and wavefronts frozen at the reduce-motion values, catches token drift and the tint cap.
- One test asserts `ticker.isTicking == false` after settling into `disconnected`, `connected` and `error`, and after `AppLifecycleState.paused` in every state. That test is the battery contract.

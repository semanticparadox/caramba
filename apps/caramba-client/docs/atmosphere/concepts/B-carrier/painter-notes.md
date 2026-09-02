# Concept B: Carrier. Flutter translation

Painter, not shader. See concept.md section 5 for why. Everything below is line work plus two full-viewport fills, which is exactly what `Canvas` is good at.

---

## 1. Files

```
lib/theme/atmosphere.dart          AppAtmosphere colors + scalars, dark/light, lerpable
lib/atmosphere/atmosphere_layer.dart   the widget: ticker gate, lifecycle, state tweens
lib/atmosphere/atmosphere_painter.dart the CustomPainter
lib/atmosphere/atmosphere_tables.dart  const STRATA, BEACONS, NOISE, DUTY_PHASE, DROPOUTS
lib/atmosphere/grain.dart              one-time ui.Image tile builder
```

`AppAtmosphere` hangs off the existing `AppTokens` ThemeExtension as `tokens.atmosphere`, so it lerps with the theme for free and nothing outside `lib/atmosphere/` ever reads it.

---

## 2. Widget tree

The layer is mounted once in the app shell, under the `Navigator`, not per screen. Screens do not build it and cannot forget it.

```dart
Stack(
  children: [
    // 1. the atmosphere. Its own boundary so the content above never dirties it,
    //    and its own repaints never climb back up the tree.
    Positioned.fill(
      child: RepaintBoundary(
        child: AtmosphereLayer(
          state:    connectionState,          // from the VPN state notifier
          strength: AtmosphereStrength.of(context),  // route-driven, tweened 220ms
        ),
      ),
    ),
    // 2. everything else, on its own boundary so the session timer ticking
    //    at 1Hz does not mark the atmosphere dirty.
    const Positioned.fill(child: RepaintBoundary(child: AppNavigator())),
  ],
)
```

`AtmosphereStrength` is an `InheritedWidget` fed by a route observer: `1.00` on Home, `0.35` on Servers / Market / Browser, `0.22` on Settings / Profile, `0.60` on Splash / Login / Autotune. The layer tweens between strength values over 220ms on route change and drops the horizon, beacon and carrier passes entirely below `0.5`.

**Geometry is measured, never hard-coded.** Home publishes the dial's and the carrier band's laid-out rects up to the layer through a `ValueNotifier<AtmosphereAnchors>` written from a post-frame callback on the dial's `RenderBox`:

```dart
class AtmosphereAnchors {
  final double horizonY;   // dial center in global logical px
  final double dialCX;
  final double dialRadius;
  final double carrierY;   // center of the 52px band under the sub-label
}
```

On any other route the layer falls back to proportional defaults (`horizonY = 0.26 * size.height`), which is fine because the horizon is not drawn there anyway. This is what makes the layer survive dynamic type: grow the labels and the carrier band moves, and the trace follows it.

---

## 3. The ticker gate

This is the part that decides whether the concept ships or gets reverted for battery. It is not an optimization, it is the design.

```dart
class _AtmosphereLayerState extends State<AtmosphereLayer>
    with SingleTickerProviderStateMixin, WidgetsBindingObserver {

  late final Ticker _ticker = createTicker(_onTick);
  final _frame = ValueNotifier<int>(0);      // the painter's only repaint trigger

  _AtmoProps _props = _AtmoProps.rest(ConnState.disconnected);
  _AtmoProps _from  = _props;
  _AtmoProps _to    = _props;
  _Choreo    _plan  = _Choreo.fallback;
  Duration   _t0    = Duration.zero;
  Duration   _elapsed = Duration.zero;
  bool       _animating = false;
  Duration   _lastEmit = const Duration(days: -1);  // first frame always paints

  int get _targetFps {
    if (_reduceMotion) return _xfade == null ? 0 : 30;
    if (_animating || _elapsed < _shakeUntil) return 30;   // the error shake needs
    return _restFps[widget.state]!;                        // frames even at rest fps 0
  }

  void _onTick(Duration now) {
    _elapsed = now;
    if (_animating) _stepProps(now - _t0);

    final fps = _targetFps;
    if (fps > 0 && now - _lastEmit >= Duration(microseconds: 1000000 ~/ fps)) {
      _lastEmit = now;
      _frame.value++;                                   // the only thing that repaints
    }
    if (fps == 0 && !_animating) {
      _frame.value++;                                   // one last settled frame
      _ticker.stop();
    }
  }
}
```

Notes that matter:

- **`_frame` is an `int` on a `ValueNotifier`, not an `Animation<double>`.** The painter takes it as its `repaint:` listenable. Bumping it only when the fps interval has elapsed is how the effective frame rate is controlled. Handing the painter a raw ticker value would repaint at display refresh and quietly cost 60 or 120fps.
- **`_lastEmit` starts at a large negative**, and is reset to it on every state change, so the first frame after any state change always paints. Without that, a state whose resting rate is 6fps shows nothing for 166ms. (This was a real bug in the prototype and it is worth keeping the comment.)
- **`fps == 0` stops the ticker outright**, it does not run a ticker that emits nothing.
- The `_shakeUntil` clause exists because Error rests at 0fps but still owes a 120ms shake. Anything that must move has to force the rate up, not rely on the resting one.

Lifecycle and platform gates:

```dart
@override
void didChangeAppLifecycleState(AppLifecycleState s) {
  if (s == AppLifecycleState.resumed) _ensureTicker(); else _ticker.stop();
}

@override
void didChangeDependencies() {
  super.didChangeDependencies();
  _reduceMotion = MediaQuery.disableAnimationsOf(context);
  _highContrast = MediaQuery.highContrastOf(context);
  // TickerMode already muffles us when Home is not the current route.
  // Battery saver: on Android read PowerManager.isPowerSaveMode via a channel,
  // on iOS ProcessInfo.processInfo.isLowPowerModeEnabled. Either one forces
  // the connected resting rate from 6 to 0.
}
```

`TickerMode` handles the "Servers pushed over Home" case for free, so there is no manual route check. Settings and Profile never start the ticker at all: the layer sees `strength <= 0.25` and paints once.

---

## 4. The painter

```dart
class AtmospherePainter extends CustomPainter {
  AtmospherePainter({
    required this.frame,        // Listenable<int>, passed as repaint:
    required this.props,        // interpolated scalars, a value type
    required this.state,        // ConnState, for the two state-shaped behaviours
    required this.anchors,
    required this.atmo,         // AppAtmosphere for the active theme
    required this.strength,
    required this.reduceMotion,
    required this.grain,        // ui.Image, 96x96, built once per theme
    required this.phaseSeconds, // carrier phase, only advances when the ticker runs
  }) : super(repaint: frame);

  @override
  bool shouldRepaint(AtmospherePainter old) =>
      old.props      != props      ||    // _AtmoProps is an equatable value type
      old.state      != state      ||
      old.anchors    != anchors    ||
      old.atmo       != atmo       ||
      old.strength   != strength   ||
      old.reduceMotion != reduceMotion ||
      old.grain      != grain;
  // frame is handled by repaint:, so it is deliberately absent here.
}
```

`paint()` in order. Each numbered block is one to a few draw calls.

1. **Sky field.** `LinearGradient` top / horizon / bottom, the horizon stop lerped `atmoSkyHorizonClosed -> atmoSkyHorizonOpen` by `clear`, where `clear = ((restDisconnected.ceiling - props.ceiling) / (restDisconnected.ceiling - restConnected.ceiling)).clamp(0,1)`. **Cache the `Shader`** in a `_SkyCache` keyed by `(clear rounded to 1/256, size, atmo)`; rebuilding a gradient shader every frame is the classic cost here.

2. **Ceiling strata.** `canvas.save(); canvas.clipRect(Rect.fromLTWH(0,0,w, atmoTypeBandTop * h));` then loop the 9 `const` strata. Each is one `drawRect` with a horizontal `LinearGradient` shader (feathered at any end that does not reach the frame edge, which is what keeps the deck from reading as even stripes) plus one 1px `drawRect` for the lit edge. Fractional `props.strata` fades the last visible stratum in and out: `alpha *= (props.strata - k).clamp(0, 1)`.

3. **Underside feather.** One vertical gradient rect over the bottom 52px of the deck.

4. **Grain.** `Paint()..shader = ImageShader(grain, TileMode.repeated, TileMode.repeated, Matrix4.translationValues(-jx, -jy, 0).storage)`, drawn over the deck zone in 12 horizontal strips with a descending alpha ramp on the last 38% so the murk fades out instead of ending on a rule. Jitter offsets come from a `const [Offset]` table of 4, indexed by `frame ~/ 2`, and are pinned to 0 under reduce motion. `canvas.restore()` closes the clip. **Draw the tile at 1:1 device pixels**: build it at `96 * devicePixelRatio` and scale the shader matrix by `1/dpr`, otherwise it resamples and stops looking like interference.

5. **State cast.** A `RadialGradient` centered on `anchors.horizonY / dialCX`, radius `0.92 * width`, stops `[a, a*0.35 @ .45, 0 @ 1]`. Two of these during a transition, the outgoing at `a * (1 - castMix)` and the incoming at `a * castMix`. Alpha is asserted `<= 0.08` in debug.

6. **Horizon lift.** One vertical gradient rect, 52px tall, centered on the horizon, alpha `atmoHorizonLiftAlpha * clear`. Skipped when `clear < 0.02`.

7. **Horizon.** Built as a `Float32List` of rects and issued as a single `drawRawPoints(PointMode.lines, ...)` or, simpler and just as cheap at this count, one `Path` of 3px segments and one `drawPath`. Per segment: `alpha = ((visibility - d) / 0.20).clamp(0,1)` where `visibility = 0.66 + 0.50 * props.reach`; dash phase from `lerp(19,10,fence)` on / `lerp(0.5,16,fence)` off, sliding by `12 * seconds` px only in the reconnecting state. In the error state, skip the 14px around `atmoFractureX` and offset every segment past it by 2px.

8. **Fence ticks.** Verticals of height `9 * props.fence` every 26px, gated by the same visibility envelope. One path, one draw.

9. **Beacons.** 26 fixed x, skipping any within `dialRadius + 8` of `dialCX`. `alpha = ((visibility - d)/0.20).clamp(0,1) * (1 - 0.55*d)`. In reconnecting, the first six get the deterministic duty cycle `((frame ~/ 2) + DUTY_PHASE[i]) % 5 < 3 ? 1 : 0.15`, and a flat `0.60` under reduce motion. One `drawRawPoints(PointMode.points)` with a 1.4px stroke width, plus a second pass for the near ones that get a faint 3px halo.

10. **Carrier.** One `Path` of 131 points across the width. Noise from the 512-entry `Float32List` value table sampled at `fx * 26 + phase * 5.5` and `fx * 74 + phase * 13`. The clean wave is `sin(x * 0.045 + phase * 0.38) * 3.2 + sin(x * 0.017 - phase * 0.21) * 1.6`. Blend by a per-x lock amount: the global `props.lock`, raised behind the hunt cursor while connecting. Dropout ranges break the path (`moveTo` instead of `lineTo`) whenever `lock < 0.6`. One `drawPath` with a 1px stroke. Plus one 1px baseline rect 20px below it.

`strength` multiplies a `saveLayer` alpha around the whole thing, or, cheaper, is folded into every paint's alpha. Fold it in. `saveLayer` on a full-screen bounds is the one thing here that would actually cost something.

---

## 5. Precomputation

| Thing | Where | When |
|---|---|---|
| `grain` `ui.Image`, 96x96 at dpr, seeded LCG through `decodeImageFromPixels` | `grain.dart` | once per theme, at app start and on theme change. ~36KB, ~2ms |
| `NOISE` 512-entry `Float32List` | `atmosphere_tables.dart` | once, lazily. Deterministic, so goldens are stable |
| `BEACONS` 26 irregular x | `atmosphere_tables.dart` | `const` |
| `STRATA` 9 x `[off, height, alpha, x0, x1]` | `atmosphere_tables.dart` | `const` |
| `DUTY_PHASE`, `DROPOUTS` | `atmosphere_tables.dart` | `const` |
| Sky gradient `Shader` | painter-local `_SkyCache` | rebuilt only when `clear` or size or theme changes |

No `Random()` anywhere in the paint path. Every value that looks random comes from a seeded table, which is what makes the whole layer golden-testable.

**Tier-2, only if a real device needs it:** on state settle, record the sky + strata + grain + cast passes into a `ui.Picture`, rasterize once to a `ui.Image`, and let the resting frame be one `drawImage` plus the line work. That removes both full-viewport fills from the 6fps and 8fps idle paths.

---

## 6. Feeding the state machine

The VPN state notifier already emits `ConnState { disconnected, connecting, connected, error, reconnecting }`. The layer listens and runs the tween set:

```dart
void _onState(ConnState next) {
  if (next == widget.state) return;
  _from = _props;
  _to   = _AtmoProps.rest(next);
  _plan = _Choreo.forTransition(widget.state, next);  // per-property [delay, dur, curve]
  _t0   = _elapsed;
  _animating = true;
  _lastEmit  = const Duration(days: -1);              // paint the first frame now
  if (next == ConnState.error && !_reduceMotion) _shakeUntil = _elapsed + 120.ms;

  if (_reduceMotion) {
    _props = _to;              // settled composition immediately
    _animating = false;        // nothing tweens. Ever.
    _shakeUntil = Duration.zero;
    _xfade = _Xfade(start: _elapsed, duration: 200.ms);
  }
  _ensureTicker();
}
```

`_Choreo` is a table of `{property: (delay, duration, Curve)}` keyed by `from>to`, with the tables from concept.md section 3 and a fallback. `_stepProps` lerps each property independently, which is what gives the connecting-to-connected moment its shape: the carrier locks at 180ms with `Curves.easeOutBack`, the ceiling starts 60ms later and runs 700ms on `Curves.easeOutQuint`, and the beacons do not start until 200ms and take 900ms on `Curves.easeOutCubic`.

**Reduce motion** is a separate render path, not a set of shorter durations. Two offscreen `ui.Image` layers hold the outgoing and incoming settled compositions and the layer cross-fades their opacity over 200ms, then stops the ticker. Nothing translates, nothing morphs. Setting `_animating = false` there is load-bearing: leaving it true would tween the geometry underneath the cross-fade and quietly defeat the whole fallback.

---

## 7. Tests worth writing

1. **The type-band invariant.** Paint each of the five states into a `ui.Image` and assert no pixel below `0.395 * height` differs from the pure sky gradient by more than 1/255 outside the carrier band. This is what guarantees text contrast, and it is what a future change will break first.
2. **Cast alpha ceiling.** Assert every `atmoCast*` alpha used in a paint is `<= 0.08`.
3. **Frame budget.** In debug, count `paint()` calls per second and log a warning above `atmoFps + 2`. Cheap insurance against someone handing the painter a raw animation value.
4. **Goldens per state, both themes, reduce motion on and off.** All ten are deterministic because there is no `Random()` in the paint path.
5. **Horizon anchoring.** Assert the drawn horizon y equals the dial's laid-out center within 0.5px at three text scale factors. Two pixels off and the image stops working.

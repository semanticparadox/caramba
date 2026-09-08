import 'dart:math' as math;

import 'package:flutter/animation.dart';
import 'package:flutter/foundation.dart';

import 'package:caramba_client/atmosphere/atmosphere_tokens.dart';

/// The five compositions the chart can hold. Deliberately its own enum rather
/// than `VpnStage`: the solver is plain Dart with no dependency on the tunnel
/// package, so the whole transition table is unit testable.
enum AtmoState { disconnected, connecting, connected, error, reconnecting }

/// Which token family the routes are drawn in.
enum RouteTint { idle, probe, live }

/// Everything the layer needs to decide what to draw, as one immutable value.
/// Built by the widget from `VpnStage` + `MediaQuery.disableAnimations` +
/// [AtmosphereBudget] + the resolved [AtmosphereTokens].
@immutable
class AtmosphereParams {
  final AtmoState state;
  final bool reduceMotion;
  final bool lowPower;
  final AtmosphereTokens tokens;

  /// 1.0 on Home, 0.45 on Servers, 0.30 on Settings and Profile, 0 elsewhere.
  final double strength;

  const AtmosphereParams({
    required this.state,
    required this.tokens,
    this.reduceMotion = false,
    this.lowPower = false,
    this.strength = 1,
  });

  /// True when this composition holds still once its transition has landed, so
  /// the ticker can be stopped rather than merely ignored.
  bool get isStatic => reduceMotion || _defs[state]!.probe == null;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AtmosphereParams &&
          other.state == state &&
          other.reduceMotion == reduceMotion &&
          other.lowPower == lowPower &&
          other.tokens == tokens &&
          other.strength == strength;

  @override
  int get hashCode =>
      Object.hash(state, reduceMotion, lowPower, tokens, strength);
}

/// The probe loop parameters for a state that animates.
@immutable
class _Probe {
  final double lo;
  final double hi;
  final int periodMs;
  const _Probe(this.lo, this.hi, this.periodMs);
}

/// The static field scalars of one composition.
@immutable
class _StateDef {
  final double grid, hatch, shade, lift, barrier, gap, cap, label, fault;
  final RouteTint tint;
  final double reach;
  final _Probe? probe;

  const _StateDef({
    required this.grid,
    required this.hatch,
    required this.shade,
    required this.lift,
    required this.barrier,
    required this.gap,
    required this.cap,
    required this.label,
    required this.fault,
    required this.tint,
    this.reach = 0,
    this.probe,
  });
}

const Map<AtmoState, _StateDef> _defs = <AtmoState, _StateDef>{
  AtmoState.disconnected: _StateDef(
    grid: 0.18,
    hatch: 1,
    shade: 1,
    lift: 0,
    barrier: 1,
    gap: 0,
    cap: 1,
    label: 0,
    fault: 0,
    tint: RouteTint.idle,
  ),
  AtmoState.connecting: _StateDef(
    grid: 0.55,
    hatch: 0.35,
    shade: 0.45,
    lift: 0,
    barrier: 0.70,
    gap: 1,
    cap: 0,
    label: 0,
    fault: 0,
    tint: RouteTint.probe,
    probe: _Probe(0.15, 0.78, 2200),
  ),
  AtmoState.connected: _StateDef(
    grid: 1,
    hatch: 0,
    shade: 0,
    lift: 1,
    barrier: 0,
    gap: 1,
    cap: 0,
    label: 1,
    fault: 0,
    tint: RouteTint.live,
    reach: 1,
  ),
  AtmoState.error: _StateDef(
    grid: 0.18,
    hatch: 1,
    shade: 1,
    lift: 0,
    barrier: 1,
    gap: 0,
    cap: 1,
    label: 0,
    fault: 1,
    tint: RouteTint.idle,
  ),
  AtmoState.reconnecting: _StateDef(
    grid: 0.40,
    hatch: 0.50,
    shade: 0.60,
    lift: 0,
    barrier: 0.85,
    gap: 0.55,
    cap: 0,
    label: 0,
    fault: 0,
    tint: RouteTint.probe,
    probe: _Probe(0.20, 0.60, 3000),
  ),
};

/// Transition length, keyed by the state being entered. Opening a world takes
/// longer than losing it: 1200ms in, 700ms out, 420ms when it slams.
const Map<AtmoState, int> kAtmoDurationMs = <AtmoState, int>{
  AtmoState.disconnected: 700,
  AtmoState.connecting: 620,
  AtmoState.connected: 1200,
  AtmoState.error: 420,
  AtmoState.reconnecting: 480,
};

/// The reduce-motion path is a different rendering path, not a slower one:
/// geometry never animates, only opacity, and only for this long.
const Duration kAtmoReduceMotionFade = Duration(milliseconds: 200);

/// The error shake, chart layer only, in the same gesture as the dial's.
const int kAtmoShakeMs = 260;

/// Number of routes on the chart. Kept here so the solver stays free of the
/// geometry module.
const int kAtmoRouteCount = 8;

/// Ignition order, nearest station first. Precomputed from the route lengths in
/// `atmosphere_painter.dart` and asserted there, so the solver needs no
/// geometry to stagger correctly.
const List<int> kAtmoOpenRank = <int>[4, 2, 5, 0, 1, 3, 6, 7];

double _clamp01(double v) => v < 0 ? 0 : (v > 1 ? 1 : v);

double _easeOutCubic(double t) => 1 - math.pow(1 - t, 3).toDouble();
double _easeInCubic(double t) => t * t * t;
double _easeInOut(double t) =>
    t < 0.5 ? 4 * t * t * t : 1 - math.pow(-2 * t + 2, 3).toDouble() / 2;
double _easeInOutSine(double t) => -(math.cos(math.pi * t) - 1) / 2;

/// Everything the painter needs for ONE frame, as an immutable value type.
/// `shouldRepaint` never returns a bare `true`; it compares these.
@immutable
class ChartFrame {
  final double grid, hatch, shade, lift, barrier, gap, cap, label, fault;

  /// Per route extension, 0 (stops at the boundary) to 1 (reaches its station).
  final List<double> reach;

  /// Per route stroke color, already crossed from the previous tint to the
  /// next one along that route's OWN extension.
  final List<Color> tint;

  /// Horizontal offset of the whole chart during the error shake.
  final double shakeDx;

  const ChartFrame({
    required this.grid,
    required this.hatch,
    required this.shade,
    required this.lift,
    required this.barrier,
    required this.gap,
    required this.cap,
    required this.label,
    required this.fault,
    required this.reach,
    required this.tint,
    required this.shakeDx,
  });

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is ChartFrame &&
          other.grid == grid &&
          other.hatch == hatch &&
          other.shade == shade &&
          other.lift == lift &&
          other.barrier == barrier &&
          other.gap == gap &&
          other.cap == cap &&
          other.label == label &&
          other.fault == fault &&
          other.shakeDx == shakeDx &&
          listEquals(other.reach, reach) &&
          listEquals(other.tint, tint);

  @override
  int get hashCode => Object.hash(
    grid,
    hatch,
    shade,
    lift,
    barrier,
    gap,
    cap,
    label,
    fault,
    shakeDx,
    Object.hashAll(reach),
    Object.hashAll(tint),
  );
}

/// The pure transition machine. No Flutter widget, no ticker, no clock of its
/// own: the layer feeds it elapsed time and it returns a [ChartFrame].
///
/// Because it is pure, the transition tables are golden testable: for every
/// from/to pair `frameAt(startedAt + duration)` equals the target state's
/// settled frame exactly, which is what stops a future edit from leaving a
/// state mid transition forever.
class AtmosphereSolver {
  AtmoState _from;
  AtmoState _to;
  Duration _startedAt = Duration.zero;
  Duration _duration = Duration.zero;
  Duration _shakeAt = const Duration(days: -1);
  bool _reduceMotion;

  /// Wall clock phase offset for the probe loop. On resume it is rebased from
  /// how long the real handshake has actually been running, so returning to the
  /// app does not rewind the animation to where it was when the screen went off.
  Duration _phaseBase = Duration.zero;

  AtmosphereSolver({
    AtmoState initial = AtmoState.disconnected,
    bool reduceMotion = false,
  }) : _from = initial,
       _to = initial,
       _reduceMotion = reduceMotion;

  AtmoState get state => _to;
  AtmoState get previousState => _from;
  bool get reduceMotion => _reduceMotion;

  /// True when the state being held animates forever until it changes.
  bool get isStaticState => _reduceMotion || _defs[_to]!.probe == null;

  set reduceMotion(bool value) {
    if (_reduceMotion == value) return;
    _reduceMotion = value;
    _duration = Duration.zero;
  }

  /// Enter [next] at [at] on the layer's own clock.
  void transitionTo(AtmoState next, {required Duration at}) {
    if (next == _to) return;
    _from = _to;
    _to = next;
    _startedAt = at;
    _duration = _reduceMotion
        ? kAtmoReduceMotionFade
        : Duration(milliseconds: kAtmoDurationMs[next]!);
    if (next == AtmoState.error && !_reduceMotion) {
      _shakeAt = at;
    } else {
      _shakeAt = const Duration(days: -1);
    }
    if (_defs[next]!.probe != null) _phaseBase = at;
  }

  /// Recompute the probe phase against how long the real handshake has been
  /// running, so a resume from background lands where the tunnel actually is.
  void rebasePhase(Duration elapsedSinceConnectStart, {required Duration now}) {
    _phaseBase = now - elapsedSinceConnectStart;
  }

  bool isSettled(Duration now) {
    final p = _duration > Duration.zero
        ? (now - _startedAt).inMicroseconds / _duration.inMicroseconds
        : 1.0;
    if (p < 1) return false;
    return (now - _shakeAt).inMilliseconds > kAtmoShakeMs;
  }

  /// The settled composition of the current state, with no transition in
  /// flight. Used by the reduce-motion path and by the golden tests.
  ChartFrame settledFrame({Duration now = Duration.zero}) {
    final def = _defs[_to]!;
    final reach = <double>[
      for (var i = 0; i < kAtmoRouteCount; i++) _reachOf(def, i, now),
    ];
    final color = _tintColor(def.tint);
    return ChartFrame(
      grid: def.grid,
      hatch: def.hatch,
      shade: def.shade,
      lift: def.lift,
      barrier: def.barrier,
      gap: def.gap,
      cap: def.cap,
      label: def.label,
      fault: def.fault,
      reach: reach,
      tint: List<Color>.filled(kAtmoRouteCount, color),
      shakeDx: 0,
    );
  }

  /// Tokens are needed to resolve a [RouteTint] into a real color. The layer
  /// sets this whenever the theme changes.
  AtmosphereTokens tokens = AtmosphereTokens.dark();

  Color _tintColor(RouteTint t) => switch (t) {
    RouteTint.idle => tokens.routeIdle,
    RouteTint.probe => tokens.routeProbe,
    RouteTint.live => tokens.routeLive,
  };

  double _reachOf(_StateDef def, int route, Duration now) {
    final probe = def.probe;
    if (probe == null) return def.reach;
    if (_reduceMotion) return (probe.lo + probe.hi) / 2;
    final ms = (now - _phaseBase).inMicroseconds / 1000.0;
    var phase = (ms / probe.periodMs + route * 0.11) % 1.0;
    if (phase < 0) phase += 1;
    if (phase < 0.62) {
      return probe.lo + (probe.hi - probe.lo) * _easeInOutSine(phase / 0.62);
    }
    if (phase < 0.72) return probe.hi;
    return probe.hi -
        (probe.hi - probe.lo) * _easeInOutSine((phase - 0.72) / 0.28);
  }

  /// Staggered progress. Opening runs inside out (nearest station first),
  /// closing runs outside in, so a disconnect visibly draws back to the thumb.
  double _staggerProg(double p, int rank, bool opening) {
    const n = kAtmoRouteCount;
    final w = opening ? 0.07 : 0.045;
    final r = opening ? rank : (n - 1 - rank);
    final span = 1 - w * (n - 1);
    return _clamp01((p - r * w) / span);
  }

  /// Pure: `(from, to, elapsed) -> ChartFrame`.
  ChartFrame frameAt(Duration now) {
    if (_reduceMotion) return settledFrame(now: now);

    final from = _defs[_from]!;
    final to = _defs[_to]!;
    final p = _duration > Duration.zero
        ? _clamp01((now - _startedAt).inMicroseconds / _duration.inMicroseconds)
        : 1.0;
    final pe = _easeInOut(p);

    double mix(double a, double b) => a + (b - a) * pe;

    final cFrom = _tintColor(from.tint);
    final cTo = _tintColor(to.tint);

    final reach = List<double>.filled(kAtmoRouteCount, 0);
    final tint = List<Color>.filled(kAtmoRouteCount, cTo);
    for (var i = 0; i < kAtmoRouteCount; i++) {
      final rank = kAtmoOpenRank[i];
      final double k;
      if (_to == AtmoState.connected) {
        k = _easeOutCubic(_staggerProg(p, rank, true));
      } else if (_from == AtmoState.connected) {
        k = _easeOutCubic(_staggerProg(p, rank, false));
      } else if (_to == AtmoState.error) {
        k = _easeInCubic(p);
      } else {
        k = pe;
      }
      final rF = _reachOf(from, i, now);
      final rT = _reachOf(to, i, now);
      reach[i] = _clamp01(rF + (rT - rF) * k);
      tint[i] = Color.lerp(cFrom, cTo, k)!;
    }

    var shakeDx = 0.0;
    final sinceShake = (now - _shakeAt).inMilliseconds;
    if (sinceShake >= 0 && sinceShake < kAtmoShakeMs) {
      final t = sinceShake / kAtmoShakeMs;
      shakeDx = math.sin(t * math.pi * 3) * (1 - t) * 5;
    }

    return ChartFrame(
      grid: mix(from.grid, to.grid),
      hatch: mix(from.hatch, to.hatch),
      shade: mix(from.shade, to.shade),
      lift: mix(from.lift, to.lift),
      barrier: mix(from.barrier, to.barrier),
      gap: mix(from.gap, to.gap),
      cap: mix(from.cap, to.cap),
      label: mix(from.label, to.label),
      fault: mix(from.fault, to.fault),
      reach: reach,
      tint: tint,
      shakeDx: shakeDx,
    );
  }
}

/// Curves used by the choreography, exported so DESIGN.md's motion table and
/// the code cannot drift apart.
abstract final class AtmoCurves {
  static const Curve fieldMorph = Curves.easeInOutCubic;
  static const Curve extend = Curves.easeOutCubic;
  static const Curve slam = Curves.easeInCubic;
}

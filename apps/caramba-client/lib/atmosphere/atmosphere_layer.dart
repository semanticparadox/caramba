import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';

import 'package:caramba_client/atmosphere/atmosphere_painter.dart';
import 'package:caramba_client/atmosphere/atmosphere_state.dart';
import 'package:caramba_client/atmosphere/atmosphere_tokens.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Where the chart is registered to the screen. The chart's home station IS the
/// connect dial, and the boundary encloses the dial plus the connect block, so
/// both come from the real layout rather than from constants.
@immutable
class AtmosphereAnchor {
  /// Dial centre, in the coordinate space of the atmosphere layer.
  final Offset? dialCenter;

  /// The state label plus the sub line, in the same space.
  final Rect? labelRect;

  /// Bottom of the screen header (wordmark row). The boundary never rises
  /// above it.
  final double? headerBottom;

  const AtmosphereAnchor({this.dialCenter, this.labelRect, this.headerBottom});

  static const AtmosphereAnchor unmeasured = AtmosphereAnchor();

  /// Fallback used for the first frame, before the dial has been laid out.
  /// Mirrors Home's vertical rhythm: 20 list padding, 44 title row, 16 gap,
  /// then the 196px dial.
  Offset resolvedCenter(Size size, double topInset) =>
      dialCenter ?? Offset(size.width / 2, topInset + 20 + 44 + 32 + 98);

  double resolvedHeaderBottom(double topInset) =>
      headerBottom ?? topInset + 20 + 44;

  Rect resolvedLabelRect(Size size, double topInset) {
    final rect = labelRect;
    if (rect != null) return rect;
    final c = resolvedCenter(size, topInset);
    final w = (size.width - 48).clamp(160.0, 340.0);
    final top = c.dy + 98 + 16;
    return Rect.fromLTWH(c.dx - w / 2, top, w, 48);
  }

  @override
  bool operator ==(Object other) =>
      other is AtmosphereAnchor &&
      other.dialCenter == dialCenter &&
      other.labelRect == labelRect &&
      other.headerBottom == headerBottom;

  @override
  int get hashCode => Object.hash(dialCenter, labelRect, headerBottom);
}

/// The atmosphere layer: a survey chart of the network the client can reach,
/// with the connect dial as its home station.
///
/// Off, the chart is closed: a boundary is drawn around you and every route out
/// is capped at a dead end. On, the chart is open: the boundary is gone, the
/// graticule is visible, and every route runs unbroken to its station.
///
/// The layer is decorative and is wrapped in [ExcludeSemantics]. State is
/// already reported by the dial ring, the glyph and the live region on the
/// connect label; nothing here is the only carrier of information.
class AtmosphereLayer extends StatefulWidget {
  final VpnStage stage;
  final AtmosphereAnchor anchor;

  /// 1.0 on Home. Servers and Settings can run the same geometry dimmed.
  final double strength;

  const AtmosphereLayer({
    required this.stage,
    this.anchor = AtmosphereAnchor.unmeasured,
    this.strength = 1,
    super.key,
  });

  static AtmoState stateOf(VpnStage stage) => switch (stage) {
    VpnStage.disconnected => AtmoState.disconnected,
    VpnStage.connecting => AtmoState.connecting,
    VpnStage.connected => AtmoState.connected,
    VpnStage.reconnecting => AtmoState.reconnecting,
    VpnStage.error => AtmoState.error,
  };

  @override
  State<AtmosphereLayer> createState() => AtmosphereLayerState();
}

class AtmosphereLayerState extends State<AtmosphereLayer>
    with SingleTickerProviderStateMixin, WidgetsBindingObserver {
  late final AtmosphereSolver _solver;
  late final ChartFrames _frames;
  Ticker? _ticker;

  ChartGeometry? _geo;

  /// The layer's own monotonic clock. A [Ticker] restarts its elapsed time at
  /// zero every time it is started, so the offset is carried across stops.
  Duration _elapsed = Duration.zero;
  Duration _tickerOffset = Duration.zero;

  /// Reset on every state change so the first frame after a transition always
  /// paints. Without this, a state entered less than one frame gap after the
  /// previous tick shows the OLD composition until the gate opens.
  Duration _lastFrame = const Duration(days: -1);

  DateTime? _probeStartedAt;
  bool _resumed = true;
  AtmosphereBudget _budget = AtmosphereBudget.normal;
  bool _reduceMotion = false;
  AtmosphereTokens _tokens = AtmosphereTokens.dark();

  // Reduce-motion path: one frozen composition per state, cross faded.
  ChartFrames? _snapshot;
  AtmoState? _snapshotState;

  // Debug frame accounting (verdict item 9): shout in debug builds if the layer
  // paints faster than its own cap.
  int _paintsThisSecond = 0;
  Duration _secondMark = Duration.zero;

  @visibleForTesting
  bool get debugIsTicking => _ticker?.isTicking ?? false;

  @visibleForTesting
  bool get debugHasTicker => _ticker != null;

  @visibleForTesting
  ChartFrame get debugFrame => _frames.frame;

  @override
  void initState() {
    super.initState();
    _solver = AtmosphereSolver(initial: AtmosphereLayer.stateOf(widget.stage));
    _frames = ChartFrames(_solver.settledFrame());
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final media = MediaQuery.maybeOf(context);
    _reduceMotion = media?.disableAnimations ?? false;
    _budget = AtmosphereBudgetScope.of(context);
    _tokens = AtmosphereTokens.of(context);
    _solver
      ..tokens = _tokens
      ..reduceMotion = _reduceMotion;
    if (_reduceMotion) {
      _stopTicker();
      _frames.push(_solver.settledFrame(now: _elapsed));
    } else {
      _frames.push(_solver.frameAt(_elapsed));
      _kick();
    }
  }

  @override
  void didUpdateWidget(covariant AtmosphereLayer oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.stage != oldWidget.stage) {
      final next = AtmosphereLayer.stateOf(widget.stage);
      _solver.transitionTo(next, at: _elapsed);
      _probeStartedAt = _solver.isStaticState ? null : DateTime.now();
      // Verdict item 9: the first frame after a state change always paints.
      _lastFrame = const Duration(days: -1);
      _frames.push(
        _reduceMotion
            ? _solver.settledFrame(now: _elapsed)
            : _solver.frameAt(_elapsed),
      );
      _kick();
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    final resumed = state == AppLifecycleState.resumed;
    if (resumed == _resumed) return;
    _resumed = resumed;
    if (!resumed) {
      _stopTicker();
      if (mounted) setState(() {});
      return;
    }
    // Recompute the probe phase against how long the real handshake has been
    // running, so coming back does not rewind the animation.
    final started = _probeStartedAt;
    if (started != null) {
      _solver.rebasePhase(DateTime.now().difference(started), now: _elapsed);
    }
    _lastFrame = const Duration(days: -1);
    _kick();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _ticker?.dispose();
    _frames.dispose();
    super.dispose();
  }

  /// Start the ticker if there is work: a transition in flight, or a state that
  /// animates. Never under reduce motion, never while backgrounded.
  void _kick() {
    if (_reduceMotion || !_resumed || widget.strength <= 0) {
      _stopTicker();
      return;
    }
    if (_solver.isSettled(_elapsed) && _solver.isStaticState) return;
    final ticker = _ticker ??= createTicker(_onTick);
    if (!ticker.isTicking) {
      _tickerOffset = _elapsed;
      ticker.start();
      if (mounted) {
        // Flips willChange to true. Runs once per transition, not per frame.
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) setState(() {});
        });
      }
    }
  }

  void _stopTicker() {
    final ticker = _ticker;
    if (ticker != null && ticker.isTicking) ticker.stop();
  }

  void _onTick(Duration raw) {
    final elapsed = raw + _tickerOffset;
    _elapsed = elapsed;
    if (elapsed - _lastFrame < _budget.minFrameGap) return;
    _lastFrame = elapsed;
    _frames.push(_solver.frameAt(elapsed));

    assert(() {
      if (elapsed - _secondMark >= const Duration(seconds: 1)) {
        if (_paintsThisSecond > _budget.targetFps + 2) {
          debugPrint(
            'AtmosphereLayer painted $_paintsThisSecond frames in a second, '
            'over the ${_budget.targetFps}fps cap.',
          );
        }
        _secondMark = elapsed;
        _paintsThisSecond = 0;
      }
      _paintsThisSecond++;
      return true;
    }());

    if (_solver.isSettled(elapsed) && _solver.isStaticState) {
      _stopTicker();
      if (mounted) setState(() {}); // flips willChange back to false
    }
  }

  ChartGeometry _geometry(Size size, double topInset) {
    final home = widget.anchor.resolvedCenter(size, topInset);
    final label = widget.anchor.resolvedLabelRect(size, topInset);
    final header = widget.anchor.resolvedHeaderBottom(topInset);
    final cached = _geo;
    if (cached != null &&
        cached.matches(
          size: size,
          home: home,
          labelRect: label,
          topInset: topInset,
          headerBottom: header,
          tokens: _tokens,
        )) {
      return cached;
    }
    return _geo = ChartGeometry.build(
      size: size,
      home: home,
      labelRect: label,
      topInset: topInset,
      headerBottom: header,
      tokens: _tokens,
    );
  }

  @override
  Widget build(BuildContext context) {
    final topInset = MediaQuery.viewPaddingOf(context).top;
    final dpr = MediaQuery.maybeOf(context)?.devicePixelRatio ?? 1;
    final running = _ticker?.isTicking ?? false;

    Widget paint(ChartFrames source, {required bool listen}) => LayoutBuilder(
      builder: (context, constraints) {
        final size = Size(constraints.maxWidth, constraints.maxHeight);
        return CustomPaint(
          size: size,
          isComplex: true,
          willChange: running,
          painter: ChartPainter(
            geo: _geometry(size, topInset),
            tokens: _tokens,
            frames: source,
            strength: widget.strength,
            revision: source.revision,
            devicePixelRatio: dpr,
            listen: listen,
          ),
        );
      },
    );

    // Reduce motion is a genuinely different rendering path, not the same
    // animation slowed down: three static compositions, opacity cross faded,
    // no ticker of our own and no geometry in flight. Each composition holds a
    // frozen frame so the outgoing one keeps showing what it showed.
    final Widget child;
    if (_reduceMotion) {
      if (_snapshotState != _solver.state || _snapshot == null) {
        _snapshotState = _solver.state;
        _snapshot = ChartFrames(_solver.settledFrame(now: _elapsed));
      }
      child = AnimatedSwitcher(
        duration: kAtmoReduceMotionFade,
        switchInCurve: Curves.easeOutCubic,
        switchOutCurve: Curves.easeOutCubic,
        child: KeyedSubtree(
          key: ValueKey<AtmoState>(_solver.state),
          child: paint(_snapshot!, listen: false),
        ),
      );
    } else {
      child = paint(_frames, listen: true);
    }

    return ExcludeSemantics(
      child: RepaintBoundary(
        child: ColoredBox(color: _tokens.basePlane, child: child),
      ),
    );
  }
}

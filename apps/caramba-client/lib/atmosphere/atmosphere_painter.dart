import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/rendering.dart';

import 'package:caramba_client/atmosphere/atmosphere_state.dart';
import 'package:caramba_client/atmosphere/atmosphere_tokens.dart';
import 'package:caramba_client/theme/typography.dart';

/// One plotted course: an orthogonal polyline with 45 degree elbows that leaves
/// the dial ring, crosses the boundary, and ends at a station or at the frame.
@immutable
class RouteGeom {
  final List<Offset> pts;

  /// Cumulative arc length, `cum.last == total`.
  final List<double> cum;
  final double total;

  /// Arc length at the first exit from the boundary, and the point there.
  final double crossLen;
  final Offset crossPt;

  /// Unit direction of travel at the crossing. The dead end cap and the fault
  /// cross are built off its perpendicular.
  final Offset crossDir;

  /// Where the boundary opens for this route, as an arc length on the boundary.
  final double gapAt;

  /// Elbow vertices outside the boundary: the relay hops.
  final List<Offset> waypoints;

  /// Mono two letter code, or null. Capped at three, connected only.
  final String? code;

  /// Code is drawn to the right of the station rather than to its left.
  final bool codeRight;

  /// The route leaves the frame instead of ending at a station.
  final bool edgeTie;

  /// Ignition order, nearest station first.
  final int openRank;

  const RouteGeom({
    required this.pts,
    required this.cum,
    required this.total,
    required this.crossLen,
    required this.crossPt,
    required this.crossDir,
    required this.gapAt,
    required this.waypoints,
    required this.code,
    required this.codeRight,
    required this.edgeTie,
    required this.openRank,
  });

  Offset get end => pts.last;
}

/// A single point on a polyline plus the direction of travel there.
class _PointOn {
  final Offset p;
  final Offset dir;
  const _PointOn(this.p, this.dir);
}

/// The whole chart as fixed geometry. Built once per
/// `(size, dialCentre, labelRect, topInset)` and never touched again: nothing
/// on this chart ever changes position, in any state.
class ChartGeometry {
  final Size size;

  /// The dial centre in layer local coordinates. The chart's home station.
  final Offset home;

  /// The connect block (state label plus sub line), layer local. The boundary
  /// bottom and the quiet lens are both derived from it rather than from a
  /// constant, so a text scale change cannot crop the sub line.
  final Rect labelRect;

  /// Top safe area inset. No atmosphere line work is painted above it.
  final double topInset;

  /// Bottom of the screen header (wordmark row). The boundary never rises above
  /// it, so the octagon cannot cut through the brand line at any text scale.
  final double headerBottom;

  final AtmosphereTokens tokens;

  /// Closed boundary polyline (last point repeats the first).
  final List<Offset> barrierPts;
  final List<double> barrierCum;
  final double barrierLen;

  /// The boundary as a path, for the even odd clip of the restricted field.
  final Path barrierPath;

  /// The region outside the boundary, even odd.
  final Path outsidePath;

  final List<RouteGeom> routes;

  /// Boundary gap arc lengths, sorted, so stroking skips ranges in one walk.
  final List<double> gapCuts;

  final Path gridMinorPath;
  final Path gridMajorPath;
  final Path hatchPath;

  /// Quiet lens core, in the base plane. Feathered outward by the painter.
  final RRect lensCore;

  final Paint _shadePaint = Paint();
  final Map<int, List<TextPainter>> _labelCache = <int, List<TextPainter>>{};

  ChartGeometry._({
    required this.size,
    required this.home,
    required this.labelRect,
    required this.topInset,
    required this.headerBottom,
    required this.tokens,
    required this.barrierPts,
    required this.barrierCum,
    required this.barrierLen,
    required this.barrierPath,
    required this.outsidePath,
    required this.routes,
    required this.gapCuts,
    required this.gridMinorPath,
    required this.gridMajorPath,
    required this.hatchPath,
    required this.lensCore,
  });

  // ---------------------------------------------------------------------
  // Design space. Authored at 390x844 with the home station at (195, 240);
  // every value below is relative to the home station, so the chart follows
  // the real dial instead of assuming a phone size.
  // ---------------------------------------------------------------------

  /// Where routes leave the dial: ring radius 98 plus a 6px gap.
  static const double ringOffset = 104;

  /// Gutter inset of the two routes that run down the sides and off frame.
  static const double gutterInset = 14;

  static const List<
    ({List<Offset> pts, String? code, bool codeRight, bool tie})
  >
  _routeSpec = [
    // NL, upper left
    (
      pts: [
        Offset(-74, -74),
        Offset(-92, -92),
        Offset(-92, -116),
        Offset(-149, -116),
      ],
      code: 'NL',
      codeRight: true,
      tie: false,
    ),
    // top edge tie
    (
      pts: [
        Offset(0, -104),
        Offset(0, -120),
        Offset(24, -144),
        Offset(24, -182),
      ],
      code: null,
      codeRight: false,
      tie: true,
    ),
    // DE, upper right. This is the route that reports the failure.
    (
      pts: [
        Offset(74, -74),
        Offset(92, -92),
        Offset(92, -114),
        Offset(157, -114),
      ],
      code: 'DE',
      codeRight: false,
      tie: false,
    ),
    // US, right
    (
      pts: [Offset(104, 0), Offset(136, 0), Offset(162, 26)],
      code: 'US',
      codeRight: false,
      tie: false,
    ),
    // SE, left. Station drawn, code cut per the connected mark budget.
    (
      pts: [Offset(-104, 0), Offset(-136, 0), Offset(-164, 28)],
      code: null,
      codeRight: true,
      tie: false,
    ),
    // JP, lower right. Station drawn, code cut.
    (
      pts: [Offset(74, 74), Offset(92, 92), Offset(157, 92)],
      code: null,
      codeRight: false,
      tie: false,
    ),
    // left gutter rail, off the bottom edge
    (
      pts: [
        Offset(-74, 74),
        Offset(-92, 92),
        Offset(-92, 230),
        Offset(-181, 230),
        Offset(-181, 620),
      ],
      code: null,
      codeRight: false,
      tie: true,
    ),
    // right gutter rail, off the bottom edge
    (
      pts: [
        Offset(0, 104),
        Offset(18, 122),
        Offset(18, 260),
        Offset(181, 260),
        Offset(181, 620),
      ],
      code: null,
      codeRight: false,
      tie: true,
    ),
  ];

  /// The route that carries the fault cross on error: DE, the upper right one.
  static const int faultRoute = 2;

  static ChartGeometry build({
    required Size size,
    required Offset home,
    required Rect labelRect,
    required double topInset,
    required AtmosphereTokens tokens,
    double? headerBottom,
  }) {
    final header = headerBottom ?? topInset + 64;
    // ---- boundary octagon. Both horizontal edges come from the laid-out
    // screen, not from a constant (verdict must-fix 3): the top clears the
    // header, the bottom encloses the connect block at any text scale.
    final hw = tokens.barrierHw;
    final cut = tokens.barrierCut;
    final top = math.min(
      math.max(home.dy - tokens.barrierRise, header + 8),
      home.dy - ringOffset - 8,
    );
    final bottom = math.max(
      home.dy + ringOffset + 8,
      labelRect.bottom + tokens.barrierDrop,
    );
    final cx = home.dx;
    final pts = <Offset>[
      Offset(cx - hw + cut, top),
      Offset(cx + hw - cut, top),
      Offset(cx + hw, top + cut),
      Offset(cx + hw, bottom - cut),
      Offset(cx + hw - cut, bottom),
      Offset(cx - hw + cut, bottom),
      Offset(cx - hw, bottom - cut),
      Offset(cx - hw, top + cut),
    ];
    final closed = <Offset>[...pts, pts.first];
    final barrierCum = _cumOf(closed);
    final barrierLen = barrierCum.last;

    final barrierPath = Path()..addPolygon(pts, true);
    final outsidePath = Path()
      ..fillType = PathFillType.evenOdd
      ..addRect(Rect.fromLTWH(-40, -40, size.width + 80, size.height + 80))
      ..addPath(barrierPath, Offset.zero);

    // ---- routes
    final routes = <RouteGeom>[];
    final lengths = <double>[];
    final raw = <List<Offset>>[];
    for (var i = 0; i < _routeSpec.length; i++) {
      final spec = _routeSpec[i];
      final p = [for (final o in spec.pts) home + o];
      // Frame anchored fixups: the two gutter rails hug the frame edges and run
      // off the bottom, the top tie stops clear of the status bar.
      if (i == 6) {
        p[3] = Offset(gutterInset, p[3].dy);
        p[4] = Offset(gutterInset, size.height + 40);
      } else if (i == 7) {
        p[3] = Offset(size.width - gutterInset, p[3].dy);
        p[4] = Offset(size.width - gutterInset, size.height + 40);
      } else if (i == 1) {
        p[3] = Offset(p[3].dx, math.max(p[3].dy, topInset + 12));
        p[2] = Offset(p[2].dx, math.max(p[2].dy, p[3].dy + 20));
        p[1] = Offset(p[1].dx, math.max(p[1].dy, p[2].dy + 8));
      }
      raw.add(p);
      lengths.add(_cumOf(p).last);
    }
    // Ignition order, nearest station first.
    final order = List<int>.generate(raw.length, (i) => i)
      ..sort((a, b) => lengths[a].compareTo(lengths[b]));
    final rank = List<int>.filled(raw.length, 0);
    for (var i = 0; i < order.length; i++) {
      rank[order[i]] = i;
    }
    assert(
      listEquals(rank, kAtmoOpenRank),
      'Route ignition order drifted from kAtmoOpenRank: $rank',
    );

    for (var i = 0; i < raw.length; i++) {
      final spec = _routeSpec[i];
      final p = raw[i];
      final cum = _cumOf(p);
      final total = cum.last;

      // First exit from the boundary, sampled at 1px like the reference.
      var crossLen = total;
      var cross = _pointAt(p, cum, total);
      for (var s = 0.0; s <= total; s += 1) {
        final q = _pointAt(p, cum, s);
        if (!_inPoly(q.p, pts)) {
          crossLen = s;
          cross = q;
          break;
        }
      }

      // Elbow vertices outside the boundary become relay hops.
      final way = <Offset>[
        for (var k = 1; k < p.length - 1; k++)
          if (!_inPoly(p[k], pts)) p[k],
      ];

      // Project the crossing onto the boundary to find where it opens.
      var gapAt = 0.0;
      var best = double.infinity;
      for (var k = 0; k < closed.length - 1; k++) {
        final a = closed[k];
        final b = closed[k + 1];
        final v = b - a;
        final l2 = v.dx * v.dx + v.dy * v.dy;
        var t = l2 == 0
            ? 0.0
            : ((cross.p.dx - a.dx) * v.dx + (cross.p.dy - a.dy) * v.dy) / l2;
        t = t.clamp(0.0, 1.0);
        final proj = a + v * t;
        final d = (cross.p - proj).distance;
        if (d < best) {
          best = d;
          gapAt = barrierCum[k] + (proj - a).distance;
        }
      }

      routes.add(
        RouteGeom(
          pts: p,
          cum: cum,
          total: total,
          crossLen: crossLen,
          crossPt: cross.p,
          crossDir: cross.dir,
          gapAt: gapAt,
          waypoints: way,
          code: spec.code,
          codeRight: spec.codeRight,
          edgeTie: spec.tie,
          openRank: rank[i],
        ),
      );
    }

    final gapCuts = [for (final r in routes) r.gapAt]..sort();

    // ---- graticule, registered so one major line passes through the dial
    final minor = Path();
    final major = Path();
    final pitch = tokens.gridPitch;
    final every = tokens.gridMajorEvery;
    final kx = (size.width / pitch).ceil() + 2;
    final ky = (size.height / pitch).ceil() + 2;
    for (var k = -kx; k <= kx; k++) {
      final x = (home.dx + k * pitch).roundToDouble() + 0.5;
      if (x < -2 || x > size.width + 2) continue;
      final target = k % every == 0 ? major : minor;
      target
        ..moveTo(x, 0)
        ..lineTo(x, size.height);
    }
    for (var k = -ky; k <= ky; k++) {
      final y = (home.dy + k * pitch).roundToDouble() + 0.5;
      if (y < -2 || y > size.height + 2) continue;
      final target = k % every == 0 ? major : minor;
      target
        ..moveTo(0, y)
        ..lineTo(size.width, y);
    }

    // ---- 45 degree restricted hatch. Never animated, only cross faded, and
    // the phase is anchored to whole logical pixels so it cannot shimmer.
    final hatch = Path();
    final step = tokens.hatchPitch * math.sqrt2;
    final span = size.height + 80;
    final first = (-(span) / step).floor() * step;
    for (var d = first; d < size.width + 80; d += step) {
      hatch
        ..moveTo(d, -40)
        ..lineTo(d + span, size.height + 40);
    }

    // ---- quiet lens core: the label block plus a margin, painted back in the
    // base plane. The painter feathers outward from this rect.
    final lensCoreRect = labelRect.inflate(tokens.lensPad * 0.5);
    final lensCore = RRect.fromRectAndRadius(
      lensCoreRect,
      Radius.circular(lensCoreRect.height / 2),
    );

    return ChartGeometry._(
      size: size,
      home: home,
      labelRect: labelRect,
      topInset: topInset,
      headerBottom: header,
      tokens: tokens,
      barrierPts: closed,
      barrierCum: barrierCum,
      barrierLen: barrierLen,
      barrierPath: barrierPath,
      outsidePath: outsidePath,
      routes: routes,
      gapCuts: gapCuts,
      gridMinorPath: minor,
      gridMajorPath: major,
      hatchPath: hatch,
      lensCore: lensCore,
    );
  }

  bool matches({
    required Size size,
    required Offset home,
    required Rect labelRect,
    required double topInset,
    required double headerBottom,
    required AtmosphereTokens tokens,
  }) =>
      this.size == size &&
      this.home == home &&
      this.labelRect == labelRect &&
      this.topInset == topInset &&
      this.headerBottom == headerBottom &&
      this.tokens == tokens;

  /// Station code painters, laid out once per quantised alpha. Text is never
  /// laid out inside `paint`.
  List<TextPainter> labelPainters(Color color) {
    final key = color.toARGB32();
    final cached = _labelCache[key];
    if (cached != null) return cached;
    final built = <TextPainter>[];
    for (final r in routes) {
      if (r.code == null) continue;
      built.add(
        TextPainter(
          text: TextSpan(
            text: r.code,
            style: AppType.monoSm.copyWith(fontSize: 9, color: color),
          ),
          textDirection: TextDirection.ltr,
        )..layout(),
      );
    }
    if (_labelCache.length > 20) _labelCache.clear();
    _labelCache[key] = built;
    return built;
  }

  Paint get shadePaint => _shadePaint;

  static List<double> _cumOf(List<Offset> pts) {
    final c = <double>[0];
    for (var i = 1; i < pts.length; i++) {
      c.add(c[i - 1] + (pts[i] - pts[i - 1]).distance);
    }
    return c;
  }

  static _PointOn _pointAt(List<Offset> pts, List<double> cum, double s) {
    final clamped = s.clamp(0.0, cum.last);
    for (var i = 0; i < pts.length - 1; i++) {
      if (clamped <= cum[i + 1] || i == pts.length - 2) {
        final span = cum[i + 1] - cum[i];
        final f = span == 0 ? 0.0 : (clamped - cum[i]) / span;
        final d = pts[i + 1] - pts[i];
        final len = d.distance;
        return _PointOn(
          pts[i] + d * f,
          len == 0 ? const Offset(1, 0) : d / len,
        );
      }
    }
    return _PointOn(pts.first, const Offset(1, 0));
  }

  static bool _inPoly(Offset p, List<Offset> poly) {
    var inside = false;
    for (var i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      final a = poly[i];
      final b = poly[j];
      if ((a.dy > p.dy) != (b.dy > p.dy) &&
          p.dx < (b.dx - a.dx) * (p.dy - a.dy) / (b.dy - a.dy) + a.dx) {
        inside = !inside;
      }
    }
    return inside;
  }
}

/// The live frame, and the painter's repaint [Listenable] in one object.
///
/// The layer pushes a new [ChartFrame] on every gated tick; the painter reads
/// it inside `paint`, so an animation frame repaints ONLY the CustomPaint and
/// never runs the build phase.
class ChartFrames extends ChangeNotifier {
  ChartFrame _frame;
  int _revision = 0;

  ChartFrames(this._frame);

  ChartFrame get frame => _frame;
  int get revision => _revision;

  void push(ChartFrame next) {
    _frame = next;
    _revision++;
    notifyListeners();
  }
}

/// Draws one [ChartFrame] of the chart. Roughly 40 stroke and fill operations
/// over flat geometry: no `saveLayer`, no blur, no shadow, no image filter.
class ChartPainter extends CustomPainter {
  final ChartGeometry geo;
  final AtmosphereTokens tokens;
  final ChartFrames frames;

  /// 1.0 on Home, lower elsewhere. Multiplies every alpha in the layer.
  final double strength;

  /// Bumped by the layer on every frame it wants painted. `shouldRepaint` is a
  /// value comparison and never returns a bare true.
  final int revision;

  /// Device pixel ratio, used to snap translations so the hatch cannot alias.
  final double devicePixelRatio;

  ChartPainter({
    required this.geo,
    required this.tokens,
    required this.frames,
    required this.strength,
    required this.revision,
    required this.devicePixelRatio,
    bool listen = true,
  }) : super(repaint: listen ? frames : null);

  final Path _scratch = Path();

  ChartFrame get frame => frames.frame;

  Color _a(Color base, double mul) {
    final v = (base.a * mul * strength).clamp(0.0, 1.0);
    return base.withValues(alpha: v);
  }

  double _snap(double v) {
    if (devicePixelRatio <= 0) return v;
    return (v * devicePixelRatio).roundToDouble() / devicePixelRatio;
  }

  @override
  void paint(Canvas canvas, Size size) {
    final full = Offset.zero & size;

    // 1. The base plane. Painted untranslated and unclipped so the whole layer
    //    is opaque and the composite under text is deterministic.
    canvas.drawRect(full, Paint()..color = tokens.basePlane);

    if (strength <= 0) return;

    // 2. Nothing above the status bar inset (verdict must fix 6). The system
    //    draws the clock and the battery there and we do not control them.
    canvas.save();
    canvas.clipRect(Rect.fromLTRB(0, geo.topInset, size.width, size.height));
    canvas.translate(_snap(frame.shakeDx), 0);

    _paintLift(canvas, size);
    _paintGraticule(canvas);
    _paintRestrictedField(canvas, size);
    _paintRoutes(canvas);
    // The lens clears the graticule, the hatch and the routes behind the
    // connect block. The boundary is drawn AFTER it and simply skipped where it
    // would cross the words, the same treatment the route crossings get, so the
    // "you are enclosed" mark survives at full strength right under the label.
    _paintQuietLens(canvas);
    _paintBarrier(canvas, size);
    _paintFault(canvas);
    _paintMarks(canvas);

    canvas.restore();

    // 3. Feather the top edge back into the base plane so the clip above does
    //    not leave a visible seam under the status bar.
    _paintTopFeather(canvas, size);
  }

  void _paintLift(Canvas canvas, Size size) {
    if (frame.lift <= 0.001 || tokens.lift.a == 0) return;
    canvas.drawRect(
      Offset.zero & size,
      Paint()..color = _a(tokens.lift, frame.lift),
    );
  }

  void _paintGraticule(Canvas canvas) {
    if (frame.grid <= 0.001) return;
    final p = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1;
    canvas.drawPath(geo.gridMinorPath, p..color = _a(tokens.grid, frame.grid));
    canvas.drawPath(
      geo.gridMajorPath,
      p..color = _a(tokens.gridMajor, frame.grid),
    );
  }

  void _paintRestrictedField(Canvas canvas, Size size) {
    if (frame.shade <= 0.001 && frame.hatch <= 0.001) return;
    canvas.save();
    canvas.clipPath(geo.outsidePath);
    if (frame.shade > 0.001) {
      final r = math.max(size.width, size.height) * 0.72 + 200;
      geo.shadePaint.shader = ui.Gradient.radial(
        geo.home,
        r,
        <Color>[
          tokens.shade.withValues(alpha: 0),
          _a(tokens.shade, frame.shade),
        ],
        <double>[0.25, 1.0],
      );
      canvas.drawRect(
        Rect.fromLTWH(-40, -40, size.width + 80, size.height + 80),
        geo.shadePaint,
      );
    }
    if (frame.hatch > 0.001) {
      canvas.drawPath(
        geo.hatchPath,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1
          ..color = _a(tokens.hatch, frame.hatch),
      );
    }
    canvas.restore();
  }

  void _paintRoutes(Canvas canvas) {
    final ghost = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.2
      ..strokeCap = StrokeCap.butt;
    final live = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.5
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round;
    final cap = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.6
      ..strokeCap = StrokeCap.butt;

    for (var i = 0; i < geo.routes.length; i++) {
      final r = geo.routes[i];
      final reach = frame.reach[i];
      final lit = r.crossLen + (r.total - r.crossLen) * reach.clamp(0.0, 1.0);

      if (reach < 0.985) {
        _scratch.reset();
        _dashRange(_scratch, r, lit, r.total, on: 2, off: 4);
        canvas.drawPath(
          _scratch,
          ghost..color = _a(tokens.routeGhost, 1 - reach),
        );
      }

      _scratch.reset();
      _strokeRange(_scratch, r.pts, r.cum, 0, lit);
      canvas.drawPath(_scratch, live..color = _a(frame.tint[i], 1));

      // Dead end cap: a short bar perpendicular to the route, exactly where it
      // meets the wall.
      if (frame.cap > 0.02 && reach < 0.4) {
        final alpha = frame.cap * (1 - reach / 0.4);
        final n = Offset(-r.crossDir.dy, r.crossDir.dx);
        canvas.drawLine(
          r.crossPt + n * 5.5,
          r.crossPt - n * 5.5,
          cap..color = _a(tokens.routeIdle, alpha * 1.6),
        );
      }
    }
  }

  void _paintBarrier(Canvas canvas, Size size) {
    if (frame.barrier <= 0.005) return;
    final gw = frame.gap * geo.tokens.gapHalfWidth;
    _scratch.reset();
    var s = 0.0;
    for (final c in geo.gapCuts) {
      final a = c - gw;
      final b = c + gw;
      if (a > s) _strokeRange(_scratch, geo.barrierPts, geo.barrierCum, s, a);
      s = math.max(s, b);
    }
    if (s < geo.barrierLen) {
      _strokeRange(_scratch, geo.barrierPts, geo.barrierCum, s, geo.barrierLen);
    }
    canvas.save();
    canvas.clipPath(
      Path()
        ..fillType = PathFillType.evenOdd
        ..addRect(Rect.fromLTWH(-60, -60, size.width + 120, size.height + 120))
        ..addRRect(geo.lensCore),
    );
    canvas.drawPath(
      _scratch,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.2
        ..strokeCap = StrokeCap.butt
        ..color = _a(tokens.barrier, frame.barrier),
    );
    canvas.restore();
  }

  void _paintFault(Canvas canvas) {
    if (frame.fault <= 0.02) return;
    final r = geo.routes[ChartGeometry.faultRoute];
    const k = 4.5;
    final p = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.8
      ..strokeCap = StrokeCap.round
      ..color = _a(tokens.fault, frame.fault);
    canvas
      ..drawLine(
        r.crossPt + const Offset(-k, -k),
        r.crossPt + const Offset(k, k),
        p,
      )
      ..drawLine(
        r.crossPt + const Offset(k, -k),
        r.crossPt + const Offset(-k, k),
        p,
      );
  }

  void _paintMarks(Canvas canvas) {
    final stroke = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.2;
    final fill = Paint()..style = PaintingStyle.fill;

    var labelIndex = 0;
    final labels = frame.label > 0.02
        ? geo.labelPainters(_a(tokens.label, (frame.label * 16).round() / 16))
        : const <TextPainter>[];

    for (var i = 0; i < geo.routes.length; i++) {
      final r = geo.routes[i];
      final reach = frame.reach[i];
      final on = ((reach - 0.88) / 0.12).clamp(0.0, 1.0);

      for (final w in r.waypoints) {
        canvas.drawCircle(
          w,
          2.6,
          stroke
            ..strokeWidth = 1.2
            ..color = on > 0.5
                ? _a(tokens.nodeLive, 0.7)
                : _a(tokens.nodeIdle, 0.85),
        );
      }

      if (r.edgeTie) {
        final e = r.end;
        final last = r.pts[r.pts.length - 2];
        final vertical = (e.dx - last.dx).abs() < 1;
        final ey = vertical ? math.min(e.dy, geo.size.height - 6) : e.dy;
        final tie = stroke
          ..strokeWidth = 1.4
          ..color = on > 0.5
              ? _a(tokens.nodeLive, 0.8)
              : _a(tokens.nodeIdle, 0.8);
        if (vertical) {
          canvas.drawLine(Offset(e.dx - 5, ey), Offset(e.dx + 5, ey), tie);
        } else {
          canvas.drawLine(Offset(e.dx, ey - 5), Offset(e.dx, ey + 5), tie);
        }
        continue;
      }

      final e = r.end;
      const sz = 5.0;
      if (on < 0.5) {
        canvas.drawRect(
          Rect.fromCenter(
            center: Offset(e.dx + 0.5, e.dy + 0.5),
            width: sz,
            height: sz,
          ),
          stroke
            ..strokeWidth = 1.2
            ..color = _a(tokens.nodeIdle, 1),
        );
      } else {
        canvas
          ..drawRect(
            Rect.fromCenter(center: e, width: sz, height: sz),
            fill..color = _a(tokens.nodeLive, on),
          )
          ..drawCircle(
            e,
            8.5,
            stroke
              ..strokeWidth = 1
              ..color = _a(tokens.nodeLive, on * 0.42),
          );
      }

      if (r.code != null && labelIndex < labels.length) {
        final tp = labels[labelIndex++];
        final dx = r.codeRight ? e.dx + 11 : e.dx - 11 - tp.width;
        tp.paint(canvas, Offset(dx, e.dy - tp.height / 2));
      }
    }
  }

  /// The one place where atmosphere and text genuinely collide: the connect
  /// state label is the only text on the screen painted in a status color, and
  /// status greens and ambers have far less contrast headroom than the neutral
  /// text does. The layer paints the base plane back over itself in a feathered
  /// clearing around the label block, so the composite under those words is
  /// exactly `bgBase` and the contrast table holds at every text scale.
  void _paintQuietLens(Canvas canvas) {
    final core = geo.lensCore;
    final base = tokens.basePlane;
    canvas.drawRRect(core, Paint()..color = base);

    const rings = 22;
    final feather = geo.tokens.lensFeather;
    final band = feather / rings;
    final p = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = band + 0.4;
    for (var i = 0; i < rings; i++) {
      final grow = feather * (i + 0.5) / rings;
      final alpha = (rings - i) / (rings + 1);
      canvas.drawRRect(
        RRect.fromRectAndRadius(
          core.outerRect.inflate(grow),
          Radius.circular(core.brRadiusX + grow),
        ),
        p..color = base.withValues(alpha: alpha),
      );
    }
  }

  void _paintTopFeather(Canvas canvas, Size size) {
    final h = geo.tokens.topFeather;
    if (geo.topInset <= 0 || h <= 0) return;
    final rect = Rect.fromLTWH(0, geo.topInset, size.width, h);
    canvas.drawRect(
      rect,
      Paint()
        ..shader = ui.Gradient.linear(rect.topLeft, rect.bottomLeft, <Color>[
          tokens.basePlane,
          tokens.basePlane.withValues(alpha: 0),
        ]),
    );
  }

  // ---- polyline helpers. Routes are short polylines with a cumulative length
  // table, so partial strokes are a direct walk: no PathMetric, no per frame
  // Path allocation beyond the one reused scratch.

  void _strokeRange(
    Path out,
    List<Offset> pts,
    List<double> cum,
    double a,
    double b,
  ) {
    if (b <= a + 0.01) return;
    var started = false;
    for (var i = 0; i < pts.length - 1; i++) {
      final s0 = cum[i];
      final s1 = cum[i + 1];
      final lo = math.max(a, s0);
      final hi = math.min(b, s1);
      if (hi <= lo || s1 == s0) continue;
      final f1 = (lo - s0) / (s1 - s0);
      final f2 = (hi - s0) / (s1 - s0);
      final d = pts[i + 1] - pts[i];
      final p1 = pts[i] + d * f1;
      final p2 = pts[i] + d * f2;
      if (!started) {
        out.moveTo(p1.dx, p1.dy);
        started = true;
      } else {
        out.lineTo(p1.dx, p1.dy);
      }
      out.lineTo(p2.dx, p2.dy);
    }
  }

  /// Flutter has no dash effect and the ghost segments are short straight runs,
  /// so the dashes are emitted in the same walk. Do not reach for a package.
  void _dashRange(
    Path out,
    RouteGeom r,
    double from,
    double to, {
    required double on,
    required double off,
  }) {
    var s = from;
    while (s < to) {
      final e = math.min(s + on, to);
      _strokeRange(out, r.pts, r.cum, s, e);
      s = e + off;
    }
  }

  @override
  bool shouldRepaint(ChartPainter oldDelegate) =>
      oldDelegate.revision != revision ||
      oldDelegate.strength != strength ||
      oldDelegate.tokens != tokens ||
      !identical(oldDelegate.frames, frames) ||
      !identical(oldDelegate.geo, geo);

  @override
  bool shouldRebuildSemantics(ChartPainter oldDelegate) => false;
}

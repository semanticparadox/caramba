import 'package:flutter/material.dart';

/// Atmosphere layer tokens (DESIGN.md section 4, "Open Chart").
///
/// These sit UNDER the component palette. They never replace a status color and
/// no screen component is allowed to read them: only `lib/atmosphere/` imports
/// this file. Every value is an alpha over the base plane, so the composite is
/// deterministic and the contrast math in DESIGN.md holds.
///
/// Two rules the review checklist enforces:
///  1. Field colors (grid, gridMajor, hatch, lift) stay at or under 8% alpha.
///     `shade` is a radial ramp that starts at 0 alpha near the dial, so its
///     nominal value is the far-edge stop, not a flat field.
///  2. Only three hues exist: probe = warning, live = success, fault = danger.
///     No fourth family, no gradient between hues, no glow anywhere.
///
/// The light values are NOT derived from the dark ones. They were chosen
/// against white: the field alphas are pulled DOWN (dark hairlines on paper
/// read heavier than light hairlines on black) and the structural marks
/// (barrier, routes, stations) are pushed UP, so light carries the state
/// through structure instead of through a grey wash.
@immutable
class AtmosphereTokens extends ThemeExtension<AtmosphereTokens> {
  // ---- Field colors (alpha ceiling 8%)
  /// Graticule minor line, 1px.
  final Color grid;

  /// Every fourth graticule line, 1px.
  final Color gridMajor;

  /// 45 degree restricted hatch, 1px at 9px pitch.
  final Color hatch;

  /// Radial darkening outside the boundary. Alpha 0 at the dial, this value at
  /// the frame edge, so it is a ramp and not a flat field.
  final Color shade;

  /// Full field lift on connected. In a dark theme, light means lift.
  final Color lift;

  // ---- Line work
  /// Route inside the boundary while closed. 1.5px.
  final Color routeIdle;

  /// Route beyond the boundary, dotted 2 on / 4 off.
  final Color routeGhost;

  /// Route while connecting or reconnecting. Derived from `warning`.
  final Color routeProbe;

  /// Route when connected. Derived from `success`.
  final Color routeLive;

  /// Boundary stroke, 1.2px.
  final Color barrier;

  // ---- Point marks
  /// Hollow 5px station mark.
  final Color nodeIdle;

  /// Filled 5px station mark plus hairline ring.
  final Color nodeLive;

  /// The single dead route cross on error.
  final Color fault;

  /// Mono country codes on stations, connected only.
  final Color label;

  /// The base plane painted back over the atmosphere behind the status label
  /// and inside the status bar inset. Opaque; the alpha ramp is in the painter.
  final Color quietLens;

  // ---- Structural (non color)
  /// Half width of the boundary octagon.
  final double barrierHw;

  /// Distance from the dial centre to the boundary top edge. The BOTTOM edge is
  /// derived from the laid out connect block, not from a constant (verdict
  /// must-fix 3), so there is no `barrierHh` here on purpose.
  final double barrierRise;

  /// Slack between the connect block bottom and the boundary bottom edge.
  final double barrierDrop;

  /// Corner cut of the octagon.
  final double barrierCut;

  /// Graticule cell size.
  final double gridPitch;

  /// Every Nth graticule line is major.
  final int gridMajorEvery;

  /// Perpendicular distance between hatch lines.
  final double hatchPitch;

  /// Half width of the gap the boundary opens at each route crossing.
  final double gapHalfWidth;

  /// Extra padding around the label rect that the quiet lens covers opaquely.
  final double lensPad;

  /// How far the quiet lens fades out past its opaque core.
  final double lensFeather;

  /// Height of the soft ramp that fades the atmosphere in below the status bar.
  final double topFeather;

  const AtmosphereTokens({
    required this.grid,
    required this.gridMajor,
    required this.hatch,
    required this.shade,
    required this.lift,
    required this.routeIdle,
    required this.routeGhost,
    required this.routeProbe,
    required this.routeLive,
    required this.barrier,
    required this.nodeIdle,
    required this.nodeLive,
    required this.fault,
    required this.label,
    required this.quietLens,
    this.barrierHw = 134,
    this.barrierRise = 132,
    this.barrierDrop = 12,
    this.barrierCut = 36,
    this.gridPitch = 44,
    this.gridMajorEvery = 4,
    this.hatchPitch = 9,
    this.gapHalfWidth = 13,
    this.lensPad = 16,
    this.lensFeather = 40,
    this.topFeather = 18,
  });

  /// Dark theme. Values as measured on the concept prototype.
  factory AtmosphereTokens.dark() => const AtmosphereTokens(
    grid: Color(0x08FAFAFA), // 3.0%
    gridMajor: Color(0x0DFAFAFA), // 5.2%
    hatch: Color(0x07FAFAFA), // 2.6%
    shade: Color(0x73000000), // 45% at the far edge
    lift: Color(0x06FAFAFA), // 2.5%
    routeIdle: Color(0x1AFAFAFA), // 10%
    routeGhost: Color(0x0BFAFAFA), // 4.5%
    routeProbe: Color(0x3DFF9F0A), // 24%
    routeLive: Color(0x3830D158), // 22%
    barrier: Color(0x24FAFAFA), // 14%
    nodeIdle: Color(0x29FAFAFA), // 16%
    nodeLive: Color(0x8030D158), // 50%
    fault: Color(0x8CFF453A), // 55%
    label: Color(0x33FAFAFA), // 20%
    quietLens: Color(0xFF0A0A0A),
  );

  /// Light theme. Tuned against white, not inverted from dark:
  ///  * field alphas DOWN (grid 3.8 / major 6.0 / hatch 3.2 / shade 5.0)
  ///  * structure UP (barrier 22, routeIdle 16, nodeIdle 24)
  ///  * live green is `success` = #177A41, the deepened light value from
  ///    verdict must-fix 1, not the old #1E9E54.
  factory AtmosphereTokens.light() => const AtmosphereTokens(
    grid: Color(0x0A0A0A0A), // 3.8%
    gridMajor: Color(0x0F0A0A0A), // 6.0%
    hatch: Color(0x080A0A0A), // 3.2%
    shade: Color(0x0D0A0A0A), // 5.0% at the far edge
    lift: Color(0x00FAFAFA), // none
    routeIdle: Color(0x290A0A0A), // 16%
    routeGhost: Color(0x120A0A0A), // 7%
    routeProbe: Color(0x57A85D00), // 34%
    routeLive: Color(0x57177A41), // 34%
    barrier: Color(0x380A0A0A), // 22%
    nodeIdle: Color(0x3D0A0A0A), // 24%
    nodeLive: Color(0xA8177A41), // 66%
    fault: Color(0xA6C8102E), // 65%
    label: Color(0x3D0A0A0A), // 24%
    quietLens: Color(0xFFFAFAFA),
  );

  /// The base plane this layer paints under everything. Equal to `bgBase`.
  Color get basePlane => quietLens;

  static AtmosphereTokens of(BuildContext context) {
    final theme = Theme.of(context);
    return theme.extension<AtmosphereTokens>() ??
        (theme.brightness == Brightness.dark
            ? AtmosphereTokens.dark()
            : AtmosphereTokens.light());
  }

  @override
  AtmosphereTokens copyWith({
    Color? grid,
    Color? gridMajor,
    Color? hatch,
    Color? shade,
    Color? lift,
    Color? routeIdle,
    Color? routeGhost,
    Color? routeProbe,
    Color? routeLive,
    Color? barrier,
    Color? nodeIdle,
    Color? nodeLive,
    Color? fault,
    Color? label,
    Color? quietLens,
  }) => AtmosphereTokens(
    grid: grid ?? this.grid,
    gridMajor: gridMajor ?? this.gridMajor,
    hatch: hatch ?? this.hatch,
    shade: shade ?? this.shade,
    lift: lift ?? this.lift,
    routeIdle: routeIdle ?? this.routeIdle,
    routeGhost: routeGhost ?? this.routeGhost,
    routeProbe: routeProbe ?? this.routeProbe,
    routeLive: routeLive ?? this.routeLive,
    barrier: barrier ?? this.barrier,
    nodeIdle: nodeIdle ?? this.nodeIdle,
    nodeLive: nodeLive ?? this.nodeLive,
    fault: fault ?? this.fault,
    label: label ?? this.label,
    quietLens: quietLens ?? this.quietLens,
    barrierHw: barrierHw,
    barrierRise: barrierRise,
    barrierDrop: barrierDrop,
    barrierCut: barrierCut,
    gridPitch: gridPitch,
    gridMajorEvery: gridMajorEvery,
    hatchPitch: hatchPitch,
    gapHalfWidth: gapHalfWidth,
    lensPad: lensPad,
    lensFeather: lensFeather,
    topFeather: topFeather,
  );

  /// Atmosphere palettes are discrete, like [ThemeExtension] siblings in this
  /// app: snap rather than interpolate, so a theme switch never produces a
  /// half-lit chart.
  @override
  AtmosphereTokens lerp(ThemeExtension<AtmosphereTokens>? other, double t) {
    if (other is! AtmosphereTokens) return this;
    return t < 0.5 ? this : other;
  }

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AtmosphereTokens &&
          other.grid == grid &&
          other.gridMajor == gridMajor &&
          other.hatch == hatch &&
          other.shade == shade &&
          other.lift == lift &&
          other.routeIdle == routeIdle &&
          other.routeGhost == routeGhost &&
          other.routeProbe == routeProbe &&
          other.routeLive == routeLive &&
          other.barrier == barrier &&
          other.nodeIdle == nodeIdle &&
          other.nodeLive == nodeLive &&
          other.fault == fault &&
          other.label == label &&
          other.quietLens == quietLens;

  @override
  int get hashCode => Object.hash(
    grid,
    gridMajor,
    hatch,
    shade,
    lift,
    routeIdle,
    routeGhost,
    routeProbe,
    routeLive,
    barrier,
    nodeIdle,
    nodeLive,
    fault,
    label,
    quietLens,
  );
}

/// Frame budget for the atmosphere layer.
///
/// `lowPower` is plumbed in rather than read here: the app sets it from
/// `PowerManager.isPowerSaveMode` (Android) or
/// `ProcessInfo.isLowPowerModeEnabled` (iOS) over a MethodChannel and pushes it
/// down with [AtmosphereBudgetScope]. Until that channel exists the default is
/// `false`, which costs nothing and keeps the layer honest about what it knows.
@immutable
class AtmosphereBudget {
  final bool lowPower;

  const AtmosphereBudget({this.lowPower = false});

  static const AtmosphereBudget normal = AtmosphereBudget();
  static const AtmosphereBudget saving = AtmosphereBudget(lowPower: true);

  /// Resting cap while a transition or the probe loop is running.
  int get targetFps => lowPower ? 20 : 30;

  Duration get minFrameGap =>
      Duration(microseconds: (1000000 / targetFps).round());

  @override
  bool operator ==(Object other) =>
      other is AtmosphereBudget && other.lowPower == lowPower;

  @override
  int get hashCode => lowPower.hashCode;
}

/// Pushes an [AtmosphereBudget] down the tree. The app can rebuild this from a
/// platform low-power listener; the layer reads it and drops its cap to 20fps.
class AtmosphereBudgetScope extends InheritedWidget {
  final AtmosphereBudget budget;

  const AtmosphereBudgetScope({
    required this.budget,
    required super.child,
    super.key,
  });

  static AtmosphereBudget of(BuildContext context) =>
      context
          .dependOnInheritedWidgetOfExactType<AtmosphereBudgetScope>()
          ?.budget ??
      AtmosphereBudget.normal;

  @override
  bool updateShouldNotify(AtmosphereBudgetScope old) => old.budget != budget;
}

import 'package:flutter/widgets.dart';

/// exarobot spacing, radius, border, motion & orb tokens.
/// Mirrors DESIGN.md §3 + §7.

/// 4px base, 8px rhythm spacing scale.
abstract final class AppSpace {
  static const double s0 = 0;
  static const double s1 = 4;
  static const double s2 = 8;
  static const double s3 = 12;
  static const double s4 = 16;
  static const double s5 = 20;
  static const double s6 = 24;
  static const double s8 = 32;
  static const double s10 = 40;
  static const double s12 = 48;
  static const double s16 = 64;
  static const double s20 = 80;

  static const double screenPadMobile = 20;
  static const double screenPadDesktop = 32;
}

/// Radius scale (DESIGN.md §3.2 / demo). Default card = [md] (16);
/// buttons = [button] (14); sheets top = [lg] (22).
abstract final class AppRadius {
  static const double xs = 8; // code/mono boxes
  static const double sm = 12; // icon buttons, chips-as-box, toasts
  static const double button = 14; // buttons, list items
  static const double md = 16; // default card
  static const double lg = 22; // sheets, large cards
  static const double xl = 28; // hero card
  static const double pill = 999;

  static const r8 = BorderRadius.all(Radius.circular(xs));
  static const r12 = BorderRadius.all(Radius.circular(sm));
  static const r14 = BorderRadius.all(Radius.circular(button));
  static const r16 = BorderRadius.all(Radius.circular(md));
  static const r22 = BorderRadius.all(Radius.circular(lg));
  static const r28 = BorderRadius.all(Radius.circular(xl));
  static const sheetTop = BorderRadius.vertical(top: Radius.circular(lg));
}

/// Border widths. See DESIGN.md §3.4.
abstract final class AppBorders {
  static const double hairline = 1;
  static const double input = 1.5;
  static const double focus = 2;
}

/// Motion durations & curves. See DESIGN.md §7.
abstract final class AppMotion {
  static const micro = Duration(milliseconds: 120);
  static const standard = Duration(milliseconds: 220);
  static const large = Duration(milliseconds: 320);
  static const ringMorph = Duration(milliseconds: 400);

  static const Curve enter = Curves.easeOutCubic;
  static const Curve morph = Curves.easeInOutCubic;
  // Toggle/knob spring: SpringDescription(stiffness:180, damping:22, mass:1).
}

/// Connect dial geometry. See DESIGN.md §4 / demo. A neutral ring whose stroke
/// color carries connection status (green/amber/red), with a flat face. No glow,
/// no breathing aura.
abstract final class AppOrb {
  static const double diameterMobile = 196;
  static const double diameterDesktop = 232;
  static const double ringStroke = 2;
  static const double faceMobile = 146;
}

/// Layout breakpoint at which the app switches to the desktop layout
/// (left rail instead of bottom nav, centered content column).
abstract final class AppBreakpoints {
  static const double desktop = 840;
  static const double contentMaxWidth = 960;
  static const double dialogMaxWidth = 520;
}

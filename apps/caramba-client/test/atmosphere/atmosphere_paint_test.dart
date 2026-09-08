// Type-band invariant for the atmosphere layer (DESIGN.md section 4).
//
// The chart is allowed to deviate from the base plane anywhere EXCEPT two
// protected bands: the OS status bar inset, whose text the system draws in a
// color we do not control, and the connect block, which is the only text on
// the screen painted in a status color. Both are painted back to the base
// plane, and this test is what stops a future edit from eroding either one.

import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/atmosphere/atmosphere_painter.dart';
import 'package:caramba_client/atmosphere/atmosphere_state.dart';
import 'package:caramba_client/atmosphere/atmosphere_tokens.dart';

const Size kFrame = Size(390, 844);
const Offset kHome = Offset(195, 240);
const double kTopInset = 47;
const Rect kLabelRect = Rect.fromLTWH(45, 354, 300, 48);

ChartGeometry buildGeometry(AtmosphereTokens tokens) => ChartGeometry.build(
  size: kFrame,
  home: kHome,
  labelRect: kLabelRect,
  topInset: kTopInset,
  tokens: tokens,
);

/// Paints one settled composition into a [ui.Image] at 1:1.
Future<ui.Image> renderState(
  AtmoState state,
  AtmosphereTokens tokens, {
  double strength = 1,
}) async {
  final solver = AtmosphereSolver(initial: state)..tokens = tokens;
  return renderFrame(solver.settledFrame(), tokens, strength: strength);
}

Future<ui.Image> renderFrame(
  ChartFrame frame,
  AtmosphereTokens tokens, {
  double strength = 1,
}) async {
  final recorder = ui.PictureRecorder();
  final canvas = Canvas(recorder, Offset.zero & kFrame);
  ChartPainter(
    geo: buildGeometry(tokens),
    tokens: tokens,
    frames: ChartFrames(frame),
    strength: strength,
    revision: 0,
    devicePixelRatio: 1,
    listen: false,
  ).paint(canvas, kFrame);
  return recorder.endRecording().toImage(
    kFrame.width.round(),
    kFrame.height.round(),
  );
}

/// Worst per-channel deviation from [base] inside [band], in 0 to 255 levels.
int worstDeviation(ByteData pixels, int width, Rect band, Color base) {
  final bytes = pixels.buffer.asUint8List();
  final br = (base.r * 255).round();
  final bg = (base.g * 255).round();
  final bb = (base.b * 255).round();
  var worst = 0;
  for (var y = band.top.floor(); y < band.bottom.ceil(); y++) {
    for (var x = band.left.floor(); x < band.right.ceil(); x++) {
      final i = (y * width + x) * 4;
      final d = <int>[
        (bytes[i] - br).abs(),
        (bytes[i + 1] - bg).abs(),
        (bytes[i + 2] - bb).abs(),
      ].reduce((a, b) => a > b ? a : b);
      if (d > worst) worst = d;
    }
  }
  return worst;
}

void main() {
  final themes = <String, AtmosphereTokens>{
    'dark': AtmosphereTokens.dark(),
    'light': AtmosphereTokens.light(),
  };

  group('type-band invariant', () {
    for (final entry in themes.entries) {
      final name = entry.key;
      final tokens = entry.value;

      for (final state in AtmoState.values) {
        test(
          '$name / ${state.name}: protected bands hold the base plane',
          () async {
            final image = await renderState(state, tokens);
            final pixels = (await image.toByteData())!;
            final base = tokens.basePlane;

            final statusBand = Rect.fromLTWH(0, 0, kFrame.width, kTopInset);
            expect(
              worstDeviation(pixels, image.width, statusBand, base),
              lessThanOrEqualTo(2),
              reason:
                  'atmosphere leaked into the status bar band ($name/$state)',
            );

            expect(
              worstDeviation(pixels, image.width, kLabelRect, base),
              lessThanOrEqualTo(2),
              reason:
                  'quiet lens failed under the connect block ($name/$state)',
            );
            image.dispose();
          },
        );
      }
    }

    test(
      'the lens holds mid-transition, when the field lift is ramping',
      () async {
        final tokens = AtmosphereTokens.dark();
        final solver = AtmosphereSolver(initial: AtmoState.connecting)
          ..tokens = tokens
          ..transitionTo(AtmoState.connected, at: Duration.zero);
        for (final ms in <int>[120, 400, 800, 1100]) {
          final image = await renderFrame(
            solver.frameAt(Duration(milliseconds: ms)),
            tokens,
          );
          final pixels = (await image.toByteData())!;
          expect(
            worstDeviation(pixels, image.width, kLabelRect, tokens.basePlane),
            lessThanOrEqualTo(2),
            reason: 'quiet lens failed at t=${ms}ms of the open transition',
          );
          expect(
            worstDeviation(
              pixels,
              image.width,
              Rect.fromLTWH(0, 0, kFrame.width, kTopInset),
              tokens.basePlane,
            ),
            lessThanOrEqualTo(2),
            reason: 'status bar band failed at t=${ms}ms',
          );
          image.dispose();
        }
      },
    );

    test('the lens follows the label at 200 percent text scale', () async {
      // Verdict must-fix 3 and 4: the boundary bottom and the lens are derived
      // from the laid-out connect block, so a bigger block must still be clean.
      final tokens = AtmosphereTokens.dark();
      const big = Rect.fromLTWH(20, 354, 350, 96);
      final geo = ChartGeometry.build(
        size: kFrame,
        home: kHome,
        labelRect: big,
        topInset: kTopInset,
        tokens: tokens,
      );
      final solver = AtmosphereSolver(initial: AtmoState.disconnected)
        ..tokens = tokens;
      final recorder = ui.PictureRecorder();
      final canvas = Canvas(recorder, Offset.zero & kFrame);
      ChartPainter(
        geo: geo,
        tokens: tokens,
        frames: ChartFrames(solver.settledFrame()),
        strength: 1,
        revision: 0,
        devicePixelRatio: 1,
        listen: false,
      ).paint(canvas, kFrame);
      final image = await recorder.endRecording().toImage(390, 844);
      final pixels = (await image.toByteData())!;
      expect(
        worstDeviation(pixels, image.width, big, tokens.basePlane),
        lessThanOrEqualTo(2),
      );
      // The boundary still encloses the block: its bottom edge sits below it.
      final bottom = geo.barrierPts
          .map((p) => p.dy)
          .reduce((a, b) => a > b ? a : b);
      expect(bottom, greaterThan(big.bottom));
      image.dispose();
    });
  });

  group('field alpha ceiling', () {
    // The contrast table in DESIGN.md is built on this ceiling. Point marks and
    // hairlines may go higher because they cover a negligible pixel area.
    for (final entry in themes.entries) {
      test('${entry.key}: no field element exceeds 8 percent', () {
        final t = entry.value;
        for (final field in <(String, Color)>[
          ('grid', t.grid),
          ('gridMajor', t.gridMajor),
          ('hatch', t.hatch),
          ('lift', t.lift),
        ]) {
          expect(
            field.$2.a,
            lessThanOrEqualTo(0.08),
            reason: '${field.$1} is over the field alpha ceiling',
          );
        }
      });
    }
  });

  group('connected is not the busiest state', () {
    // Verdict must-fix 2: count what is actually on screen per state and keep
    // connected below connecting. Country codes are capped at three.
    test('country codes are capped at three, connected only', () {
      final geo = buildGeometry(AtmosphereTokens.dark());
      final coded = geo.routes.where((r) => r.code != null).length;
      expect(coded, 3);
      final solver = AtmosphereSolver(initial: AtmoState.connecting)
        ..tokens = AtmosphereTokens.dark();
      expect(solver.settledFrame().label, 0);
    });

    test('connected drops the hatch, the shade and the boundary', () {
      final tokens = AtmosphereTokens.dark();
      ChartFrame frameOf(AtmoState s) =>
          (AtmosphereSolver(initial: s)..tokens = tokens).settledFrame();
      final connected = frameOf(AtmoState.connected);
      final connecting = frameOf(AtmoState.connecting);
      expect(connected.hatch, 0);
      expect(connected.shade, 0);
      expect(connected.barrier, 0);
      // Connecting keeps all three, so it carries strictly more ink.
      expect(connecting.hatch, greaterThan(0));
      expect(connecting.shade, greaterThan(0));
      expect(connecting.barrier, greaterThan(0));
    });
  });
}

// Renders the atmosphere layer plus a mock of Home's chrome to PNG files, so
// the composition can be reviewed against the concept without a device.
//
// Skipped unless ATMO_SHOTS_DIR points at a writable directory:
//   ATMO_SHOTS_DIR=/tmp/shots flutter test test/atmosphere/render_shots_test.dart

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/atmosphere/atmosphere_painter.dart';
import 'package:caramba_client/atmosphere/atmosphere_state.dart';
import 'package:caramba_client/atmosphere/atmosphere_tokens.dart';
import 'package:caramba_client/theme/colors.dart';

const Size frame = Size(390, 844);
const double topInset = 47;
const Offset home = Offset(195, 241);

/// Realistic connect-block widths: "Защищено" is short, "Не удалось
/// подключиться" is the widest string the label ever holds.
Rect labelRectFor(AtmoState state) {
  final w = switch (state) {
    AtmoState.connected => 132.0,
    AtmoState.disconnected => 128.0,
    AtmoState.error => 244.0,
    AtmoState.connecting => 148.0,
    AtmoState.reconnecting => 186.0,
  };
  return Rect.fromLTWH(195 - w / 2, 355, w, 48);
}

const double headerBottom = 111;

void main() {
  final dir = Platform.environment['ATMO_SHOTS_DIR'];

  test(
    'render atmosphere shots',
    skip: dir == null ? 'set ATMO_SHOTS_DIR' : null,
    () async {
      Directory(dir!).createSync(recursive: true);

      for (final dark in <bool>[true, false]) {
        final tokens = dark
            ? AtmosphereTokens.dark()
            : AtmosphereTokens.light();
        final colors = dark ? AppColors.dark : AppColors.light;
        for (final state in AtmoState.values) {
          final geo = ChartGeometry.build(
            size: frame,
            home: home,
            labelRect: labelRectFor(state),
            topInset: topInset,
            headerBottom: headerBottom,
            tokens: tokens,
          );
          final solver = AtmosphereSolver(initial: state)..tokens = tokens;
          // Probe states are sampled a beat into the loop so the routes sit at
          // visibly different lengths, which is the point of the stagger.
          final frameData = solver.frameAt(const Duration(milliseconds: 900));

          const scale = 2.0;
          final recorder = ui.PictureRecorder();
          final canvas = Canvas(recorder, Offset.zero & (frame * scale))
            ..scale(scale);
          ChartPainter(
            geo: geo,
            tokens: tokens,
            frames: ChartFrames(frameData),
            strength: 1,
            revision: 0,
            devicePixelRatio: 2,
            listen: false,
          ).paint(canvas, frame);
          _paintChrome(canvas, colors, state, labelRectFor(state));

          final image = await recorder.endRecording().toImage(
            (frame.width * scale).round(),
            (frame.height * scale).round(),
          );
          final png = await image.toByteData(format: ui.ImageByteFormat.png);
          final name = '${state.name}${dark ? '' : '-light'}.png';
          File('$dir/$name').writeAsBytesSync(png!.buffer.asUint8List());
          image.dispose();
        }
      }
    },
  );
}

/// A deliberately plain mock of Home's chrome: the dial, the two label lines
/// and the card block. Enough to judge whether the chart is registered to the
/// layout, without dragging the whole provider graph into a render pass.
void _paintChrome(Canvas canvas, AppColors c, AtmoState state, Rect labelRect) {
  final stateColor = switch (state) {
    AtmoState.connected => c.success,
    AtmoState.error => c.danger,
    AtmoState.connecting || AtmoState.reconnecting => c.warning,
    AtmoState.disconnected => c.borderStrong,
  };
  final glyphColor = switch (state) {
    AtmoState.connected => c.success,
    AtmoState.error => c.danger,
    AtmoState.connecting || AtmoState.reconnecting => c.warning,
    AtmoState.disconnected => c.textMed,
  };

  // Wordmark and plan chip.
  canvas
    ..drawRRect(
      RRect.fromRectAndRadius(
        const Rect.fromLTWH(20, 79, 96, 17),
        const Radius.circular(3),
      ),
      Paint()..color = c.textHi,
    )
    ..drawRRect(
      RRect.fromRectAndRadius(
        const Rect.fromLTWH(272, 78, 48, 22),
        const Radius.circular(6),
      ),
      Paint()
        ..style = PaintingStyle.stroke
        ..color = c.borderStrong,
    )
    ..drawRRect(
      RRect.fromRectAndRadius(
        const Rect.fromLTWH(326, 78, 44, 44),
        const Radius.circular(12),
      ),
      Paint()
        ..style = PaintingStyle.stroke
        ..color = c.borderSubtle,
    );

  // Dial: 2px status ring, 146px face on surface1.
  canvas
    ..drawCircle(
      home,
      96,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..color = stateColor,
    )
    ..drawCircle(home, 73, Paint()..color = c.surface1)
    ..drawCircle(
      home,
      73,
      Paint()
        ..style = PaintingStyle.stroke
        ..color = state == AtmoState.connected
            ? c.success
            : state == AtmoState.error
            ? c.danger
            : c.borderSubtle,
    )
    ..drawCircle(home, 16, Paint()..color = glyphColor);

  // The connect block: state label in the status color, sub-line in textMed.
  canvas
    ..drawRRect(
      RRect.fromRectAndRadius(
        Rect.fromCenter(
          center: Offset(home.dx, labelRect.top + 13),
          width: labelRect.width,
          height: 20,
        ),
        const Radius.circular(4),
      ),
      Paint()..color = stateColor,
    )
    ..drawRRect(
      RRect.fromRectAndRadius(
        Rect.fromCenter(
          center: Offset(home.dx, labelRect.top + 38),
          width: labelRect.width * 0.78,
          height: 12,
        ),
        const Radius.circular(3),
      ),
      Paint()..color = c.textMed,
    );

  // The cards backdrop and the two card blocks.
  const cardsTop = 403.0;
  canvas.drawRect(
    const Rect.fromLTWH(0, cardsTop, 390, 844 - cardsTop),
    Paint()
      ..shader = ui.Gradient.linear(
        const Offset(0, cardsTop),
        const Offset(0, cardsTop + 40),
        <Color>[
          c.bgBase.withValues(alpha: 0),
          c.bgBase.withValues(alpha: 0.82),
        ],
      ),
  );
  for (final r in <Rect>[
    const Rect.fromLTWH(20, 423, 350, 224),
    const Rect.fromLTWH(20, 663, 350, 130),
  ]) {
    canvas
      ..drawRRect(
        RRect.fromRectAndRadius(r, const Radius.circular(14)),
        Paint()..color = c.surface1,
      )
      ..drawRRect(
        RRect.fromRectAndRadius(r, const Radius.circular(14)),
        Paint()
          ..style = PaintingStyle.stroke
          ..color = c.borderSubtle,
      );
  }

  // Bottom nav.
  canvas.drawRect(
    const Rect.fromLTWH(0, 760, 390, 84),
    Paint()..color = c.bgCanvas.withValues(alpha: 0.92),
  );
  canvas.drawLine(
    const Offset(0, 760),
    const Offset(390, 760),
    Paint()..color = c.borderSubtle,
  );
}

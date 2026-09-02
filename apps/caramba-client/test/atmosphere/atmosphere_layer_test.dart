// The battery contract and the transition contract for the atmosphere layer.
//
// The concept's main claim is that the two states a user lives in cost zero
// frames. Prose does not enforce that, so these tests do: after settling into
// disconnected, connected or error the ticker must be STOPPED, not merely
// idle, and it must be stopped in every state once the app is backgrounded.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/atmosphere/atmosphere_layer.dart';
import 'package:caramba_client/atmosphere/atmosphere_state.dart';
import 'package:caramba_client/atmosphere/atmosphere_tokens.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

const List<VpnStage> settledStages = <VpnStage>[
  VpnStage.disconnected,
  VpnStage.connected,
  VpnStage.error,
];

const List<VpnStage> busyStages = <VpnStage>[
  VpnStage.connecting,
  VpnStage.reconnecting,
];

Widget _host({
  required VpnStage stage,
  bool reduceMotion = false,
  bool lowPower = false,
  double strength = 1,
}) {
  return MediaQuery(
    data: MediaQueryData(
      size: const Size(390, 844),
      devicePixelRatio: 2,
      viewPadding: const EdgeInsets.only(top: 47),
      padding: const EdgeInsets.only(top: 47),
      disableAnimations: reduceMotion,
    ),
    child: MaterialApp(
      theme: AppTheme.dark(),
      home: AtmosphereBudgetScope(
        budget: lowPower ? AtmosphereBudget.saving : AtmosphereBudget.normal,
        child: Scaffold(
          body: AtmosphereLayer(stage: stage, strength: strength),
        ),
      ),
    ),
  );
}

AtmosphereLayerState _layer(WidgetTester tester) =>
    tester.state<AtmosphereLayerState>(find.byType(AtmosphereLayer));

/// Drives the whole transition without `pumpAndSettle`, which would hang on a
/// state that animates forever by design.
Future<void> _run(WidgetTester tester, int ms) async {
  const step = Duration(milliseconds: 33);
  for (var t = 0; t < ms; t += 33) {
    await tester.pump(step);
  }
}

void main() {
  group('battery contract', () {
    for (final stage in settledStages) {
      testWidgets('$stage stops the ticker once it has settled', (
        tester,
      ) async {
        await tester.pumpWidget(_host(stage: VpnStage.connecting));
        await _run(tester, 200);
        expect(_layer(tester).debugIsTicking, isTrue);

        await tester.pumpWidget(_host(stage: stage));
        await _run(tester, 1600);

        expect(
          _layer(tester).debugIsTicking,
          isFalse,
          reason: '$stage must cost zero frames once it has landed',
        );
      });
    }

    for (final stage in <VpnStage>[...settledStages, ...busyStages]) {
      testWidgets('$stage stops the ticker when the app is backgrounded', (
        tester,
      ) async {
        await tester.pumpWidget(_host(stage: stage));
        await _run(tester, 200);

        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.inactive,
        );
        tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
        tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
        await tester.pump();

        expect(
          _layer(tester).debugIsTicking,
          isFalse,
          reason: '$stage kept ticking while backgrounded',
        );

        tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.inactive,
        );
        tester.binding.handleAppLifecycleStateChanged(
          AppLifecycleState.resumed,
        );
        await tester.pump();
        // Only the states that animate come back.
        expect(_layer(tester).debugIsTicking, busyStages.contains(stage));
        await _run(tester, 400);
      });
    }

    testWidgets('strength 0 never starts a ticker', (tester) async {
      await tester.pumpWidget(_host(stage: VpnStage.connecting, strength: 0));
      await _run(tester, 300);
      expect(_layer(tester).debugIsTicking, isFalse);
    });
  });

  group('transition sanity', () {
    testWidgets('connecting keeps ticking past a full probe cycle', (
      tester,
    ) async {
      await tester.pumpWidget(_host(stage: VpnStage.connecting));
      await _run(tester, 2600);
      expect(_layer(tester).debugIsTicking, isTrue);
      await tester.pumpWidget(_host(stage: VpnStage.disconnected));
      await _run(tester, 900);
    });

    testWidgets('the first frame after a state change is painted', (
      tester,
    ) async {
      // The 33ms gate has a latent bug: a state entered less than one frame gap
      // after the previous tick would show the OLD composition until the gate
      // reopens. The layer resets its last-frame stamp on every state change.
      await tester.pumpWidget(_host(stage: VpnStage.disconnected));
      await tester.pump();
      final before = _layer(tester).debugFrame;
      expect(before.barrier, 1);

      await tester.pumpWidget(_host(stage: VpnStage.connected));
      await tester.pump(const Duration(milliseconds: 1));
      final after = _layer(tester).debugFrame;
      expect(
        after,
        isNot(before),
        reason: 'the transition did not paint on its first frame',
      );
    });

    testWidgets('reduce motion creates no ticker at all', (tester) async {
      await tester.pumpWidget(
        _host(stage: VpnStage.connecting, reduceMotion: true),
      );
      await _run(tester, 900);
      expect(_layer(tester).debugHasTicker, isFalse);

      await tester.pumpWidget(
        _host(stage: VpnStage.connected, reduceMotion: true),
      );
      await tester.pump();
      expect(_layer(tester).debugHasTicker, isFalse);
      // The composition is the target state's settled frame, immediately: the
      // probe loop resolves to a deterministic mid range, never "wherever the
      // loop happened to be".
      expect(_layer(tester).debugFrame.reach.every((r) => r == 1), isTrue);
      await tester.pump(const Duration(milliseconds: 400));
    });

    testWidgets('low power drops the cap from 30 to 20 fps', (tester) async {
      expect(AtmosphereBudget.normal.targetFps, 30);
      expect(AtmosphereBudget.saving.targetFps, 20);
      await tester.pumpWidget(
        _host(stage: VpnStage.connecting, lowPower: true),
      );
      await _run(tester, 200);
      expect(_layer(tester).debugIsTicking, isTrue);
      await tester.pumpWidget(_host(stage: VpnStage.disconnected));
      await _run(tester, 900);
    });
  });

  group('solver', () {
    test('every from/to pair lands exactly on the target settled frame', () {
      final tokens = AtmosphereTokens.dark();
      for (final from in AtmoState.values) {
        for (final to in AtmoState.values) {
          if (from == to) continue;
          final solver = AtmosphereSolver(initial: from)..tokens = tokens;
          solver.transitionTo(to, at: Duration.zero);
          final end = Duration(milliseconds: kAtmoDurationMs[to]!);
          final landed = solver.frameAt(end);
          final target = (AtmosphereSolver(
            initial: to,
          )..tokens = tokens).settledFrame(now: end);
          expect(
            landed.grid,
            closeTo(target.grid, 1e-9),
            reason: '$from -> $to left the grid mid transition',
          );
          expect(landed.barrier, closeTo(target.barrier, 1e-9));
          expect(landed.hatch, closeTo(target.hatch, 1e-9));
          expect(landed.lift, closeTo(target.lift, 1e-9));
          for (var i = 0; i < kAtmoRouteCount; i++) {
            expect(
              landed.reach[i],
              closeTo(target.reach[i], 1e-6),
              reason: '$from -> $to left route $i mid transition',
            );
          }
        }
      }
    });

    test('opening runs inside out and closing runs outside in', () {
      final tokens = AtmosphereTokens.dark();
      // The nearest station (rank 0, route 3) leads on the way out.
      final opening = AtmosphereSolver(initial: AtmoState.disconnected)
        ..tokens = tokens
        ..transitionTo(AtmoState.connected, at: Duration.zero);
      final mid = opening.frameAt(const Duration(milliseconds: 420));
      expect(mid.reach[3], greaterThan(mid.reach[7]));

      // Closing reverses it: the furthest station goes dark first, so the
      // disconnect visibly draws back to the thumb.
      final closing = AtmosphereSolver(initial: AtmoState.connected)
        ..tokens = tokens
        ..transitionTo(AtmoState.disconnected, at: Duration.zero);
      final out = closing.frameAt(const Duration(milliseconds: 260));
      expect(out.reach[7], lessThan(out.reach[3]));
    });

    test('the error shake runs once and then stops', () {
      final tokens = AtmosphereTokens.dark();
      final solver = AtmosphereSolver(initial: AtmoState.connected)
        ..tokens = tokens
        ..transitionTo(AtmoState.error, at: Duration.zero);
      expect(
        solver.frameAt(const Duration(milliseconds: 60)).shakeDx.abs(),
        greaterThan(0),
      );
      expect(solver.frameAt(const Duration(milliseconds: 300)).shakeDx, 0);
      expect(solver.isSettled(const Duration(milliseconds: 300)), isFalse);
      expect(solver.isSettled(const Duration(milliseconds: 500)), isTrue);
      expect(solver.isStaticState, isTrue);
    });

    test('reduce motion resolves the probe loop to a deterministic value', () {
      final tokens = AtmosphereTokens.dark();
      final solver = AtmosphereSolver(
        initial: AtmoState.connecting,
        reduceMotion: true,
      )..tokens = tokens;
      final a = solver.frameAt(const Duration(milliseconds: 10));
      final b = solver.frameAt(const Duration(milliseconds: 1700));
      expect(a.reach, b.reach);
      expect(a.reach.first, closeTo((0.15 + 0.78) / 2, 1e-9));
    });
  });
}

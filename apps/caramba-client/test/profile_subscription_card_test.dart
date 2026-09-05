// Карточка подписки в профиле показывала пользователю ДВЕ лжи одновременно:
//
//   1. `Tag(sub.isActive ? 'Активна' : sub.status, ...)` печатал сырой статус
//      панели буквами — «throttled», «expired», «pending» — слова, которых
//      человек, оплативший VPN, никогда не должен видеть на своём экране.
//   2. Строка квоты бесплатного тарифа была подписана «ГБ в неделю», хотя
//      enforcement считает МБ в СУТКИ (`plans.daily_traffic_mb`,
//      `quota_period == "day"`); `weekly_free_refill_gb`, который она читала,
//      это тот же суточный лимит, домноженный панелью на 7 для другой витрины.
//
// Тесты фиксируют на РЕНДЕРЕ (а не только на модели, как в access_quota_test),
// что ни одно из старых внутренних слов панели больше не долетает до текста,
// и что цифры подписаны тем периодом, который реально считает enforcement.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/branding.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/features/profile/profile_screen.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/theme/app_theme.dart';

/// Сырые слова панели, которых пользователь не должен увидеть НИКОГДА —
/// ни в статус-бейдже, ни в подписи квоты.
const _forbidden = <String>[
  'throttled',
  'expired',
  'pending',
  'banned',
  'disabled',
  'suspended',
  'active', // англ. «active» — не путать с русским «Активна»
  'в неделю',
];

Widget _app(Widget child, {Branding branding = Branding.fallback}) =>
    ProviderScope(
      overrides: <Override>[
        // AccessCard читает эту вставку ТОЛЬКО для ссылки на оплату (когда ей
        // явно передан `access`, состояние карточки берётся из него) — здесь
        // подставляем `null`, чтобы не тянуть весь стек connection-profile /
        // subscriptionProvider ради простого рендер-теста одной карточки.
        subscriptionAccessProvider.overrideWithValue(null),
        activeBrandingProvider.overrideWithValue(branding),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(body: SingleChildScrollView(child: child)),
      ),
    );

/// Свежая подписка free-тарифа в том виде, в каком её отдаёт `/app/subscriptions`
/// (см. `app_account.rs`): `quota_period` для free — всегда `"day"`, а
/// `weekly_free_refill_gb` — производная витрина той же суточной цифры.
Map<String, dynamic> _freePlanJson({
  required String status,
  int usedBytes = 0,
  int limitBytes = 209715200, // 200 МБ
  int dailyMb = 200,
}) => <String, dynamic>{
  'id': 27,
  'plan_name': 'Free',
  'kind': 'free',
  'status': status,
  'used_traffic_bytes': usedBytes,
  'traffic_limit_bytes': limitBytes,
  'quota_period': 'day',
  'is_free': true,
  'daily_traffic_mb': dailyMb,
  // Домноженное на 7 суточное значение — ИМЕННО то поле, которое старая
  // карточка ошибочно подписывала «в неделю».
  'weekly_free_refill_gb': (dailyMb * 7) / 1024,
  'device_used': 1,
  'device_limit': 1,
};

void _expectNoRawStatus(WidgetTester tester) {
  for (final word in _forbidden) {
    expect(
      find.textContaining(word, findRichText: true),
      findsNothing,
      reason: 'на экране осталось внутреннее слово панели «$word»',
    );
  }
}

void main() {
  group('бейдж статуса подписки', () {
    testWidgets('дневная норма исчерпана: не "throttled", а человеческая причина', (
      tester,
    ) async {
      final sub = SubPlan.fromJson(
        _freePlanJson(status: 'throttled', usedBytes: 275775488),
      );
      await tester.pumpWidget(_app(SubscriptionCard(sub: sub)));
      await tester.pump();

      expect(tester.takeException(), isNull);
      _expectNoRawStatus(tester);
      // Бейдж уходит в верхний регистр (см. `Tag`), поэтому ищем регистро-
      // независимо; заодно проверяем полный текст причины из AccessCard.
      expect(
        find.textContaining(RegExp('дневной лимит', caseSensitive: false)),
        findsWidgets,
      );
      expect(find.text('Лимит на сегодня закончился'), findsOneWidget);
    });

    for (final status in <String>[
      'expired',
      'pending',
      'banned',
      'disabled',
      'suspended',
    ]) {
      testWidgets('статус панели "$status" не долетает до экрана как есть', (
        tester,
      ) async {
        final sub = SubPlan.fromJson(_freePlanJson(status: status));
        await tester.pumpWidget(_app(SubscriptionCard(sub: sub)));
        await tester.pump();

        expect(tester.takeException(), isNull);
        _expectNoRawStatus(tester);
      });
    }

    testWidgets('активная подписка показывает "Активна", а не "active"', (
      tester,
    ) async {
      final sub = SubPlan.fromJson(_freePlanJson(status: 'active'));
      await tester.pumpWidget(_app(SubscriptionCard(sub: sub)));
      await tester.pump();

      expect(tester.takeException(), isNull);
      expect(find.text('АКТИВНА'), findsOneWidget);
      _expectNoRawStatus(tester);
    });
  });

  group('подпись квоты free-тарифа', () {
    testWidgets('подписана "в день", числами из access, а не "в неделю"', (
      tester,
    ) async {
      final sub = SubPlan.fromJson(
        _freePlanJson(
          status: 'active',
          usedBytes: 100 * 1024 * 1024,
          limitBytes: 200 * 1024 * 1024,
        ),
      );
      await tester.pumpWidget(_app(SubscriptionCard(sub: sub)));
      await tester.pump();

      expect(tester.takeException(), isNull);
      expect(find.textContaining('в неделю'), findsNothing);
      expect(find.textContaining('100 МБ из 200 МБ в день'), findsOneWidget);
    });

    testWidgets(
      'исчерпанная норма: числа подписи совпадают с access, а не с weekly_free_refill_gb',
      (tester) async {
        // used=263 МБ, limit=200 МБ — ровно вектор из access_quota_test.dart.
        final sub = SubPlan.fromJson(
          _freePlanJson(
            status: 'throttled',
            usedBytes: 275775488,
            limitBytes: 209715200,
          ),
        );
        await tester.pumpWidget(_app(SubscriptionCard(sub: sub)));
        await tester.pump();

        expect(tester.takeException(), isNull);
        expect(find.textContaining('263 МБ'), findsWidgets);
        expect(find.textContaining('200 МБ'), findsWidgets);
        expect(find.textContaining('1,37'), findsNothing); // weekly_free_refill_gb
      },
    );
  });

  group('путь к оплате переиспользует AccessCard', () {
    testWidgets('заблокированная подписка ведёт к оплате оператора', (
      tester,
    ) async {
      final sub = SubPlan.fromJson(
        _freePlanJson(status: 'throttled', usedBytes: 275775488),
      );
      await tester.pumpWidget(
        _app(
          SubscriptionCard(sub: sub),
          branding: const Branding(botUrl: 'https://t.me/exa_robot'),
        ),
      );
      await tester.pump();

      expect(tester.takeException(), isNull);
      expect(find.text('Оплатить и не ждать'), findsOneWidget);
    });

    testWidgets('активная подписка не показывает карточку отказа', (
      tester,
    ) async {
      final sub = SubPlan.fromJson(_freePlanJson(status: 'active'));
      await tester.pumpWidget(_app(SubscriptionCard(sub: sub)));
      await tester.pump();

      expect(tester.takeException(), isNull);
      expect(find.textContaining('Оплатить'), findsNothing);
    });
  });
}

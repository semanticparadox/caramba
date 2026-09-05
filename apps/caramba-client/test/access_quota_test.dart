// Исчерпанный лимит как СОСТОЯНИЕ, а не как четыре слоя Go-обёрток.
//
// Владелец добавлял свою подписку и получил на экран вот это, дословно:
//
//   api: загрузка узлов подписки для замера: api: загрузка серверов подписки:
//   subscription: запрос подписки: transport: ни одна включённая ступень не
//   вернула ответ: transport: код состояния 403
//
// Настоящая причина — израсходованные 263 МБ из 200 МБ дневной нормы
// бесплатного тарифа — в этой строке не названа ни разу. Тест фиксирует три
// свойства, которых тогда не было: перевод вместо цепочки, живой список
// серверов под закрытым лимитом и осмысленный ответ на голый 403 старой
// панели.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/branding.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/data/subscription_fetch.dart';
import 'package:caramba_client/features/home/home_screen.dart'
    show dialErrorLabel;
import 'package:caramba_client/features/servers/access_card.dart';
import 'package:caramba_client/features/servers/servers_screen.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_error.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/state/subscription_state.dart';
import 'package:caramba_client/theme/app_theme.dart';

import 'support/fake_core.dart';

/// Ровно та строка, которую увидел владелец.
const _wallOfText =
    'api: загрузка узлов подписки для замера: api: загрузка серверов подписки: '
    'subscription: запрос подписки: transport: ни одна включённая ступень не '
    'вернула ответ: transport: код состояния 403';

/// Внутренние слова, которых пользователь не должен увидеть НИКОГДА.
const _forbidden = <String>[
  'throttled',
  'expired',
  'transport',
  'код состояния',
  '403',
  'subscription:',
  'api:',
];

void _expectHuman(String text) {
  for (final word in _forbidden) {
    expect(
      text.toLowerCase().contains(word.toLowerCase()),
      isFalse,
      reason: 'на экране осталось внутреннее слово «$word»: $text',
    );
  }
}

class _MemoryStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles = <ConnectionProfile>[];
  String? activeId;

  @override
  Future<List<ConnectionProfile>> readProfiles() async => profiles;
  @override
  Future<String?> readActiveId() async => activeId;
  @override
  Future<void> writeProfiles(List<ConnectionProfile> next) async =>
      profiles = next;
  @override
  Future<void> writeActiveId(String? id) async => activeId = id;
  @override
  Future<void> clear() async {
    profiles = <ConnectionProfile>[];
    activeId = null;
  }
}

ConnectionProfile _panelProfile() => const ConnectionProfile(
  id: 'cp_panel',
  type: ProfileType.panelAccount,
  displayName: 'Оператор',
  source: 'https://panel.example',
);

const _servers = <Server>[
  Server(id: 1, name: 'DE-1', countryCode: 'DE', pingMs: 40, status: 'active'),
  Server(id: 5, name: 'CA-1', countryCode: 'CA', pingMs: 120, status: 'active'),
];

/// Подписка в том виде, в каком её отдаёт СЕГОДНЯШНЯЯ панель: объекта `access`
/// в ответе нет, есть только `status` и числа квоты.
Map<String, dynamic> _legacyThrottled({
  int used = 275775488, // 263 МБ
  int limit = 209715200, // 200 МБ
}) => <String, dynamic>{
  'id': 27,
  'subscription_uuid': 'uuid-27',
  'plan_name': 'Free',
  'status': 'throttled',
  'used_traffic_bytes': used,
  'traffic_limit_bytes': limit,
  'quota_period': 'day',
  'is_free': true,
  'daily_traffic_mb': 200,
  'clash_url': 'https://sub.example/clash',
  'config_url': 'https://sub.example/clash',
};

Future<ProviderContainer> _boot({
  required List<Server> servers,
  Map<String, dynamic>? subscriptionJson,
  _MemoryStore? store,
}) async {
  final s =
      store ??
      (_MemoryStore()
        ..profiles = <ConnectionProfile>[_panelProfile()]
        ..activeId = 'cp_panel');
  final container = ProviderContainer(
    overrides: <Override>[
      connectionProfilesStoreProvider.overrideWithValue(s),
      vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
      serversProvider.overrideWith((ref) async => servers),
      if (subscriptionJson != null)
        subscriptionProvider.overrideWith(
          (ref) async => Subscription.fromJson(subscriptionJson),
        ),
    ],
  );
  addTearDown(container.dispose);
  container.read(connectionProfilesProvider);
  for (var i = 0; i < 100; i++) {
    if (!container.read(connectionProfilesProvider).loading) break;
    await Future<void>.delayed(Duration.zero);
  }
  await container.read(serversProvider.future);
  if (subscriptionJson != null) {
    await container.read(subscriptionProvider.future);
  }
  return container;
}

void main() {
  group('состояние доступа из полей старой панели', () {
    test('263 МБ из 200 МБ читаются как исчерпанная дневная норма', () {
      final sub = Subscription.fromJson(_legacyThrottled());
      final a = sub.access;

      expect(a.mayConnect, isFalse);
      expect(sub.isActive, isFalse);
      expect(a.kind, AccessKind.dailyQuota);
      expect(a.usedBytes, 275775488);
      expect(a.limitBytes, 209715200);
      // Панель ВЫЧИТАЕТ дневную норму, а не обнуляет счётчик: 263 - 200 = 63
      // израсходовано, значит завтра останется 137 МБ, а не полные 200.
      expect(a.bytesAfterReset, 209715200 - (275775488 - 209715200));
      expect(a.resetsAt!.isUtc, isTrue);
      expect(a.resetsAt!.hour, 0);
      expect(a.resetsAt!.minute, 0);
      expect(a.resetsAt!.isAfter(DateTime.now().toUtc()), isTrue);
      // Пополнение идёт получасовым тиком, и обещать «ровно в полночь» нельзя.
      expect(a.resetLagSeconds, 1800);
      expect(a.selfHealing, isTrue);
    });

    test('текст называет числа, срок и ни одного внутреннего слова', () {
      final a = Subscription.fromJson(_legacyThrottled()).access;
      final text = '${a.title}. ${a.body}';

      _expectHuman(text);
      expect(text, contains('263 МБ'));
      expect(text, contains('200 МБ'));
      expect(text, contains('137 МБ'));
      expect(text, contains('UTC'));
    });

    test(
      'сожжено больше двух норм: завтра всё равно закрыто, и это сказано',
      () {
        final a = Subscription.fromJson(
          _legacyThrottled(used: 500 * 1024 * 1024),
        ).access;

        expect(a.bytesAfterReset, 0);
        expect(a.body, contains('не хватит'));
        _expectHuman(a.body);
      },
    );

    test('активная подписка не поднимает карточку', () {
      final sub = Subscription.fromJson(<String, dynamic>{
        ..._legacyThrottled(),
        'status': 'active',
      });
      expect(sub.access.mayConnect, isTrue);
      expect(sub.access.isBlocked, isFalse);
      expect(sub.isActive, isTrue);
    });

    test('лимит устройств у элемента списка подписок', () {
      final plan = SubPlan.fromJson(<String, dynamic>{
        'id': 27,
        'plan_name': 'Free',
        'kind': 'free',
        'status': 'expired',
        'device_used': 1,
        'device_limit': 1,
        'expires_at': '2020-01-01T00:00:00Z',
      });
      expect(plan.access.isBlocked, isTrue);
      expect(plan.access.kind, AccessKind.expired);
      expect(plan.isActive, isFalse);
    });
  });

  group('объект access новой панели', () {
    test('коды CSM разбираются в состояние и переживают незнакомый rc', () {
      final json = <String, dynamic>{
        ..._legacyThrottled(),
        'access': <String, dynamic>{
          'may_connect': false,
          'state': 'quota_exceeded',
          'st': 7,
          'rc': 3003,
          'reason': 'daily_allowance_exhausted',
          'used_bytes': 275775488,
          'limit_bytes': 209715200,
          'period': 'day',
          'resets_at': '2030-01-02T00:00:00Z',
          'reset_lag_seconds': 1800,
          'bytes_after_reset': 143654912,
          'devices': <String, dynamic>{'used': 1, 'limit': 1},
          'pay': <String, dynamic>{
            'miniapp_url': 'https://t.me/exa_robot/exaconnect?startapp=plans',
            'bot_url': 'https://t.me/exa_robot',
          },
        },
      };
      final a = Subscription.fromJson(json).access;

      expect(a.kind, AccessKind.dailyQuota);
      expect(a.rc, 3003);
      expect(a.bytesAfterReset, 143654912);
      expect(a.pay.link, 'https://t.me/exa_robot/exaconnect?startapp=plans');
      _expectHuman('${a.title}. ${a.body}');

      // Незнакомый код своей полосы читается как вся полоса, а не как «ошибка».
      final future = AccessState.fromJson(<String, dynamic>{
        'may_connect': false,
        'state': 'quota_exceeded',
        'rc': 3099,
      });
      expect(future!.kind, AccessKind.planQuota);
      _expectHuman(future.body);
    });
  });

  group('перевод отказа', () {
    test('стена текста ядра превращается в предложение о лимите', () {
      final access = Subscription.fromJson(_legacyThrottled()).access;
      final failure = describeText(_wallOfText, access: access);

      expect(failure, isNotNull);
      _expectHuman(failure!.text);
      expect(failure.text, contains('263 МБ'));
      expect(failure.access, AccessKind.dailyQuota);
      expect(failure.payable, isTrue);
      // Повтор вернул бы тот же отказ — кнопки «Повторить» здесь быть не должно.
      expect(failure.retryable, isFalse);
      // Улика сохранена целиком и достаётся по явному запросу.
      expect(failure.technical, _wallOfText);
      expect(technicalDetailFor(failure.text), _wallOfText);
    });

    test(
      'голый 403 старой панели: без состояния подписки — но с действием',
      () {
        final failure = describeText(_wallOfText);

        expect(failure, isNotNull);
        _expectHuman(failure!.text);
        // Панель прячет за этим кодом ровно три ситуации; все три названы.
        expect(failure.text, contains('трафик'));
        expect(failure.text, contains('срок'));
        expect(failure.text, contains('устройств'));
        expect(failure.payable, isTrue);
        expect(failure.technical, _wallOfText);
      },
    );

    test('тексты панели узнаются по телу ответа', () {
      final quota = describeFailure(
        const ApiException(
          'Traffic limit reached. Subscription is expired.',
          statusCode: 403,
        ),
      );
      expect(quota!.access, AccessKind.planQuota);
      _expectHuman(quota.text);

      final device = describeFailure(
        const ApiException('Device limit reached', statusCode: 403),
      );
      expect(device!.access, AccessKind.deviceLimit);
      expect(device.text, contains('устройств'));
      _expectHuman(device.text);

      final inactive = describeFailure(
        const ApiException('Subscription inactive or expired', statusCode: 403),
      );
      expect(inactive!.text, contains('неактивна'));
      _expectHuman(inactive.text);
    });

    test('отказ при импорте подписки переводится тем же правилом', () {
      // fetchSubscriptionBody отдаёт «ответ сервера 403» своей строкой: это
      // тот же 403 панели, и второго словаря для него быть не должно.
      final failure = describeFailure(
        const SubscriptionFetchException('ответ сервера 403'),
      );
      expect(failure, isNotNull);
      _expectHuman(failure!.text);
      expect(failure.payable, isTrue);
    });

    test('сеть и подписка не путаются', () {
      final net = describeText(
        'transport: ни одна включённая ступень не вернула ответ',
      );
      expect(net!.access, isNull);
      expect(net.text, contains('интернет'));
      _expectHuman(net.text);

      final auth = describeFailure(const ApiException('', statusCode: 401));
      expect(auth!.text, contains('Сессия истекла'));
    });

    test('чужое исключение остаётся вызывающему', () {
      expect(coreErrorText(StateError('boom')), isNull);
      expect(describeFailure(StateError('boom')), isNull);
    });
  });

  group('подпись под дайлом', () {
    test('вместо «проверьте сеть» — настоящая причина', () {
      final access = Subscription.fromJson(_legacyThrottled()).access;
      final label = dialErrorLabel(detail: _wallOfText, access: access);

      expect(label, 'Дневной лимит израсходован');
      expect(label.contains('Проверьте сеть'), isFalse);
      _expectHuman(label);
    });

    test('причины нет вовсе — честное предложение повторить', () {
      final label = dialErrorLabel();
      expect(label, contains('Проверьте сеть'));
    });
  });

  group('флот под закрытым лимитом', () {
    test('страны и узлы остаются в списке, помеченные причиной', () async {
      final container = await _boot(
        servers: _servers,
        subscriptionJson: _legacyThrottled(),
      );
      final inventory = container.read(exitInventoryProvider);

      // Главное свойство: список НЕ пустеет.
      expect(inventory.isEmpty, isFalse);
      expect(inventory.locations.length, 2);
      expect(inventory.nodes.length, 2);
      expect(inventory.blockedBy, isNotNull);
      expect(inventory.blockedBy!.kind, AccessKind.dailyQuota);

      for (final l in inventory.locations) {
        expect(l.isAvailable, isFalse);
        expect(l.availability.reason, ExitUnavailableReason.panelRejected);
        expect(l.availability.detail, 'Дневной лимит израсходован');
        _expectHuman(exitUnavailableText(l.availability));
        // Числа задержки, которые пользователь уже видел, не исчезают.
        expect(l.bestPingMs, isNotNull);
      }
      for (final n in inventory.nodes) {
        expect(n.isAvailable, isFalse);
        expect(n.availability.reason, ExitUnavailableReason.panelRejected);
      }
      // Автоподбору идти некуда, и это тоже названо, а не скрыто.
      expect(inventory.bestAvailable, isNull);
    });

    test('активная подписка ничего не помечает', () async {
      final container = await _boot(
        servers: _servers,
        subscriptionJson: <String, dynamic>{
          ..._legacyThrottled(),
          'status': 'active',
        },
      );
      final inventory = container.read(exitInventoryProvider);

      expect(inventory.blockedBy, isNull);
      expect(inventory.locations.every((l) => l.isAvailable), isTrue);
      expect(inventory.remembered, isFalse);
    });

    test('панель перестала отдавать узлы: показан последний список', () async {
      // Сегодняшняя панель выкидывает подписку с исчерпанной нормой из выдачи
      // `/servers` целиком. Сказать «серверов нет» значило бы соврать.
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[_panelProfile()]
        ..activeId = 'cp_panel';
      var servers = _servers;
      final container = ProviderContainer(
        overrides: <Override>[
          connectionProfilesStoreProvider.overrideWithValue(store),
          vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
          serversProvider.overrideWith((ref) async => servers),
          subscriptionProvider.overrideWith(
            (ref) async => Subscription.fromJson(_legacyThrottled()),
          ),
        ],
      );
      addTearDown(container.dispose);
      container.read(connectionProfilesProvider);
      for (var i = 0; i < 100; i++) {
        if (!container.read(connectionProfilesProvider).loading) break;
        await Future<void>.delayed(Duration.zero);
      }
      await container.read(serversProvider.future);
      await container.read(subscriptionProvider.future);
      expect(container.read(exitInventoryProvider).nodes.length, 2);

      servers = const <Server>[];
      container.invalidate(serversProvider);
      await container.read(serversProvider.future);

      final after = container.read(exitInventoryProvider);
      expect(after.nodes.length, 2, reason: 'список не должен исчезать');
      expect(after.remembered, isTrue);
      expect(after.locations.every((l) => !l.isAvailable), isTrue);
    });
  });

  group('карточка доступа', () {
    Widget app(Widget child, {Branding branding = Branding.fallback}) =>
        ProviderScope(
          overrides: <Override>[
            activeBrandingProvider.overrideWithValue(branding),
          ],
          child: MaterialApp(
            theme: AppTheme.dark(),
            home: Scaffold(body: SingleChildScrollView(child: child)),
          ),
        );

    testWidgets('называет причину числами и ведёт к оплате', (tester) async {
      final access = Subscription.fromJson(_legacyThrottled()).access;
      await tester.pumpWidget(
        app(
          AccessCard(access: access),
          branding: const Branding(botUrl: 'https://t.me/exa_robot'),
        ),
      );
      await tester.pump();

      expect(find.text('Лимит на сегодня закончился'), findsOneWidget);
      expect(find.text('Оплатить и не ждать'), findsOneWidget);
      expect(find.textContaining('263 МБ'), findsWidgets);
    });

    testWidgets('бренд не публикует бота: говорим это, а не выдумываем', (
      tester,
    ) async {
      final access = Subscription.fromJson(_legacyThrottled()).access;
      await tester.pumpWidget(app(AccessCard(access: access)));
      await tester.pump();

      expect(find.text('Оплатить и не ждать'), findsNothing);
      expect(
        find.textContaining('не опубликовал адрес для оплаты'),
        findsOneWidget,
      );
    });

    testWidgets('экран серверов: страны видны, помечены и рядом оплата', (
      tester,
    ) async {
      tester.view
        ..physicalSize = const Size(900, 2600)
        ..devicePixelRatio = 1;
      addTearDown(tester.view.reset);

      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[_panelProfile()]
        ..activeId = 'cp_panel';
      await tester.pumpWidget(
        ProviderScope(
          overrides: <Override>[
            connectionProfilesStoreProvider.overrideWithValue(store),
            vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
            serversProvider.overrideWith((ref) async => _servers),
            subscriptionProvider.overrideWith(
              (ref) async => Subscription.fromJson(_legacyThrottled()),
            ),
            apiRelaysProvider.overrideWith((ref) async => const []),
            activeBrandingProvider.overrideWithValue(
              const Branding(botUrl: 'https://t.me/exa_robot'),
            ),
          ],
          child: MaterialApp(
            theme: AppTheme.dark(),
            home: const ServersScreen(),
          ),
        ),
      );
      for (var i = 0; i < 8; i++) {
        await tester.pump(const Duration(milliseconds: 16));
      }

      // Флот на месте: страны видны, а не заменены пустым состоянием.
      expect(find.textContaining('Герман'), findsWidgets);
      // И рядом с ними — причина и путь к оплате.
      expect(find.text('Лимит на сегодня закончился'), findsOneWidget);
      expect(find.text('Оплатить и не ждать'), findsOneWidget);
      expect(find.text('Серверы недоступны'), findsNothing);
    });

    testWidgets('улика доступна, но не в лицо', (tester) async {
      await tester.pumpWidget(
        app(
          const FailureNotice(
            message: 'Лимит на сегодня закончился.',
            technical: _wallOfText,
          ),
        ),
      );
      await tester.pump();

      expect(find.textContaining('код состояния'), findsNothing);
      expect(find.text('Подробности'), findsOneWidget);

      await tester.tap(find.text('Подробности'));
      await tester.pump();
      expect(find.textContaining('код состояния 403'), findsOneWidget);
    });
  });

  test('отказ панели с цифрами объясняется цифрами, а не «тремя случаями»', () {
    // Живой ответ v0.9.81 через RU-зеркало: тело то же самое, что и раньше,
    // а причина и числа приехали заголовками. Пока их не разбирали на импорте,
    // человек читал общий текст поверх конкретного ответа.
    final f = describeFailure(
      const SubscriptionFetchException(
        'ответ сервера 403',
        statusCode: 403,
        body: 'Subscription inactive or expired',
        headers: <String, String>{
          'x-caramba-st': '7',
          'x-caramba-reason': '3003',
          'x-caramba-state': 'quota_exceeded',
          'x-caramba-reason-name': 'daily_allowance_exhausted',
          'x-caramba-used': '262144000',
          'x-caramba-limit': '209715200',
          'x-caramba-period': 'day',
          'x-caramba-resets-at': '1788652800',
          'x-caramba-reset-lag': '1800',
          'x-caramba-bytes-after-reset': '157286400',
        },
      ),
    );

    expect(f, isNotNull);
    expect(f!.text, contains('200 МБ'));
    expect(f.text, contains('250 МБ'));
    expect(f.payable, isTrue);
    expect(f.text, isNot(contains('в трёх случаях')));
    expect(f.text, isNot(contains('throttled')));
  });
}

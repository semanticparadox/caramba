// Автопереподключение при смене пути: окно тишины, отчёт человеку, откат.
//
// Проверяется ровно то, что могло бы тихо сломаться:
//   * смена пути НЕ рвёт туннель мгновенно (окно тишины) и перебор вариантов
//     даёт ОДИН разрыв, а не череду;
//   * первое разрешение профиля на холодном старте событием НЕ считается —
//     иначе каждый запуск поверх живого туннеля рвал бы его без нажатия;
//   * неудача возвращает ТУННЕЛЬ и не трогает ВЫБОР;
//   * половина «настройки» по-прежнему ждёт человека, половина «путь» — нет.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/state/auto_reconnect.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'support/fake_csm_device.dart';

// --------------------------------------------------------------- двойники

/// Записывает вызовы вместо разговора с платформой.
class FakeVpnConnection with FakeCsmDevice implements VpnConnection {
  final _statusCtrl = StreamController<VpnStatus>.broadcast();

  final List<String> calls = <String>[];

  Server? lastServer;
  Map<String, Object?>? lastRawArgs;
  CorePolicy? lastPolicy;
  TunnelMode? lastMode;

  VpnStatus _current = const VpnStatus.disconnected();

  @override
  VpnStatus get currentStatus => _current;

  @override
  Stream<VpnStatus> get status => _statusCtrl.stream;

  @override
  Stream<TrafficStats> get traffic => const Stream<TrafficStats>.empty();

  @override
  Future<void> connect(Server server) async {
    calls.add('connect:${server.id}');
    lastServer = server;
  }

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {
    calls.add('connectRaw:${serverId ?? '-'}');
    lastRawArgs = <String, Object?>{'format': format, 'serverId': serverId};
  }

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async => ImportResult.empty;

  @override
  Future<List<ProbeResult>> probe({
    Duration timeout = const Duration(seconds: 5),
  }) async => const <ProbeResult>[];

  @override
  Future<void> setPolicy(CorePolicy policy) async {
    calls.add('setPolicy');
    lastPolicy = policy;
  }

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 7890}) async {
    calls.add('setTunnelMode');
    lastMode = mode;
  }

  @override
  Future<void> disconnect() async {
    calls.add('disconnect');
    // Живой мост докладывает об остановке кадром, а не возвратом из вызова:
    // на Android `disconnected` печатается следующей строкой после возврата из
    // `core.down()`. Подъём теперь ждёт именно этого кадра, поэтому заглушка,
    // которая его не шлёт, изображала бы не ядро, а зависшую разборку.
    emit(const VpnStatus.disconnected());
  }

  @override
  Future<VpnStatus> refreshStatus() async => _current;

  void emit(VpnStatus status) {
    _current = status;
    _statusCtrl.add(status);
  }

  @override
  Future<void> dispose() async => _statusCtrl.close();
}

/// Стенд для машины переподключения: стадия под управлением теста, попытки —
/// счётчик вызовов, результат приходит наблюдением стадии, как в жизни.
class Bench {
  VpnStage stage = VpnStage.connected;
  final List<String> calls = <String>[];

  /// Команда ушла в ядро (не «туннель поднялся» — это разные факты).
  bool reconnectSent = true;
  bool restoreSent = true;
  bool hasLastGood = true;

  late final AutoReconnectNotifier notifier;

  Bench() {
    notifier = AutoReconnectNotifier(
      reconnect: () async {
        calls.add('reconnect');
        return reconnectSent;
      },
      restoreLastGood: () async {
        calls.add('restore');
        return restoreSent;
      },
      hasLastGood: () => hasLastGood,
      stage: () => stage,
      window: const Duration(milliseconds: 30),
      linger: const Duration(milliseconds: 30),
      attemptLimit: const Duration(milliseconds: 200),
    );
  }

  /// Туннель поднят и план известен — обычная исходная точка.
  void up(RoutePlan plan) {
    notifier.observePlan(plan);
    notifier.observeStage(VpnStage.connected);
  }

  /// Ядро доложило новую стадию.
  void say(VpnStage next) {
    stage = next;
    notifier.observeStage(next);
  }

  Future<void> settle([int ms = 60]) =>
      Future<void>.delayed(Duration(milliseconds: ms));
}

const _base = RoutePlan(profileId: 'cp_1', exitCountry: 'DE');
const _canada = RoutePlan(profileId: 'cp_1', exitCountry: 'CA');
const _canadaHy = RoutePlan(profileId: 'cp_1', exitCountry: 'CA', protocol: 3);

ConnectionProfile rawProfile({String? pin}) => ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Резерв',
  source: 'https://sub.example/x',
  rawConfig: 'proxies: []',
  format: 'clash',
  selectedServerId: pin,
);

VpnNotifier buildVpn(
  FakeVpnConnection conn, {
  ConnectionProfile? profile,
  Server? recommended,
  CorePolicy policy = const CorePolicy(preset: 'ru-smart'),
  TunnelMode mode = TunnelMode.proxy,
}) => VpnNotifier(
  conn,
  () => recommended,
  () => profile,
  () => policy,
  () => mode,
);

void main() {
  group('окно тишины', () {
    test('смена пути не рвёт туннель сразу — сначала обещание', () async {
      final b = Bench()..up(_base);

      b.notifier.observePlan(_canada);

      expect(b.notifier.state.phase, AutoReconnectPhase.pending);
      expect(b.notifier.state.message, startsWith('Применю через '));
      expect(b.notifier.state.message, contains('Канада'));
      // Окно у стенда короткое; боевое — то, что видит человек.
      expect(kQuietWindow.inSeconds, 3);
      expect(b.calls, isEmpty, reason: 'туннель ещё цел');
      b.notifier.dispose();
    });

    test('три смены подряд дают ОДИН разрыв на итоговой комбинации', () async {
      final b = Bench()..up(_base);

      b.notifier.observePlan(_canada);
      await b.settle(10);
      b.notifier.observePlan(_base);
      await b.settle(10);
      b.notifier.observePlan(_canadaHy);

      await b.settle(80);
      expect(b.calls, <String>['reconnect']);
      expect(b.notifier.state.summary, contains('Канада'));
      b.notifier.dispose();
    });

    test('вне сессии смена пути ядро не трогает', () async {
      final b = Bench()..stage = VpnStage.disconnected;
      b.notifier.observePlan(_base);
      b.notifier.observeStage(VpnStage.disconnected);

      b.notifier.observePlan(_canada);

      await b.settle();
      expect(b.notifier.state.phase, AutoReconnectPhase.idle);
      expect(b.calls, isEmpty);
      b.notifier.dispose();
    });

    test('первое разрешение профиля — база, а не событие', () async {
      final b = Bench();
      // Холодный старт поверх живого туннеля: профиль приезжает из пустоты.
      b.notifier.observeStage(VpnStage.connected);
      b.notifier.observePlan(const RoutePlan());
      b.notifier.observePlan(_base);

      await b.settle();
      expect(b.notifier.state.phase, AutoReconnectPhase.idle);
      expect(b.calls, isEmpty, reason: 'никто ничего не менял');
      b.notifier.dispose();
    });

    test('окно истекло на подъёме — ждём up, потом рвём', () async {
      final b = Bench()..up(_base);
      b.stage = VpnStage.connecting;

      b.notifier.observePlan(_canada);
      await b.settle(80);
      expect(b.calls, isEmpty, reason: 'рвать нечего, туннель ещё поднимается');

      b.say(VpnStage.connected);
      await b.settle(20);
      expect(b.calls, <String>['reconnect']);
      b.notifier.dispose();
    });

    test('туннель ушёл сам во время окна — обещание снимается', () async {
      final b = Bench()..up(_base);
      b.notifier.observePlan(_canada);

      b.say(VpnStage.disconnected);

      expect(b.notifier.state.phase, AutoReconnectPhase.idle);
      await b.settle();
      expect(b.calls, isEmpty, reason: 'поднимать туннель без спроса нельзя');
      b.notifier.dispose();
    });
  });

  group('отчёт человеку', () {
    test('успех: «Переподключил», затем баннер гаснет сам', () async {
      final b = Bench()..up(_base);
      b.notifier.observePlan(_canada);
      await b.settle(50);

      expect(b.notifier.state.phase, AutoReconnectPhase.reconnecting);
      expect(b.notifier.state.message, startsWith('Переподключаюсь: '));

      b.say(VpnStage.connected);
      await b.settle(10);
      expect(b.notifier.state.phase, AutoReconnectPhase.done);
      expect(b.notifier.state.message, contains('Переподключил: '));

      await b.settle(60);
      expect(b.notifier.state.phase, AutoReconnectPhase.idle);
      b.notifier.dispose();
    });

    test('ручной баннер не переживает туннель, о котором говорит', () async {
      final b = Bench()..up(_base);
      b.notifier.observePlan(_canada);
      b.notifier.dismiss();
      expect(b.notifier.state.phase, AutoReconnectPhase.manual);

      b.say(VpnStage.disconnected);

      expect(b.notifier.state.phase, AutoReconnectPhase.idle);
      b.notifier.dispose();
    });

    test('«Не сейчас» отменяет автоматику, выбор остаётся', () async {
      final b = Bench()..up(_base);
      b.notifier.observePlan(_canada);

      b.notifier.dismiss();

      expect(b.notifier.state.phase, AutoReconnectPhase.manual);
      await b.settle(80);
      expect(b.calls, isEmpty, reason: 'окно отменено');
      // Выбор не откатывается: план остался тем, что выбрал человек.
      b.notifier.observePlan(_canada);
      expect(b.notifier.state.phase, AutoReconnectPhase.manual);
      b.notifier.dispose();
    });

    test(
      'промежуточные стадии своего же разрыва результатом не считаются',
      () async {
        final b = Bench()..up(_base);
        b.notifier.observePlan(_canada);
        await b.settle(50);

        b.say(VpnStage.disconnected);
        b.say(VpnStage.connecting);
        expect(b.notifier.state.phase, AutoReconnectPhase.reconnecting);

        b.say(VpnStage.connected);
        await b.settle(10);
        expect(b.notifier.state.phase, AutoReconnectPhase.done);
        b.notifier.dispose();
      },
    );
  });

  group('неудача', () {
    test('не поднялось — возвращаем туннель, выбор сохраняем', () async {
      final b = Bench()..up(_base);
      b.notifier.observePlan(_canada);
      await b.settle(50);

      b.say(VpnStage.error);
      await b.settle(20);

      expect(b.calls, <String>['reconnect', 'restore']);
      expect(b.notifier.state.phase, AutoReconnectPhase.reconnecting);
      expect(b.notifier.state.message, startsWith('Возвращаю: '));
      expect(b.notifier.state.message, contains('Германия'));

      b.say(VpnStage.connected);
      await b.settle(20);
      expect(b.notifier.state.phase, AutoReconnectPhase.failed);
      expect(b.notifier.state.message, contains('Канада'));
      expect(b.notifier.state.message, contains('вернул'));
      expect(b.notifier.state.message, contains('Выбор сохранён'));
      b.notifier.dispose();
    });

    test('повторных попыток по таймеру нет', () async {
      final b = Bench()
        ..up(_base)
        ..hasLastGood = false;
      b.notifier.observePlan(_canada);
      await b.settle(50);
      b.say(VpnStage.error);

      await b.settle(150);
      expect(
        b.calls.join(','),
        'reconnect',
        reason: 'цикл разрывов недопустим',
      );
      expect(b.notifier.state.phase, AutoReconnectPhase.failed);
      expect(b.notifier.state.message, contains('вернуть нечем'));
      b.notifier.dispose();
    });

    test('откат тоже не поднялся — говорим прямо', () async {
      final b = Bench()..up(_base);
      b.notifier.observePlan(_canada);
      await b.settle(50);
      b.say(VpnStage.error);
      await b.settle(20);
      b.say(VpnStage.error);
      await b.settle(20);

      expect(b.notifier.state.phase, AutoReconnectPhase.failed);
      expect(b.notifier.state.message, contains('тоже не вышло'));
      b.notifier.dispose();
    });

    test('«Повторить» после отказа делает ровно одну попытку', () async {
      final b = Bench()
        ..up(_base)
        ..hasLastGood = false;
      b.notifier.observePlan(_canada);
      await b.settle(50);
      b.say(VpnStage.error);
      await b.settle(20);
      b.calls.clear();

      unawaited(b.notifier.reconnectNow());
      await b.settle(20);
      b.say(VpnStage.connected);
      await b.settle(20);

      expect(b.calls, <String>['reconnect']);
      expect(b.notifier.state.phase, AutoReconnectPhase.done);
      b.notifier.dispose();
    });

    test('зависшая попытка не оставляет вечное «Переподключаюсь»', () async {
      final b = Bench()
        ..up(_base)
        ..hasLastGood = false;
      b.notifier.observePlan(_canada);
      await b.settle(50);

      // Ядро молчит: ни connected, ни error.
      await b.settle(260);
      expect(b.notifier.state.phase, AutoReconnectPhase.failed);
      b.notifier.dispose();
    });
  });

  group('снимок рабочей комбинации', () {
    test('lastGood записывается только по connected', () async {
      final conn = FakeVpnConnection();
      const server = Server(id: 7, name: 'Node #7', countryCode: 'NL');
      final vpn = buildVpn(conn, recommended: server);

      expect(await vpn.connect(), isTrue);
      expect(vpn.appliedExitKey, '7');
      expect(vpn.lastGood, isNull, reason: 'команда отправлена, но не принята');

      conn.emit(const VpnStatus(stage: VpnStage.connected, server: server));
      await Future<void>.delayed(Duration.zero);
      expect(vpn.lastGood?.exitKey, '7');

      vpn.dispose();
      await conn.dispose();
    });

    test('откат поднимает записанную комбинацию, а не текущий выбор', () async {
      final conn = FakeVpnConnection();
      const germany = Server(id: 1, name: 'DE', countryCode: 'DE');
      const canada = Server(id: 2, name: 'CA', countryCode: 'CA');
      var recommended = germany;
      final vpn = VpnNotifier(
        conn,
        () => recommended,
        () => null,
        () => const CorePolicy(preset: 'ru-smart'),
        () => TunnelMode.proxy,
      );

      await vpn.connect();
      conn.emit(const VpnStatus(stage: VpnStage.connected, server: germany));
      await Future<void>.delayed(Duration.zero);

      // Человек выбрал Канаду, туннель туда не пошёл.
      recommended = canada;
      await vpn.connect();
      conn.calls.clear();

      expect(await vpn.restoreLastGood(), isTrue);
      expect(conn.calls.last, 'connect:1', reason: 'вернулись в Германию');
      // Выбор пользователя откат не трогает.
      expect(recommended, canada);

      vpn.dispose();
      await conn.dispose();
    });

    test(
      'возвращаться некуда — говорим false, а не поднимаем что попало',
      () async {
        final conn = FakeVpnConnection();
        final vpn = buildVpn(conn, profile: rawProfile(pin: 'NL-01'));

        expect(await vpn.restoreLastGood(), isFalse);
        expect(conn.calls, isEmpty);

        vpn.dispose();
        await conn.dispose();
      },
    );

    test('сырой откат несёт записанный пин, а не текущий', () async {
      final conn = FakeVpnConnection();
      var profile = rawProfile(pin: 'DE-01');
      final vpn = VpnNotifier(
        conn,
        () => null,
        () => profile,
        () => const CorePolicy(preset: 'ru-smart'),
        () => TunnelMode.proxy,
      );

      await vpn.connect();
      conn.emit(const VpnStatus(stage: VpnStage.connected));
      await Future<void>.delayed(Duration.zero);

      profile = rawProfile(pin: 'CA-01');
      await vpn.connect();
      expect(conn.calls.last, 'connectRaw:CA-01');

      await vpn.restoreLastGood();
      expect(conn.calls.last, 'connectRaw:DE-01');

      vpn.dispose();
      await conn.dispose();
    });

    test('reconnect() опускает ЖИВОЙ туннель перед подъёмом', () async {
      final conn = FakeVpnConnection();
      const server = Server(id: 9, name: 'x', countryCode: 'NL');
      final vpn = buildVpn(conn, recommended: server);
      conn.emit(const VpnStatus(stage: VpnStage.connected, server: server));
      await Future<void>.delayed(Duration.zero);

      expect(await vpn.reconnect(), isTrue);

      expect(conn.calls.first, 'disconnect');
      expect(conn.calls.last, 'connect:9');

      vpn.dispose();
      await conn.dispose();
    });

    test(
      'переподключение опущенного туннеля не опускает его ещё раз',
      () async {
        // Опускать нечего, ждать подтверждения нечего: обычное подключение
        // обязано остаться таким же коротким, каким было.
        final conn = FakeVpnConnection();
        const server = Server(id: 9, name: 'x', countryCode: 'NL');
        final vpn = buildVpn(conn, recommended: server);

        expect(await vpn.reconnect(), isTrue);

        expect(conn.calls, isNot(contains('disconnect')));
        expect(conn.calls.last, 'connect:9');

        vpn.dispose();
        await conn.dispose();
      },
    );
  });

  group('деление политики на путь и настройки', () {
    test('путь из отпечатка настроек исключён', () {
      const a = CorePolicy(protocol: 'TUIC', preset: 'global', relay: 'TR');
      const b = CorePolicy(
        protocol: 'Hysteria2',
        preset: 'ru-smart',
        relay: 'KZ',
      );
      expect(settingsSignature(a), settingsSignature(b));
    });

    test('настройка в отпечаток входит', () {
      const a = CorePolicy(preset: 'global', adblock: false);
      const b = CorePolicy(preset: 'global', adblock: true);
      expect(settingsSignature(a), isNot(settingsSignature(b)));
    });

    test('новое поле политики попадает в НАСТРОЙКИ, а не в путь', () {
      // Вычитание, а не перечисление: поле, которого не было вчера, обязано
      // уехать в сторону «спросить человека».
      const a = CorePolicy(preset: 'global', mtu: 1280);
      const b = CorePolicy(preset: 'global', mtu: 1400);
      expect(settingsSignature(a), isNot(settingsSignature(b)));
    });
  });

  group('план пути', () {
    test('план без профиля событием быть не может', () {
      expect(const RoutePlan().resolved, isFalse);
      expect(const RoutePlan(profileId: 'cp_1').resolved, isTrue);
    });

    test('имя комбинации читается человеком', () {
      expect(_canada.summary, startsWith('Канада · '));
      expect(const RoutePlan(profileId: 'x').summary, startsWith('Авто · '));
    });

    test('индекс вне списка имя не выдумывает', () {
      const wild = RoutePlan(profileId: 'x', protocol: 99, route: 99);
      expect(wild.summary, 'Авто');
    });
  });

  group('баннер', () {
    /// Баннер с подставленной машиной: фазы гоняются напрямую, без ядра.
    ///
    /// Время внутри `testWidgets` фальшивое, поэтому окно тишины двигается
    /// `tester.pump(Duration)`, а не `Future.delayed`: настоящая задержка в
    /// этой зоне не наступает никогда.
    Future<void> pump(WidgetTester tester, AutoReconnectNotifier n) async {
      await tester.pumpWidget(
        ProviderScope(
          overrides: <Override>[autoReconnectProvider.overrideWith((_) => n)],
          child: MaterialApp(
            theme: AppTheme.dark(),
            home: const Scaffold(body: ReconnectBanner()),
          ),
        ),
      );
      await tester.pump();
    }

    testWidgets('обещание называет комбинацию и даёт отказаться', (
      tester,
    ) async {
      final b = Bench()..up(_base);
      b.notifier.observePlan(_canada);
      await pump(tester, b.notifier);

      expect(find.textContaining('Применю через'), findsOneWidget);
      expect(find.textContaining('Канада'), findsOneWidget);

      await tester.tap(find.text('Не сейчас'));
      await tester.pump();

      expect(find.text(kManualReconnectText), findsOneWidget);
      expect(find.text('Переподключить'), findsOneWidget);
      expect(b.calls, isEmpty, reason: 'отказ туннель не рвёт');
    });

    testWidgets('пока попытка в полёте, кнопки нет; успех сам гаснет', (
      tester,
    ) async {
      final b = Bench()..up(_base);
      b.notifier.observePlan(_canada);
      await pump(tester, b.notifier);

      await tester.pump(const Duration(milliseconds: 40));
      expect(find.textContaining('Переподключаюсь: '), findsOneWidget);
      expect(find.byType(TextButton), findsNothing);

      b.say(VpnStage.connected);
      await tester.pump();
      expect(find.textContaining('Переподключил: '), findsOneWidget);

      await tester.pump(const Duration(milliseconds: 40));
      expect(find.text(kManualReconnectText), findsOneWidget);
    });

    testWidgets('отказ предлагает повтор, а не тупик', (tester) async {
      final b = Bench()
        ..up(_base)
        ..hasLastGood = false;
      b.notifier.observePlan(_canada);
      await pump(tester, b.notifier);

      await tester.pump(const Duration(milliseconds: 40));
      b.say(VpnStage.error);
      await tester.pump();

      expect(find.textContaining('не поднялось'), findsOneWidget);
      expect(find.text('Повторить'), findsOneWidget);
    });
  });

  _outcomeUnchangedTests();
}

// План меняется, а провод — нет.
//
// Снято на устройстве: ядро стояло на «🇨🇦 Stream», человек нажал «Канада», и
// приложение закрепило «🇨🇦 Stealth» — другой вход ТОЙ ЖЕ машины. Выходной
// адрес до и после разрыва совпадал байт в байт, а туннель всё равно порвался.
// Сравнение по стране этот случай пропускало: вместе со страной менялся узел.
void _outcomeUnchangedTests() {
  group('переподключение по результату, а не по плану', () {
    // Флот: две машины, у каждой по два входа.
    const machines = <String, String>{
      'ca-stream': 'ca-1',
      'ca-stealth': 'ca-1',
      'de-stream': 'de-1',
      'de-secure': 'de-1',
    };

    AutoReconnectNotifier make({
      required String liveNode,
      required List<String> calls,
    }) => AutoReconnectNotifier(
      reconnect: () async {
        calls.add('reconnect');
        return true;
      },
      restoreLastGood: () async => true,
      hasLastGood: () => false,
      stage: () => VpnStage.connected,
      liveCountry: () => liveNode.startsWith('ca') ? 'CA' : 'DE',
      liveMachine: () => machines[liveNode] ?? '',
      machineOf: (k) => machines[k] ?? '',
      window: const Duration(milliseconds: 10),
    );

    Future<void> settle() =>
        Future<void>.delayed(const Duration(milliseconds: 60));

    test('другой вход той же машины туннель не рвёт', () async {
      final calls = <String>[];
      final n = make(liveNode: 'ca-stream', calls: calls);

      n.observePlan(const RoutePlan(profileId: 'p1', exitNode: 'ca-stream'));
      n.observePlan(
        const RoutePlan(
          profileId: 'p1',
          exitCountry: 'CA',
          exitNode: 'ca-stealth',
        ),
      );
      await settle();

      expect(calls, isEmpty);
    });

    test('снятие закрепления на «авто» не рвёт', () async {
      final calls = <String>[];
      final n = make(liveNode: 'ca-stream', calls: calls);

      n.observePlan(
        const RoutePlan(
          profileId: 'p1',
          exitCountry: 'CA',
          exitNode: 'ca-stream',
        ),
      );
      n.observePlan(const RoutePlan(profileId: 'p1'));
      await settle();

      expect(calls, isEmpty);
    });

    test('вход ДРУГОЙ машины рвёт', () async {
      final calls = <String>[];
      final n = make(liveNode: 'ca-stream', calls: calls);

      n.observePlan(const RoutePlan(profileId: 'p1', exitNode: 'ca-stream'));
      n.observePlan(
        const RoutePlan(
          profileId: 'p1',
          exitCountry: 'DE',
          exitNode: 'de-stream',
        ),
      );
      await settle();

      expect(calls, contains('reconnect'));
    });

    test('смена типа рвёт даже на той же машине', () async {
      final calls = <String>[];
      final n = make(liveNode: 'ca-stream', calls: calls);

      n.observePlan(const RoutePlan(profileId: 'p1', exitNode: 'ca-stream'));
      n.observePlan(
        const RoutePlan(profileId: 'p1', exitNode: 'ca-stealth', protocol: 2),
      );
      await settle();

      expect(calls, contains('reconnect'));
    });

    test('панельный путь: ключ узла это сразу ключ машины', () async {
      // На панельном профиле в план едет `nodes.id`, а не имя прокси, и поиск
      // по именам не найдёт ничего никогда. Такой ключ должен опознаваться
      // среди самих машин, иначе правило на панели не работает вовсе.
      final calls = <String>[];
      final n = AutoReconnectNotifier(
        reconnect: () async {
          calls.add('reconnect');
          return true;
        },
        restoreLastGood: () async => true,
        hasLastGood: () => false,
        stage: () => VpnStage.connected,
        liveMachine: () => 'ca-1',
        machineOf: (k) => machines[k] ?? (k == 'ca-1' ? 'ca-1' : ''),
        window: const Duration(milliseconds: 10),
      );

      n.observePlan(const RoutePlan(profileId: 'p1', exitNode: 'ca-stream'));
      n.observePlan(
        const RoutePlan(profileId: 'p1', exitCountry: 'CA', exitNode: 'ca-1'),
      );
      await settle();

      expect(calls, isEmpty);
    });

    test('незнакомая машина — рвём, а не гадаем', () async {
      final calls = <String>[];
      final n = make(liveNode: 'ca-stream', calls: calls);

      n.observePlan(const RoutePlan(profileId: 'p1', exitNode: 'ca-stream'));
      n.observePlan(const RoutePlan(profileId: 'p1', exitNode: 'неизвестный'));
      await settle();

      expect(calls, contains('reconnect'));
    });
  });
}

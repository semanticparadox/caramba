// Флаг страны и АВТОР задержки.
//
// Две просьбы владельца продукта: показывать флаги стран и показывать пинг
// пользователя, а не панели. Обе легко выполнить неправдиво — нарисовать флаг
// там, где страна неизвестна, и оставить чужое число под подписью «задержка», —
// поэтому проверяется именно граница правдивости:
//   * неизвестная (и ненадёжно угаданная) страна получает нейтральный глиф,
//     а не чей-то флаг;
//   * пока своего замера нет, показанное число НАЗВАНО операторским;
//   * пришедший собственный замер вытесняет операторский и назван своим именем;
//   * строка без единого числа честно говорит «меряю», а не рисует прочерк.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/features/servers/servers_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_models.dart';
import 'package:caramba_client/vpn/vpn_service.dart' show VpnConfig;
import 'package:caramba_client/widgets/ui.dart';

import 'support/fake_core.dart';

/// Ядро, чей замер можно задержать: без этого «меряю» существует один кадр и
/// проверить его нельзя.
class _HeldProbeCore extends FakeVpnCore {
  final List<ProbeResult> results;
  final Completer<List<ProbeResult>> _gate = Completer<List<ProbeResult>>();

  _HeldProbeCore(this.results);

  /// Отпускает замер: строки до этого момента показывают то, что у них есть.
  void release() {
    if (!_gate.isCompleted) _gate.complete(results);
  }

  @override
  Future<List<ProbeResult>> probe({Duration timeout = Duration.zero}) =>
      _gate.future;
}

class _MemStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles;
  String? activeId;

  _MemStore(this.profiles, this.activeId);

  @override
  Future<List<ConnectionProfile>> readProfiles() async => profiles;

  @override
  Future<String?> readActiveId() async => activeId;

  @override
  Future<void> writeProfiles(List<ConnectionProfile> next) async {
    profiles = next;
  }

  @override
  Future<void> writeActiveId(String? id) async {
    activeId = id;
  }

  @override
  Future<void> clear() async {
    profiles = const <ConnectionProfile>[];
    activeId = null;
  }
}

/// Строка `/servers`, как её отдаёт панель: `flag` считается ей алгоритмически,
/// а `inbounds[].proxy_name` — тот самый мост между узлом и именем прокси, под
/// которым ядро возвращает замер.
Map<String, dynamic> _serverRow({
  required int id,
  required String? country,
  required String flag,
  int? latencyMs,
  String proxyName = '',
}) => <String, dynamic>{
  'id': id,
  'name': 'Node #$id',
  'country_code': country,
  'flag': flag,
  'latency_ms': latencyMs,
  'load_pct': 10.0,
  'status': 'online',
  'inbounds': <Map<String, dynamic>>[
    <String, dynamic>{
      'tag': 'vless-in',
      'protocol': 'vless',
      'network': 'tcp',
      'security': 'tls',
      'port': 443,
      'label': 'VLESS',
      'proxy_name': proxyName.isEmpty ? '🇩🇪 DE-$id' : proxyName,
      'available': true,
    },
  ],
};

/// Панельный шов из полей активного профиля — ровно то, что настоящий
/// резолвер собрал бы, будь секретное хранилище доступно.
VpnConfig? _seamOf(_MemStore store) {
  final profile = store.profiles.firstWhere((p) => p.isPanel);
  return VpnConfig(
    panelUrl: profile.panelUrl ?? '',
    subscriptionUuid: profile.subscriptionUuid ?? '',
    accessToken: 'access-token',
    refreshToken: 'refresh-token',
    accessExpiry: DateTime.now().add(const Duration(minutes: 15)),
  );
}

ConnectionProfile _panelProfile() => const ConnectionProfile(
  id: 'cp_panel',
  type: ProfileType.panelAccount,
  displayName: 'Панель',
  source: 'https://panel.example',
  panelUrl: 'https://panel.example',
  subscriptionUuid: 'sub-uuid',
);

Widget _app({
  required List<Server> servers,
  required FakeVpnCore core,
  required _MemStore store,
}) => ProviderScope(
  overrides: [
    vpnConnectionProvider.overrideWithValue(core),
    connectionProfilesStoreProvider.overrideWithValue(store),
    serversProvider.overrideWith((ref) async => servers),
    // Шов сессии для замера. Настоящий резолвер читает пару токенов из
    // платформенного secure storage, а его канал в тесте не зарегистрирован —
    // такой `read` не бросает, а НИКОГДА не завершается, и замер повис бы на
    // нём. Здесь проверяется порядок вызовов (шов уходит до замера), а не
    // содержимое сессии, поэтому подменяем резолвер целиком.
    probeSeamResolverProvider.overrideWithValue(() async => _seamOf(store)),
  ],
  child: MaterialApp(theme: AppTheme.dark(), home: const ServersScreen()),
);

/// Высокое окно: список строится ленивым сливером, и строка вне вьюпорта в
/// дерево не попадает — тест проверял бы прокрутку, а не содержимое.
void _useTallView(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(900, 2600)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

/// Канал плагина: на платформе он есть, и `CarambaVpn.instance.configure`
/// уходит именно в него. Мок нужен, чтобы тест проверял ПОРЯДОК вызовов —
/// панельный шов обязан уйти ДО замера, иначе ядру нечего мерить.
final List<String> nativeCalls = <String>[];

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() {
    nativeCalls.clear();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel('com.caramba/vpn'), (
          call,
        ) async {
          nativeCalls.add(call.method);
          return null;
        });
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel('com.caramba/vpn'), null);
  });

  group('флаг страны', () {
    test('панельный флаг проходит как есть, когда сходится с кодом', () {
      expect(flagOf('DE', '🇩🇪'), '🇩🇪');
      expect(flagOf('nl', '🇳🇱'), '🇳🇱');
    });

    test('без панельного флага он выводится из кода', () {
      // Это не догадка: та же арифметика Regional Indicator, по которой считает
      // и сама панель.
      expect(flagOf('DE'), '🇩🇪');
      expect(flagOf('DE', ''), '🇩🇪');
      expect(flagOf('DE', '🌐'), '🇩🇪');
    });

    test('код авторитетнее панельного флага при расхождении', () {
      // Флаг, спорящий с кодом, — это флаг не той страны на строке: список
      // сгруппирован по коду, и глиф обязан описывать ту же группу.
      expect(flagOf('DE', '🇺🇸'), '🇩🇪');
    });

    test('страна неизвестна — нейтральный глиф, а не чужой флаг', () {
      expect(flagOf(null), kNeutralFlag);
      expect(flagOf(''), kNeutralFlag);
      expect(flagOf('Германия'), kNeutralFlag);
      // Панель подставляет узлу без страны `US` и присылает уверенное 🇺🇸.
      // Именно этот случай нейтральный глиф и закрывает.
      expect(flagOf(null, '🇺🇸'), kNeutralFlag);
      expect(flagOf('', '🇺🇸'), kNeutralFlag);
    });

    test('узел панели без country_code не получает флага панели', () {
      final node = ExitNode.fromServer(
        Server.fromJson(
          _serverRow(id: 7, country: null, flag: '🇺🇸', latencyMs: 12),
        ),
      );
      expect(node.countryCode, '');
      expect(node.flag, kNeutralFlag);
    });

    test('узел панели со страной получает её флаг', () {
      final node = ExitNode.fromServer(
        Server.fromJson(
          _serverRow(id: 3, country: 'DE', flag: '🇩🇪', latencyMs: 40),
        ),
      );
      expect(node.countryCode, 'DE');
      expect(node.flag, '🇩🇪');
    });

    test('импорт: флаг только там, где оператор написал его сам', () {
      // Флаг в имени написал человек — это не догадка.
      final strong = ExitNode.fromImported(
        const ImportedServer(
          id: 'p1',
          name: '🇩🇪 Frankfurt',
          type: 'vless',
          server: 'a.example',
          port: 443,
          country: 'DE',
        ),
      );
      expect(strong.flag, '🇩🇪');

      // А здесь «страна» — двухбуквенное слово из свободного имени: ядро
      // выведет из `my-node` Малайзию. Код остаётся (список по нему и
      // группируется), флага нет.
      final weak = ExitNode.fromImported(
        const ImportedServer(
          id: 'p2',
          name: 'my-node',
          type: 'vless',
          server: 'b.example',
          port: 443,
          country: 'MY',
        ),
      );
      expect(weak.countryCode, 'MY');
      expect(weak.flag, kNeutralFlag);
    });

    test('страна без единого твёрдого узла остаётся без флага', () {
      final nodes = <ExitNode>[
        ExitNode.fromImported(
          const ImportedServer(
            id: 'p2',
            name: 'my-node',
            type: 'vless',
            server: 'b.example',
            port: 443,
            country: 'MY',
          ),
        ),
      ];
      final loc = ExitLocation.fromNodes(
        'MY',
        nodes,
        source: ExitInventorySource.importedSub,
      );
      expect(loc.flag, kNeutralFlag);
    });

    testWidgets('чип неизвестной страны: нейтральный глиф и «··»', (
      tester,
    ) async {
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.dark(),
          home: const Scaffold(
            body: FlagChip(flag: kNeutralFlag, code: ''),
          ),
        ),
      );
      expect(find.text(kNeutralFlag), findsOneWidget);
      expect(find.text('··'), findsOneWidget);
    });
  });

  group('автор задержки', () {
    test('число панели названо операторским', () {
      const node = ExitNode(
        key: '1',
        name: 'Node #1',
        countryCode: 'DE',
        source: ExitInventorySource.panelRest,
        pingMs: 42,
      );
      expect(node.latency.source, LatencySource.operator);
      expect(node.latency.ms, 42);
    });

    test('собственный замер вытесняет операторский', () {
      const node = ExitNode(
        key: '1',
        name: 'Node #1',
        countryCode: 'DE',
        source: ExitInventorySource.panelRest,
        pingMs: 42,
        measuredMs: 188,
      );
      expect(node.latency.source, LatencySource.client);
      expect(node.latency.ms, 188);
    });

    test('без числа вовсе идущий замер виден как «меряю»', () {
      const node = ExitNode(
        key: 'p1',
        name: 'proxy',
        countryCode: 'DE',
        source: ExitInventorySource.importedSub,
        measuring: true,
      );
      expect(node.latency.source, LatencySource.measuring);
      expect(node.latency.ms, isNull);
      expect(node.latency.bucket, isNull);
    });

    test('замер не отнимает уже показанное число оператора', () {
      const node = ExitNode(
        key: '1',
        name: 'Node #1',
        countryCode: 'DE',
        source: ExitInventorySource.panelRest,
        pingMs: 42,
        measuring: true,
      );
      expect(node.latency.source, LatencySource.operator);
      expect(node.latency.ms, 42);
    });

    test('страна берёт собственный замер, а не лучший операторский', () {
      const nodes = <ExitNode>[
        ExitNode(
          key: '1',
          name: 'a',
          countryCode: 'DE',
          source: ExitInventorySource.panelRest,
          pingMs: 12,
        ),
        ExitNode(
          key: '2',
          name: 'b',
          countryCode: 'DE',
          source: ExitInventorySource.panelRest,
          pingMs: 30,
          measuredMs: 180,
        ),
      ];
      final loc = ExitLocation.fromNodes(
        'DE',
        nodes,
        source: ExitInventorySource.panelRest,
      );
      // 12 мс оператора «лучше» 180 мс собственных, но это разные величины:
      // смешав их, страна показала бы расстояние узла до его цели как
      // расстояние пользователя.
      expect(loc.bestLatency.source, LatencySource.client);
      expect(loc.bestPingMs, 180);
    });

    testWidgets('подпись называет автора числа', (tester) async {
      await tester.pumpWidget(
        MaterialApp(
          theme: AppTheme.dark(),
          home: const Scaffold(
            body: Column(
              children: [
                LatencyReadout(Latency.fromOperator(42)),
                LatencyReadout(Latency.fromClient(188)),
                LatencyReadout(Latency.measuring),
                LatencyReadout(Latency.none),
              ],
            ),
          ),
        ),
      );
      expect(find.text('42 мс'), findsOneWidget);
      expect(find.text('от оператора'), findsOneWidget);
      expect(find.text('188 мс'), findsOneWidget);
      expect(find.text('ваш пинг'), findsOneWidget);
      expect(find.text('меряю'), findsOneWidget);
      // «Не мерили» — прочерк, а не ноль и не чужое число.
      expect(find.text('-'), findsOneWidget);
    });
  });

  group('экран серверов: панельный путь меряет сам', () {
    testWidgets(
      'до своего замера показано число оператора и названо операторским',
      (tester) async {
        _useTallView(tester);
        final core = _HeldProbeCore(const <ProbeResult>[]);
        await tester.pumpWidget(
          _app(
            servers: <Server>[
              Server.fromJson(
                _serverRow(
                  id: 3,
                  country: 'DE',
                  flag: '🇩🇪',
                  latencyMs: 42,
                  proxyName: '🇩🇪 DE-3',
                ),
              ),
            ],
            core: core,
            store: _MemStore(<ConnectionProfile>[_panelProfile()], 'cp_panel'),
          ),
        );
        await tester.pump();
        await tester.pump();
        await tester.pump();

        expect(find.text('42 мс'), findsWidgets);
        expect(find.text('от оператора'), findsWidgets);
        expect(find.text('ваш пинг'), findsNothing);
        // Шапка говорит это же словами, один раз и до всякой цифры.
        expect(
          find.textContaining('показаны задержки, которые сообщил оператор'),
          findsOneWidget,
        );

        core.release();
        await tester.pumpAndSettle();
      },
    );

    testWidgets('пришедший собственный замер вытесняет число оператора', (
      tester,
    ) async {
      _useTallView(tester);
      // Ядро отвечает ИМЕНАМИ ПРОКСИ; узел панели — числом. Мост между ними
      // это `inbounds[].proxy_name`, и если он оборвётся, замер некуда будет
      // положить: тест сломается именно здесь, а не «где-то в UI».
      final core = _HeldProbeCore(const <ProbeResult>[
        ProbeResult(
          id: '🇩🇪 DE-3',
          name: '🇩🇪 DE-3',
          country: 'DE',
          latencyMs: 188,
        ),
      ]);
      await tester.pumpWidget(
        _app(
          servers: <Server>[
            Server.fromJson(
              _serverRow(
                id: 3,
                country: 'DE',
                flag: '🇩🇪',
                latencyMs: 42,
                proxyName: '🇩🇪 DE-3',
              ),
            ),
          ],
          core: core,
          store: _MemStore(<ConnectionProfile>[_panelProfile()], 'cp_panel'),
        ),
      );
      await tester.pump();
      await tester.pump();
      await tester.pump();

      core.release();
      await tester.pumpAndSettle();

      expect(find.text('188 мс'), findsWidgets);
      expect(find.text('ваш пинг'), findsWidgets);
      expect(find.text('42 мс'), findsNothing);
      expect(find.text('от оператора'), findsNothing);

      // Панельный шов ушёл на нативную сторону в ходе замера. Без него ядро,
      // которое приложение держит под метаданные, о панели не знает: раньше шов
      // отправлялся только из `connect`, то есть уже после того, как
      // пользователь выбрал узел вслепую.
      expect(
        nativeCalls,
        contains('configure'),
        reason: 'замер на панельном пути обязан отдать ядру шов: $nativeCalls',
      );
    });

    testWidgets('узел без числа оператора виден как «меряю», а не пустым', (
      tester,
    ) async {
      _useTallView(tester);
      final core = _HeldProbeCore(const <ProbeResult>[]);
      await tester.pumpWidget(
        _app(
          servers: <Server>[
            Server.fromJson(
              _serverRow(
                id: 5,
                country: 'DE',
                flag: '🇩🇪',
                proxyName: '🇩🇪 DE-5',
              ),
            ),
          ],
          core: core,
          store: _MemStore(<ConnectionProfile>[_panelProfile()], 'cp_panel'),
        ),
      );
      await tester.pump();
      await tester.pump();
      await tester.pump();

      // Список НЕ ждёт замера: страна уже на экране, а её задержка ещё меряется.
      expect(find.text('Германия'), findsOneWidget);
      expect(find.text('меряю'), findsWidgets);

      core.release();
      await tester.pumpAndSettle();
    });
  });
}

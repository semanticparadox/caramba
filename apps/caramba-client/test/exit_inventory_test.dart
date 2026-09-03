// Инвентарь выходов одинаков во всех трёх мирах приложения.
//
// Панель по REST, импортированная подписка без панели и подписанный каталог
// CSM отдают узлы по-разному, но экран обязан видеть один список стран. Тест
// фиксирует именно это: имена провайдеров, форму [ExitInventory] и — отдельно
// — что недоступный вариант остаётся В СПИСКЕ и несёт машиночитаемую причину.
// Спрятанный вариант тут был бы регрессией, а не оптимизацией.

import 'dart:convert';

import 'package:dio/dio.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/state/subscription_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

import 'support/fake_core.dart';

/// Хранилище профилей в памяти: тесту не нужен платформенный keychain.
class _MemoryStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles = <ConnectionProfile>[];
  String? activeId;

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
    profiles = <ConnectionProfile>[];
    activeId = null;
  }
}

/// Отдаёт заданный ответ вместо сети и запоминает тело запроса.
class _StubAdapter implements HttpClientAdapter {
  final String body;
  final List<String> paths = <String>[];
  final List<String> bodies = <String>[];

  _StubAdapter({this.body = '{}'});

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<List<int>>? requestStream,
    Future<void>? cancelFuture,
  ) async {
    paths.add(options.path);
    bodies.add(jsonEncode(options.data));
    return ResponseBody.fromString(
      body,
      200,
      headers: <String, List<String>>{
        Headers.contentTypeHeader: <String>[Headers.jsonContentType],
      },
    );
  }

  @override
  void close({bool force = false}) {}
}

ConnectionProfile _panelProfile({
  String? country,
  int? nodeId,
  String? panelUrl,
}) => ConnectionProfile(
  id: 'cp_panel',
  type: ProfileType.panelAccount,
  displayName: 'Оператор',
  source: 'https://panel.example',
  panelUrl: panelUrl,
  selectedExitCountry: country,
  selectedExitNodeId: nodeId,
);

ConnectionProfile _rawProfile({
  List<ImportedServer> servers = const <ImportedServer>[],
  ProbeSnapshot? probe,
  String? country,
  String? pinned,
}) => ConnectionProfile(
  id: 'cp_raw',
  type: ProfileType.rawSub,
  displayName: 'Импорт',
  source: 'https://sub.example/x',
  servers: servers,
  lastProbe: probe,
  selectedExitCountry: country,
  selectedServerId: pinned,
);

/// Панельная выдача: живой и переполненный узел в DE, живой CA, страна целиком
/// переполнена в RU и один узел без страны.
///
/// Живые узлы носят `active` — ровно ту строку, которую отдаёт боевая панель.
/// Дефолт конструктора (`online`) здесь был бы фикстурой из документации, а не
/// из продакшена, и любой белый список статусов проходил бы тесты, оставив
/// пользователю список стран, в котором не нажимается ни одна строка.
const _servers = <Server>[
  Server(id: 1, name: 'DE-1', countryCode: 'DE', pingMs: 40, status: 'active'),
  Server(id: 2, name: 'DE-2', countryCode: 'DE', pingMs: 30, status: 'full'),
  Server(id: 5, name: 'CA-1', countryCode: 'CA', pingMs: 120, status: 'active'),
  Server(id: 9, name: 'RU-1', countryCode: 'RU', pingMs: 10, status: 'full'),
  Server(id: 11, name: 'Node #11', pingMs: 15, status: 'active'),
];

Subscription _subscription() => const Subscription(
  id: 7,
  subscriptionUuid: 'uuid',
  status: 'active',
  clashUrl: 'https://sub.example/clash',
  configUrl: 'https://sub.example/clash',
);

Future<ProviderContainer> _boot({
  required List<ConnectionProfile> profiles,
  List<Server>? servers,
  ExitInventory? catalog,
  _MemoryStore? store,
  ApiClient? api,
  Subscription? subscription,
  _RecordingCore? core,
}) async {
  final s =
      store ??
      (_MemoryStore()
        ..profiles = profiles
        ..activeId = profiles.isEmpty ? null : profiles.first.id);
  final container = ProviderContainer(
    overrides: <Override>[
      connectionProfilesStoreProvider.overrideWithValue(s),
      if (servers != null) serversProvider.overrideWith((ref) async => servers),
      if (catalog != null) csmExitCatalogProvider.overrideWithValue(catalog),
      if (api != null) apiClientProvider.overrideWithValue(api),
      if (core != null) vpnConnectionProvider.overrideWithValue(core),
      if (subscription != null)
        subscriptionProvider.overrideWith((ref) async => subscription),
    ],
  );
  addTearDown(container.dispose);
  // Нотифаер грузится из стора асинхронно: дожидаемся, иначе первое чтение
  // придётся на пустой список профилей.
  container.read(connectionProfilesProvider);
  for (var i = 0; i < 100; i++) {
    if (!container.read(connectionProfilesProvider).loading) break;
    await Future<void>.delayed(Duration.zero);
  }
  if (servers != null) await container.read(serversProvider.future);
  if (subscription != null) await container.read(subscriptionProvider.future);
  return container;
}

/// Ядро, запоминающее, какое имя прокси ему передали на сыром пути.
class _RecordingCore extends FakeVpnCore {
  String? rawServerId;
  var connectedRaw = false;

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {
    connectedRaw = true;
    rawServerId = serverId;
  }
}

/// ApiClient, у которого панель ЕСТЬ, а сеть подменена стабом.
ApiClient _panelApi(_StubAdapter adapter) {
  final dio = Dio(
    BaseOptions(
      baseUrl: 'https://p.example/api/v2/app',
      validateStatus: (s) => s != null && s < 500,
    ),
  )..httpClientAdapter = adapter;
  return ApiClient(tokens: TokenStore(), dio: dio);
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const secureStorage = MethodChannel(
    'plugins.it_nomads.com/flutter_secure_storage',
  );
  late TestDefaultBinaryMessenger messenger;

  setUp(() {
    messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(secureStorage, (call) async => null);
  });

  tearDown(() => messenger.setMockMethodCallHandler(secureStorage, null));

  group('панельный REST', () {
    test('страны собираются из /servers и сортируются по лучшему пингу', () async {
      final c = await _boot(
        profiles: <ConnectionProfile>[_panelProfile()],
        servers: _servers,
      );
      final inv = c.read(exitInventoryProvider);

      expect(inv.source, ExitInventorySource.panelRest);
      expect(inv.loading, isFalse);
      expect(inv.error, isNull);
      // Доступные страны раньше недоступных; узлы без страны — в хвосте своей
      // группы, даже когда их пинг лучший.
      expect(inv.locations.map((l) => l.countryCode).toList(), <String>[
        'DE',
        'CA',
        '',
        'RU',
      ]);

      final de = inv.locationOf('de')!;
      expect(de.displayName, 'Германия');
      expect(de.nodeCount, 2);
      // Лучший пинг считается ТОЛЬКО по доступным узлам: 30 мс переполненного
      // узла обещали бы скорость, которой пользователь не получит.
      expect(de.bestPingMs, 40);
      expect(de.isAvailable, isTrue);
      // Панельный /servers протоколы не отдаёт — пусто, а не выдуманный набор.
      expect(de.protocols, isEmpty);
    });

    test(
      'страна без живых узлов остаётся в списке с названной причиной',
      () async {
        final c = await _boot(
          profiles: <ConnectionProfile>[_panelProfile()],
          servers: _servers,
        );
        final inv = c.read(exitInventoryProvider);

        final ru = inv.locationOf('RU')!;
        expect(ru.isAvailable, isFalse);
        expect(ru.availability.reason, ExitUnavailableReason.allNodesBusy);
        expect(ru.availability.message, isNotEmpty);
        expect(ru.nodeCount, 1, reason: 'узел виден, а не вычеркнут');

        final full = inv.nodesIn('DE').firstWhere((n) => n.panelNodeId == 2);
        expect(full.isAvailable, isFalse);
        expect(full.availability.reason, ExitUnavailableReason.nodeFull);
      },
    );

    test(
      'пригодность узла берётся у Server.isSelectable, а не у своего словаря',
      () async {
        // Боевая панель отдаёт `active`; исторические/документные значения
        // (`online`, `busy`) обязаны читаться так же, как их читает путь
        // подключения. Расхождение здесь — это список стран, в котором нечего
        // нажать, кроме «Авто».
        const statuses = <Server>[
          Server(id: 21, name: 'A', countryCode: 'NL', status: 'active'),
          Server(id: 22, name: 'B', countryCode: 'NL', status: 'online'),
          Server(id: 23, name: 'C', countryCode: 'NL', status: 'busy'),
          Server(id: 24, name: 'D', countryCode: 'NL', status: 'maintenance'),
          Server(id: 25, name: 'E', countryCode: 'NL', status: 'full'),
        ];
        final c = await _boot(
          profiles: <ConnectionProfile>[_panelProfile()],
          servers: statuses,
        );
        final inv = c.read(exitInventoryProvider);

        for (final s in statuses) {
          final node = inv.nodes.firstWhere((n) => n.panelNodeId == s.id);
          expect(node.isAvailable, s.isSelectable, reason: s.status);
        }
        expect(inv.locationOf('NL')!.isAvailable, isTrue);
        expect(
          inv.nodes.firstWhere((n) => n.panelNodeId == 25).availability.reason,
          ExitUnavailableReason.nodeFull,
        );
      },
    );

    test('вход через другую страну доступен на панельном пути', () async {
      final c = await _boot(
        profiles: <ConnectionProfile>[_panelProfile()],
        servers: _servers,
      );
      expect(c.read(relayAvailabilityProvider).isAvailable, isTrue);
    });

    test('узлы страны отдаются семейным провайдером', () async {
      final c = await _boot(
        profiles: <ConnectionProfile>[_panelProfile()],
        servers: _servers,
      );
      final nodes = c.read(exitNodesInCountryProvider('DE'));
      expect(nodes.map((n) => n.panelNodeId).toList(), <int>[1, 2]);
      expect(c.read(exitNodesInCountryProvider('')).single.panelNodeId, 11);
    });
  });

  group('импортированная подписка', () {
    test('страны берутся из узлов ядра, протоколы — из их типов', () async {
      final c = await _boot(
        profiles: <ConnectionProfile>[
          _rawProfile(
            servers: const <ImportedServer>[
              ImportedServer(
                id: 'de-vless',
                name: 'DE vless',
                type: 'vless',
                server: 'a.example',
                port: 443,
                country: 'de',
              ),
              ImportedServer(
                id: 'de-hy2',
                name: 'DE hysteria2',
                type: 'hysteria2',
                server: 'b.example',
                port: 443,
                country: 'DE',
              ),
              ImportedServer(
                id: 'nowhere',
                name: 'Node',
                type: 'ss',
                server: 'c.example',
                port: 443,
                country: '',
              ),
            ],
            probe: const ProbeSnapshot(
              latencyMs: <String, int>{
                'de-vless': 45,
                'de-hy2': -1,
                'nowhere': 12,
              },
              updatedMs: 1,
            ),
          ),
        ],
      );
      final inv = c.read(exitInventoryProvider);

      expect(inv.source, ExitInventorySource.importedSub);
      final de = inv.locationOf('DE')!;
      expect(de.nodeCount, 2);
      expect(de.protocols, <String>['hysteria2', 'vless']);
      // Таймаут (-1) — это качество связи, а не недоступность узла.
      expect(de.bestPingMs, 45);
      expect(de.isAvailable, isTrue);

      final unknown = inv.locationOf('')!;
      expect(unknown.displayName, 'Без страны');
      expect(unknown.isUnknownCountry, isTrue);
    });

    test(
      'вход через другую страну выключен с причиной, а не спрятан',
      () async {
        final c = await _boot(profiles: <ConnectionProfile>[_rawProfile()]);
        final relay = c.read(relayAvailabilityProvider);

        expect(relay.isAvailable, isFalse);
        expect(relay.reason, ExitUnavailableReason.relayChainingUnsupported);
        expect(relay.message, contains('импортированной'));
      },
    );
  });

  group('каталог CSM', () {
    test('готовый инвентарь подставляется целиком', () async {
      const catalog = ExitInventory(
        source: ExitInventorySource.csmCatalog,
        locations: <ExitLocation>[
          ExitLocation(
            countryCode: 'NL',
            displayName: 'Нидерланды',
            nodeCount: 2,
            source: ExitInventorySource.csmCatalog,
            bestPingMs: 22,
          ),
        ],
        nodes: <ExitNode>[
          ExitNode(
            key: 'nl-1',
            name: 'NL-1',
            countryCode: 'NL',
            source: ExitInventorySource.csmCatalog,
            pingMs: 22,
          ),
        ],
      );
      final c = await _boot(
        profiles: <ConnectionProfile>[_panelProfile()],
        servers: _servers,
        catalog: catalog,
      );
      final inv = c.read(exitInventoryProvider);

      // Каталог перекрывает панельный REST: инвентарь у него собственный, и
      // пересобирать его здесь значило бы завести вторую истину.
      expect(inv.source, ExitInventorySource.csmCatalog);
      expect(inv.locations.single.countryCode, 'NL');
      expect(inv.nodesIn('NL').single.key, 'nl-1');
    });

    test('каталог без инвентаря объясняет причину, а не молчит', () async {
      final c = await _boot(
        profiles: <ConnectionProfile>[_panelProfile()],
        catalog: ExitInventory.unavailable(
          ExitInventorySource.csmCatalog,
          ExitUnavailableReason.catalogNotReady,
        ),
      );
      final inv = c.read(exitInventoryProvider);

      expect(inv.source, ExitInventorySource.csmCatalog);
      expect(inv.isEmpty, isTrue);
      expect(
        inv.relayAvailability.reason,
        ExitUnavailableReason.catalogNotReady,
      );
      expect(inv.relayAvailability.message, isNotEmpty);
    });
  });

  test('без профиля инвентарь пуст с причиной, а не с пустотой', () async {
    final c = await _boot(profiles: const <ConnectionProfile>[]);
    final inv = c.read(exitInventoryProvider);

    expect(inv.source, ExitInventorySource.none);
    expect(inv.isEmpty, isTrue);
    expect(inv.relayAvailability.reason, ExitUnavailableReason.noProfile);
  });

  group('выбор страны', () {
    test('переживает перезапуск и ведёт автоподбор connect', () async {
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[_panelProfile()]
        ..activeId = 'cp_panel';

      final first = await _boot(
        profiles: <ConnectionProfile>[],
        servers: _servers,
        store: store,
      );
      // До выбора автоподбор берёт самый быстрый живой узел вообще.
      expect(first.read(recommendedServerProvider)?.id, 1);

      final outcome = await first
          .read(exitSelectionControllerProvider)
          .selectCountry('ca');

      expect(outcome.applied, isTrue);
      // Панель у профиля не подключена: выбор применён локально, а синхронизация
      // отсутствует по НАЗВАННОЙ причине, а не падает ошибкой.
      expect(outcome.syncedWithPanel, isFalse);
      expect(outcome.sync.reason, ExitUnavailableReason.panelRequired);
      expect(store.profiles.single.selectedExitCountry, 'CA');
      expect(store.profiles.single.selectedExitNodeId, 5);

      // Перезапуск: новый контейнер поверх того же хранилища.
      final second = await _boot(
        profiles: <ConnectionProfile>[],
        servers: _servers,
        store: store,
      );
      expect(second.read(selectedExitCountryProvider), 'CA');
      expect(second.read(selectedExitLocationProvider)?.countryCode, 'CA');
      // Пин восстановлен из профиля, и connect идёт в CA, а не в самый
      // быстрый DE.
      expect(second.read(resolvedSelectedServerProvider)?.id, 5);
      expect(second.read(recommendedServerProvider)?.id, 5);
    });

    test(
      'страна без живых узлов не роняет connect, но остаётся объяснённой',
      () async {
        final c = await _boot(
          profiles: <ConnectionProfile>[_panelProfile(country: 'RU')],
          servers: _servers,
        );

        expect(
          c.read(selectedExitLocationProvider)?.availability.reason,
          ExitUnavailableReason.allNodesBusy,
        );
        // Живых узлов в закреплённой стране нет — подключаемся в лучшую другую.
        expect(c.read(recommendedServerProvider)?.id, 1);
      },
    );

    test(
      'заголовок называет страну, через которую трафик выходит НА САМОМ ДЕЛЕ',
      () async {
        // Живых узлов в RU нет, автоподбор уходит в DE — и раньше заголовок
        // строки «Сервер» продолжал печатать закреплённую страну, оставляя имя
        // узла вторичной подписью. На боевом флоте это достижимо одним
        // переполнением: в DE узел один, и «Германия» становилась подписью к
        // канадскому выходу. Пин — это намерение, а заголовок читается как
        // утверждение о том, где пользователь виден сети.
        final c = await _boot(
          profiles: <ConnectionProfile>[_panelProfile(country: 'RU')],
          servers: _servers,
          core: _RecordingCore(),
        );

        expect(c.read(recommendedServerProvider)?.countryCode, 'DE');
        final headline = c.read(exitHeadlineProvider);
        expect(headline.countryCode, 'DE');
        expect(headline.title, 'Германия');
        // И подмена НАЗВАНА: без причины исправленный заголовок выглядел бы
        // как «настройка сама сбросилась».
        expect(headline.diverged, isTrue);
        expect(headline.unavailableCountry, 'RU');
        expect(headline.divergenceMessage, contains('Россия'));
        expect(headline.divergenceMessage, contains('Германия'));
      },
    );

    test('узел закреплённой страны расхождением не считается', () async {
      // Обратная сторона: DE-2 переполнен, но DE-1 жив — автоподбор остался в
      // стране. Баннер о недоступности здесь был бы шумом, а не честностью.
      final c = await _boot(
        profiles: <ConnectionProfile>[_panelProfile(country: 'DE')],
        servers: _servers,
        core: _RecordingCore(),
      );

      expect(c.read(recommendedServerProvider)?.id, 1);
      final headline = c.read(exitHeadlineProvider);
      expect(headline.title, 'Германия');
      expect(headline.diverged, isFalse);
      expect(headline.divergenceMessage, isEmpty);
    });

    test('без единого узла заголовком остаётся намерение пользователя', () async {
      // Выдача пуста: трафик никуда не идёт, врать нечем, и закреплённая
      // страна — единственное, что вообще можно сказать.
      final c = await _boot(
        profiles: <ConnectionProfile>[_panelProfile(country: 'DE')],
        servers: const <Server>[],
        core: _RecordingCore(),
      );

      final headline = c.read(exitHeadlineProvider);
      expect(headline.title, 'Германия');
      expect(headline.diverged, isFalse);
    });

    test('недоступный узел не выбирается и отдаёт СВОЮ причину', () async {
      final c = await _boot(
        profiles: <ConnectionProfile>[_panelProfile()],
        servers: _servers,
      );
      final full = c
          .read(exitInventoryProvider)
          .nodesIn('DE')
          .firstWhere((n) => n.panelNodeId == 2);

      final outcome = await c
          .read(exitSelectionControllerProvider)
          .selectNode(full);

      expect(outcome.applied, isFalse);
      expect(outcome.sync.reason, ExitUnavailableReason.nodeFull);
      expect(c.read(selectedExitCountryProvider), isNull);
    });

    test('в режиме импорта выбор узла пишется в пин подписки', () async {
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[
          _rawProfile(
            servers: const <ImportedServer>[
              ImportedServer(
                id: 'de-1',
                name: 'DE',
                type: 'vless',
                server: 'a',
                port: 443,
                country: 'DE',
              ),
            ],
          ),
        ]
        ..activeId = 'cp_raw';
      final c = await _boot(profiles: <ConnectionProfile>[], store: store);

      final node = c.read(exitInventoryProvider).nodes.single;
      final outcome = await c
          .read(exitSelectionControllerProvider)
          .selectNode(node);

      expect(outcome.applied, isTrue);
      // Панели нет вовсе: закреплять выбор негде, и это состояние режима.
      expect(outcome.sync.reason, ExitUnavailableReason.panelRequired);
      expect(store.profiles.single.selectedServerId, 'de-1');
      expect(store.profiles.single.selectedExitCountry, 'DE');
    });

    test('смена страны закрепляет узел ЭТОЙ страны, а не отпускает пин', () async {
      // Раньше здесь ожидался `selectedServerId == null`, и это ожидание
      // фиксировало настоящую ошибку: `connectRaw` не знает про страны и читает
      // только `selectedServerId`, поэтому «страна выбрана, пин снят» означало
      // галочку «Нидерланды» при выходе через Германию. Страна на сыром пути
      // обязана стать именем прокси ДО подключения.
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[
          _rawProfile(
            servers: const <ImportedServer>[
              ImportedServer(
                id: 'de-1',
                name: 'DE',
                type: 'vless',
                server: 'a',
                port: 443,
                country: 'DE',
              ),
              ImportedServer(
                id: 'nl-1',
                name: 'NL',
                type: 'vless',
                server: 'b',
                port: 443,
                country: 'NL',
              ),
            ],
            pinned: 'de-1',
          ),
        ]
        ..activeId = 'cp_raw';
      final c = await _boot(profiles: <ConnectionProfile>[], store: store);

      await c.read(exitSelectionControllerProvider).selectCountry('NL');

      expect(store.profiles.single.selectedExitCountry, 'NL');
      expect(store.profiles.single.selectedServerId, 'nl-1');
    });

    test('«Авто» в импорте отпускает пин: узел выбирает ядро', () async {
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[
          _rawProfile(
            servers: const <ImportedServer>[
              ImportedServer(
                id: 'nl-1',
                name: 'NL',
                type: 'vless',
                server: 'b',
                port: 443,
                country: 'NL',
              ),
            ],
            country: 'NL',
            pinned: 'nl-1',
          ),
        ]
        ..activeId = 'cp_raw';
      final c = await _boot(profiles: <ConnectionProfile>[], store: store);

      final outcome = await c
          .read(exitSelectionControllerProvider)
          .selectCountry(null);

      expect(outcome.applied, isTrue);
      expect(store.profiles.single.selectedExitCountry, isNull);
      expect(store.profiles.single.selectedServerId, isNull);
    });

    test('connect на сыром пути получает узел закреплённой страны', () async {
      // Состояние после «Обновить подписку»: узла, на котором стоял пин, в
      // новом составе нет, и он снят — а закреплённая страна осталась.
      // `connectRaw` страны не знает и с пустым serverId уходит в любой узел,
      // поэтому страну разрешаем в имя прокси перед самой отправкой в ядро.
      final core = _RecordingCore();
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[
          _rawProfile(
            servers: const <ImportedServer>[
              ImportedServer(
                id: 'ca-1',
                name: 'CA',
                type: 'vless',
                server: 'a',
                port: 443,
                country: 'CA',
              ),
              ImportedServer(
                id: 'de-1',
                name: 'DE',
                type: 'vless',
                server: 'b',
                port: 443,
                country: 'DE',
              ),
            ],
            country: 'DE',
          ),
        ]
        ..activeId = 'cp_raw';
      final c = await _boot(
        profiles: <ConnectionProfile>[],
        store: store,
        core: core,
      );

      expect(await c.read(vpnProvider.notifier).connect(), isTrue);
      expect(core.connectedRaw, isTrue);
      expect(core.rawServerId, 'de-1');
    });

    test('обновление подписки снимает страну, узлов которой в ней не осталось', () async {
      // Тот же разъезд, что и на панельном пути, только триггер уже: «Обновить
      // подписку» вернула состав без DE. Пин на de-1 снимался и раньше, а
      // страна оставалась — и разрешать её было НЕ ВО ЧТО: connectRaw уходил с
      // пустым именем прокси, то есть в любой узел на выбор ядра, пока выбор
      // «Германия» продолжал числиться за профилем.
      const ca = ImportedServer(
        id: 'ca-1',
        name: 'CA',
        type: 'vless',
        server: 'a',
        port: 443,
        country: 'CA',
      );
      const de = ImportedServer(
        id: 'de-1',
        name: 'DE',
        type: 'vless',
        server: 'b',
        port: 443,
        country: 'DE',
      );
      final core = _RecordingCore();
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[
          _rawProfile(
            servers: const <ImportedServer>[ca, de],
            country: 'DE',
            pinned: 'de-1',
          ),
        ]
        ..activeId = 'cp_raw';
      final c = await _boot(
        profiles: <ConnectionProfile>[],
        store: store,
        core: core,
      );
      expect(c.read(selectedExitCountryProvider), 'DE');

      await c
          .read(connectionProfilesProvider.notifier)
          .setImported(
            'cp_raw',
            rawConfig: 'proxies: []',
            format: 'clash',
            servers: const <ImportedServer>[ca],
          );

      expect(store.profiles.single.selectedServerId, isNull);
      expect(store.profiles.single.selectedExitCountry, isNull);
      expect(c.read(selectedExitCountryProvider), isNull);
      expect(c.read(selectedExitLocationProvider), isNull);

      // Показано «Авто» — и в ядро уходит «Авто». Разъехаться теперь нечему.
      expect(await c.read(vpnProvider.notifier).connect(), isTrue);
      expect(core.connectedRaw, isTrue);
      expect(core.rawServerId, isNull);
    });

    test('страна переживает обновление, если её узел в составе есть', () async {
      // Снимать страну на КАЖДОМ обновлении значило бы терять выбор на ровном
      // месте: узел мог просто сменить имя. Пин уезжает вместе с de-1, страна
      // остаётся и разрешается в новый немецкий узел перед самым connect.
      const ca = ImportedServer(
        id: 'ca-1',
        name: 'CA',
        type: 'vless',
        server: 'a',
        port: 443,
        country: 'CA',
      );
      const deOld = ImportedServer(
        id: 'de-1',
        name: 'DE',
        type: 'vless',
        server: 'b',
        port: 443,
        country: 'DE',
      );
      const deNew = ImportedServer(
        id: 'de-9',
        name: 'DE new',
        type: 'vless',
        server: 'c',
        port: 443,
        country: 'DE',
      );
      final core = _RecordingCore();
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[
          _rawProfile(
            servers: const <ImportedServer>[ca, deOld],
            country: 'DE',
            pinned: 'de-1',
          ),
        ]
        ..activeId = 'cp_raw';
      final c = await _boot(
        profiles: <ConnectionProfile>[],
        store: store,
        core: core,
      );

      await c
          .read(connectionProfilesProvider.notifier)
          .setImported(
            'cp_raw',
            rawConfig: 'proxies: []',
            format: 'clash',
            servers: const <ImportedServer>[ca, deNew],
          );

      expect(store.profiles.single.selectedServerId, isNull);
      expect(store.profiles.single.selectedExitCountry, 'DE');
      expect(c.read(selectedExitLocationProvider)?.countryCode, 'DE');

      expect(await c.read(vpnProvider.notifier).connect(), isTrue);
      expect(core.rawServerId, 'de-9');
    });

    test('неразрешимая страна не стирает пин на подписке', () async {
      // Живых узлов в RU нет. Отправить сюда `node_id: null` значило бы выдать
      // наш неуспех за просьбу пользователя вернуть дефолт: панель стирает
      // закреплённый узел и ставит долгоживущий маркер владения выбором.
      final adapter = _StubAdapter();
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[
          _panelProfile(
            country: 'DE',
            nodeId: 1,
            panelUrl: 'https://p.example',
          ),
        ]
        ..activeId = 'cp_panel';
      final c = await _boot(
        profiles: <ConnectionProfile>[],
        servers: _servers,
        store: store,
        api: _panelApi(adapter),
        subscription: _subscription(),
      );

      final outcome = await c
          .read(exitSelectionControllerProvider)
          .selectCountry('RU');

      expect(outcome.applied, isFalse);
      expect(outcome.sync.reason, ExitUnavailableReason.allNodesBusy);
      expect(outcome.sync.message, isNotEmpty);
      expect(adapter.paths, isEmpty, reason: 'панель не должна ничего узнать');
      // Профиль не тронут: прежний выбор пережил неудачную попытку.
      expect(store.profiles.single.selectedExitCountry, 'DE');
      expect(store.profiles.single.selectedExitNodeId, 1);
    });

    test('«Авто» на панели — это осознанный сброс в дефолт', () async {
      final adapter = _StubAdapter();
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[
          _panelProfile(
            country: 'DE',
            nodeId: 1,
            panelUrl: 'https://p.example',
          ),
        ]
        ..activeId = 'cp_panel';
      final c = await _boot(
        profiles: <ConnectionProfile>[],
        servers: _servers,
        store: store,
        api: _panelApi(adapter),
        subscription: _subscription(),
      );

      final outcome = await c
          .read(exitSelectionControllerProvider)
          .selectCountry(null);

      expect(outcome.applied, isTrue);
      // Явный null здесь — намерение, а не побочный эффект неудачи.
      expect(adapter.paths.single, '/subscriptions/7/selection');
      expect(adapter.bodies.single, '{"node_id":null}');
      expect(store.profiles.single.selectedExitCountry, isNull);
      expect(store.profiles.single.selectedExitNodeId, isNull);
    });

    test('страна с живым узлом закрепляется на панели явным node_id', () async {
      final adapter = _StubAdapter(body: '{"node_id": 5}');
      final store = _MemoryStore()
        ..profiles = <ConnectionProfile>[
          _panelProfile(panelUrl: 'https://p.example'),
        ]
        ..activeId = 'cp_panel';
      final c = await _boot(
        profiles: <ConnectionProfile>[],
        servers: _servers,
        store: store,
        api: _panelApi(adapter),
        subscription: _subscription(),
      );

      final outcome = await c
          .read(exitSelectionControllerProvider)
          .selectCountry('CA');

      expect(outcome.applied, isTrue);
      expect(adapter.bodies.single, '{"node_id":5}');
      expect(outcome.resolved?.nodeId, 5);
      expect(store.profiles.single.selectedExitNodeId, 5);
    });
  });

  group('PUT /subscriptions/{id}/selection', () {
    ApiClient client(
      _StubAdapter adapter, {
      String base = 'https://p.example',
    }) {
      final dio = Dio(
        BaseOptions(
          baseUrl: base.isEmpty ? '' : '$base/api/v2/app',
          validateStatus: (s) => s != null && s < 500,
        ),
      )..httpClientAdapter = adapter;
      return ApiClient(tokens: TokenStore(), dio: dio);
    }

    test('отсутствующее поле не попадает в тело, null попадает', () async {
      final adapter = _StubAdapter(
        body: '{"node_id": 5, "relay_country": "tr"}',
      );
      final api = client(adapter);

      final resolved = await api.putSubscriptionSelection(
        subscriptionId: 7,
        nodeId: const SelectionField<int>.of(5),
      );

      expect(adapter.paths.single, '/subscriptions/7/selection');
      // relay_country не передавали — панель обязана оставить его как есть.
      expect(adapter.bodies.single, '{"node_id":5}');
      expect(resolved.nodeId, 5);
      expect(resolved.relayCountry, 'TR');
    });

    test('сброс в дефолт отправляет явный null', () async {
      final adapter = _StubAdapter(body: '{"node_id": null}');
      final api = client(adapter);

      final resolved = await api.putSubscriptionSelection(
        subscriptionId: 7,
        nodeId: const SelectionField<int>.reset(),
        relayCountry: const SelectionField<String>.of('NL'),
      );

      expect(adapter.bodies.single, '{"node_id":null,"relay_country":"NL"}');
      expect(resolved.nodeId, isNull);
      expect(resolved.relayCountry, isNull);
    });

    test('оба поля «не трогать» — запроса нет', () async {
      final adapter = _StubAdapter();
      final api = client(adapter);

      final resolved = await api.putSubscriptionSelection(subscriptionId: 7);

      expect(adapter.paths, isEmpty);
      expect(resolved, ExitSelection.none);
    });

    test('без панели — отсутствие возможности, а не сетевая ошибка', () async {
      final adapter = _StubAdapter();
      final api = client(adapter, base: '');

      expect(
        () => api.putSubscriptionSelection(
          subscriptionId: 7,
          nodeId: const SelectionField<int>.of(5),
        ),
        throwsA(isA<ApiNotAvailableException>()),
      );
      expect(adapter.paths, isEmpty);
    });
  });
}

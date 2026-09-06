// Экраны флота на ПАНЕЛЬНОМ пути: сервер-машина, протокол-инбаунд, вход-узел,
// и отчёт ядра о том, что применилось на самом деле.
//
// Фикстуры сняты с живой системы, а не придуманы для теста:
//  * узел 1 (Германия) — восемь включённых инбаундов ОДНОЙ машины, восьмой
//    (`naive`) генератор Clash не выпускает, и панель говорит об этом
//    машинной причиной `protocol_not_emitted_by_clash`;
//  * узел 5 (Канада) — шесть инбаундов и никакого релэя;
//  * узел 9 (Нидерланды) — панель не смогла прочитать его инбаунды
//    (`inbounds: null` + `inbounds_error`);
//  * релэй — РФ-узел, привязанный к Германии, с честным
//    `chained_in_config: false`: метка есть, цепочки в теле нет.
//
// Проверяется ровно то, на что жаловался владелец: «восемь серверов» больше не
// могут означать одну машину, список протоколов зависит от выбранного узла,
// вход — это узлы оператора с названной причиной недоступности, а блок рекламы
// и стриминг перестают быть обещанием.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_vpn/caramba_vpn.dart' show ProbeResult, ProbeVerdict;

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/features/protocol/inbound_latency.dart';
import 'package:caramba_client/features/protocol/protocol_screen.dart';
import 'package:caramba_client/features/servers/relay_screen.dart';
import 'package:caramba_client/features/servers/servers_screen.dart';
import 'package:caramba_client/features/settings/applied_route_card.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/widgets/ui.dart';

import 'support/fake_core.dart';

// ─── фикстуры панели ───────────────────────────────────────────────────────

Map<String, dynamic> _inbound(
  int id,
  String tag,
  String protocol,
  String network,
  String security,
  String label, {
  String? proxyName,
  bool available = true,
  String? reason,
}) =>
    <String, dynamic>{
      'id': id,
      'tag': tag,
      'protocol': protocol,
      'network': network,
      'security': security,
      'port': 443,
      'label': label,
      'proxy_name': proxyName,
      'available': available,
      'unavailable_reason': reason,
    };

final _germany = Server.fromJson(<String, dynamic>{
  'id': 1,
  'name': 'Node #1 (Germany)',
  'country_code': 'DE',
  'latency_ms': 40,
  'load_pct': 12.0,
  'status': 'active',
  'via_relay': <String, dynamic>{
    'node_id': 2,
    'name': 'RU relay',
    'country_code': 'RU',
    'chained_in_config': false,
  },
  'inbounds': <Map<String, dynamic>>[
    _inbound(
      11,
      'reality-in',
      'vless',
      'tcp',
      'reality',
      'Stealth',
      proxyName: 'DE Stealth',
    ),
    _inbound(
      12,
      'tls-in',
      'vless',
      'tcp',
      'tls',
      'Secure',
      proxyName: 'DE Secure',
    ),
    _inbound(
      13,
      'ws-in',
      'vless',
      'ws',
      'tls',
      'WebSocket',
      proxyName: 'DE WebSocket',
    ),
    _inbound(
      14,
      'grpc-in',
      'vless',
      'grpc',
      'tls',
      'Stream',
      proxyName: 'DE Stream',
    ),
    _inbound(
      15,
      'hy2-in',
      'hysteria2',
      'udp',
      'tls',
      'Speed',
      proxyName: 'DE Speed',
    ),
    _inbound(16, 'tuic-in', 'tuic', 'udp', 'tls', 'TUIC', proxyName: 'DE TUIC'),
    _inbound(
      17,
      'ss-in',
      'shadowsocks',
      'tcp',
      'none',
      'Shadow',
      proxyName: 'DE Shadow',
    ),
    // Оператор его включил, а в тело конфига он не попадает.
    _inbound(
      18,
      'naive-in',
      'naive',
      'tcp',
      'tls',
      'Naive',
      available: false,
      reason: 'protocol_not_emitted_by_clash',
    ),
  ],
  'inbounds_error': null,
});

final _canada = Server.fromJson(<String, dynamic>{
  'id': 5,
  'name': 'Node #5 (Canada)',
  'country_code': 'CA',
  'latency_ms': 120,
  'load_pct': 30.0,
  'status': 'active',
  'inbounds': <Map<String, dynamic>>[
    _inbound(
      51,
      'reality-in',
      'vless',
      'tcp',
      'reality',
      'Stealth',
      proxyName: 'CA Stealth',
    ),
    _inbound(
      52,
      'hy2-in',
      'hysteria2',
      'udp',
      'tls',
      'Speed',
      proxyName: 'CA Speed',
    ),
  ],
});

/// Узел, инбаунды которого панель прочитать не смогла. Это НЕ «их нет».
final _mute = Server.fromJson(<String, dynamic>{
  'id': 9,
  'name': 'Node #9 (Netherlands)',
  'country_code': 'NL',
  'status': 'active',
  'inbounds': null,
  'inbounds_error': 'panel_could_not_read_inbounds',
});

/// Панельный узел с ЕДИНСТВЕННЫМ инбаундом: случай, в котором имя оператора
/// раньше подменялось тегом инбаунда.
final _singleInbound = Server.fromJson(<String, dynamic>{
  'id': 7,
  'name': 'Node #7 (Single)',
  'country_code': 'DE',
  'status': 'active',
  'inbounds': <Map<String, dynamic>>[
    <String, dynamic>{
      'tag': 'only-in',
      'protocol': 'vless',
      'network': 'tcp',
      'security': 'tls',
      'port': 443,
      'label': 'VLESS',
      'proxy_name': '🇩🇪 Only',
      'available': true,
    },
  ],
});

/// Входы, как их отдаёт `GET /app/relays`: страна и число узлов в ней.
final _panelRelays = Relay.fromCountries(<Relay>[
  Relay.fromApiJson(const <String, dynamic>{
    'country_code': 'RU',
    'country_name': 'Россия',
    'node_count': 1,
  }),
]);

/// Ядро, которое на `probe` отвечает числами и вердиктами, а не молчанием.
///
/// [FakeVpnCore] возвращает пустой список — это состояние «ядру нечего
/// сказать», и в нём экран задержек не покажет по праву. Чтобы проверить сами
/// числа, нужен ответ; имена в нём — те же `proxy_name`, что панель объявляет
/// побайтово совпадающими с телом конфига, иначе мост «строка → замер» был бы
/// проверен на выдуманном ключе.
///
/// Вердикт в фикстуре обязателен, а не удобен: с тех пор как ядро перестало
/// подменять провал URL-теста временем TCP, число и вердикт — одна запись, и
/// замер без вердикта означает «сборка старше этого поля», то есть отдельный
/// проверяемый случай, а не «всё хорошо».
class _ProbingCore extends FakeVpnCore {
  final Map<String, (int, ProbeVerdict)> results;
  final Map<String, int> tcpMs;

  _ProbingCore(this.results, {this.tcpMs = const <String, int>{}});

  @override
  Future<List<ProbeResult>> probe({Duration timeout = Duration.zero}) async => [
        for (final e in results.entries)
          ProbeResult(
            id: e.key,
            name: e.key,
            latencyMs: e.value.$1,
            tcpMs: tcpMs[e.key] ?? -1,
            verdict: e.value.$2,
          ),
      ];
}

class _Store implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles;
  String? activeId;
  _Store(this.profiles, this.activeId);

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
    profiles = const [];
    activeId = null;
  }
}

ConnectionProfile _panelProfile({int? nodeId, String? country}) =>
    ConnectionProfile(
      id: 'cp_panel',
      type: ProfileType.panelAccount,
      displayName: 'Оператор',
      source: 'https://panel.example',
      selectedExitNodeId: nodeId,
      selectedExitCountry: country,
    );

Widget _app(
  Widget screen, {
  required List<Server> servers,
  ConnectionProfile? profile,
  FakeVpnCore? core,
  int? relayIndex,
}) =>
    ProviderScope(
      overrides: [
        vpnConnectionProvider.overrideWithValue(core ?? FakeVpnCore()),
        connectionProfilesStoreProvider.overrideWithValue(
          _Store(<ConnectionProfile>[profile ?? _panelProfile()], 'cp_panel'),
        ),
        serversProvider.overrideWith((ref) async => servers),
        apiRelaysProvider.overrideWith((ref) async => _panelRelays),
        if (relayIndex != null)
          coreConfigProvider.overrideWith(
            (ref) =>
                CoreConfigNotifier()..hydrate(CoreConfig(relay: relayIndex)),
          ),
      ],
      child: MaterialApp(theme: AppTheme.dark(), home: screen),
    );

/// Высокое окно: списки строятся ленивым сливером, и строка, не попавшая в
/// вьюпорт, не попадает и в дерево элементов.
void _useTallView(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(900, 3400)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

/// Профили и панельная выдача читаются асинхронно.
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump();
  }
}

/// Строка видна, приглушена, названа причиной и не нажимается.
void _expectDisabledRow(WidgetTester tester, String title, String reason) {
  expect(find.text(title), findsOneWidget, reason: 'строка «$title» пропала');
  final opacities = tester.widgetList<Opacity>(
    find.ancestor(of: find.text(title), matching: find.byType(Opacity)),
  );
  expect(
    opacities.any((o) => o.opacity == 0.45),
    isTrue,
    reason: 'строка «$title» обязана быть приглушённой',
  );
  expect(find.textContaining(reason), findsWidgets);
  final card = tester.widget<ListItemCard>(
    find.ancestor(of: find.text(title), matching: find.byType(ListItemCard)),
  );
  expect(card.onTap, isNull, reason: 'у строки «$title» не должно быть тапа');
}

ListItemCard _cardOf(WidgetTester tester, String title) =>
    tester.widget<ListItemCard>(
      find.ancestor(of: find.text(title), matching: find.byType(ListItemCard)),
    );

void main() {
  group('экран серверов: строка — машина, а не инбаунд', () {
    testWidgets('узел называет, сколько инбаундов он предлагает', (
      tester,
    ) async {
      // Именно здесь жила жалоба «восемь серверов»: восемь инбаундов одной
      // машины показывались восемью строками. Машина одна, и она обязана сама
      // сказать, сколько инбаундов у неё есть.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(const ServersScreen(), servers: <Server>[_germany, _canada]),
      );
      await _settle(tester);
      // Раньше висящий таймер подписки добирал `pumpAndSettle` после нажатия
      // на страну. Захода в страну больше нет, а таймер остался — досыпаем
      // явно, иначе тест падает на нём, а не на проверке.
      await tester.pumpAndSettle();

      // Список плоский: машина видна сразу, без захода в страну.
      expect(find.text('Node #1 (Germany)'), findsOneWidget);
      // Семь из восьми доезжают до конфига; восьмой (`naive`) — нет.
      expect(find.text('ИНБАУНДОВ: 7'), findsOneWidget);
      expect(
        find.textContaining('ещё 1 не доезжает до конфига'),
        findsOneWidget,
      );
      // Страна названа на самой строке — теперь это её единственное место.
      expect(find.text('DE'), findsWidgets);
      expect(find.text('Германия'), findsNothing);
    });

    testWidgets('строка называет узел именем оператора, а не тегом инбаунда', (
      tester,
    ) async {
      // Панельный узел с ОДНИМ инбаундом назывался тегом этого инбаунда
      // («vless-in») вместо имени, которое дал оператор. Пока выбор шёл через
      // страну, заголовок машины почти никто не видел; в плоском списке он
      // стал главным словом строки.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(const ServersScreen(), servers: <Server>[_singleInbound]),
      );
      await _settle(tester);
      await tester.pumpAndSettle();

      expect(find.text('Node #7 (Single)'), findsOneWidget);
      expect(find.text('only-in'), findsNothing);
    });

    testWidgets(
      'узел, инбаунды которого панель не прочитала, говорит об этом',
      (tester) async {
        _useTallView(tester);
        await tester.pumpWidget(
          _app(const ServersScreen(), servers: <Server>[_mute]),
        );
        await _settle(tester);
        await tester.pumpAndSettle();

        // Ноль и «неизвестно» — разные ответы, и счётчик их не путает.
        expect(find.text('ИНБАУНДЫ: ?'), findsOneWidget);
        expect(
          find.textContaining('Панель не смогла прочитать инбаунды'),
          findsOneWidget,
        );
        // Узел при этом рабочий: молчание про инбаунды его не выключает.
        expect(_cardOf(tester, 'Node #9 (Netherlands)').onTap, isNotNull);
      },
    );
  });

  group('экран протокола: инбаунды ВЫБРАННОГО узла', () {
    testWidgets('у узла с меньшим числом инбаундов меньше и протоколов', (
      tester,
    ) async {
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany, _canada],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
        ),
      );
      await _settle(tester);

      // Германия: все её формы, каждая отдельной строкой. `vless/tcp/reality`
      // и `vless/tcp/tls` — РАЗНЫЕ строки: склеенные в слово «vless», они
      // предлагали бы выбор, который ничего не выбирает.
      expect(find.text('VLESS · tcp · reality'), findsOneWidget);
      expect(find.text('VLESS · tcp · tls'), findsOneWidget);
      expect(find.text('VLESS · ws · tls'), findsOneWidget);
      expect(find.text('VLESS · grpc · tls'), findsOneWidget);
      expect(find.text('Hysteria2 · udp · tls'), findsOneWidget);
      expect(find.text('TUIC · udp · tls'), findsOneWidget);
      expect(find.text('Shadowsocks · tcp · none'), findsOneWidget);
      expect(find.textContaining('Node #1 (Germany)'), findsOneWidget);

      // Тот же экран на узле с двумя инбаундами: список короче ровно на то,
      // чего у этой машины нет.
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany, _canada],
          profile: _panelProfile(nodeId: 5, country: 'CA'),
        ),
      );
      await _settle(tester);

      expect(find.text('VLESS · tcp · reality'), findsOneWidget);
      expect(find.text('Hysteria2 · udp · tls'), findsOneWidget);
      for (final absent in <String>[
        'VLESS · tcp · tls',
        'VLESS · ws · tls',
        'VLESS · grpc · tls',
        'TUIC · udp · tls',
        'Shadowsocks · tcp · none',
        'NaiveProxy · tcp · tls',
      ]) {
        expect(find.text(absent), findsNothing, reason: absent);
      }
      expect(find.textContaining('Node #5 (Canada)'), findsOneWidget);
    });

    testWidgets('инбаунд, не доезжающий до конфига, виден и объяснён', (
      tester,
    ) async {
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
        ),
      );
      await _settle(tester);

      // Оператор его включил — прятать нельзя; в тело конфига он не попадает —
      // выбирать нечего. Причина приходит от панели, а не сочиняется экраном.
      _expectDisabledRow(
        tester,
        'NaiveProxy · tcp · tls',
        'в конфиг, который читает приложение, он не попадает',
      );
    });

    testWidgets('источник промолчал — строки остаются, но не подтверждены', (
      tester,
    ) async {
      // Панель вернула `inbounds: null`: она сама их не прочитала. Это не
      // разрешение и не запрет. Пустой список тут был бы худшим исходом —
      // узел рабочий, настройка применится на следующем подъёме, а показать
      // нечего.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_mute],
          profile: _panelProfile(nodeId: 9, country: 'NL'),
        ),
      );
      await _settle(tester);

      expect(find.text('VLESS · Reality'), findsOneWidget);
      expect(
        find.textContaining('Панель не смогла прочитать инбаунды'),
        findsWidgets,
      );
      // Помечена непроверенной — и всё же нажимаемой: запрет по молчанию
      // источника отнял бы рабочий выбор ровно так же, как разрешение по
      // молчанию его выдумывало.
      expect(find.text('НЕ ПРОВЕРЕНО'), findsWidgets);
      expect(_cardOf(tester, 'VLESS · Reality').onTap, isNotNull);
    });
  });

  group('экран протокола: семейство считает ЯДРО, а не пикер', () {
    // `protocolClashType` в libs/caramba-core/profile/profile.go отображает и
    // "VLESS-Reality", и "VLESS" в ОДИН тип `vless`, а `applyProtocol`
    // отбирает прокси сравнением `m["type"] == want`. Значит служебная
    // url-test группа `Caramba-Proto` собирается по ВСЕМ vless-прокси узла, и
    // выбор Reality поднимает туннель хоть на TLS-инбаунде.
    //
    // Пикер же делит vless на две строки списка ядра (`VLESS-Reality` и
    // `VLESS`) — и, считая соседей по индексу опции, объявлял Reality
    // единственным в своём семействе. Ровно это и есть «пикер, который врёт»:
    // строка обещала точность, которой в ядре нет.
    testWidgets(
        'строка Reality признаёт, что ядро не отличает её от vless '
        'на TLS', (tester) async {
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
        ),
      );
      await _settle(tester);

      // У узла 1 четыре vless-инбаунда: reality на tcp и TLS на tcp/ws/grpc.
      // Ядро видит их как один `vless`, поэтому «ещё 3» обязано стоять на
      // КАЖДОЙ из четырёх строк, включая Reality, и семейство обязано
      // называться так, как его знает ядро, — «VLESS», а не «VLESS · Reality».
      const disclosure =
          'Ядро закрепляет семейство «VLESS»: этот инбаунд и ещё 3 в нём '
          'неразличимы.';
      for (final title in <String>[
        'VLESS · tcp · reality',
        'VLESS · tcp · tls',
        'VLESS · ws · tls',
        'VLESS · grpc · tls',
      ]) {
        expect(
          _cardOf(tester, title).subtitle,
          contains(disclosure),
          reason: 'строка «$title» молчит о схлопывании в семейство ядра',
        );
      }
      expect(find.textContaining(disclosure), findsNWidgets(4));

      // Семейства, в которых узел даёт ровно один инбаунд, ничего не
      // схлопывают — и молчат. Счёт настоящий, а не приписанный всем подряд.
      expect(_cardOf(tester, 'Hysteria2 · udp · tls').subtitle, isNull);
      expect(_cardOf(tester, 'TUIC · udp · tls').subtitle, isNull);
      expect(_cardOf(tester, 'Shadowsocks · tcp · none').subtitle, isNull);
    });

    testWidgets('на узле с единственным vless строка Reality точна и молчит', (
      tester,
    ) async {
      // Узел 5 даёт reality и hysteria2 — по одному инбаунду на семейство.
      // Здесь пин действительно точен, и приписывать строке схлопывание было
      // бы такой же ложью, только в другую сторону.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_canada],
          profile: _panelProfile(nodeId: 5, country: 'CA'),
        ),
      );
      await _settle(tester);

      expect(_cardOf(tester, 'VLESS · tcp · reality').subtitle, isNull);
      expect(_cardOf(tester, 'Hysteria2 · udp · tls').subtitle, isNull);
    });

    testWidgets(
        'молчащий источник не отменяет схлопывания: оно в ядре, а не '
        'во флоте', (tester) async {
      // Узел 9 — панель не прочитала его инбаунды, и экран печатает список
      // ЗАПРОСОВ ядра. Две строки этого списка, `VLESS · Reality` и `VLESS`,
      // ведут в один и тот же `vless`: молчание панели на `protocolClashType`
      // не влияет, и признание обязано стоять на обеих.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_mute],
          profile: _panelProfile(nodeId: 9, country: 'NL'),
        ),
      );
      await _settle(tester);

      const disclosure =
          'Ядро закрепляет семейство «VLESS»: для него эта строка и ещё 1 в '
          'списке неразличимы.';
      expect(_cardOf(tester, 'VLESS · Reality').subtitle, contains(disclosure));
      expect(_cardOf(tester, 'VLESS').subtitle, contains(disclosure));

      // Строки, у которых семейство своё, ничего не признают.
      for (final title in <String>['AmneziaWG', 'Hysteria2', 'TUIC']) {
        expect(
          _cardOf(tester, title).subtitle,
          isNot(contains('Ядро закрепляет семейство')),
          reason: 'строка «$title» приписала себе чужое схлопывание',
        );
      }
    });
  });

  group('экран «Тип подключения»: имя и пинг каждого инбаунда', () {
    testWidgets('экран называется типом подключения, а не протоколом', (
      tester,
    ) async {
      // «Протокол» врал дважды: строк в списке больше, чем протоколов
      // (`vless/tcp/reality` и `vless/ws/tls` — один протокол, две строки), и
      // слово звучало как выбор технологии, а не входа на конкретную машину.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
          core: _ProbingCore(const <String, (int, ProbeVerdict)>{}),
        ),
      );
      await _settle(tester);
      await tester.pumpAndSettle();

      expect(find.text('Тип подключения'), findsOneWidget);
      expect(find.text('Протокол'), findsNothing);
      // Слово владельца («тип конфига») сказано ровно один раз — в подзаголовке.
      expect(find.textContaining('типом конфига'), findsOneWidget);
    });

    testWidgets('у каждого инбаунда своё число, и оно померено устройством', (
      tester,
    ) async {
      // Владелец: «когда пользователь заходил в этот набор тоже показывался
      // пинг этих инбаундов». Панель тут бесполезна: у неё одно
      // `latency_ms: 40` на всю немецкую машину, и все семь входов получили бы
      // одно и то же число. Числа ниже — разные, потому что мерил их клиент.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
          core: _ProbingCore(
            const <String, (int, ProbeVerdict)>{
              'DE Stealth': (118, ProbeVerdict.ok),
              'DE Secure': (205, ProbeVerdict.ok),
              'DE WebSocket': (340, ProbeVerdict.ok),
              'DE Stream': (291, ProbeVerdict.ok),
              'DE Speed': (96, ProbeVerdict.ok),
              'DE TUIC': (101, ProbeVerdict.ok),
              // Адрес отвечает, ключ не принят: до вердиктов эта строка
              // показывала бы 118 мс и стояла бы первой в списке.
              'DE Shadow': (-1, ProbeVerdict.authRejected),
            },
            tcpMs: const <String, int>{'DE Shadow': 118},
          ),
        ),
      );
      await _settle(tester);
      await tester.pumpAndSettle();

      // Число на строке — и подпись «ваш пинг» у каждого: операторского числа
      // у инбаунда не существует в принципе.
      expect(find.text('118 мс'), findsOneWidget);
      expect(find.text('96 мс'), findsOneWidget);
      expect(find.text('340 мс'), findsOneWidget);
      expect(find.text('ваш пинг'), findsWidgets);
      expect(find.text('от оператора'), findsNothing);
      // Панельное число машины (40 мс) не подменяет собой замер входа.
      expect(find.text('40 мс'), findsNothing);
      // Вход, отвергнувший ключ, числа не получает вовсе — вместо него стоит
      // причина. Именно он до вердиктов и показывался самым быстрым в списке.
      expect(find.text('-1 мс'), findsNothing);
      expect(
        _cardOf(tester, 'Shadowsocks · tcp · none').subtitle,
        contains('вход не принял ключ подписки'),
      );
      // И заодно называет, что адрес при этом жив: это отличает «оператор
      // сломал узел» от «сеть режет протокол».
      expect(
        _cardOf(tester, 'Shadowsocks · tcp · none').subtitle,
        contains('118 мс'),
      );
    });

    testWidgets('число, добытое настоящим запросом, оговорок не получает', (
      tester,
    ) async {
      // Вердикт `ok` означает, что сквозь вход прошёл HTTP-запрос. Дописать к
      // такому числу «а может, это просто TCP» значило бы обесценить его
      // ровно так же, как молчание обесценивало проверку раньше.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
          core: _ProbingCore(const <String, (int, ProbeVerdict)>{
            'DE Stealth': (118, ProbeVerdict.ok),
          }),
        ),
      );
      await _settle(tester);
      await tester.pumpAndSettle();

      expect(find.text(kProbeMeaningNote), findsOneWidget);
      expect(find.text(kProbeTcpOnlyNote), findsNothing);
    });

    testWidgets('«проверен только адрес» не выдаётся за «вход принял ключ»', (
      tester,
    ) async {
      // Сборка без ядра (probe_default.go) меряет TCP и говорит об этом
      // вердиктом `tcp_only`. Число выглядит как задержка входа, а отвечает на
      // вопрос слабее — и узел с отозванным ключом отвечает на TCP так же
      // бодро, как рабочий. Оговорка обязана появиться на экране, а не в логах.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
          core: _ProbingCore(const <String, (int, ProbeVerdict)>{
            'DE Stealth': (118, ProbeVerdict.tcpOnly),
          }),
        ),
      );
      await _settle(tester);
      await tester.pumpAndSettle();

      expect(find.text(kProbeTcpOnlyNote), findsOneWidget);
      expect(
        _cardOf(tester, 'VLESS · tcp · reality').subtitle,
        contains('Проверен только адрес'),
      );
    });

    testWidgets('инбаунд без имени прокси числа не получает вовсе', (
      tester,
    ) async {
      // `naive` на узле 1: генератор Clash его не выпускает, имени в теле
      // конфига нет, ядро его не назовёт никогда. Прочерк в колонке обещал бы,
      // что число появится.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
          core: _ProbingCore(const <String, (int, ProbeVerdict)>{
            'DE Stealth': (118, ProbeVerdict.ok),
          }),
        ),
      );
      await _settle(tester);
      await tester.pumpAndSettle();

      expect(_cardOf(tester, 'NaiveProxy · tcp · tls').trailing, isNull);
      expect(_cardOf(tester, 'VLESS · tcp · reality').trailing, isNotNull);
    });

    testWidgets('замер прошёл, а чисел нет — это НЕ «не мерили»', (
      tester,
    ) async {
      // Ядро вернуло пустой список (так выглядел панельный путь без шва).
      // Отметка «Ваш замер: только что» рядом с пустой колонкой обещала бы,
      // что числа где-то есть; пустая колонка молча читается как «быстро».
      // Оба ответа неверны, и экран обязан назвать третий.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
          core: _ProbingCore(const <String, (int, ProbeVerdict)>{}),
        ),
      );
      await _settle(tester);
      await tester.pumpAndSettle();

      expect(
        find.textContaining('но ядро не назвало ни одного входа'),
        findsOneWidget,
      );
      expect(find.textContaining('Ваш замер:'), findsNothing);
      // Причина рядом, а не в логах: замер вернул ноль узлов.
      expect(
        find.textContaining('Ядро не вернуло ни одного узла'),
        findsOneWidget,
      );
    });

    testWidgets(
        'порядок строк подсказывает выбор, а не повторяет порядок '
        'оператора', (tester) async {
      // Пока чисел не было, список шёл в порядке инбаундов у узла — то есть в
      // порядке, в котором оператор их завёл. С числами это стало вредным:
      // вход, отвергнувший ключ, стоял бы первым просто потому, что он старше.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
          core: _ProbingCore(
            const <String, (int, ProbeVerdict)>{
              'DE Stealth': (118, ProbeVerdict.ok),
              'DE Speed': (96, ProbeVerdict.ok),
              'DE Shadow': (-1, ProbeVerdict.authRejected),
            },
            tcpMs: const <String, int>{'DE Shadow': 118},
          ),
        ),
      );
      await _settle(tester);
      await tester.pumpAndSettle();

      double y(String title) => tester.getTopLeft(find.text(title)).dy;

      // Быстрый подтверждённый выше медленного подтверждённого.
      expect(y('Hysteria2 · udp · tls'), lessThan(y('VLESS · tcp · reality')));
      // Неизмеренные ниже измеренных, но выше отвергнувшего ключ:
      // неизвестность — не приговор.
      expect(y('VLESS · tcp · reality'), lessThan(y('TUIC · udp · tls')));
      expect(y('TUIC · udp · tls'), lessThan(y('Shadowsocks · tcp · none')));
      // Строка, которую выбрать нечем, — в самом конце.
      expect(
        y('Shadowsocks · tcp · none'),
        lessThan(y('NaiveProxy · tcp · tls')),
      );
    });

    testWidgets('«Авто» без туннеля обещает выбор, а не называет его', (
      tester,
    ) async {
      // Ядро молчит — и выводить выбор из списка («наверное, Reality — он
      // быстрее») нельзя: это догадка тем же шрифтом, что и факт. Бейджа
      // «умный» на строке больше нет: похвала выбору — это не сам выбор.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const ProtocolScreen(),
          servers: <Server>[_germany],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
          core: _ProbingCore(const <String, (int, ProbeVerdict)>{
            'DE Stealth': (118, ProbeVerdict.ok),
          }),
        ),
      );
      await _settle(tester);
      await tester.pumpAndSettle();

      expect(find.text('Авто'), findsOneWidget);
      expect(find.text('УМНЫЙ'), findsNothing);
      expect(
        _cardOf(tester, 'Авто').subtitle,
        contains('Выберется при подключении'),
      );
    });
  });

  group('экран relay: узлы оператора, а не вписанная страна', () {
    testWidgets('вход есть в списке, выключен и назван причиной', (
      tester,
    ) async {
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const RelayScreen(),
          servers: <Server>[_germany, _canada],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
        ),
      );
      await _settle(tester);

      // Панель называет релэй УЗЛОМ у выхода (`via_relay`) — узел и показан,
      // а не одна строка «Россия», которую отдаёт агрегирующий `GET /relays`.
      expect(find.text('RU relay'), findsOneWidget);
      expect(find.text('Россия'), findsNothing);
      expect(find.text('RU'), findsOneWidget);

      // `chained_in_config: false` — метка есть, цепочки в теле нет. Причина
      // приходит от панели через слой предложения.
      _expectDisabledRow(tester, 'RU relay', 'цепочка в конфиге не строится');

      // «Выкл» и «Авто» цепочкой НЕ являются: первый просит ядро её не
      // строить, второй оставляет решение оператору, и оба уходят на провод
      // пустой строкой при любом флоте. Раньше они гасились возможностью
      // цепочки — и на живом флоте («chained_in_config: false» у КАЖДОЙ
      // подписки) экран приходил целиком мёртвым: «Выкл» стоял приглушённым и
      // нежатким, с подписью, которая описывала работу самого «Выкл» как
      // причину его недоступности.
      for (final title in <String>['Выкл', 'Авто']) {
        expect(find.text(title), findsOneWidget);
        final opacities = tester.widgetList<Opacity>(
          find.ancestor(of: find.text(title), matching: find.byType(Opacity)),
        );
        expect(
          opacities.any((o) => o.opacity == 0.45),
          isFalse,
          reason: '«$title» истинен при любом флоте и гаснуть не должен',
        );
        expect(
          _cardOf(tester, title).onTap,
          isNotNull,
          reason: '«$title» обязан оставаться выбираемым',
        );
      }

      // Причина названа один раз — баннером под заголовком «Входы оператора»,
      // к строкам которого она относится, — плюс подписью самой строки входа.
      // Под «Выкл» её нет: она его не описывает.
      expect(
        find.textContaining('цепочка в конфиге не строится'),
        findsNWidgets(2),
      );

      // Значение, которое СЕЙЧАС в силе, обязано быть видно. Сохранённый
      // индекс по умолчанию ноль — это «Выкл», и галочка стоит на нём. Раньше
      // `selected` был завязан на ту же возможность, и экран не мог показать
      // ничего: пользователь не видел, что в силе, и не мог снять вход,
      // которого не выбирал.
      expect(_cardOf(tester, 'Выкл').selected, isTrue);
      expect(_cardOf(tester, 'Авто').selected, isFalse);
      expect(_cardOf(tester, 'RU relay').selected, isFalse);
    });

    testWidgets('вход, который в силе, помечен галочкой даже недоступным', (
      tester,
    ) async {
      // Вход мог поставить оператор через CSM или он остался от прошлой
      // версии. Он записан и он не исполняется — пользователь обязан видеть
      // ОБА факта: иначе он стирает то, чего не видит.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const RelayScreen(),
          servers: <Server>[_germany, _canada],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
          // 0 — Выкл, 1 — Авто, 2 — страна RU.
          relayIndex: 2,
        ),
      );
      await _settle(tester);

      expect(_cardOf(tester, 'RU relay').selected, isTrue);
      expect(_cardOf(tester, 'Выкл').selected, isFalse);
      // И путь наружу открыт: «Выкл» нажимается.
      expect(_cardOf(tester, 'Выкл').onTap, isNotNull);
    });

    testWidgets('индекс вне списка показан так же, как уходит на провод', (
      tester,
    ) async {
      // Сохранённый индекс достался от удалённых выдуманных стран
      // (Турция/Казахстан/Финляндия занимали 2..4). Кодировщик провода на нём
      // отдаёт пустую строку — «входа не выбрано». Кламп к последней строке
      // назвал бы вход RU: экран и ядро расходились по построению.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const RelayScreen(),
          servers: <Server>[_germany, _canada],
          profile: _panelProfile(nodeId: 1, country: 'DE'),
          relayIndex: 7,
        ),
      );
      await _settle(tester);

      expect(_cardOf(tester, 'Выкл').selected, isTrue);
      expect(
        _cardOf(tester, 'RU relay').selected,
        isFalse,
        reason: 'ядру этот вход никто не называл',
      );
    });
  });

  group('карточка «Что применилось»', () {
    /// Отчёт ядра: пресет поднят, один список правил доехал, второй — нет.
    const report =
        '{"known":true,"raised_at_ms":1756800000000,"tunnel_up":true,'
        '"source":"preset",'
        '"preset":{"preset_id":"ru-smart","preset_name":"Россия (умный)",'
        '"emoji":"","country":"RU","final_action":"DIRECT",'
        '"rules":10,"dropped_rules":1,'
        '"rules_by_type":{"GEOIP":2,"GEOSITE":8},'
        '"geosite_tags":["telegram"],'
        '"sources":['
        '{"name":"ru-blocked","state":"file","rules":4,"kept_rules":4},'
        '{"name":"ru-blocked-ip","state":"dropped","reason":"no_mirror",'
        '"detail":"no verified file and no mirror base URL","rules":1,'
        '"kept_rules":0}]},'
        '"rules":10,'
        '"geosite":{"required":true,"tags":["telegram"],"state":"verified",'
        '"path":"/data/caramba/GeoSite.dat","size_bytes":1048576},'
        '"relay":{"state":"not_requested","dialer_proxy_seen":false}}';

    testWidgets('называет и доехавший список правил, и не доехавший', (
      tester,
    ) async {
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const Scaffold(
            body: SingleChildScrollView(child: AppliedRouteCard()),
          ),
          servers: <Server>[_germany],
          core: FakeVpnCore()..routeReportJson = report,
        ),
      );
      await _settle(tester);

      // SectionTitle рисует заголовок капсом.
      expect(find.text('ЧТО ПРИМЕНИЛОСЬ'), findsOneWidget);
      expect(find.text('Россия (умный)'), findsOneWidget);
      expect(find.text('10'), findsOneWidget);

      // Разрешившийся источник назван поимённо: без этого «пресет применён» и
      // «его правила доехали» остаются неотличимы.
      expect(
        find.textContaining('«ru-blocked»: список на месте, правил: 4'),
        findsOneWidget,
      );
      // Не доехавший назван вместе с причиной и ценой.
      expect(
        find.textContaining(
          '«ru-blocked-ip»: выброшен: файла нет и адреса зеркала тоже',
        ),
        findsOneWidget,
      );
      expect(find.text('1'), findsOneWidget, reason: 'правил потеряно');
      expect(find.textContaining('База GEOSITE проверена'), findsOneWidget);
    });

    testWidgets('блок рекламы: пресет его не включает — так и сказано', (
      tester,
    ) async {
      // Владелец: «настройки по типу блок рекламы или стриминг непонятно
      // работают или нет». `ru-smart` рекламу не режет — и карточка отвечает
      // на вопрос прямо, а не молчит.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const Scaffold(
            body: SingleChildScrollView(child: AppliedRouteCard()),
          ),
          servers: <Server>[_germany],
          core: FakeVpnCore()..routeReportJson = report,
        ),
      );
      await _settle(tester);

      expect(find.text('Блок рекламы'), findsOneWidget);
      expect(find.text('Стриминг через VPN'), findsOneWidget);
      expect(find.text('этот пресет его не включает'), findsNWidgets(2));
    });

    testWidgets('база GEOSITE не подтверждена — включённый блок не зеленеет', (
      tester,
    ) async {
      // Самый частый реальный случай: доверенного каталога нет, `geox-url` не
      // пишется, и обещать работающий блок рекламы нечем.
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          const Scaffold(
            body: SingleChildScrollView(child: AppliedRouteCard()),
          ),
          servers: <Server>[_germany],
          core: FakeVpnCore()
            ..routeReportJson = '{"known":true,"tunnel_up":true,"source":"preset",'
                '"preset":{"preset_id":"adblock",'
                '"preset_name":"Только блок рекламы","emoji":"",'
                '"rules":2,"dropped_rules":0,'
                '"geosite_tags":["category-ads-all"]},'
                '"rules":2,'
                '"geosite":{"required":true,"tags":["category-ads-all"],'
                '"state":"unknown","reason":"geox_unmanaged",'
                '"path":"/data/caramba/GeoSite.dat","size_bytes":0},'
                '"relay":{"state":"not_requested","dialer_proxy_seen":false}}',
        ),
      );
      await _settle(tester);

      expect(find.text('включён, подтвердить нечем'), findsOneWidget);
      expect(
        find.textContaining('Подтвердить базу GEOSITE нечем'),
        findsOneWidget,
      );
    });
  });
}

/// Сборка [Offering] из того, что источник действительно сказал.
///
/// Чистые функции: ни Riverpod, ни сети, ни времени. Всё, что нужно для
/// проверки, передаётся аргументами — поэтому поведение на живых формах панели
/// и импортированного тела проверяется тестом, а не наблюдением за экраном.
library;

import 'package:caramba_client/data/models/exit_location.dart'
    show countryNameOf, normalizeCountryCode;
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/panel_fleet.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/vpn/vpn_models.dart' show ImportedServer;

/// Один вход `GET /app/relays` — страна и число релэй-узлов в ней.
class RelayCountryRow {
  final String countryCode;
  final String? countryName;
  final int nodeCount;

  const RelayCountryRow({
    required this.countryCode,
    this.countryName,
    this.nodeCount = 0,
  });
}

/// Предложение, когда профиля нет. Пустых списков без причины слой не отдаёт
/// ни в одном режиме, поэтому «нечего показывать» тоже описано.
Offering buildEmptyOffering({
  OfferingReason reason = OfferingReason.noProfile,
  OfferingSource source = OfferingSource.none,
}) {
  final unavailable = Availability.unavailable(reason, Provenance.nothing);
  return Offering(
    source: source,
    exits: const <ExitOffer>[],
    relays: const <RelayOffer>[],
    // Пресеты остаются: они принадлежат ядру, а не источнику, и существуют
    // даже когда подключаться некуда. Скрыть их значило бы соврать, что маршрут
    // зависит от подписки.
    routePresets: _routePresetOffers(),
    capabilities: Capabilities(
      relayChaining: CapabilityOffer(
        availability: unavailable,
        verification: unavailable,
      ),
      adBlock: _presetCapability(blocksAds: true),
      streaming: _presetCapability(routesStreaming: true),
      protocolPin: CapabilityOffer(
        availability: unavailable,
        verification: unavailable,
      ),
      nodePin: CapabilityOffer(
        availability: unavailable,
        verification: unavailable,
      ),
    ),
  );
}

/// Панельный режим: `GET /app/servers` (с инбаундами) + `GET /app/relays`.
///
/// [servers] — то же, что показывает список серверов; сырые строки ответа
/// приносит [Server.rawJson]. Узел без сырой строки не выпадает из выдачи: он
/// остаётся выходом, у которого инбаунды в состоянии «неизвестно».
Offering buildPanelOffering({
  required List<Server> servers,
  List<RelayCountryRow> relayCountries = const <RelayCountryRow>[],
  String? selectedExitKey,
  String? selectedRelayCountry,
  bool loading = false,
  Object? error,
}) {
  final exits = <ExitOffer>[];
  final hops = <int, _HopAccumulator>{};
  final relayIds = _panelRelayNodeIds(servers);

  for (final s in servers) {
    final raw = s.rawJson;
    final read = PanelInboundsRead.fromRow(raw);
    final hop = panelRelayHopOf(raw);
    final key = s.id.toString();
    final code = normalizeCountryCode(s.countryCode);
    // `/servers` объявлен списком ВЫХОДОВ и сам вырезает релэй-узлы, поэтому
    // строка без встречной ссылки — выход по контракту эндпоинта, а не по
    // догадке. Ссылка же — прямое свидетельство обратного, и она сильнее
    // контракта: панель отдала как выход машину, которую сама называет входом
    // соседа.
    final role = relayIds.contains(s.id) ? NodeRole.relay : NodeRole.exit;

    if (hop != null && hop.nodeId != 0) {
      hops
          .putIfAbsent(hop.nodeId, () => _HopAccumulator(hop))
          .exitKeys
          .add(key);
    }

    exits.add(
      ExitOffer(
        key: key,
        panelNodeId: s.id,
        countryCode: code,
        countryName: countryNameOf(code),
        label: s.name,
        pingMs: s.pingMs,
        loadPct: s.load,
        inbounds: read.rows
            .map((r) => r.toOffer(role: role))
            .toList(growable: false),
        inboundsKnown: read.known,
        viaRelay: hop?.toRef(),
        role: role,
        // Пригодность узла решает тот же предикат, по которому подключение
        // выбирает путь: второй словарь статусов гарантированно разошёлся бы с
        // первым, и список рисовался бы живым там, где подключиться нельзя.
        //
        // Роль стоит ПЕРЕД статусом: «переполнен» описывает выход, которым
        // сейчас нельзя воспользоваться, а вход не станет выходом и на пустой
        // машине. Это разные новости, и вторая важнее.
        availability: role.isRelay
            ? const Availability.unavailable(
                OfferingReason.nodeIsRelay,
                kPanelViaRelayWire,
              )
            : (s.isSelectable
                  ? const Availability.available(kPanelServersWire)
                  : Availability.unavailable(
                      OfferingReason.nodeFull,
                      kPanelServersWire,
                      detail: s.status,
                    )),
      ),
    );
  }

  _sortExits(exits);

  final relays = _panelRelays(hops, relayCountries);

  return Offering(
    source: OfferingSource.panelRest,
    exits: List<ExitOffer>.unmodifiable(exits),
    relays: relays,
    routePresets: _routePresetOffers(),
    capabilities: Capabilities(
      relayChaining: _relayChaining(relays, hops.isNotEmpty),
      adBlock: _presetCapability(blocksAds: true),
      streaming: _presetCapability(routesStreaming: true),
      protocolPin: _protocolPin(exits, kPanelInboundsWire),
      nodePin: const CapabilityOffer(
        // Панель закрепляет узел (`PUT /subscriptions/{id}/selection`) и
        // отвечает тем, что применилось фактически, — это и есть канал
        // подтверждения.
        availability: Availability.available(
          Provenance(
            OfferingSource.panelRest,
            'PUT /app/subscriptions/{id}/selection node_id',
          ),
        ),
        verification: Availability.available(
          Provenance(
            OfferingSource.panelRest,
            'PUT /app/subscriptions/{id}/selection → node_id',
          ),
        ),
      ),
    ),
    selectedExitKey: selectedExitKey,
    selectedRelayCountry: selectedRelayCountry,
    loading: loading,
    error: error,
  );
}

/// Импортированная подписка: единственный источник — тело, которое разобрало
/// ядро.
///
/// Узел здесь не сообщается вовсе, поэтому машины разделяются по одинаковому
/// адресу `server:` — так же, как их различает глаз в теле конфига. Это
/// восстановление тождества, а не выдумка: адрес в теле есть, id узла в нём нет,
/// и слой честно помечает всю группу
/// [OfferingReason.sourceDoesNotReportExitIdentity].
///
/// ДУБЛИ «via 🇷🇺» НЕ РАЗВОДЯТСЯ ПО СТРОКАМ. Панельный sing-box-генератор
/// выпускает каждый вход дважды — прямым набором и набором через вход
/// (`detour: relay 🇷🇺`, у mihomo это `dialer-proxy`); снято на подписке 34:
/// 16 прокси немецкого узла при 8 инбаундах. Оба прокси упираются в ОДИН И ТОТ
/// ЖЕ вход одной машины — `85.215.196.151:13400`, vless/httpupgrade/tls, — и
/// экран отвечает на вопрос «чем узел принимает соединение», а не «каким путём
/// до него набирать». Второй вопрос принадлежит контролу «Relay (вход)»;
/// вынести его сюда значило бы поставить рядом две строки с побуквенно
/// одинаковым заголовком и разными числами. Оба имени прокси остаются в
/// строке, поэтому ни замер, ни закрепление не теряют ни одного из них.
Offering buildImportedOffering({
  required List<ImportedServer> servers,
  Map<String, int> latencyByProxy = const <String, int>{},
  String? selectedProxyName,
  bool loading = false,
  Object? error,
}) {
  const wire = Provenance(
    OfferingSource.subscriptionBody,
    'clash proxies[] (server/type/name/network/tls)',
  );
  const identityUnknown = Availability.unknown(
    OfferingReason.sourceDoesNotReportExitIdentity,
    wire,
  );
  const transportUnknown = Availability.unknown(
    OfferingReason.sourceDoesNotReportTransport,
    wire,
  );

  // Порядок групп — порядок первого появления прокси в теле: он единственный,
  // который у источника вообще есть.
  final order = <String>[];
  final byHost = <String, List<ImportedServer>>{};
  for (final s in servers) {
    // Адрес пустой (источник его не отдал) — машина остаётся своей группой по
    // имени прокси, иначе все безадресные схлопнулись бы в один «сервер».
    final host = s.server.trim().isEmpty ? 'proxy:${s.id}' : s.server.trim();
    if (!byHost.containsKey(host)) order.add(host);
    byHost.putIfAbsent(host, () => <ImportedServer>[]).add(s);
  }

  final exits = <ExitOffer>[];
  for (final host in order) {
    final group = byHost[host]!;
    final code = normalizeCountryCode(group.first.country);
    int? best;
    for (final s in group) {
      final ms = latencyByProxy[s.id];
      if (ms == null || ms < 0) continue;
      if (best == null || ms < best) best = ms;
    }
    final machineRole = _machineRoleOf(group);
    exits.add(
      ExitOffer(
        key: host,
        countryCode: code,
        countryName: countryNameOf(code),
        label: host,
        pingMs: best,
        role: machineRole,
        inbounds: group
            .map(
              (s) => InboundOffer(
                tag: s.name.isNotEmpty ? s.name : s.id,
                key: _importedKey(s),
                port: s.port > 0 ? s.port : null,
                label: s.type,
                // Имя прокси в теле — тот самый ключ, которым ядро закрепляет
                // выбор на сыром пути (`connectRaw` читает только его).
                proxyName: s.id,
                role: _importedRoleOf(s),
                availability: const Availability.available(wire),
              ),
            )
            .toList(growable: false),
        // Прокси в теле есть — значит инбаунды известны. Известна ли их ФОРМА
        // — утверждение отдельное, и его несёт [ProtocolSlate.known]: тело
        // называет `network` и `tls`, но ядро старше этих полей молчит о них,
        // и тогда список честно схлопывается обратно в семейства.
        inboundsKnown: const Availability.available(wire),
        // Статуса у прокси в теле нет: ядро отдало то, что разобрало, и
        // объявлять узел недоступным было бы выдумкой. Страна, которую ядро не
        // вывело, делает группу «без страны», но не выключает её.
        //
        // Роль — единственное исключение, и оно не про качество узла. Машина,
        // которую ядро назвало входом, выходом не является вовсе: в живом теле
        // подписки 34 это `relay 🇷🇺` (hysteria2, 141.98.191.214:11464), через
        // который набираются 14 прокси «via 🇷🇺». Оставить её нажимаемой —
        // предложить человеку выйти в интернет из страны входа, то есть ровно
        // оттуда, откуда он VPN и ставил. Строка при этом ОСТАЁТСЯ: её мерили,
        // она есть в теле, и исчезнуть без объяснения она не должна.
        availability: machineRole.isRelay
            ? const Availability.unavailable(OfferingReason.nodeIsRelay, wire)
            : const Availability.available(wire),
      ),
    );
  }

  _sortExits(exits);

  const relayImpossible = Availability.unavailable(
    OfferingReason.relayChainingUnsupportedBySource,
    wire,
  );

  return Offering(
    source: OfferingSource.subscriptionBody,
    exits: List<ExitOffer>.unmodifiable(exits),
    // Входов у импортированного тела нет как понятия: цепочка в clash-форме
    // невыразима, и придумать их из имён было бы ровно тем враньём, которое
    // слой запрещает.
    relays: const <RelayOffer>[],
    routePresets: _routePresetOffers(),
    capabilities: Capabilities(
      relayChaining: const CapabilityOffer(
        availability: relayImpossible,
        verification: relayImpossible,
      ),
      adBlock: _presetCapability(blocksAds: true),
      streaming: _presetCapability(routesStreaming: true),
      protocolPin: CapabilityOffer(
        availability: _protocolPin(exits, wire).availability,
        // Форму подтверждает само тело: `network` и `tls` у прокси называют
        // транспорт и защиту, и по ним строка отличает `vless · ws · tls` от
        // `vless · httpupgrade · tls`. Не назвало ни одного — подтверждать
        // нечем, и это говорится причиной, а не молчанием.
        verification: _anyFullyQualified(exits)
            ? const Availability.available(wire)
            : transportUnknown,
      ),
      nodePin: const CapabilityOffer(
        availability: Availability.available(
          Provenance(
            OfferingSource.subscriptionBody,
            'connectRaw(selected proxy name)',
          ),
        ),
        verification: identityUnknown,
      ),
    ),
    selectedExitKey: _hostOfProxy(servers, selectedProxyName),
    loading: loading,
    error: error,
  );
}

/// Список протоколов для выбранного узла — правило владельца буквально:
/// «протокол это выбор инбаундов, доступных в конфиге», отфильтрованных по
/// выбранному серверу.
///
/// [exitKey] `null` — узел не закреплён; тогда возвращается объединение по
/// всем доступным узлам с областью [ProtocolScope.wholeFleet]. Это ответ на
/// другой вопрос («что вообще бывает во флоте»), и область названа явно, чтобы
/// экран не выдал его за «что применится».
ProtocolSlate protocolSlateOf(Offering offering, {String? exitKey}) {
  if (offering.exits.isEmpty) {
    return ProtocolSlate(
      scope: ProtocolScope.none,
      rows: const <ProtocolRow>[],
      known: Availability.unavailable(
        offering.source == OfferingSource.none
            ? OfferingReason.noProfile
            : OfferingReason.sourceEmpty,
        Provenance.nothing,
      ),
    );
  }

  final selected = offering.exitByKey(exitKey);
  final scoped = selected != null
      ? <ExitOffer>[selected]
      : offering.exits.where((e) => e.isAvailable).toList(growable: false);

  // «Не знает форму» — свойство ИСТОЧНИКА, а не строки: у импортированного тела
  // неизвестен транспорт у каждого прокси сразу.
  final unknownForms = <Availability>[
    for (final e in scoped)
      if (!e.inboundsKnown.isAvailable) e.inboundsKnown,
  ];
  final collapsed = scoped.every(
    (e) => e.inbounds.every((i) => !i.key.isFullyQualified),
  );

  final Availability known;
  if (unknownForms.isNotEmpty && scoped.every((e) => e.inbounds.isEmpty)) {
    // Ни один узел области инбаундов не назвал: перечислять нечего, и это
    // молчание, а не пустота.
    known = unknownForms.first;
  } else if (collapsed && scoped.any((e) => e.inbounds.isNotEmpty)) {
    known = Availability.unknown(
      OfferingReason.sourceDoesNotReportTransport,
      scoped.first.inbounds.isEmpty
          ? Provenance.nothing
          : scoped.first.inbounds.first.origin,
    );
  } else if (unknownForms.isNotEmpty) {
    known = unknownForms.first;
  } else {
    known = Availability.available(
      scoped.first.inbounds.isEmpty
          ? scoped.first.inboundsKnown.origin
          : scoped.first.inbounds.first.origin,
    );
  }

  // Порядок строк — порядок инбаундов у узлов; одинаковые тройки на разных
  // узлах сливаются в одну строку, но помнят все узлы и все имена прокси.
  final order = <ProtocolKey>[];
  final byKey = <ProtocolKey, _RowAccumulator>{};
  for (final e in scoped) {
    for (final i in e.inbounds) {
      if (!byKey.containsKey(i.key)) order.add(i.key);
      final acc = byKey.putIfAbsent(i.key, () => _RowAccumulator(i));
      // Узел в строке считается ОДИН РАЗ, сколько бы прокси он в неё ни
      // положил. Панельный генератор выпускает один и тот же вход дважды —
      // прямым набором и через вход («via 🇷🇺»), — и список без этой проверки
      // объявлял бы «узлов с таким инбаундом: 2» про одну машину.
      acc.exitKeys.add(e.key);
      final pn = i.proxyName;
      if (pn != null && pn.isNotEmpty) acc.proxyNames.add(pn);
      // Тройка доступна, если её выпускает хотя бы один узел области: строка,
      // выключенная из-за соседней машины, отняла бы рабочий выбор.
      if (i.isAvailable) acc.best = i;
    }
  }

  final rows = <ProtocolRow>[
    for (final k in order)
      ProtocolRow(
        key: k,
        label: byKey[k]!.best.label,
        exitKeys: List<String>.unmodifiable(byKey[k]!.exitKeys),
        proxyNames: List<String>.unmodifiable(byKey[k]!.proxyNames),
        availability: byKey[k]!.best.availability,
      ),
  ];

  return ProtocolSlate(
    scope: selected != null
        ? ProtocolScope.singleExit
        : ProtocolScope.wholeFleet,
    exitKey: selected?.key,
    rows: List<ProtocolRow>.unmodifiable(rows),
    known: known,
  );
}

// ─── роль узла ─────────────────────────────────────────────────────────────

/// `nodes.id` тех строк `/servers`, на которые ДРУГИЕ строки того же ответа
/// ссылаются как на свой вход (`via_relay.node_id`).
///
/// Зачем это вообще нужно, если панель релэи из `/servers` вырезает сама
/// (`app.rs`, `filter(!n.is_relay)`): вырезает она по ОДНОЙ колонке `is_relay`,
/// хотя роль узла в её же модели считается шире — `Node::normalized_node_type`
/// объявляет релэем и узел с `node_type = 'relay'`. Узел, заведённый по
/// второму признаку и не помеченный первым, проходит фильтр и приезжает в
/// приложение как выход. Здесь он опознаётся тем же ответом, в котором
/// приехал: сосед уже назвал его своим входом.
///
/// Ссылка на себя не считается: `via_relay` на самого себя — не цепочка, а
/// испорченная строка, и выключать по ней рабочий узел нельзя.
Set<int> _panelRelayNodeIds(List<Server> servers) {
  final out = <int>{};
  for (final s in servers) {
    final hop = panelRelayHopOf(s.rawJson);
    if (hop == null || hop.nodeId == 0 || hop.nodeId == s.id) continue;
    out.add(hop.nodeId);
  }
  return out;
}

/// Роль ОДНОГО прокси импортированного тела — ровно то, что сказало ядро.
///
/// Ядро выводит её не из имени, а из ссылки: `dialer-proxy` (в sing-box —
/// `detour`) у соседнего прокси и есть единственное свидетельство, что узел
/// промежуточный (`subscription.ServersFromProxies` / `relayNames`).
///
/// Чего здесь СОЗНАТЕЛЬНО нет — догадки по имени. Живой `relay 🇷🇺` называется
/// так только потому, что так его назвал генератор оператора; чужая подписка
/// вправе назвать «relay-de-01» обычный выход, и слово в имени выключило бы
/// человеку рабочий узел. Порт тоже ничего не доказывает: у входа подписки 34
/// это hysteria2/11464 — то же семейство и соседний порт, что у выходов
/// (11466 DE, 11474 CA). Поэтому ядро старше поля `role` (пустая строка)
/// оставляет узел в списке: молчание — не запрет, и лучше показать лишнее, чем
/// спрятать чужой рабочий выход.
NodeRole _importedRoleOf(ImportedServer s) => switch (s.role) {
  'relay' => NodeRole.relay,
  'exit' => NodeRole.exit,
  _ => NodeRole.unknown,
};

/// Роль МАШИНЫ по ролям её прокси.
///
/// Входной машина считается только тогда, когда входными оказались ВСЕ её
/// прокси с известной ролью. Смешанный набор оставляет машину выходом: у неё
/// есть чем выйти, и убрать её из списка значило бы отнять рабочий выбор ради
/// соседнего прокси. Запрет в таком случае несёт сам прокси
/// ([InboundOffer.role]) — его читает автоподбор.
NodeRole _machineRoleOf(List<ImportedServer> group) {
  var known = 0;
  var relays = 0;
  for (final s in group) {
    final role = _importedRoleOf(s);
    if (role == NodeRole.unknown) continue;
    known++;
    if (role.isRelay) relays++;
  }
  if (known == 0) return NodeRole.unknown;
  return relays == known ? NodeRole.relay : NodeRole.exit;
}

// ─── внутреннее ────────────────────────────────────────────────────────────

/// Тройка инбаунда из строки импортированного тела.
///
/// Строка экрана — это ВХОД, а не семейство протокола. Пока ключ строился по
/// одному `type`, пять транспортов немецкой машины (reality, grpc, ws,
/// httpupgrade, tcp+tls) сливались в одну строку «VLESS» с лучшим числом из
/// пятерых, и сломанный httpupgrade был невидим: его отказ растворялся в
/// соседе того же семейства. Тело подписки транспорт и защиту НАЗЫВАЕТ
/// (`network`, `tls`, `reality-opts`), и ядро приносит их полями
/// `transport`/`security` — читать только `type` значило выбрасывать сказанное.
///
/// Части, которых источник не назвал, остаются пустыми, а не дописываются: у
/// hysteria2 и tuic транспорта поверх TCP нет вовсе, а ядро старше этих полей
/// молчит о форме для всех сразу. Пустая часть — это «не сказано», и
/// [ProtocolKey.isFullyQualified] отвечает на этот вопрос за строку.
ProtocolKey _importedKey(ImportedServer s) => ProtocolKey(
  protocol: s.type.trim().toLowerCase(),
  transport: s.transport.trim().toLowerCase(),
  security: s.security.trim().toLowerCase(),
);

/// Назвал ли источник форму хотя бы одного инбаунда целиком.
bool _anyFullyQualified(List<ExitOffer> exits) =>
    exits.any((e) => e.inbounds.any((i) => i.key.isFullyQualified));

class _HopAccumulator {
  final PanelRelayHopRow hop;
  final List<String> exitKeys = <String>[];
  _HopAccumulator(this.hop);
}

class _RowAccumulator {
  /// Узлы строки, каждый по одному разу и в порядке первой встречи. Множество,
  /// а не список: строка отвечает на вопрос «сколько МАШИН предлагают этот
  /// вход», и повтор машины в нём был бы выдуманной второй машиной.
  final Set<String> exitKeys = <String>{};
  final List<String> proxyNames = <String>[];
  InboundOffer best;
  _RowAccumulator(this.best);
}

void _sortExits(List<ExitOffer> exits) {
  exits.sort((a, b) {
    if (a.isAvailable != b.isAvailable) return a.isAvailable ? -1 : 1;
    // Узлы без страны уходят вниз: это «прочее», а не выбор.
    final ua = a.countryCode.isEmpty;
    final ub = b.countryCode.isEmpty;
    if (ua != ub) return ua ? 1 : -1;
    final pa = (a.pingMs == null || a.pingMs! < 0) ? 1 << 30 : a.pingMs!;
    final pb = (b.pingMs == null || b.pingMs! < 0) ? 1 << 30 : b.pingMs!;
    if (pa != pb) return pa.compareTo(pb);
    return a.label.compareTo(b.label);
  });
}

List<RelayOffer> _panelRelays(
  Map<int, _HopAccumulator> hops,
  List<RelayCountryRow> countries,
) {
  final out = <RelayOffer>[];
  final coveredCountries = <String>{};

  final ids = hops.keys.toList()..sort();
  for (final id in ids) {
    final acc = hops[id]!;
    final code = normalizeCountryCode(acc.hop.countryCode);
    coveredCountries.add(code);
    out.add(
      RelayOffer(
        panelNodeId: id,
        countryCode: code,
        countryName: countryNameOf(code),
        label: acc.hop.name.isNotEmpty ? acc.hop.name : countryNameOf(code),
        reachableFromExitKeys: List<String>.unmodifiable(acc.exitKeys),
        reachability: const Availability.available(kPanelViaRelayWire),
        // Единственный источник этого вывода — сама панель: она сообщает,
        // строит ли генератор цепочку. Захардкоженного «на mihomo нельзя»
        // здесь нет намеренно — сменится генератор, сменится и ответ.
        availability: acc.hop.chainedInConfig
            ? const Availability.available(kPanelViaRelayWire)
            : const Availability.unavailable(
                OfferingReason.relayNotChainedByGenerator,
                kPanelViaRelayWire,
              ),
      ),
    );
  }

  for (final c in countries) {
    final code = normalizeCountryCode(c.countryCode);
    if (code.isEmpty || coveredCountries.contains(code)) continue;
    final name = (c.countryName ?? '').trim();
    out.add(
      RelayOffer(
        countryCode: code,
        countryName: name.isNotEmpty ? name : countryNameOf(code),
        label: name.isNotEmpty ? name : countryNameOf(code),
        nodeCountInCountry: c.nodeCount,
        reachableFromExitKeys: const <String>[],
        // Ни один выход на этот вход не сослался: какие узлы за страной стоят и
        // строится ли через них цепочка, панель на этом эндпоинте не говорит.
        reachability: const Availability.unknown(
          OfferingReason.panelReportsRelaysByCountryOnly,
          kPanelRelaysWire,
        ),
        availability: const Availability.unknown(
          OfferingReason.panelReportsRelaysByCountryOnly,
          kPanelRelaysWire,
        ),
      ),
    );
  }

  return List<RelayOffer>.unmodifiable(out);
}

CapabilityOffer _relayChaining(List<RelayOffer> relays, bool anyHop) {
  if (relays.isEmpty) {
    return const CapabilityOffer(
      availability: Availability.unavailable(
        OfferingReason.notInFleet,
        kPanelRelaysWire,
      ),
      verification: Availability.available(kPanelRelaysWire),
    );
  }
  final chained = relays.any((r) => r.availability.isAvailable);
  if (chained) {
    return const CapabilityOffer(
      availability: Availability.available(kPanelViaRelayWire),
      verification: Availability.available(kPanelViaRelayWire),
    );
  }
  if (anyHop) {
    return const CapabilityOffer(
      availability: Availability.unavailable(
        OfferingReason.relayNotChainedByGenerator,
        kPanelViaRelayWire,
      ),
      verification: Availability.available(kPanelViaRelayWire),
    );
  }
  return const CapabilityOffer(
    availability: Availability.unknown(
      OfferingReason.panelReportsRelaysByCountryOnly,
      kPanelRelaysWire,
    ),
    verification: Availability.unknown(
      OfferingReason.panelReportsRelaysByCountryOnly,
      kPanelRelaysWire,
    ),
  );
}

/// Закрепление протокола возможно, когда у инбаунда есть имя прокси: именно им
/// выбор адресуется в конфиге. Без имени строка пикера ни к чему не привязана.
CapabilityOffer _protocolPin(List<ExitOffer> exits, Provenance wire) {
  final pinnable = exits.any(
    (e) => e.inbounds.any(
      (i) => i.isAvailable && (i.proxyName?.isNotEmpty ?? false),
    ),
  );
  if (pinnable) {
    return CapabilityOffer(
      availability: Availability.available(wire),
      verification: Availability.available(wire),
    );
  }
  final silent = exits.any((e) => !e.inboundsKnown.isAvailable);
  return CapabilityOffer(
    availability: silent
        ? Availability.unknown(OfferingReason.panelDidNotReportInbounds, wire)
        : Availability.unavailable(OfferingReason.notInFleet, wire),
    verification: Availability.unknown(
      OfferingReason.panelDidNotReportInbounds,
      wire,
    ),
  );
}

/// Блок рекламы и стриминг живут не в тумблере, а в пресетах ядра. Возможность
/// есть ровно тогда, когда такой пресет в реестре есть; подтвердить её работу
/// нечем — правила применяются молча, канала обратной связи нет ни у панели, ни
/// у ядра. Именно поэтому «непонятно, работает или нет»: это не догадка, а
/// свойство системы, и слой называет его, а не сглаживает.
CapabilityOffer _presetCapability({
  bool blocksAds = false,
  bool routesStreaming = false,
}) {
  final matching = kCoreRoutePresets.where(
    (p) => (blocksAds && p.blocksAds) || (routesStreaming && p.routesStreaming),
  );
  return CapabilityOffer(
    availability: matching.isEmpty
        ? const Availability.unavailable(
            OfferingReason.notInFleet,
            CoreRoutePreset.origin,
          )
        : const Availability.available(CoreRoutePreset.origin),
    verification: const Availability.unknown(
      OfferingReason.noFeedbackChannel,
      CoreRoutePreset.origin,
    ),
  );
}

List<RoutePresetOffer>
_routePresetOffers() => List<RoutePresetOffer>.unmodifiable(
  kCoreRoutePresets.map(
    (p) => RoutePresetOffer(
      preset: p,
      legacyIndex: kLegacyRouteIndexByCoreId[p.id],
      // Пресет на встроенных базах mihomo применяется целиком. Пресету с
      // внешними списками нужен живой /rulesets, и НИКТО не сообщает, отдаёт ли
      // его зеркало: ни панель, ни ядро. Это «неизвестно», а не «недоступно» —
      // остальные правила пресета работают в любом случае.
      availability: p.isSelfContained
          ? const Availability.available(CoreRoutePreset.origin)
          : Availability.unknown(
              OfferingReason.rulesetMirrorUnverified,
              CoreRoutePreset.origin,
              detail: p.requiredRulesets.join(', '),
            ),
    ),
  ),
);

/// Хост машины, которой принадлежит закреплённый прокси; `null` — прокси не
/// закреплён или его в теле нет.
String? _hostOfProxy(List<ImportedServer> servers, String? proxyName) {
  if (proxyName == null || proxyName.isEmpty) return null;
  for (final s in servers) {
    if (s.id != proxyName) continue;
    final host = s.server.trim();
    return host.isEmpty ? 'proxy:${s.id}' : host;
  }
  return null;
}

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

  for (final s in servers) {
    final raw = s.rawJson;
    final read = PanelInboundsRead.fromRow(raw);
    final hop = panelRelayHopOf(raw);
    final key = s.id.toString();
    final code = normalizeCountryCode(s.countryCode);

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
        inbounds: read.rows.map((r) => r.toOffer()).toList(growable: false),
        inboundsKnown: read.known,
        viaRelay: hop?.toRef(),
        // Пригодность узла решает тот же предикат, по которому подключение
        // выбирает путь: второй словарь статусов гарантированно разошёлся бы с
        // первым, и список рисовался бы живым там, где подключиться нельзя.
        availability: s.isSelectable
            ? const Availability.available(kPanelServersWire)
            : Availability.unavailable(
                OfferingReason.nodeFull,
                kPanelServersWire,
                detail: s.status,
              ),
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
Offering buildImportedOffering({
  required List<ImportedServer> servers,
  Map<String, int> latencyByProxy = const <String, int>{},
  String? selectedProxyName,
  bool loading = false,
  Object? error,
}) {
  const wire = Provenance(
    OfferingSource.subscriptionBody,
    'clash proxies[] (server/type/name)',
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
    exits.add(
      ExitOffer(
        key: host,
        countryCode: code,
        countryName: countryNameOf(code),
        label: host,
        pingMs: best,
        inbounds: group
            .map(
              (s) => InboundOffer(
                tag: s.name.isNotEmpty ? s.name : s.id,
                key: ProtocolKey(protocol: s.type.toLowerCase()),
                port: s.port > 0 ? s.port : null,
                label: s.type,
                // Имя прокси в теле — тот самый ключ, которым ядро закрепляет
                // выбор на сыром пути (`connectRaw` читает только его).
                proxyName: s.id,
                availability: const Availability.available(wire),
              ),
            )
            .toList(growable: false),
        // Прокси в теле есть — значит инбаунды известны. Неизвестна их ФОРМА, и
        // это отдельное утверждение, которое несёт [ProtocolSlate.known].
        inboundsKnown: const Availability.available(wire),
        // Статуса у прокси в теле нет: ядро отдало то, что разобрало, и
        // объявлять узел недоступным было бы выдумкой. Страна, которую ядро не
        // вывело, делает группу «без страны», но не выключает её.
        availability: const Availability.available(wire),
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
        // Закрепить прокси можно, а вот назвать его форму — нет.
        verification: transportUnknown,
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

// ─── внутреннее ────────────────────────────────────────────────────────────

class _HopAccumulator {
  final PanelRelayHopRow hop;
  final List<String> exitKeys = <String>[];
  _HopAccumulator(this.hop);
}

class _RowAccumulator {
  final List<String> exitKeys = <String>[];
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

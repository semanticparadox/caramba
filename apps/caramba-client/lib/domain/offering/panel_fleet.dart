/// Разбор ответа панели `GET /api/v2/app/servers` — единственного источника,
/// который знает про узлы ВСЁ: `nodes.id`, страну, инбаунды тройками
/// `protocol/network/security` и релэй-привязку.
///
/// Форма зеркалит `AppServer` / `AppInbound` / `AppRelayHop` в
/// apps/caramba-panel/src/api/v2/app.rs. Разбор живёт здесь, а не в
/// `data/models/server.dart`, потому что это форма ПРЕДЛОЖЕНИЯ, а не строки
/// списка серверов.
///
/// Главное различие, которое разбор обязан сохранить: `inbounds: null` («панель
/// не смогла прочитать», причина в `inbounds_error`) и `inbounds: []` («у узла
/// нет ни одного включённого инбаунда») — РАЗНЫЕ ответы. Первый даёт
/// [OfferingStatus.unknown], второй — честный пустой список.
library;

import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';

/// Где на проводе живут инбаунды узла.
const Provenance kPanelInboundsWire = Provenance(
  OfferingSource.panelRest,
  'GET /app/servers[].inbounds[]',
);

/// Где на проводе живёт сам узел.
const Provenance kPanelServersWire = Provenance(
  OfferingSource.panelRest,
  'GET /app/servers[]',
);

/// Где на проводе живут страны входа.
const Provenance kPanelRelaysWire = Provenance(
  OfferingSource.panelRest,
  'GET /app/relays[]',
);

/// Где на проводе живёт привязка выхода к входу.
const Provenance kPanelViaRelayWire = Provenance(
  OfferingSource.panelRest,
  'GET /app/servers[].via_relay',
);

/// Один инбаунд из ответа панели, ещё не превращённый в предложение.
class PanelInboundRow {
  final int? id;
  final String tag;
  final String protocol;
  final String network;
  final String security;
  final int? port;
  final String label;
  final String? proxyName;
  final bool available;

  /// Машиночитаемая причина панели: `protocol_not_emitted_by_clash`,
  /// `amneziawg_disabled`, `transport_not_supported_by_clash`.
  final String? unavailableReason;

  const PanelInboundRow({
    required this.tag,
    required this.protocol,
    required this.network,
    required this.security,
    required this.label,
    required this.available,
    this.id,
    this.port,
    this.proxyName,
    this.unavailableReason,
  });

  factory PanelInboundRow.fromJson(Map<String, dynamic> json) =>
      PanelInboundRow(
        id: (json['id'] as num?)?.toInt(),
        tag: _str(json['tag']),
        protocol: _str(json['protocol']).toLowerCase(),
        network: _str(json['network']).toLowerCase(),
        security: _str(json['security']).toLowerCase(),
        port: (json['port'] as num?)?.toInt(),
        label: _str(json['label']),
        proxyName: _strOrNull(json['proxy_name']),
        // Отсутствие ключа читается как «недоступен»: панель, которая про
        // пригодность промолчала, не даёт права считать инбаунд рабочим.
        available: json['available'] == true,
        unavailableReason: _strOrNull(json['unavailable_reason']),
      );

  /// Превращает строку панели в предложение.
  ///
  /// [role] приходит СНАРУЖИ: сама строка инбаунда про роль узла не знает
  /// ничего, а вывод «этот узел — вход» делается сравнением всего ответа
  /// `/servers` целиком (см. `_panelRelayNodeIds` в offering_builder.dart).
  InboundOffer toOffer({NodeRole role = NodeRole.unknown}) => InboundOffer(
    role: role,
    panelInboundId: id,
    tag: tag,
    key: ProtocolKey(
      protocol: protocol,
      transport: network,
      security: security,
    ),
    port: port,
    label: label,
    proxyName: proxyName,
    availability: available
        ? const Availability.available(kPanelInboundsWire)
        : Availability.unavailable(
            _reasonOf(unavailableReason),
            kPanelInboundsWire,
            detail: unavailableReason,
          ),
  );

  /// Причина панели → причина слоя. Незнакомая строка не проглатывается: она
  /// доезжает как [OfferingReason.inboundNotEmittedByClash] с исходным текстом
  /// в `detail`, чтобы новая ветка панели не стала молчаливым «доступно».
  static OfferingReason _reasonOf(String? raw) {
    switch (raw) {
      case 'amneziawg_disabled':
        return OfferingReason.inboundAmneziawgDisabled;
      case 'transport_not_supported_by_clash':
        return OfferingReason.inboundTransportNotSupported;
      case 'protocol_not_emitted_by_clash':
      default:
        return OfferingReason.inboundNotEmittedByClash;
    }
  }
}

/// Привязка выхода к входу (`via_relay`).
class PanelRelayHopRow {
  final int nodeId;
  final String name;
  final String countryCode;
  final bool chainedInConfig;

  const PanelRelayHopRow({
    required this.nodeId,
    required this.name,
    required this.countryCode,
    required this.chainedInConfig,
  });

  factory PanelRelayHopRow.fromJson(Map<String, dynamic> json) =>
      PanelRelayHopRow(
        nodeId: (json['node_id'] as num?)?.toInt() ?? 0,
        name: _str(json['name']),
        countryCode: _str(json['country_code']).toUpperCase(),
        // Как и с инбаундами: молчание про цепочку не означает, что она есть.
        chainedInConfig: json['chained_in_config'] == true,
      );

  RelayHopRef toRef() => RelayHopRef(
    nodeId: nodeId,
    name: name,
    countryCode: countryCode,
    chainedInConfig: chainedInConfig,
  );
}

/// Инбаунды узла из сырой строки `/servers`, с сохранением различия
/// «не сообщила» / «их нет».
class PanelInboundsRead {
  /// Разобранные строки; пусто и при «не сообщила», и при «их нет».
  final List<PanelInboundRow> rows;

  /// Знает ли панель инбаунды узла.
  final Availability known;

  const PanelInboundsRead(this.rows, this.known);

  /// [raw] — строка `/servers`, как её прислала панель; `null` — приложение её
  /// не донесло (объект собран не из JSON).
  factory PanelInboundsRead.fromRow(Map<String, dynamic>? raw) {
    if (raw == null) {
      return const PanelInboundsRead(
        <PanelInboundRow>[],
        Availability.unknown(
          OfferingReason.panelDidNotReportInbounds,
          kPanelServersWire,
          detail: 'ответ панели не сохранён',
        ),
      );
    }
    final list = raw['inbounds'];
    if (list is! List) {
      // Ключ есть и он null (или его нет вовсе) — панель сама не смогла.
      return PanelInboundsRead(
        const <PanelInboundRow>[],
        Availability.unknown(
          OfferingReason.panelDidNotReportInbounds,
          kPanelInboundsWire,
          detail: _strOrNull(raw['inbounds_error']),
        ),
      );
    }
    final rows = list
        .whereType<Map<dynamic, dynamic>>()
        .map((e) => PanelInboundRow.fromJson(e.cast<String, dynamic>()))
        .toList(growable: false);
    return PanelInboundsRead(
      rows,
      const Availability.available(kPanelInboundsWire),
    );
  }
}

/// Релэй-привязка узла из сырой строки `/servers`.
PanelRelayHopRow? panelRelayHopOf(Map<String, dynamic>? raw) {
  final via = raw?['via_relay'];
  if (via is! Map) return null;
  return PanelRelayHopRow.fromJson(via.cast<String, dynamic>());
}

String _str(Object? v) => v is String ? v.trim() : '';

String? _strOrNull(Object? v) {
  final s = _str(v);
  return s.isEmpty ? null : s;
}

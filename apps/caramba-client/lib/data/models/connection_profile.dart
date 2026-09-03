import 'dart:convert';

import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

/// Тип подключения профиля.
///
/// - [rawSub] — импортированная подписка/конфиг (вставка URL/QR/файла). Ядро
///   поднимает туннель из локального mihomo-конфига без обращения к панели.
/// - [panelAccount] — аккаунт панели (subscription UUID + JWT). Ядро тянет
///   конфиг через панель, как и раньше.
///
/// Имя в JSON совпадает с `name` (стабильный ключ), чтобы старые записи не
/// ломались при добавлении новых типов.
enum ProfileType {
  rawSub,
  panelAccount;

  static ProfileType fromName(String? raw) {
    switch (raw) {
      case 'panelAccount':
        return ProfileType.panelAccount;
      case 'rawSub':
      default:
        return ProfileType.rawSub;
    }
  }
}

/// Профиль подключения — клиентский MULTI-PROFILE примитив (план §4.4).
///
/// ВНИМАНИЕ (коллизия имён): это НЕ аккаунт-профиль (`lib/features/profile/`,
/// `profile_state.dart`, `models/subscription.dart` — там подписки/устройства/
/// рефералы/семья). [ConnectionProfile] описывает одно подключение: либо
/// импортированную подписку ([ProfileType.rawSub]), либо аккаунт панели
/// ([ProfileType.panelAccount]).
///
/// Ровно один профиль активен в любой момент. Активный профиль ведёт И
/// подключение, И (в следующих фазах) тему. В P1 тему НЕ трогаем — только
/// храним [brandingCache] и [lastActiveMs] на будущее.
class ConnectionProfile {
  /// Стабильный идентификатор (генерируется при создании, не меняется).
  final String id;

  /// Тип подключения: импорт-подписка или аккаунт панели.
  final ProfileType type;

  /// Человекочитаемое имя в списке/свитчере.
  final String displayName;

  /// Источник: URL подписки (rawSub) или origin/панель-URL (panelAccount).
  final String source;

  /// URL панели. Заполнен только для [ProfileType.panelAccount].
  final String? panelUrl;

  /// UUID подписки. Заполнен только для [ProfileType.panelAccount].
  final String? subscriptionUuid;

  /// JWT access-токен. Заполнен только для [ProfileType.panelAccount].
  final String? accessToken;

  /// Импортированный mihomo YAML (rawSub), когда хранится локально. Передаётся
  /// нативной стороне через `connectRaw` (Build C).
  final String? rawConfig;

  /// Формат [rawConfig] на проводе (`auto` / `clash` / `singbox` / `v2ray` /
  /// `uri`) — ровно то, что ждёт `subimport.Import`. Пустая строка в старых
  /// записях читается как `auto`.
  final String format;

  /// Узлы подписки, полученные от ядра при `importSubscription`. Кэш: список
  /// серверов показывается до подключения и переживает перезапуск.
  final List<ImportedServer> servers;

  /// `id` узла (имя прокси), к которому прикреплён селектор CARAMBA.
  /// `null` — автоматический выбор ядром.
  final String? selectedServerId;

  /// Последний замер задержек: `id узла -> мс` (-1 = таймаут) + отметка времени.
  /// `null` — ни разу не мерили.
  final ProbeSnapshot? lastProbe;

  /// Когда список [servers] последний раз обновлялся (мс эпохи); 0 — никогда.
  final int serversUpdatedMs;

  /// Выбранная страна выхода (ISO-2, верхний регистр). `null` — авто.
  ///
  /// Пин узла ([selectedServerId]) переживал перезапуск и раньше, а выбор в
  /// панельном режиме жил только в памяти: один и тот же контрол вёл себя
  /// по-разному в зависимости от режима. Страна хранится ЗДЕСЬ, на профиле,
  /// потому что она свойство подключения, а не приложения: два профиля с
  /// разными операторами имеют разные списки стран.
  final String? selectedExitCountry;

  /// Пин конкретного узла панели (`nodes.id`). `null` — выбирает приложение.
  /// Импортированная подписка пинится через [selectedServerId] (там ключ —
  /// имя прокси, числа не существует).
  final int? selectedExitNodeId;

  /// Кэш брендинга (произвольный JSON). В P1 только хранится, тему не ведёт.
  final Map<String, dynamic>? brandingCache;

  /// Время последней активации (мс эпохи). 0 — ещё не активировался.
  final int lastActiveMs;

  /// Состояние CSM/1: закреплённый корневой ключ, отметки максимума версий,
  /// временной пол, возможности оператора, последние проверенные документы,
  /// настройки с происхождением и предпочтения лестницы.
  ///
  /// `null` означает «профиль никогда не закреплял корневой ключ»: запись,
  /// сделанная до CSM, и профиль из legacy-импорта `carambaconnect://import`
  /// таким и остаются. Молча повышать их до CSM нельзя, потому что закреплять
  /// нечего (02-SPEC.md 9.8).
  final CsmProfileState? csm;

  const ConnectionProfile({
    required this.id,
    required this.type,
    required this.displayName,
    required this.source,
    this.panelUrl,
    this.subscriptionUuid,
    this.accessToken,
    this.rawConfig,
    this.format = 'auto',
    this.servers = const <ImportedServer>[],
    this.selectedServerId,
    this.lastProbe,
    this.serversUpdatedMs = 0,
    this.selectedExitCountry,
    this.selectedExitNodeId,
    this.brandingCache,
    this.lastActiveMs = 0,
    this.csm,
  });

  bool get isRaw => type == ProfileType.rawSub;
  bool get isPanel => type == ProfileType.panelAccount;

  /// Профиль закрепил корневой ключ оператора. Обратной дороги в
  /// непроверяемый legacy-режим у такого профиля нет ни по какой причине
  /// (INV-13).
  bool get isCsmPinned => csm?.stage.isPinned ?? false;

  /// Сколько узлов известно по кэшу импорта.
  int get serverCount => servers.length;

  /// Задержка узла из последнего замера (мс), `null` — узел не мерили.
  int? latencyOf(String serverId) => lastProbe?.latencyMs[serverId];

  factory ConnectionProfile.fromJson(Map<String, dynamic> json) =>
      ConnectionProfile(
        id: json['id'] as String,
        type: ProfileType.fromName(json['type'] as String?),
        displayName: (json['display_name'] as String?) ?? '',
        source: (json['source'] as String?) ?? '',
        panelUrl: json['panel_url'] as String?,
        subscriptionUuid: json['subscription_uuid'] as String?,
        accessToken: json['access_token'] as String?,
        rawConfig: json['raw_config'] as String?,
        // Записи до generic-режима не несут этих полей: читаем их мягко, чтобы
        // старый JSON грузился без миграции.
        format: _decodeFormat(json['format']),
        servers: _decodeServers(json['servers']),
        selectedServerId: _nullIfEmpty(json['selected_server_id']),
        lastProbe: ProbeSnapshot.fromJson(json['last_probe']),
        serversUpdatedMs: _decodeMs(json['servers_updated_ms']),
        // Записи до выбора страны этих ключей не несут: мягкий разбор, как и
        // у format выше, иначе старый профиль перестал бы читаться целиком.
        selectedExitCountry: _decodeCountry(json['selected_exit_country']),
        selectedExitNodeId: _decodeNodeId(json['selected_exit_node_id']),
        brandingCache: _decodeBranding(json['branding_cache']),
        lastActiveMs: (json['last_active_ms'] as num?)?.toInt() ?? 0,
        // Запись, сделанная до CSM, не несёт этого ключа: читаем мягко, ровно
        // как поле format выше, и получаем «корень не закреплён».
        csm: CsmProfileState.fromJson(json['csm']),
      );

  Map<String, dynamic> toJson() => {
    'id': id,
    'type': type.name,
    'display_name': displayName,
    'source': source,
    'panel_url': panelUrl,
    'subscription_uuid': subscriptionUuid,
    'access_token': accessToken,
    'raw_config': rawConfig,
    'format': format,
    'servers': servers.map((s) => s.toJson()).toList(growable: false),
    'selected_server_id': selectedServerId,
    'last_probe': lastProbe?.toJson(),
    'servers_updated_ms': serversUpdatedMs,
    'selected_exit_country': selectedExitCountry,
    'selected_exit_node_id': selectedExitNodeId,
    'branding_cache': brandingCache,
    'last_active_ms': lastActiveMs,
    'csm': csm?.toJson(),
  };

  ConnectionProfile copyWith({
    String? id,
    ProfileType? type,
    String? displayName,
    String? source,
    String? panelUrl,
    String? subscriptionUuid,
    String? accessToken,
    String? rawConfig,
    String? format,
    List<ImportedServer>? servers,
    String? selectedServerId,
    ProbeSnapshot? lastProbe,
    int? serversUpdatedMs,
    String? selectedExitCountry,
    int? selectedExitNodeId,
    Map<String, dynamic>? brandingCache,
    int? lastActiveMs,
    CsmProfileState? csm,
    bool clearSelectedServer = false,
    bool clearExitCountry = false,
    bool clearExitNode = false,
    bool clearCsm = false,
  }) => ConnectionProfile(
    id: id ?? this.id,
    type: type ?? this.type,
    displayName: displayName ?? this.displayName,
    source: source ?? this.source,
    panelUrl: panelUrl ?? this.panelUrl,
    subscriptionUuid: subscriptionUuid ?? this.subscriptionUuid,
    accessToken: accessToken ?? this.accessToken,
    rawConfig: rawConfig ?? this.rawConfig,
    format: format ?? this.format,
    servers: servers ?? this.servers,
    selectedServerId: clearSelectedServer
        ? null
        : (selectedServerId ?? this.selectedServerId),
    lastProbe: lastProbe ?? this.lastProbe,
    serversUpdatedMs: serversUpdatedMs ?? this.serversUpdatedMs,
    // «Авто» — это осознанный сброс, а не отсутствие аргумента, поэтому у
    // страны и узла свои явные флаги очистки (как у clearSelectedServer).
    selectedExitCountry: clearExitCountry
        ? null
        : (selectedExitCountry ?? this.selectedExitCountry),
    selectedExitNodeId: clearExitNode
        ? null
        : (selectedExitNodeId ?? this.selectedExitNodeId),
    brandingCache: brandingCache ?? this.brandingCache,
    lastActiveMs: lastActiveMs ?? this.lastActiveMs,
    // csm не снимается обычной мутацией: липкое правило INV-13 требует, чтобы
    // закреплённый корень пережил любую перезапись профиля. Снять его можно
    // только явным clearCsm, и это отдельный флаг именно чтобы такой сброс был
    // видимым решением, а не побочным эффектом импорта.
    csm: clearCsm ? null : (csm ?? this.csm),
  );

  /// Формат импорта: пустая/чужая запись читается как `auto`.
  static String _decodeFormat(Object? v) {
    if (v is String && v.trim().isNotEmpty) return v.trim();
    return 'auto';
  }

  /// Отметка времени: не-число (запись чужой версии) читается как «неизвестно».
  static int _decodeMs(Object? v) => v is num ? v.toInt() : 0;

  /// Страна выхода: приводим к ISO-2 верхнего регистра. Всё, что не похоже на
  /// код страны (пустая строка, число, «auto»), читается как «авто».
  static String? _decodeCountry(Object? v) {
    if (v is! String) return null;
    final code = v.trim().toUpperCase();
    return code.length == 2 ? code : null;
  }

  /// Пин узла панели: не-число читается как «пина нет».
  static int? _decodeNodeId(Object? v) => v is num ? v.toInt() : null;

  static String? _nullIfEmpty(Object? v) {
    if (v is String && v.isNotEmpty) return v;
    return null;
  }

  /// Кэш узлов: не-список и мусорные элементы дают пустой список.
  static List<ImportedServer> _decodeServers(Object? v) {
    if (v is! List) return const <ImportedServer>[];
    return v
        .whereType<Map<Object?, Object?>>()
        .map(ImportedServer.fromMap)
        .where((s) => s.id.isNotEmpty)
        .toList(growable: false);
  }

  /// Тивкость: branding мог быть как map, так и строкой-JSON (старые записи).
  static Map<String, dynamic>? _decodeBranding(Object? v) {
    if (v == null) return null;
    if (v is Map<String, dynamic>) return v;
    if (v is Map) return v.map((k, val) => MapEntry(k.toString(), val));
    if (v is String && v.isNotEmpty) {
      try {
        final decoded = jsonDecode(v);
        if (decoded is Map<String, dynamic>) return decoded;
      } catch (_) {
        return null;
      }
    }
    return null;
  }
}

/// Снимок последнего замера задержек узлов профиля (ABI v2 `probe`).
///
/// Хранится на [ConnectionProfile], чтобы список серверов показывал пинги сразу
/// после перезапуска, а не пустые прочерки. `-1` в [latencyMs] означает таймаут
/// (узел не ответил за отведённое время) — это НЕ то же самое, что «не мерили»:
/// не измеренный узел просто отсутствует в карте.
class ProbeSnapshot {
  /// `id` узла (имя прокси) -> задержка в мс; -1 = таймаут.
  final Map<String, int> latencyMs;

  /// Когда замер выполнен (мс эпохи).
  final int updatedMs;

  const ProbeSnapshot({required this.latencyMs, required this.updatedMs});

  /// Пустой снимок (ни один узел не измерен).
  static const ProbeSnapshot empty = ProbeSnapshot(
    latencyMs: <String, int>{},
    updatedMs: 0,
  );

  bool get isEmpty => latencyMs.isEmpty;

  DateTime? get updatedAt =>
      updatedMs > 0 ? DateTime.fromMillisecondsSinceEpoch(updatedMs) : null;

  Map<String, dynamic> toJson() => {
    'latency_ms': latencyMs,
    'updated_ms': updatedMs,
  };

  /// Разбор записи из JSON профиля. `null`/мусор дают `null` (не мерили).
  static ProbeSnapshot? fromJson(Object? raw) {
    if (raw is! Map) return null;
    final rawLatency = raw['latency_ms'];
    if (rawLatency is! Map) return null;
    final latency = <String, int>{};
    rawLatency.forEach((k, v) {
      if (k is String && k.isNotEmpty && v is num) latency[k] = v.toInt();
    });
    return ProbeSnapshot(
      latencyMs: latency,
      updatedMs: (raw['updated_ms'] as num?)?.toInt() ?? 0,
    );
  }

  /// Собирает снимок из ответа ядра `probe()`.
  factory ProbeSnapshot.fromResults(
    List<ProbeResult> results, {
    DateTime? at,
  }) => ProbeSnapshot(
    latencyMs: <String, int>{
      for (final r in results)
        if (r.id.isNotEmpty) r.id: r.latencyMs,
    },
    updatedMs: (at ?? DateTime.now()).millisecondsSinceEpoch,
  );
}

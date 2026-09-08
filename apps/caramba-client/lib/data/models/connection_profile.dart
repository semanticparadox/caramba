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

  /// Что автоподбор выбрал в прошлый раз и почему. `null` — не подбирали.
  ///
  /// Живёт на ПРОФИЛЕ, а не в памяти экрана, по той же причине, что и замер:
  /// строка «Авто» на главном экране обязана называть выбор сразу после
  /// запуска приложения, а не через десять секунд нового прохода. И этот же
  /// снимок — то, что предлагается оставить, когда новый проход не нашёл
  /// ничего рабочего.
  final AutoPickRecord? autoPick;

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
    this.autoPick,
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

  /// Чем кончилась проверка узла в последнем замере.
  ProbeVerdict verdictOf(String serverId) =>
      lastProbe?.verdictOf(serverId) ?? ProbeVerdict.unknown;

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
        autoPick: AutoPickRecord.fromJson(json['auto_pick']),
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
    'auto_pick': autoPick?.toJson(),
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
    AutoPickRecord? autoPick,
    int? serversUpdatedMs,
    String? selectedExitCountry,
    int? selectedExitNodeId,
    Map<String, dynamic>? brandingCache,
    int? lastActiveMs,
    CsmProfileState? csm,
    bool clearSelectedServer = false,
    bool clearAutoPick = false,
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
    // Сброс выбора — решение, а не отсутствие аргумента: тот же приём, что у
    // страны и пина ниже.
    autoPick: clearAutoPick ? null : (autoPick ?? this.autoPick),
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

  /// `id` узла -> RTT установки TCP (-1, если адрес не ответил). Отдельно от
  /// задержки намеренно: это число про АДРЕС, а не про узел, и склеивать их
  /// значило бы вернуть ровно тот фолбэк, из-за которого узел с отвергнутым
  /// ключом показывался самым быстрым.
  final Map<String, int> tcpMs;

  /// `id` узла -> вердикт на проводе (`ok`, `auth_rejected`, ...). Хранится
  /// строкой, а не enum'ом: запись переживает обновление приложения, и
  /// незнакомый вердикт из более новой сборки должен читаться как «не знаю»,
  /// а не ронять разбор профиля.
  final Map<String, String> verdicts;

  /// Когда замер выполнен (мс эпохи).
  final int updatedMs;

  const ProbeSnapshot({
    required this.latencyMs,
    required this.updatedMs,
    this.tcpMs = const <String, int>{},
    this.verdicts = const <String, String>{},
  });

  /// Пустой снимок (ни один узел не измерен).
  static const ProbeSnapshot empty = ProbeSnapshot(
    latencyMs: <String, int>{},
    updatedMs: 0,
  );

  bool get isEmpty => latencyMs.isEmpty;

  DateTime? get updatedAt =>
      updatedMs > 0 ? DateTime.fromMillisecondsSinceEpoch(updatedMs) : null;

  /// Чем кончилась проверка узла. Запись, сделанная до вердиктов, отдаёт
  /// [ProbeVerdict.unknown] — «не знаю», а не «работает».
  ProbeVerdict verdictOf(String id) => ProbeVerdict.fromWire(verdicts[id]);

  /// Узлы, сквозь которые прошёл настоящий запрос.
  int get workingCount => verdicts.values
      .where((v) => ProbeVerdict.fromWire(v) == ProbeVerdict.ok)
      .length;

  Map<String, dynamic> toJson() => {
    'latency_ms': latencyMs,
    'tcp_ms': tcpMs,
    'verdicts': verdicts,
    'updated_ms': updatedMs,
  };

  /// Разбор записи из JSON профиля. `null`/мусор дают `null` (не мерили).
  static ProbeSnapshot? fromJson(Object? raw) {
    if (raw is! Map) return null;
    final rawLatency = raw['latency_ms'];
    if (rawLatency is! Map) return null;
    return ProbeSnapshot(
      latencyMs: _intMap(rawLatency),
      // Записи до вердиктов этих ключей не несут: мягкий разбор, как у
      // format выше, иначе старый профиль перестал бы читаться целиком.
      tcpMs: _intMap(raw['tcp_ms']),
      verdicts: _stringMap(raw['verdicts']),
      updatedMs: (raw['updated_ms'] as num?)?.toInt() ?? 0,
    );
  }

  static Map<String, int> _intMap(Object? raw) {
    if (raw is! Map) return const <String, int>{};
    final out = <String, int>{};
    raw.forEach((k, v) {
      if (k is String && k.isNotEmpty && v is num) out[k] = v.toInt();
    });
    return out;
  }

  static Map<String, String> _stringMap(Object? raw) {
    if (raw is! Map) return const <String, String>{};
    final out = <String, String>{};
    raw.forEach((k, v) {
      if (k is String && k.isNotEmpty && v is String && v.isNotEmpty) {
        out[k] = v;
      }
    });
    return out;
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
    tcpMs: <String, int>{
      for (final r in results)
        if (r.id.isNotEmpty) r.id: r.tcpMs,
    },
    verdicts: <String, String>{
      for (final r in results)
        if (r.id.isNotEmpty && r.verdict != ProbeVerdict.unknown)
          r.id: r.verdict.wire,
    },
    updatedMs: (at ?? DateTime.now()).millisecondsSinceEpoch,
  );
}

/// Что автоподбор выбрал, из чего и почему — в виде, который переживает
/// перезапуск.
///
/// Это не кэш ради скорости. Строка «Авто» обязана называть выбор ВСЕГДА, а не
/// только пока экран открыт: «Авто» без имени — это контрол, который не
/// сообщает, что он сделал, и ровно на это владелец и пожаловался.
class AutoPickRecord {
  /// Имя прокси в теле конфига — то, чем выбор закрепляется на raw-пути и что
  /// ядро называет в `activeProxy`.
  final String proxyName;

  /// Ключ машины в предложении; пусто — машину по имени прокси не разрешили.
  final String exitKey;

  /// ISO-2 страны выбранного узла; пусто — источник её не называет.
  final String countryCode;

  /// Заголовок машины, как он показан на экране серверов.
  final String machineTitle;

  /// Подпись типа подключения (`vless · tcp · reality`); пусто — источник
  /// формы не знает.
  final String protocolLabel;

  /// Задержка сквозь узел, по которой он и выбран.
  final int latencyMs;

  /// Прошёл ли сквозь узел НАСТОЯЩИЙ запрос. false означает «адрес жив, а
  /// протокол проверить было нечем» — выбирать так можно, выдавать за
  /// проверенное нельзя.
  final bool confirmed;

  /// Сколько узлов проверено, сколько из них работает, сколько всего было.
  final int checked;
  final int working;
  final int total;

  /// Когда подбор выполнен (мс эпохи).
  final int updatedMs;

  /// Каким был [ConnectionProfile.serversUpdatedMs] на момент подбора. Состав
  /// узлов сменился — выбор устарел, и это видно без второго замера.
  final int serversUpdatedMs;

  /// Ключ сети, в которой делался замер. Сегодня всегда пустой: источника
  /// такого ключа у приложения нет (плагина связности в зависимостях нет).
  /// Поле заведено, чтобы инвалидация по смене сети включалась одной строкой,
  /// а не переписыванием формата записи.
  final String networkKey;

  /// Почему именно этот узел: `best_score` — выиграл счёт; `kept_previous` —
  /// прошлый выбор удержан гистерезисом (новый лидер выиграл слишком мало,
  /// чтобы дёргать человека переключением).
  final String reasonCode;

  const AutoPickRecord({
    required this.proxyName,
    required this.latencyMs,
    required this.updatedMs,
    this.exitKey = '',
    this.countryCode = '',
    this.machineTitle = '',
    this.protocolLabel = '',
    this.confirmed = false,
    this.checked = 0,
    this.working = 0,
    this.total = 0,
    this.serversUpdatedMs = 0,
    this.networkKey = '',
    this.reasonCode = 'best_score',
  });

  DateTime get updatedAt => DateTime.fromMillisecondsSinceEpoch(updatedMs);

  /// Как назвать выбор в строке «Авто»: страна и машина, а если их нет — имя
  /// прокси. Пустой строки не бывает: «Авто ·» без продолжения хуже, чем
  /// просто «Авто».
  String get shortLabel {
    if (machineTitle.isNotEmpty) return machineTitle;
    if (countryCode.isNotEmpty) return countryCode;
    return proxyName;
  }

  Map<String, dynamic> toJson() => {
    'proxy_name': proxyName,
    'exit_key': exitKey,
    'country_code': countryCode,
    'machine_title': machineTitle,
    'protocol_label': protocolLabel,
    'latency_ms': latencyMs,
    'confirmed': confirmed,
    'checked': checked,
    'working': working,
    'total': total,
    'updated_ms': updatedMs,
    'servers_updated_ms': serversUpdatedMs,
    'network_key': networkKey,
    'reason_code': reasonCode,
  };

  /// Разбор записи. Запись без имени прокси бесполезна (закреплять нечем) и
  /// читается как «подбора не было».
  static AutoPickRecord? fromJson(Object? raw) {
    if (raw is! Map) return null;
    final proxy = raw['proxy_name'];
    if (proxy is! String || proxy.isEmpty) return null;
    String str(Object? v) => v is String ? v : '';
    int num_(Object? v) => v is num ? v.toInt() : 0;
    return AutoPickRecord(
      proxyName: proxy,
      exitKey: str(raw['exit_key']),
      countryCode: str(raw['country_code']),
      machineTitle: str(raw['machine_title']),
      protocolLabel: str(raw['protocol_label']),
      latencyMs: raw['latency_ms'] is num
          ? (raw['latency_ms'] as num).toInt()
          : -1,
      confirmed: raw['confirmed'] == true,
      checked: num_(raw['checked']),
      working: num_(raw['working']),
      total: num_(raw['total']),
      updatedMs: num_(raw['updated_ms']),
      serversUpdatedMs: num_(raw['servers_updated_ms']),
      networkKey: str(raw['network_key']),
      reasonCode: str(raw['reason_code']).isEmpty
          ? 'best_score'
          : str(raw['reason_code']),
    );
  }
}

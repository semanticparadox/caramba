/// Модели ответов ядра caramba-core, которые не относятся к жизненному циклу
/// туннеля: метаданные импорта подписки (`CarambaImportSubscription`) и замер
/// задержек узлов (`CarambaProbe`, ABI v2).
library;

import 'dart:convert';

/// Узел из импортированной подписки (элемент `servers[]` метаданных импорта).
///
/// `id` — имя прокси в mihomo-конфиге; именно его передают в `Up(serverID)`,
/// чтобы прикрепить селектор CARAMBA к конкретному узлу.
class ImportedServer {
  final String id;
  final String name;

  /// Тип outbound'а: `vless`, `hysteria2`, `ss`, `wireguard`, ...
  final String type;

  /// Хост узла.
  final String server;

  final int port;

  /// ISO-2 код страны, если ядро смогло его вывести из имени/эмодзи; иначе ''.
  final String country;

  const ImportedServer({
    required this.id,
    required this.name,
    required this.type,
    required this.server,
    required this.port,
    required this.country,
  });

  factory ImportedServer.fromMap(Map<Object?, Object?> map) => ImportedServer(
    id: _str(map['id']),
    name: _str(map['name']),
    type: _str(map['type']),
    server: _str(map['server']),
    port: (map['port'] as num?)?.toInt() ?? 0,
    country: _str(map['country']),
  );

  Map<String, Object?> toJson() => <String, Object?>{
    'id': id,
    'name': name,
    'type': type,
    'server': server,
    'port': port,
    'country': country,
  };
}

/// Результат разбора сырой подписки БЕЗ поднятия туннеля.
class ImportResult {
  /// Имя профиля, если подписка его сообщила (`name`), иначе null.
  final String? name;

  /// Узлы подписки в порядке, в котором их вернуло ядро.
  final List<ImportedServer> servers;

  const ImportResult({this.name, this.servers = const <ImportedServer>[]});

  /// Пустой результат (подписка распарсилась, узлов нет).
  static const ImportResult empty = ImportResult();

  int get count => servers.length;

  /// Разбор метаданных импорта. Терпим к отсутствию `servers` и к «мусорным»
  /// элементам: не-Map записи молча пропускаются.
  factory ImportResult.fromMap(Map<Object?, Object?> map) {
    final rawName = map['name'];
    final rawServers = map['servers'];
    return ImportResult(
      name: rawName is String && rawName.isNotEmpty ? rawName : null,
      servers: rawServers is List
          ? rawServers
                .whereType<Map<Object?, Object?>>()
                .map(ImportedServer.fromMap)
                .toList(growable: false)
          : const <ImportedServer>[],
    );
  }

  /// Разбор JSON-строки, которую отдаёт ядро/канал.
  factory ImportResult.fromJson(String json) =>
      ImportResult.fromMap(decodeJsonMap(json));
}

/// Замер задержки одного узла (`CarambaProbe`).
class ProbeResult {
  /// Имя прокси (тот же `id`, что у [ImportedServer]).
  final String id;

  final String name;

  /// ISO-2 код страны или ''.
  final String country;

  /// Задержка в миллисекундах; -1 означает таймаут/недоступность.
  final int latencyMs;

  const ProbeResult({
    required this.id,
    this.name = '',
    this.country = '',
    required this.latencyMs,
  });

  /// true, если ядро не смогло достучаться до узла за отведённое время.
  bool get timedOut => latencyMs < 0;

  factory ProbeResult.fromMap(Map<Object?, Object?> map) => ProbeResult(
    id: _str(map['id']),
    name: _str(map['name']),
    country: _str(map['country']),
    latencyMs: (map['latencyMs'] as num?)?.toInt() ?? -1,
  );

  /// Разбор ответа `{"servers":[{...,"latencyMs":42}]}`.
  static List<ProbeResult> listFromMap(Map<Object?, Object?> map) {
    final raw = map['servers'];
    if (raw is! List) return const <ProbeResult>[];
    return raw
        .whereType<Map<Object?, Object?>>()
        .map(ProbeResult.fromMap)
        .toList(growable: false);
  }

  /// Разбор JSON-строки, которую отдаёт ядро/канал.
  static List<ProbeResult> listFromJson(String json) =>
      listFromMap(decodeJsonMap(json));
}

String _str(Object? v) => v is String ? v : '';

/// Декодирует JSON-объект в `Map<Object?, Object?>`; на любой не-объект (или
/// сломанный JSON) отдаёт пустую карту, чтобы парсеры выше не падали.
///
/// Вынесено сюда, чтобы канальный и FFI-путь разбирали ответы ядра одинаково.
Map<Object?, Object?> decodeJsonMap(String source) {
  if (source.isEmpty) return const <Object?, Object?>{};
  try {
    final decoded = jsonDecode(source);
    return decoded is Map<Object?, Object?>
        ? decoded
        : const <Object?, Object?>{};
  } on FormatException {
    return const <Object?, Object?>{};
  }
}

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

/// Чем кончилась проверка узла. Ядро отдаёт это строкой в поле `verdict`.
///
/// Голого числа задержки не хватало ровно на главный вопрос: «работает ли
/// узел». Узел с отвергнутым ключом принимает TCP за сотню миллисекунд и
/// отвергает handshake — и до вердиктов выглядел САМЫМ БЫСТРЫМ в списке,
/// потому что ядро подменяло провал URL-теста временем TCP.
enum ProbeVerdict {
  /// Сквозь узел прошёл настоящий запрос. Единственный вердикт, при котором
  /// задержке можно верить как задержке узла.
  ok('ok'),

  /// TLS не сложился (чужой или самоподписанный сертификат). Сторона
  /// оператора.
  tlsUntrusted('tls_untrusted'),

  /// Адрес отвечает, узел рвёт соединение: ключ подписки не принят.
  authRejected('auth_rejected'),

  /// Адрес не отвечает вовсе; до протокола дело не дошло.
  portClosed('port_closed'),

  /// TCP есть, запрос сквозь узел не уложился в срок. Подпись фильтрации.
  timeout('timeout'),

  /// Ядро не собрало адаптер из полей прокси. Про узел это не говорит ничего.
  unsupported('unsupported'),

  /// Сборка без ядра: проверен только адрес, не протокол.
  tcpOnly('tcp_only'),

  /// До узла проход не дошёл (отмена, потолок волны). Не «мёртв».
  skipped('skipped'),

  /// Ядро вердикта не прислало: сборка старше этого поля. «Не знаю» — тоже
  /// ответ, и выдавать его за `ok` нельзя.
  unknown('');

  final String wire;
  const ProbeVerdict(this.wire);

  static ProbeVerdict fromWire(String? raw) {
    for (final v in ProbeVerdict.values) {
      if (v != ProbeVerdict.unknown && v.wire == raw) return v;
    }
    return ProbeVerdict.unknown;
  }

  /// Через узел точно проходит трафик. Только `ok`.
  bool get isConfirmedWorking => this == ProbeVerdict.ok;

  /// Узел можно попробовать, но подтверждения нет: адрес жив, а протокол
  /// проверить было нечем. Выбирать из таких можно — молча выдавать их за
  /// проверенные нельзя.
  bool get isUnconfirmed =>
      this == ProbeVerdict.tcpOnly || this == ProbeVerdict.unknown;

  /// Узел точно не годится.
  bool get isFailure =>
      this == ProbeVerdict.tlsUntrusted ||
      this == ProbeVerdict.authRejected ||
      this == ProbeVerdict.portClosed ||
      this == ProbeVerdict.timeout ||
      this == ProbeVerdict.unsupported;
}

/// Замер задержки одного узла (`CarambaProbe`).
class ProbeResult {
  /// Имя прокси (тот же `id`, что у [ImportedServer]).
  final String id;

  final String name;

  /// ISO-2 код страны или ''.
  final String country;

  /// Задержка запроса СКВОЗЬ узел в миллисекундах; -1 означает «настоящего
  /// запроса не прошло». Почему именно — в [verdict].
  final int latencyMs;

  /// RTT установки TCP-соединения с адресом; -1 — адрес не ответил.
  /// Справочное число: оно про адрес, а не про узел.
  final int tcpMs;

  final ProbeVerdict verdict;

  /// Сырой текст ошибки ядра для «Подробностей»; пусто при успехе.
  final String detail;

  const ProbeResult({
    required this.id,
    this.name = '',
    this.country = '',
    required this.latencyMs,
    this.tcpMs = -1,
    this.verdict = ProbeVerdict.unknown,
    this.detail = '',
  });

  /// true, если задержки сквозь узел нет.
  bool get timedOut => latencyMs < 0;

  /// Узел годится для подключения: подтверждён либо не опровергнут.
  bool get usable => latencyMs >= 0 && !verdict.isFailure;

  factory ProbeResult.fromMap(Map<Object?, Object?> map) => ProbeResult(
    id: _str(map['id']),
    name: _str(map['name']),
    country: _str(map['country']),
    latencyMs: (map['latencyMs'] as num?)?.toInt() ?? -1,
    tcpMs: (map['tcpMs'] as num?)?.toInt() ?? -1,
    verdict: ProbeVerdict.fromWire(_str(map['verdict'])),
    detail: _str(map['detail']),
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

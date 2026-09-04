/// Exit-сервер из `GET /api/v2/app/servers`.
///
/// Соответствует Rust-структуре `AppServer` в
/// `apps/caramba-panel/src/api/v2/app.rs`:
/// ```json
/// { "id": 7, "name": "Node #7 (...)", "country_code": "NL", "flag": "🇳🇱",
///   "latency_ms": 42, "load_pct": 18.5, "status": "online" }
/// ```
///
/// РЕШЕНИЕ ПО ФЛАГАМ ПЕРЕСМОТРЕНО (владелец продукта, сентябрь 2026:
/// «должны быть флаги стран где сервер»). Здесь стояло обратное: панель
/// присылает `flag`, а модель его не читала — страна показывалась только
/// mono-кодом (ANTI-SLOP: коды стран, не флаги).
///
/// Что из прежнего довода осталось в силе и потому не отменено:
///   * код страны остаётся В МОДЕЛИ единственным идентификатором. По нему
///     группируется список, ищется имя страны и разрешается выбор; флаг — это
///     РИСУНОК кода, а не второй источник истины, и уехать они не могут;
///   * неизвестная страна не получает флага. Панель подставляет `US` узлу без
///     `country_code` (`country_flag(...unwrap_or("US"))`) и потому уверенно
///     присылает 🇺🇸 там, где страны нет вовсе; такой флаг — это как раз
///     «уверенно неправильно», против чего был прежний довод. Поэтому
///     [flag] здесь только ПРОНОСИТСЯ, а годен он к показу или нет, решает
///     `flagOf` в `exit_location.dart` — и только вместе с непустым кодом.
class Server {
  /// `nodes.id`.
  final int id;

  /// Отображаемое имя сервера.
  final String name;

  /// ISO-2 код страны (`NL`, `DE`, ...), может быть `null`.
  final String? countryCode;

  /// Флаг-эмодзи, как его прислала панель. Пусто — ключа в ответе не было.
  ///
  /// Сырое значение: панель отдаёт `🌐` для кода, которого не знает, и 🇺🇸
  /// для узла БЕЗ страны. Ни то, ни другое здесь не чинится — вопрос «что
  /// показать» решает `flagOf`, у которого есть и код, и это поле.
  final String flag;

  /// Пинг в миллисекундах; `null` => таймаут/неизвестно.
  final int? pingMs;

  /// Загрузка узла в процентах (среднее cpu/ram), 0..100.
  final double load;

  /// Статус: `online` / `busy` / `full` / ...
  final String status;

  /// Строка ответа `/servers` целиком, как её прислала панель.
  ///
  /// [Server] — модель СТРОКИ СПИСКА: имя, страна, пинг, загрузка. Панель на
  /// том же эндпоинте отдаёт ещё и `inbounds[]` с `via_relay` — данные не про
  /// строку списка, а про то, что на этом узле можно выбрать. Разбирать их
  /// здесь значило бы вписать в модель пикера серверов знание о протоколах,
  /// которое ей ни для чего не нужно; терять их — оставить слой предложения без
  /// единственного источника, который вообще различает `vless/tcp/reality` и
  /// `vless/tcp/tls`. Поэтому строка проносится как есть, а форму читает тот,
  /// кому она принадлежит (`domain/offering/panel_fleet.dart`).
  ///
  /// `null` — объект собран не из JSON (тесты, автоподбор).
  final Map<String, dynamic>? rawJson;

  const Server({
    required this.id,
    required this.name,
    this.countryCode,
    this.flag = '',
    this.pingMs,
    this.load = 0,
    this.status = 'online',
    this.rawJson,
  });

  bool get isSelectable => status != 'full';

  /// Бакет качества пинга для цвета строки (DESIGN.md §5.2).
  PingBucket get pingBucket => pingBucketOf(pingMs);

  /// Бакет загрузки: low `<50`, med `50–80`, high `>80`.
  LoadBucket get loadBucket {
    if (load < 50) return LoadBucket.low;
    if (load <= 80) return LoadBucket.med;
    return LoadBucket.high;
  }

  factory Server.fromJson(Map<String, dynamic> json) => Server(
    id: (json['id'] as num).toInt(),
    name: (json['name'] as String?) ?? 'Server',
    countryCode: json['country_code'] as String?,
    flag: (json['flag'] as String?) ?? '',
    pingMs: (json['latency_ms'] as num?)?.toInt(),
    load: (json['load_pct'] as num?)?.toDouble() ?? 0,
    status: (json['status'] as String?) ?? 'online',
    rawJson: json,
  );

  Map<String, dynamic> toJson() => {
    'id': id,
    'name': name,
    'country_code': countryCode,
    'flag': flag,
    'latency_ms': pingMs,
    'load_pct': load,
    'status': status,
  };

  Server copyWith({int? pingMs, double? load, String? status}) => Server(
    id: id,
    name: name,
    countryCode: countryCode,
    flag: flag,
    pingMs: pingMs ?? this.pingMs,
    load: load ?? this.load,
    status: status ?? this.status,
    // Обновление пинга не должно стирать инбаунды: пробник трогает одно поле, а
    // потеря сырой строки обнулила бы весь пикер протокола до «неизвестно».
    rawJson: rawJson,
  );
}

/// Бакет качества пинга (DESIGN.md §5.2): good `<60`, fair `60..150`,
/// poor `>150`, timeout (`null` или отрицательное значение).
///
/// Вынесено из [Server]: те же пороги применяются к узлам импортированной
/// подписки (у них своя модель и `-1` вместо `null` на таймауте).
PingBucket pingBucketOf(int? ms) {
  if (ms == null || ms < 0) return PingBucket.timeout;
  if (ms < 60) return PingBucket.good;
  if (ms <= 150) return PingBucket.fair;
  return PingBucket.poor;
}

enum PingBucket { good, fair, poor, timeout }

enum LoadBucket { low, med, high }

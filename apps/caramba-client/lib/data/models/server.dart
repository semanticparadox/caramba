/// Exit-сервер из `GET /api/v2/app/servers`.
///
/// Соответствует Rust-структуре `AppServer` в
/// `apps/caramba-panel/src/api/v2/app.rs`:
/// ```json
/// { "id": 7, "name": "Node #7 (...)", "country_code": "NL",
///   "latency_ms": 42, "load_pct": 18.5, "status": "online" }
/// ```
///
/// Страна отображается через mono-код (CodeChip(countryCode)) — флаг-эмодзи
/// панели намеренно не маппится в модель (ANTI-SLOP: коды стран, не флаги).
class Server {
  /// `nodes.id`.
  final int id;

  /// Отображаемое имя сервера.
  final String name;

  /// ISO-2 код страны (`NL`, `DE`, ...), может быть `null`.
  final String? countryCode;

  /// Пинг в миллисекундах; `null` => таймаут/неизвестно.
  final int? pingMs;

  /// Загрузка узла в процентах (среднее cpu/ram), 0..100.
  final double load;

  /// Статус: `online` / `busy` / `full` / ...
  final String status;

  const Server({
    required this.id,
    required this.name,
    this.countryCode,
    this.pingMs,
    this.load = 0,
    this.status = 'online',
  });

  bool get isSelectable => status != 'full';

  /// Бакет качества пинга для цвета строки (DESIGN.md §5.2):
  /// good `<60`, fair `60–150`, poor `>150`, timeout (`null`).
  PingBucket get pingBucket {
    final p = pingMs;
    if (p == null) return PingBucket.timeout;
    if (p < 60) return PingBucket.good;
    if (p <= 150) return PingBucket.fair;
    return PingBucket.poor;
  }

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
    pingMs: (json['latency_ms'] as num?)?.toInt(),
    load: (json['load_pct'] as num?)?.toDouble() ?? 0,
    status: (json['status'] as String?) ?? 'online',
  );

  Map<String, dynamic> toJson() => {
    'id': id,
    'name': name,
    'country_code': countryCode,
    'latency_ms': pingMs,
    'load_pct': load,
    'status': status,
  };

  Server copyWith({int? pingMs, double? load, String? status}) => Server(
    id: id,
    name: name,
    countryCode: countryCode,
    pingMs: pingMs ?? this.pingMs,
    load: load ?? this.load,
    status: status ?? this.status,
  );
}

enum PingBucket { good, fair, poor, timeout }

enum LoadBucket { low, med, high }

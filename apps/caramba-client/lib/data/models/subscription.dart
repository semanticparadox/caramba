/// Подписка пользователя из `GET /api/v2/app/subscription`.
///
/// Соответствует JSON-ответу `app::get_subscription` в
/// `apps/caramba-panel/src/api/v2/app.rs`. Ключевое поле для Go-ядра (mihomo) —
/// [clashUrl]: ядро тянет именно этот mihomo/clash-конфиг (amnezia-wg уже внутри).
class Subscription {
  /// `subscriptions.id`.
  final int id;

  /// UUID подписки — основа для всех config-URL (caramba-sub отдаёт по нему).
  final String subscriptionUuid;

  /// Имя плана.
  final String? planName;

  /// Статус (`active`, `expired`, ...).
  final String status;

  /// Использованный трафик в байтах.
  final int usedTrafficBytes;

  /// Использованный трафик в ГБ (строкой `"42.10"` от панели).
  final String usedTrafficGb;

  /// Лимит трафика в ГБ (`null`/0 для безлимита — зависит от плана).
  final double? trafficLimitGb;

  /// Дата истечения (RFC3339).
  final DateTime? expiresAt;

  /// Дней до истечения (уже посчитано панелью, >= 0).
  final int daysLeft;

  /// URL mihomo/clash-конфига — **это тянет Go-ядро**.
  final String clashUrl;

  /// Алиас config_url (тот же clash-путь).
  final String configUrl;

  /// URL sing-box конфига (на случай иных клиентов).
  final String singboxUrl;

  /// URL v2ray-подписки.
  final String v2rayUrl;

  /// Базовый URL подписки (без `?client=`).
  final String subscriptionUrl;

  /// Право подключаться с объяснением. Никогда не `null`: если панель не
  /// прислала объект `access`, состояние выводится из её же старых полей
  /// ([AccessState.fromLegacy]) — приложение обязано понимать обе панели.
  final AccessState access;

  const Subscription({
    required this.id,
    required this.subscriptionUuid,
    this.planName,
    required this.status,
    this.usedTrafficBytes = 0,
    this.usedTrafficGb = '0.00',
    this.trafficLimitGb,
    this.expiresAt,
    this.daysLeft = 0,
    required this.clashUrl,
    required this.configUrl,
    this.singboxUrl = '',
    this.v2rayUrl = '',
    this.subscriptionUrl = '',
    this.access = AccessState.allowed,
  });

  /// Подключаться можно. Раньше это было `status == 'active'` — и именно из-за
  /// такой проверки `throttled` выглядел как полная поломка аккаунта, хотя
  /// серверы, устройства и оплата у такого пользователя работают.
  bool get isActive => access.mayConnect;

  /// Использовано ГБ как число (для прогресс-бара).
  double get usedGb => double.tryParse(usedTrafficGb) ?? 0;

  /// Доля использованного трафика в диапазоне 0..1 (для квота-бара).
  /// Возвращает `null`, если лимит безлимитный/неизвестен.
  double? get usageFraction {
    final limit = trafficLimitGb;
    if (limit == null || limit <= 0) return null;
    return (usedGb / limit).clamp(0.0, 1.0);
  }

  factory Subscription.fromJson(Map<String, dynamic> json) {
    final status = (json['status'] as String?) ?? 'unknown';
    return Subscription(
      id: (json['id'] as num).toInt(),
      subscriptionUuid: json['subscription_uuid'] as String,
      planName: json['plan_name'] as String?,
      status: status,
      usedTrafficBytes: (json['used_traffic_bytes'] as num?)?.toInt() ?? 0,
      usedTrafficGb: json['used_traffic_gb']?.toString() ?? '0.00',
      trafficLimitGb: (json['traffic_limit_gb'] as num?)?.toDouble(),
      expiresAt: _parseDate(json['expires_at']),
      daysLeft: (json['days_left'] as num?)?.toInt() ?? 0,
      clashUrl: (json['clash_url'] as String?) ?? '',
      configUrl:
          (json['config_url'] as String?) ??
          (json['clash_url'] as String?) ??
          '',
      singboxUrl: (json['singbox_url'] as String?) ?? '',
      v2rayUrl: (json['v2ray_url'] as String?) ?? '',
      subscriptionUrl: (json['subscription_url'] as String?) ?? '',
      // Новая панель — объект `access`; старая — свои же числа, из которых то же
      // состояние выводится без единой догадки.
      access:
          AccessState.fromJson(json['access']) ??
          AccessState.fromLegacy(
            status: status,
            usedBytes: (json['used_traffic_bytes'] as num?)?.toInt() ?? 0,
            limitBytes: (json['traffic_limit_bytes'] as num?)?.toInt() ?? 0,
            period: (json['quota_period'] as String?) ?? '',
            isFree: json['is_free'] == true,
            dailyTrafficMb: (json['daily_traffic_mb'] as num?)?.toInt() ?? 0,
            expiresAt: _parseDate(json['expires_at']),
          ),
    );
  }

  Map<String, dynamic> toJson() => {
    'id': id,
    'subscription_uuid': subscriptionUuid,
    'plan_name': planName,
    'status': status,
    'used_traffic_bytes': usedTrafficBytes,
    'used_traffic_gb': usedTrafficGb,
    'traffic_limit_gb': trafficLimitGb,
    'expires_at': expiresAt?.toIso8601String(),
    'days_left': daysLeft,
    'clash_url': clashUrl,
    'config_url': configUrl,
    'singbox_url': singboxUrl,
    'v2ray_url': v2rayUrl,
    'subscription_url': subscriptionUrl,
  };

  static DateTime? _parseDate(Object? v) {
    if (v is String && v.isNotEmpty) return DateTime.tryParse(v);
    return null;
  }
}

// ---------------------------------------------------------------------------
// Доступ подписки: право подключаться и человеческий ответ «почему нельзя».
// ---------------------------------------------------------------------------

// Право подключаться — и человеческий ответ на вопрос «почему нельзя».
//
// До этого файла приложение знало о подписке одно слово: `status == 'active'`.
// Всё остальное — что трафик кончился на сегодня, что лимит вернётся в
// полночь UTC, что занято единственное устройство — панель уже считала и
// присылала, а клиент выбрасывал при разборе JSON. Пользователь взамен видел
// голый 403, завёрнутый в четыре слоя Go-ошибок.
//
// Здесь эти данные становятся типом, у которого есть ровно один булев ответ
// ([mayConnect]) и объяснение с числами и сроком.
//
// Источников два, и второй важнее первого:
//   * НОВАЯ панель присылает объект `access` (см. контракт в
//     `apps/caramba-panel/src/services/access_state.rs`), где `st`/`rc` — те же
//     коды, что у CSM (`libs/caramba-shared/src/csm/directive.rs`);
//   * СТАРАЯ панель (а на проде она будет старее приложения ещё долго) не
//     присылает ничего, кроме `status`, `traffic_limit_bytes`, `quota_period`,
//     `is_free` и `daily_traffic_mb` — и этого достаточно, чтобы вывести то же
//     состояние самим ([AccessState.fromLegacy]). Приложение обязано работать
//     на обеих, поэтому вывод из legacy-полей — не запасной путь, а основной
//     на сегодня.
/// Что именно закрыло доступ. Это НЕ строка панели: `throttled`, `expired` и
/// `403` — внутренние слова, которые пользователю никогда не показываются.
enum AccessKind {
  /// Подключаться можно.
  ok,

  /// Дневная норма бесплатного тарифа израсходована; вернётся сама.
  dailyQuota,

  /// Трафик тарифа кончился совсем; сам не вернётся.
  planQuota,

  /// Срок подписки истёк.
  expired,

  /// Занято максимум устройств.
  deviceLimit,

  /// Подписка ждёт подтверждения оператором.
  awaitingApproval,

  /// Доступ приостановлен/закрыт.
  suspended,

  /// У оператора сейчас нет узлов.
  fleetUnavailable,

  /// Панель отказала, но причину не назвала (старая панель, голый 403).
  unknown,
}

/// Куда вести за оплатой. Адреса публикует оператор; приложение не принадлежит
/// ни одному из них и своего бота не знает.
class AccessPay {
  /// https-ссылка на мини-приложение/бота оператора. Пусто — адреса нет.
  final String url;

  /// Запасная https-ссылка на чат бота.
  final String botUrl;

  /// Нативная ссылка на мини-апп (`tg://resolve?domain=…&appname=…&startapp=plans`).
  ///
  /// Панель собирает её отдельно от https-формы именно для приложения: она
  /// открывает УЖЕ УСТАНОВЛЕННЫЙ Telegram, не прогоняя человека через браузер и
  /// экран «открыть в приложении». Схема у неё не https, поэтому пропускать её
  /// через общий [openExternal] нельзя (его allowlist — https и mailto, и
  /// расширять этот список ради одной кнопки значило бы ослабить проверку для
  /// всех ссылок приложения). Пусто — у оператора не настроен мини-апп, и тогда
  /// открывается [url]/[botUrl].
  final String nativeUrl;

  const AccessPay({this.url = '', this.botUrl = '', this.nativeUrl = ''});

  /// Лучшая доступная ссылка или `null` — оператор не опубликовал ни одной.
  /// `null` здесь обязателен: выдумать чужой @bot вместо пустоты нельзя.
  String? get link {
    final u = url.trim();
    if (u.isNotEmpty) return u;
    final b = botUrl.trim();
    return b.isEmpty ? null : b;
  }

  /// Нативная ссылка или `null`. Отдельно от [link], потому что вызывающий
  /// обязан их различать: первую открывает Telegram, вторую — браузер.
  String? get native {
    final n = nativeUrl.trim();
    return n.isEmpty ? null : n;
  }

  /// Оператор не опубликовал ни одного адреса оплаты.
  bool get isEmpty => link == null && native == null;

  factory AccessPay.fromJson(Map<String, dynamic> json) => AccessPay(
    url:
        (json['miniapp_url'] as String?) ??
        (json['pay_url'] as String?) ??
        (json['bot_url'] as String?) ??
        '',
    botUrl: (json['bot_url'] as String?) ?? '',
    nativeUrl: (json['miniapp_native'] as String?) ?? '',
  );
}

/// Тариф, который снимает текущее ограничение (панель считает сама).
class AccessUpgrade {
  final String planName;
  final int? durationDays;

  /// Цена в минорных единицах (как в биллинге), 0 — панель не назвала.
  final int priceMinor;
  final String currency;

  const AccessUpgrade({
    required this.planName,
    this.durationDays,
    this.priceMinor = 0,
    this.currency = '',
  });

  factory AccessUpgrade.fromJson(Map<String, dynamic> json) => AccessUpgrade(
    planName: (json['plan_name'] as String?) ?? '',
    durationDays: (json['duration_days'] as num?)?.toInt(),
    priceMinor: (json['price_minor'] as num?)?.toInt() ?? 0,
    currency: (json['currency'] as String?) ?? '',
  );
}

/// Состояние доступа подписки.
class AccessState {
  /// ЕДИНСТВЕННЫЙ флаг, по которому UI решает, имеет ли смысл подключение.
  /// Сравнение `status == 'active'` из старых моделей его не заменяет: у
  /// `throttled` статус не активный, а серверы, оплата и список устройств
  /// работают.
  final bool mayConnect;

  final AccessKind kind;

  /// Коды CSM (`Status` / `ReasonCode`). 0 — панель их не прислала.
  final int st;
  final int rc;

  /// Израсходовано и лимит, байты. Лимит 0 — безлимит/неизвестен.
  final int usedBytes;
  final int limitBytes;

  /// `day` — суточная норма, `total` — лимит на весь срок, пусто — неизвестно.
  final String period;

  /// Когда лимит обновится (для `day` — ближайшая полночь UTC). `null` — сам
  /// не обновится, нужна оплата.
  final DateTime? resetsAt;

  /// Честная задержка пополнения: панель добавляет норму не ровно в полночь, а
  /// на ближайшем тике (до получаса). Обещать «ровно в 00:00» значит соврать.
  final int resetLagSeconds;

  /// Сколько останется после пополнения. 0 — завтра всё равно будет закрыто
  /// (израсходовано больше двух суточных норм). `null` — не считали.
  final int? bytesAfterReset;

  final int devicesUsed;
  final int devicesLimit;

  final AccessUpgrade? upgrade;
  final AccessPay pay;

  /// Готовый текст панели. Показывается только если своего построить не из
  /// чего: свой собран из чисел и потому всегда точнее.
  final String messageRu;

  const AccessState({
    required this.mayConnect,
    required this.kind,
    this.st = 0,
    this.rc = 0,
    this.usedBytes = 0,
    this.limitBytes = 0,
    this.period = '',
    this.resetsAt,
    this.resetLagSeconds = 0,
    this.bytesAfterReset,
    this.devicesUsed = 0,
    this.devicesLimit = 0,
    this.upgrade,
    this.pay = const AccessPay(),
    this.messageRu = '',
  });

  /// Доступ есть — карточку показывать нечего.
  static const AccessState allowed = AccessState(
    mayConnect: true,
    kind: AccessKind.ok,
  );

  bool get isBlocked => !mayConnect;

  /// Ограничение снимется само (дневная норма), без денег.
  bool get selfHealing => kind == AccessKind.dailyQuota && resetsAt != null;

  /// Заголовок карточки. Ни одного внутреннего слова.
  String get title => switch (kind) {
    AccessKind.ok => 'Подписка активна',
    AccessKind.dailyQuota => 'Лимит на сегодня закончился',
    AccessKind.planQuota => 'Трафик тарифа закончился',
    AccessKind.expired => 'Подписка закончилась',
    AccessKind.deviceLimit => 'Занято максимум устройств',
    AccessKind.awaitingApproval => 'Подписка ждёт подтверждения',
    AccessKind.suspended => 'Доступ приостановлен',
    AccessKind.fleetUnavailable => 'Серверы временно недоступны',
    AccessKind.unknown => 'Оператор не выдаёт конфигурацию',
  };

  /// Объяснение с числами и сроком. Числа — те, что прислала панель; если их
  /// нет, предложение остаётся честным и без них.
  String get body {
    switch (kind) {
      case AccessKind.ok:
        return 'Подключение доступно.';
      case AccessKind.dailyQuota:
        final b = StringBuffer();
        if (limitBytes > 0) {
          b.write(
            'Бесплатный тариф даёт ${formatBytesRu(limitBytes)} в день. '
            'Сегодня израсходовано ${formatBytesRu(usedBytes)}. ',
          );
        } else {
          b.write('Дневная норма бесплатного тарифа израсходована. ');
        }
        final reset = resetLabel;
        if (reset != null) {
          b.write('Норма вернётся $reset');
          final left = bytesAfterReset;
          if (left != null && left > 0) {
            b.write(', останется ${formatBytesRu(left)}.');
          } else if (left != null) {
            b.write(
              '. Израсходовано больше двух дневных норм, поэтому одного '
              'пополнения не хватит: чтобы подключиться сегодня, нужен платный '
              'тариф.',
            );
          } else {
            b.write('.');
          }
        }
        b.write(' Платный тариф снимает дневное ограничение сразу.');
        return b.toString();
      case AccessKind.planQuota:
        return limitBytes > 0
            ? 'Трафик тарифа израсходован: ${formatBytesRu(usedBytes)} из '
                  '${formatBytesRu(limitBytes)}. Чтобы продолжить, продлите '
                  'или смените тариф.'
            : 'Трафик тарифа израсходован. Чтобы продолжить, продлите или '
                  'смените тариф.';
      case AccessKind.expired:
        return 'Срок подписки истёк. Продлите её, чтобы снова подключаться.';
      case AccessKind.deviceLimit:
        final limit = devicesLimit > 0 ? devicesLimit : 1;
        return 'Тариф разрешает $limit '
            '${_deviceWord(limit)}, и место уже занято. Отключите VPN на '
            'другом устройстве или удалите его в разделе «Устройства», либо '
            'выберите тариф, где устройств больше.';
      case AccessKind.awaitingApproval:
        return 'Оператор ещё не подтвердил подписку. Обычно это занимает '
            'несколько минут.';
      case AccessKind.suspended:
        return 'Оператор закрыл доступ по этой подписке. Напишите в поддержку, '
            'чтобы узнать причину.';
      case AccessKind.fleetUnavailable:
        return 'У оператора сейчас нет ни одного узла, готового принять '
            'подключение. Попробуйте через минуту.';
      case AccessKind.unknown:
        // Свой текст строится из чисел, а чисел здесь нет. Если панель прислала
        // собственную формулировку, она знает больше — показываем её.
        final own = messageRu.trim();
        if (own.isNotEmpty) return own;
        return 'Панель отказала в выдаче конфигурации и не назвала причину. '
            'Чаще всего это израсходованный трафик, истёкшая подписка или '
            'занятое устройство: посмотрите подписку в боте оператора.';
    }
  }

  /// Короткая причина для строки списка серверов (одно предложение).
  String get shortReason => switch (kind) {
    AccessKind.ok => 'Доступно',
    AccessKind.dailyQuota => 'Дневной лимит израсходован',
    AccessKind.planQuota => 'Трафик тарифа израсходован',
    AccessKind.expired => 'Подписка закончилась',
    AccessKind.deviceLimit => 'Занято максимум устройств',
    AccessKind.awaitingApproval => 'Подписка ещё не подтверждена',
    AccessKind.suspended => 'Доступ приостановлен',
    AccessKind.fleetUnavailable => 'Узлы временно недоступны',
    AccessKind.unknown => 'Оператор не выдаёт конфигурацию',
  };

  /// Метка на строке недоступного узла: чего именно ждать.
  String get badge => switch (kind) {
    AccessKind.ok => '',
    AccessKind.dailyQuota => selfHealing ? 'после пополнения' : 'после оплаты',
    AccessKind.fleetUnavailable => 'временно',
    AccessKind.awaitingApproval => 'после подтверждения',
    AccessKind.deviceLimit => 'занято',
    _ => 'после оплаты',
  };

  /// «в 00:00 UTC (03:00 у вас), в течение получаса» — когда есть срок.
  ///
  /// Локальное время считается от той же точки, а не подставляется по памяти:
  /// у владельца США, у большинства пользователей — не Москва, и «03:00 МСК»
  /// в тексте было бы враньём для обоих.
  String? get resetLabel {
    final at = resetsAt;
    if (at == null) return null;
    final utc = at.toUtc();
    final local = at.toLocal();
    final b = StringBuffer('в ${_hhmm(utc)} UTC');
    if (local.hour != utc.hour || local.minute != utc.minute) {
      b.write(' (${_hhmm(local)} по времени устройства)');
    }
    if (resetLagSeconds > 0) {
      final minutes = (resetLagSeconds / 60).round();
      b.write(', в течение ${minutes >= 60 ? 'часа' : '$minutes мин'}');
    }
    return b.toString();
  }

  /// Разбор объекта `access` новой панели. `null` — панель его не прислала.
  static AccessState? fromJson(Object? raw) {
    if (raw is! Map) return null;
    final json = raw.cast<String, dynamic>();
    final rc = (json['rc'] as num?)?.toInt() ?? 0;
    final st = (json['st'] as num?)?.toInt() ?? 0;
    final mayConnect = json['may_connect'] == true;
    final devices = json['devices'];
    final devMap = devices is Map
        ? devices.cast<String, dynamic>()
        : const <String, dynamic>{};
    return AccessState(
      mayConnect: mayConnect,
      kind: mayConnect
          ? AccessKind.ok
          : _kindOf(
              rc: rc,
              st: st,
              state: (json['state'] as String?) ?? '',
              reason: (json['reason'] as String?) ?? '',
            ),
      st: st,
      rc: rc,
      usedBytes: (json['used_bytes'] as num?)?.toInt() ?? 0,
      limitBytes: (json['limit_bytes'] as num?)?.toInt() ?? 0,
      period: (json['period'] as String?) ?? '',
      resetsAt: _date(json['resets_at']),
      resetLagSeconds: (json['reset_lag_seconds'] as num?)?.toInt() ?? 0,
      bytesAfterReset: (json['bytes_after_reset'] as num?)?.toInt(),
      devicesUsed: (devMap['used'] as num?)?.toInt() ?? 0,
      devicesLimit: (devMap['limit'] as num?)?.toInt() ?? 0,
      upgrade: json['upgrade'] is Map
          ? AccessUpgrade.fromJson(
              (json['upgrade'] as Map).cast<String, dynamic>(),
            )
          : null,
      pay: json['pay'] is Map
          ? AccessPay.fromJson((json['pay'] as Map).cast<String, dynamic>())
          : const AccessPay(),
      messageRu: (json['message_ru'] as String?) ?? '',
    );
  }

  /// Вывод состояния из полей, которые СТАРАЯ панель уже присылает.
  ///
  /// Это не догадка: `status`, `traffic_limit_bytes`, `quota_period`,
  /// `is_free` и `daily_traffic_mb` панель считает сама и отдаёт обоими
  /// эндпоинтами подписки — просто клиент их до сих пор не читал. Сроки
  /// пополнения (полночь UTC и получасовой тик) берутся из того же расписания,
  /// по которому панель начисляет норму.
  factory AccessState.fromLegacy({
    required String status,
    int usedBytes = 0,
    int limitBytes = 0,
    String period = '',
    bool isFree = false,
    int dailyTrafficMb = 0,
    DateTime? expiresAt,
    int devicesUsed = 0,
    int devicesLimit = 0,
    DateTime? now,
  }) {
    final s = status.trim().toLowerCase();
    if (s == 'active') {
      return AccessState(
        mayConnect: true,
        kind: AccessKind.ok,
        usedBytes: usedBytes,
        limitBytes: limitBytes,
        period: period,
        devicesUsed: devicesUsed,
        devicesLimit: devicesLimit,
      );
    }
    final daily = period == 'day' || (isFree && dailyTrafficMb > 0);
    final kind = switch (s) {
      'throttled' => daily ? AccessKind.dailyQuota : AccessKind.planQuota,
      'expired' => _expiredKind(
        expiresAt: expiresAt,
        usedBytes: usedBytes,
        limitBytes: limitBytes,
        now: now,
      ),
      'pending' => AccessKind.awaitingApproval,
      'banned' || 'suspended' || 'disabled' => AccessKind.suspended,
      _ => AccessKind.unknown,
    };
    final dailyBytes = dailyTrafficMb * 1024 * 1024;
    return AccessState(
      mayConnect: false,
      kind: kind,
      usedBytes: usedBytes,
      limitBytes: limitBytes,
      period: period,
      resetsAt: kind == AccessKind.dailyQuota ? nextUtcMidnight(now) : null,
      // Норму добавляет получасовой тик панели, а не сама полночь.
      resetLagSeconds: kind == AccessKind.dailyQuota ? 1800 : 0,
      bytesAfterReset: (kind == AccessKind.dailyQuota && limitBytes > 0)
          ? _afterReset(usedBytes, limitBytes, dailyBytes)
          : null,
      devicesUsed: devicesUsed,
      devicesLimit: devicesLimit,
    );
  }

  /// Ближайшая полночь UTC после [now].
  static DateTime nextUtcMidnight([DateTime? now]) {
    final u = (now ?? DateTime.now()).toUtc();
    return DateTime.utc(u.year, u.month, u.day).add(const Duration(days: 1));
  }

  /// Столько останется после пополнения: панель ВЫЧИТАЕТ дневную норму из
  /// израсходованного, а не обнуляет его. Разница видна ровно тем, кто сжёг
  /// больше нормы: у них завтра будет не полный лимит.
  static int _afterReset(int used, int limit, int daily) {
    final left = used - daily;
    final remainder = left < 0 ? 0 : left;
    final free = limit - remainder;
    return free < 0 ? 0 : free;
  }

  static AccessKind _expiredKind({
    DateTime? expiresAt,
    int usedBytes = 0,
    int limitBytes = 0,
    DateTime? now,
  }) {
    final at = expiresAt;
    if (at != null && at.isBefore(now ?? DateTime.now())) {
      return AccessKind.expired;
    }
    // Панель ставит `expired` и при исчерпанном трафике платного тарифа —
    // срок тут ни при чём, и говорить про срок было бы неправдой.
    if (limitBytes > 0 && usedBytes >= limitBytes) return AccessKind.planQuota;
    return AccessKind.expired;
  }

  static AccessKind _kindOf({
    required int rc,
    required int st,
    required String state,
    required String reason,
  }) {
    switch (rc) {
      case 3003:
        return AccessKind.dailyQuota;
      case 3001:
      case 3002:
        return AccessKind.planQuota;
      case 2001:
      case 2002:
      case 2003:
        return AccessKind.expired;
      case 4001:
      case 4002:
      case 4003:
        return AccessKind.deviceLimit;
      case 1001:
        return AccessKind.awaitingApproval;
      case 1002:
      case 1003:
        return AccessKind.suspended;
      case 5002:
        return AccessKind.fleetUnavailable;
    }
    // Незнакомый код своей полосы читается как вся полоса — правило то же, что
    // у CSM (02-SPEC 4.6.2): новый код панели не должен превращаться в «ошибку».
    if (rc >= 3000 && rc < 4000) return AccessKind.planQuota;
    if (rc >= 4000 && rc < 5000) return AccessKind.deviceLimit;
    if (rc >= 2000 && rc < 3000) return AccessKind.expired;
    return switch (state) {
      'quota_exceeded' => AccessKind.planQuota,
      'expired' => AccessKind.expired,
      'device_limit' => AccessKind.deviceLimit,
      'pending_approval' || 'onboarding' => AccessKind.awaitingApproval,
      'suspended' || 'revoked' => AccessKind.suspended,
      _ => switch (st) {
        4 => AccessKind.expired,
        5 || 6 => AccessKind.suspended,
        7 => AccessKind.planQuota,
        8 => AccessKind.deviceLimit,
        1 || 2 => AccessKind.awaitingApproval,
        _ => AccessKind.unknown,
      },
    };
  }

  static DateTime? _date(Object? v) {
    if (v is String && v.isNotEmpty) return DateTime.tryParse(v);
    return null;
  }

  static String _hhmm(DateTime t) =>
      '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}';

  static String _deviceWord(int n) {
    final mod100 = n % 100;
    if (mod100 >= 11 && mod100 <= 14) return 'устройств';
    return switch (n % 10) {
      1 => 'устройство',
      2 || 3 || 4 => 'устройства',
      _ => 'устройств',
    };
  }

  @override
  String toString() => 'AccessState(${kind.name}, may=$mayConnect, rc=$rc)';
}

/// Байты человеку: «200 МБ», «1,5 ГБ». Запятая, потому что текст русский.
String formatBytesRu(int bytes) {
  if (bytes <= 0) return '0 МБ';
  const unit = 1024.0;
  final kb = bytes / unit;
  if (kb < unit) return '${kb.round()} КБ';
  final mb = kb / unit;
  if (mb < unit) return '${_num(mb)} МБ';
  return '${_num(mb / unit)} ГБ';
}

String _num(double v) {
  final s = v >= 10 ? v.round().toString() : v.toStringAsFixed(1);
  return s.replaceAll('.', ',').replaceAll(',0', '');
}

/// Витрина тарифов, способов оплаты и чек-аута — ровно то, что панель отдаёт
/// приложению, без единого домысла на клиенте.
///
/// Главное правило этого файла: **цену придумывает оператор, а не приложение.**
/// Тариф, у которого панель не прислала ни одного срока (`durations: []`), —
/// это не сломанный ответ и не повод подставить «30 дней по plans.price»:
/// панель специально перестала фабриковать такую строку (см.
/// `catalog_service.rs`, где самовосстановление цены удалено), потому что
/// неверная цена хуже отсутствующей. Здесь то же решение с другой стороны
/// провода: [CatalogPlan.purchasable] у такого тарифа `false`, и карточка
/// показывается без кнопки.
///
/// Второе правило — валюта. Символ ставится только тому коду, который мы
/// действительно знаем; на незнакомом печатается сам код, а на пустом —
/// голое число. «Долларов по умолчанию» здесь нет: оператор бывает не
/// американский, и подставленный `$` — это ложь о сумме, которую человек
/// собирается заплатить.
library;

import 'package:caramba_client/data/models/subscription.dart' show AccessPay;

/// Один покупаемый срок тарифа — строка `plan_durations`.
class PlanDurationOffer {
  /// `plan_durations.id` — ровно то, что уезжает в `POST /purchase`
  /// как `duration_id`.
  final int id;
  final int days;

  /// Цена в МИНОРНЫХ единицах (центы/копейки), как в колонке `price`.
  final int priceMinor;

  const PlanDurationOffer({
    required this.id,
    required this.days,
    required this.priceMinor,
  });

  /// Панель отдаёт и `price_cents` (минорные), и `price` (мажорные, дробные).
  /// Читаем минорные: округление мажорных туда-обратно теряет копейки.
  factory PlanDurationOffer.fromJson(Map<String, dynamic> json) {
    final cents = (json['price_cents'] as num?)?.toInt();
    final major = (json['price'] as num?)?.toDouble();
    return PlanDurationOffer(
      id: (json['id'] as num?)?.toInt() ?? 0,
      days: (json['duration_days'] as num?)?.toInt() ?? 0,
      priceMinor: cents ?? (major == null ? 0 : (major * 100).round()),
    );
  }

  /// «180 дней», «1 месяц» не пишем: месяц у панели не хранится, а 30 дней и
  /// месяц — разные вещи, и подмена однажды разъедется со сроком в базе.
  String get daysLabel => '$days ${_pluralDays(days)}';
}

/// Тариф витрины (`plans` + его сроки).
class CatalogPlan {
  final int id;
  final String name;
  final String description;

  /// Лимит трафика плана в ГБ. 0 — безлимит (так это и лежит в базе).
  final int trafficLimitGb;
  final int deviceLimit;

  /// Суточная норма в МБ (бесплатные тарифы). 0 — не применяется.
  final int dailyTrafficMb;

  /// Бесплатный/пробный: кнопки покупки не получает никогда, даже если
  /// оператор случайно завёл ему срок.
  final bool isFree;
  final bool isTrial;

  final List<PlanDurationOffer> durations;

  /// Выведенные панелью характеристики витрины (`catalog_service`): сколько
  /// живых узлов и в каких странах. Панель может их не прислать — тогда 0/пусто
  /// и строка не печатается.
  final int serverCount;
  final List<String> countries;

  const CatalogPlan({
    required this.id,
    required this.name,
    this.description = '',
    this.trafficLimitGb = 0,
    this.deviceLimit = 0,
    this.dailyTrafficMb = 0,
    this.isFree = false,
    this.isTrial = false,
    this.durations = const <PlanDurationOffer>[],
    this.serverCount = 0,
    this.countries = const <String>[],
  });

  /// Тариф можно купить прямо сейчас. Пустой список сроков — осмысленное
  /// состояние «не продаётся», решение оператора в админке, а не сбой.
  bool get purchasable => !isFree && durations.isNotEmpty;

  /// Самый дешёвый срок — по нему сортируется витрина и предвыбирается
  /// сегмент. `null` у непокупаемого.
  PlanDurationOffer? get cheapest {
    if (durations.isEmpty) return null;
    final sorted = [...durations]
      ..sort((a, b) => a.priceMinor.compareTo(b.priceMinor));
    return sorted.first;
  }

  /// Строка про трафик, собранная из чисел, а не из `description`: описание
  /// пишет человек и оно устаревает, числа — нет.
  String get trafficLabel {
    if (dailyTrafficMb > 0) return '$dailyTrafficMb МБ в день';
    if (trafficLimitGb <= 0) return 'Безлимит трафика';
    return '$trafficLimitGb ГБ';
  }

  String get deviceLabel =>
      '$deviceLimit ${_pluralDevices(deviceLimit)}';

  factory CatalogPlan.fromJson(Map<String, dynamic> json) {
    final rawDurations = (json['durations'] as List?) ?? const [];
    final countries = ((json['countries'] as List?) ?? const [])
        .whereType<String>()
        .toList(growable: false);
    return CatalogPlan(
      id: (json['id'] as num?)?.toInt() ?? 0,
      name: (json['name'] as String?)?.trim().isNotEmpty == true
          ? (json['name'] as String).trim()
          : 'Тариф',
      description: (json['description'] as String?) ?? '',
      trafficLimitGb: (json['traffic_limit_gb'] as num?)?.toInt() ?? 0,
      deviceLimit: (json['device_limit'] as num?)?.toInt() ?? 0,
      dailyTrafficMb: (json['daily_traffic_mb'] as num?)?.toInt() ?? 0,
      isFree: json['is_free'] == true,
      isTrial: json['is_trial'] == true,
      durations: rawDurations
          .whereType<Map<dynamic, dynamic>>()
          .map((e) => PlanDurationOffer.fromJson(e.cast<String, dynamic>()))
          .where((d) => d.id > 0 && d.days > 0)
          .toList(growable: false),
      serverCount: (json['server_count'] as num?)?.toInt() ?? 0,
      countries: countries,
    );
  }
}

/// Ответ `GET /api/v2/app/plans` целиком.
///
/// Конверт, а не голый список, потому что вместе с тарифами приходят два факта,
/// без которых витрина соврала бы: включена ли у оператора оплата ИЗ
/// ПРИЛОЖЕНИЯ ([inAppPurchase], это `license::effective_limits().end_user_billing`)
/// и куда вести человека, если нет ([pay]).
class PlanCatalog {
  /// Код валюты панели («USD»). Пусто — оператор её не назвал, и тогда суммы
  /// печатаются без символа.
  final String currency;

  /// Лицензия оператора разрешает оплату внутри приложения. `false` — покупка
  /// уходит в Telegram, и `POST /purchase` вернул бы 403.
  final bool inAppPurchase;

  /// Адреса оплаты оператора (мини-апп/бот). Те же поля, что в `access.pay`.
  final AccessPay pay;

  final List<CatalogPlan> plans;

  const PlanCatalog({
    this.currency = '',
    this.inAppPurchase = false,
    this.pay = const AccessPay(),
    this.plans = const <CatalogPlan>[],
  });

  /// Порядок витрины: сначала то, что можно купить (от дешёвого к дорогому),
  /// затем платное, снятое с продажи, и в самом низу бесплатное.
  ///
  /// Порядок строк в базе тут не годится: он произвольный, а человек, открывший
  /// экран покупки, ищет цену. Бесплатный тариф уезжает вниз не из
  /// пренебрежения, а потому что он уже выдан и кнопки не имеет — на первом
  /// экране он занял бы место того, за чем сюда пришли.
  List<CatalogPlan> get sorted {
    final list = [...plans];
    int rank(CatalogPlan p) => p.purchasable ? 0 : (p.isFree ? 2 : 1);
    list.sort((a, b) {
      final ra = rank(a), rb = rank(b);
      if (ra != rb) return ra.compareTo(rb);
      final ap = a.cheapest?.priceMinor ?? 1 << 40;
      final bp = b.cheapest?.priceMinor ?? 1 << 40;
      if (ap != bp) return ap.compareTo(bp);
      return a.name.compareTo(b.name);
    });
    return list;
  }

  /// Хоть один тариф можно купить. `false` — оператор не завёл ни одного срока
  /// (ровно текущее состояние Starter'а), и об этом говорится словами.
  bool get anyPurchasable => plans.any((p) => p.purchasable);

  /// Разбирает и конверт, и голый массив тарифов.
  ///
  /// Голый массив терпим намеренно: так отвечает мини-апповый `/api/client/plans`,
  /// и если маршрут приложения однажды заведут на тот же хендлер, экран покажет
  /// тарифы, а не ошибку разбора. Валюты и `in_app_purchase` в таком ответе нет
  /// — значит, их и не выдумываем: суммы без символа, покупка через Telegram.
  factory PlanCatalog.fromJson(dynamic data) {
    if (data is List) {
      return PlanCatalog(plans: _plansOf(data));
    }
    if (data is! Map) return const PlanCatalog();
    final json = data.cast<String, dynamic>();
    final payJson = json['pay'];
    return PlanCatalog(
      currency: ((json['currency'] as String?) ?? '').trim(),
      inAppPurchase: json['in_app_purchase'] == true,
      pay: payJson is Map
          ? AccessPay.fromJson(payJson.cast<String, dynamic>())
          : const AccessPay(),
      plans: _plansOf(json['plans']),
    );
  }

  static List<CatalogPlan> _plansOf(dynamic raw) {
    if (raw is! List) return const <CatalogPlan>[];
    return raw
        .whereType<Map<dynamic, dynamic>>()
        .map((e) => CatalogPlan.fromJson(e.cast<String, dynamic>()))
        .where((p) => p.id > 0)
        .toList(growable: false);
  }
}

/// Как оплачивается конкретный способ.
enum PayCheckout {
  /// Внутри приложения: `POST /api/v2/app/purchase` и открытие `pay_url`.
  inApp,

  /// Только в Telegram. Telegram Stars сюда попадает всегда: `StarsProvider`
  /// исключён из `MarketplaceService` (нужны bot_token и tg_id), и попытка
  /// провести его через `/purchase` кончилась бы отказом «provider not found».
  telegram,
}

/// Один способ оплаты из `GET /api/v2/app/payment-methods`.
class PaymentMethod {
  /// Имя провайдера, как его знает `MarketplaceService` («balance», «stars»…).
  final String id;
  final String label;

  /// Сумма в минорных единицах для ЭТОГО провайдера (у него может быть свой
  /// override цены) и её валюта. `null` — панель не назвала.
  final int? amountMinor;
  final String currency;

  final PayCheckout checkout;

  /// Куда вести, если [checkout] == [PayCheckout.telegram]. Пусто — адреса нет,
  /// и об этом надо сказать словами, а не открывать чужого бота.
  final String url;

  const PaymentMethod({
    required this.id,
    required this.label,
    this.amountMinor,
    this.currency = '',
    this.checkout = PayCheckout.inApp,
    this.url = '',
  });

  bool get isBalance => id == 'balance';

  factory PaymentMethod.fromJson(Map<String, dynamic> json) {
    final id = ((json['id'] as String?) ?? '').trim();
    final raw = ((json['checkout'] as String?) ?? '').trim().toLowerCase();
    // Поля `checkout` может не быть: так отвечает мини-апповый список
    // провайдеров, у которого этого понятия нет вовсе. Тогда единственный
    // способ, который ТОЧНО не проходит через `/purchase`, — Stars; остальные
    // проходят. Догадка названа здесь, а не размазана по экрану.
    final checkout = switch (raw) {
      'telegram' => PayCheckout.telegram,
      'in_app' || 'inapp' => PayCheckout.inApp,
      _ => id == 'stars' ? PayCheckout.telegram : PayCheckout.inApp,
    };
    return PaymentMethod(
      id: id,
      label: ((json['label'] as String?) ?? '').trim().isEmpty
          ? id
          : (json['label'] as String).trim(),
      amountMinor: (json['amount'] as num?)?.toInt(),
      currency: ((json['currency'] as String?) ?? '').trim(),
      checkout: checkout,
      url:
          ((json['url'] as String?) ??
                  (json['pay_url'] as String?) ??
                  (json['bot_url'] as String?) ??
                  '')
              .trim(),
    );
  }

  static List<PaymentMethod> listFrom(dynamic data) {
    final raw = data is Map
        ? (data['providers'] ?? data['methods'] ?? data['items'])
        : data;
    if (raw is! List) return const <PaymentMethod>[];
    return raw
        .whereType<Map<dynamic, dynamic>>()
        .map((e) => PaymentMethod.fromJson(e.cast<String, dynamic>()))
        .where((m) => m.id.isNotEmpty)
        .toList(growable: false);
  }
}

/// Чем является строка `pay_url` из ответа `/purchase`.
///
/// Классифицирует панель, а не мы: `pay_url` бывает абсолютным URL,
/// ОТНОСИТЕЛЬНЫМ путём панели (`manual` -> `/manual-upload`) и сентинелом
/// `SUCCESS` при оплате с баланса. Запуск не-URL «во внешнем браузере» — это
/// именно та ошибка, ради которой панель и завела `pay_url_kind`.
enum PayUrlKind { absoluteUrl, relativePath, balanceSuccess, unknown }

/// Ответ `POST /api/v2/app/purchase`.
class PurchaseCheckout {
  final String payUrl;
  final PayUrlKind kind;

  /// UUID платёжной сессии — по нему потом спрашиваем статус.
  final String sessionId;

  final int amountMinor;
  final String currency;
  final String provider;

  /// Оплата уже прошла (списание с баланса): открывать нечего, подписка
  /// продлена.
  final bool fulfilled;

  const PurchaseCheckout({
    this.payUrl = '',
    this.kind = PayUrlKind.unknown,
    this.sessionId = '',
    this.amountMinor = 0,
    this.currency = '',
    this.provider = '',
    this.fulfilled = false,
  });

  factory PurchaseCheckout.fromJson(Map<String, dynamic> json) {
    final rawKind = ((json['pay_url_kind'] as String?) ?? '').trim();
    final payUrl = ((json['pay_url'] as String?) ?? '').trim();
    return PurchaseCheckout(
      payUrl: payUrl,
      kind: switch (rawKind) {
        'absolute_url' => PayUrlKind.absoluteUrl,
        'relative_path' => PayUrlKind.relativePath,
        'balance_success' => PayUrlKind.balanceSuccess,
        // Старая панель без `pay_url_kind`: классифицируем сами по тем же
        // правилам, что и она, — иначе `SUCCESS` уехал бы в launchUrl.
        _ =>
          payUrl == 'SUCCESS'
              ? PayUrlKind.balanceSuccess
              : (payUrl.startsWith('http')
                    ? PayUrlKind.absoluteUrl
                    : (payUrl.startsWith('/')
                          ? PayUrlKind.relativePath
                          : PayUrlKind.unknown)),
      },
      sessionId: ((json['session_id'] as String?) ?? '').trim(),
      amountMinor: (json['amount'] as num?)?.toInt() ?? 0,
      currency: ((json['currency'] as String?) ?? '').trim(),
      provider: ((json['provider'] as String?) ?? '').trim(),
      fulfilled: json['fulfilled'] == true,
    );
  }

  /// Абсолютный адрес для внешнего браузера. [panelOrigin] нужен только
  /// относительному пути; для остальных видов возвращается `null`, и это не
  /// ошибка, а «открывать нечего».
  String? absoluteUrl(String panelOrigin) => switch (kind) {
    PayUrlKind.absoluteUrl => payUrl.isEmpty ? null : payUrl,
    PayUrlKind.relativePath =>
      panelOrigin.isEmpty ? null : '$panelOrigin$payUrl',
    _ => null,
  };
}

/// Состояние платёжной сессии (`GET /api/v2/app/purchase/{id}`).
///
/// Значения статуса берутся из `payment_sessions`: `pending` -> `completed`
/// (атомарный claim в `fulfill_payment`), либо `failed` / `expired`.
class PurchaseStatus {
  final String status;
  final String provider;
  final int amountMinor;
  final String currency;

  const PurchaseStatus({
    this.status = '',
    this.provider = '',
    this.amountMinor = 0,
    this.currency = '',
  });

  bool get isPaid => const {
    'completed',
    'paid',
    'success',
    'fulfilled',
  }.contains(status.toLowerCase());

  bool get isPending => status.toLowerCase() == 'pending';

  /// Человеческая строка. Сырой статус панели («expired») наружу не выходит:
  /// это внутреннее слово, как и «throttled» в карточке подписки.
  String get label {
    if (isPaid) return 'Оплачено';
    if (isPending) return 'Ожидает оплаты';
    return 'Оплата не прошла';
  }

  factory PurchaseStatus.fromJson(Map<String, dynamic> json) => PurchaseStatus(
    status: ((json['status'] as String?) ?? '').trim(),
    provider: ((json['provider'] as String?) ?? '').trim(),
    amountMinor: (json['amount'] as num?)?.toInt() ?? 0,
    currency: ((json['currency'] as String?) ?? '').trim(),
  );
}

/// Сумма в минорных единицах -> строка для человека.
///
/// Символ ставится только известному коду валюты. Незнакомый код печатается как
/// есть («1500 XTS»), пустой — не печатается вовсе: подставить `$` оператору,
/// который берёт рубли, значит соврать о сумме платежа.
String formatMoneyMinor(int minor, String currency) {
  final amount = _majorString(minor);
  final code = currency.trim().toUpperCase();
  const symbols = <String, String>{
    'USD': r'$',
    'EUR': '€',
    'RUB': '₽',
    'UAH': '₴',
    'KZT': '₸',
    'GBP': '£',
  };
  final symbol = symbols[code];
  if (symbol != null) return '$amount $symbol';
  return code.isEmpty ? amount : '$amount $code';
}

/// Минорные -> мажорные без хвостовых нулей: 1500 -> «15», 1550 -> «15.50».
String _majorString(int minor) {
  if (minor <= 0) return '0';
  final major = minor / 100;
  return major == major.roundToDouble()
      ? major.toStringAsFixed(0)
      : major.toStringAsFixed(2);
}

String _pluralDays(int n) => _plural(n, 'день', 'дня', 'дней');

String _pluralDevices(int n) =>
    _plural(n, 'устройство', 'устройства', 'устройств');

String _plural(int n, String one, String few, String many) {
  final mod100 = n % 100;
  if (mod100 >= 11 && mod100 <= 14) return many;
  return switch (n % 10) {
    1 => one,
    2 || 3 || 4 => few,
    _ => many,
  };
}

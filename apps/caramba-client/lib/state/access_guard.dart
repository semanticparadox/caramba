/// Живая проверка права подключаться: пропустит ли оператор трафик ПРЯМО
/// СЕЙЧАС, а не пропускал ли он его в тот день, когда подписку импортировали.
///
/// ЗАЧЕМ ЭТОТ ФАЙЛ СУЩЕСТВУЕТ. Туннель поднимается из КЭША конфигурации и
/// поднимается успешно даже тогда, когда оператор уже выкинул uuid из узлов:
/// TUN живой, mihomo конфиг разобрал, ядро сообщает `connected`. Дальше VLESS
/// падает на авторизации у каждого прокси — но об этом не знает никто, и экран
/// показывает зелёный щит, «Защищено» и идущий таймер, пока ни один сайт не
/// открывается. Ровно это и воспроизводится на эмуляторе: 1м43с «защиты» при
/// 0,0 МБ скачано.
///
/// Ядро отличить одно от другого сегодня не может: `connected` означает
/// «конфиг разобран и TUN поднят», и ни одного сетевого факта за этим словом
/// нет. Значит спросить надо у того, кто ЗНАЕТ ответ, — у самой подписки.
/// Запрос уходит МИМО туннеля: `CarambaVpnService.buildInterface` держит
/// собственный пакет в `addDisallowedApplication`, поэтому HTTP приложения
/// ходит по настоящей сети и остаётся честным даже когда через туннель не
/// проходит ничего.
///
/// Гостевой режим (своя подписка, сессии панели нет) — это флоу владельца, и
/// здесь у него ровно один источник правды: ответ по ссылке подписки. Панель
/// спрашивается своим эндпоинтом, но решение читается одинаково.
library;

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/data/subscription_fetch.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/subscription_state.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Что удалось узнать о праве подключаться.
enum AccessAnswer {
  /// Спросить было не у кого или ответа не пришло. НЕ «всё плохо»: неизвестность
  /// не имеет права ни запрещать подключение, ни гасить зелёный щит.
  unknown,

  /// Оператор выдаёт конфигурацию — подключение имеет смысл.
  allowed,

  /// Оператор отказал и назвал причину.
  refused,
}

/// Результат живой проверки.
class AccessVerdict {
  final AccessAnswer answer;

  /// Причина отказа. Не `null` тогда и только тогда, когда [answer] —
  /// [AccessAnswer.refused].
  final AccessState? refusal;

  /// Код ответа, если отказ пришёл по HTTP. `null` — отказ пришёл объектом
  /// `access` в успешном ответе панели (или ответа не было вовсе).
  final int? statusCode;

  /// Когда получен ответ; `null` — ответа не было.
  final DateTime? checkedAt;

  const AccessVerdict._(
    this.answer, {
    this.refusal,
    this.statusCode,
    this.checkedAt,
  });

  /// Спросить не у кого / ответа нет.
  static const AccessVerdict unknown = AccessVerdict._(AccessAnswer.unknown);

  factory AccessVerdict.allowed({DateTime? at}) => AccessVerdict._(
    AccessAnswer.allowed,
    checkedAt: at ?? DateTime.now(),
  );

  factory AccessVerdict.refused(
    AccessState refusal, {
    int? statusCode,
    DateTime? at,
  }) => AccessVerdict._(
    AccessAnswer.refused,
    refusal: refusal,
    statusCode: statusCode,
    checkedAt: at ?? DateTime.now(),
  );

  /// Подключаться нельзя, и причина известна.
  bool get blocked => answer == AccessAnswer.refused && refusal != null;

  /// Строка для `VpnStatus.detail`: причина плюс код, по которому её узнали.
  ///
  /// Код здесь не для показа. Он остаётся в detail уликой (её достаёт
  /// «Подробности») и даёт `describeText` шанс опознать отказ даже там, где
  /// состояние доступа до экрана почему-то не доехало.
  String get detail {
    final reason = refusal?.shortReason ?? 'доступ закрыт';
    final code = statusCode;
    return code == null
        ? 'подписка: доступ закрыт — $reason'
        : 'подписка: оператор вернул статус $code — $reason';
  }

  @override
  String toString() => 'AccessVerdict(${answer.name}, $refusal)';
}

/// Отказ подписки, разобранный до состояния доступа.
class SubscriptionRefusal {
  final AccessState access;

  /// Ответ опознан как ответ ОПЕРАТОРА, а не как что угодно с кодом 403.
  ///
  /// Разница не педантичная. Портал публичного Wi-Fi отдаёт 403 на любой
  /// адрес, и принять его за «подписка закрыта» значит погасить щит на живом
  /// туннеле и назвать человеку причину, которой нет. Уликой служит то, что
  /// умеет напечатать только панель: заголовки `x-caramba-*`,
  /// `subscription-userinfo` или её собственные тексты отказов.
  final bool fromOperator;

  const SubscriptionRefusal({required this.access, required this.fromOperator});
}

/// Коды, которыми оператор отказывает в выдаче конфигурации.
///
/// 429 и 5xx сюда НЕ входят намеренно: это «сейчас не могу», а не «тебе
/// нельзя», и подписка за ними исправна.
const Set<int> kRefusalStatusCodes = <int>{401, 403, 404};

/// Разбирает отказ подписки в состояние доступа. `null` — это не отказ по
/// доступу (другой код, ответа не было).
///
/// Источников два, и оба реальные:
///   * прямой ответ панели — заголовки `x-caramba-*` несут ровно те же поля,
///     что и объект `access` в JSON, поэтому и разбираются той же функцией:
///     второй классификатор кодов разошёлся бы с первым;
///   * ответ через зеркало подписки (`caramba-sub`) — оно копирует ТРИ
///     заголовка, и `x-caramba-*` в их число не входит. Тогда остаются тело
///     отказа и `subscription-userinfo`, и вывод делается из них.
SubscriptionRefusal? refusalFromResponse({
  required int? statusCode,
  String body = '',
  Map<String, String> headers = const <String, String>{},
}) {
  if (statusCode == null || !kRefusalStatusCodes.contains(statusCode)) {
    return null;
  }
  final h = <String, String>{
    for (final e in headers.entries) e.key.toLowerCase().trim(): e.value.trim(),
  };

  final fromHeaders = _accessFromHeaders(h);
  if (fromHeaders != null) {
    return SubscriptionRefusal(access: fromHeaders, fromOperator: true);
  }

  final info = _userinfo(h['subscription-userinfo']);
  final low = body.toLowerCase();
  final quota =
      low.contains('traffic limit reached') ||
      low.contains('quota') ||
      // «Subscription inactive or expired» панель печатает и троттлингу, и
      // истёкшему сроку. Что из двух — видно по её же числам: расход, дошедший
      // до потолка, называет причину точнее, чем слово «expired» в теле, и
      // сказать про срок исчерпавшему дневную норму значило бы соврать.
      (_inactiveBody(low) &&
          info.limit > 0 &&
          info.used >= info.limit);
  final device = low.contains('device limit');
  final expired =
      !quota &&
      _inactiveBody(low) &&
      info.expire != null &&
      info.expire!.isBefore(DateTime.now());

  final state = quota
      ? 'quota_exceeded'
      : device
      ? 'device_limit'
      : expired
      ? 'expired'
      : '';

  // Панель отказывает своими текстами, и их достаточно, чтобы признать ответ
  // ответом ОПЕРАТОРА, даже когда цифр в нём нет. Это разные вопросы: «кто
  // отказал» и «по какой причине». Портал Wi-Fi отдаёт свой HTML, а не
  // «Subscription inactive or expired», поэтому спутать их нельзя.
  //
  // Пока здесь требовались ещё и числа, живой сторож на РАЗВЁРНУТОЙ панели
  // (она не шлёт subscription-userinfo при отказе) считал такой 403 чужим и
  // молчал — человек три с половиной минуты видел зелёный щит поверх мёртвого
  // туннеля. Именно этот случай и есть самый частый: дневная норма кончается
  // посреди сессии, а не до подключения.
  final operatorWording = _inactiveBody(low) || device || low.contains('traffic limit reached');

  // Причину не опознали. Отказ всё равно собирается — но помеченным как
  // неподтверждённый: на живом туннеле такому ответу верить нельзя.
  final known = state.isNotEmpty || info.limit > 0 || info.used > 0 || operatorWording;
  final access = AccessState.fromJson(<String, dynamic>{
    'may_connect': false,
    'state': state,
    'used_bytes': info.used,
    'limit_bytes': info.limit,
  });
  return SubscriptionRefusal(
    access: access ?? const AccessState(mayConnect: false, kind: AccessKind.unknown),
    fromOperator: known,
  );
}

/// Правда ли тело говорит «подписка неактивна или истекла».
bool _inactiveBody(String low) =>
    low.contains('subscription inactive') || low.contains('inactive or expired');

/// Состояние доступа из заголовков `x-caramba-*`. `null` — панель их не
/// прислала (старая панель или зеркало, которое их вырезает).
///
/// Поля один в один совпадают с объектом `access`, поэтому собираются в тот же
/// JSON и отдаются [AccessState.fromJson]: таблица кодов причин живёт в одном
/// месте, и новый код панели читается одинаково обоими путями.
AccessState? _accessFromHeaders(Map<String, String> h) {
  final st = int.tryParse(h['x-caramba-st'] ?? '');
  final rc = int.tryParse(h['x-caramba-reason'] ?? '');
  if (st == null && rc == null) return null;
  return AccessState.fromJson(<String, dynamic>{
    'may_connect': false,
    'st': st,
    'rc': rc,
    'state': h['x-caramba-state'] ?? '',
    'reason': h['x-caramba-reason-name'] ?? '',
    'used_bytes': int.tryParse(h['x-caramba-used'] ?? '') ?? 0,
    'limit_bytes': int.tryParse(h['x-caramba-limit'] ?? '') ?? 0,
    'period': h['x-caramba-period'] ?? '',
    // В заголовке — unix-секунды (заголовок обязан быть ASCII, и панель шлёт
    // число), а объект `access` в JSON несёт RFC3339. Приводим здесь, чтобы
    // разбор остался один.
    'resets_at': _isoFromUnix(h['x-caramba-resets-at']),
    'reset_lag_seconds': int.tryParse(h['x-caramba-reset-lag'] ?? '') ?? 0,
    'bytes_after_reset': int.tryParse(h['x-caramba-bytes-after-reset'] ?? ''),
  });
}

String? _isoFromUnix(String? seconds) {
  final s = int.tryParse(seconds ?? '');
  if (s == null || s <= 0) return null;
  return DateTime.fromMillisecondsSinceEpoch(
    s * 1000,
    isUtc: true,
  ).toIso8601String();
}

/// Числа из `subscription-userinfo` — грамматика Hiddify/Happ:
/// `upload=0; download=123; total=456; expire=1788652800`.
({int used, int limit, DateTime? expire}) _userinfo(String? raw) {
  if (raw == null || raw.isEmpty) return (used: 0, limit: 0, expire: null);
  var used = 0;
  var limit = 0;
  DateTime? expire;
  for (final part in raw.split(';')) {
    final kv = part.split('=');
    if (kv.length != 2) continue;
    final key = kv[0].trim().toLowerCase();
    final value = int.tryParse(kv[1].trim());
    if (value == null) continue;
    switch (key) {
      case 'download':
        used = value;
      case 'total':
        limit = value;
      case 'expire':
        // 0 и «дальше некуда» (9999 год) сроком не являются.
        if (value > 0 && value < 32503680000) {
          expire = DateTime.fromMillisecondsSinceEpoch(
            value * 1000,
            isUtc: true,
          );
        }
    }
  }
  return (used: used, limit: limit, expire: expire);
}

/// Спрашивает подписку по её ссылке.
///
/// [live] — вопрос задан при ПОДНЯТОМ туннеле. Тогда неопознанный 4xx (портал
/// Wi-Fi, чужой прокси) остаётся неизвестностью: гасить щит на работающем
/// туннеле по чужому ответу нельзя. На подключении такой осторожности нет —
/// там отказ ничего не рушит, а попытка подняться на кэше как раз и даёт ту
/// самую ложную защиту.
Future<AccessVerdict> probeSubscriptionUrl(
  String url, {
  required Future<String> Function(String url) fetch,
  required bool live,
  Duration budget = kAccessCheckBudget,
}) async {
  try {
    await fetch(url).timeout(budget);
    return AccessVerdict.allowed();
  } on SubscriptionFetchException catch (e) {
    final refusal = refusalFromResponse(
      statusCode: e.statusCode,
      body: e.body,
      headers: e.headers,
    );
    if (refusal == null) return AccessVerdict.unknown;
    if (live && !refusal.fromOperator) return AccessVerdict.unknown;
    return AccessVerdict.refused(refusal.access, statusCode: e.statusCode);
  } catch (_) {
    // Сеть, таймаут, разбор — что угодно, кроме ответа оператора. Молчим.
    return AccessVerdict.unknown;
  }
}

/// Сколько ждём ответа подписки. Держит человека на кнопке «Подключиться»,
/// поэтому короткий: не ответили — подключаемся, а не объясняем.
const Duration kAccessCheckBudget = Duration(seconds: 6);

/// Сколько живёт память о отказе, когда следующий вопрос остался без ответа.
const Duration kRefusalMemory = Duration(minutes: 10);

/// Через сколько после поднятия туннеля задаётся первый живой вопрос.
const Duration kAccessWatchFirst = Duration(seconds: 25);

/// Как часто он повторяется. Свип панели, который троттлит подписку и рвёт
/// живые соединения, ходит раз в 600 с — чаще этого спрашивать незачем, а
/// реже значит оставлять человека в ложной защите на минуты.
const Duration kAccessWatchEvery = Duration(seconds: 120);

/// Сторож доступа: спрашивает перед подключением и продолжает спрашивать, пока
/// туннель поднят.
///
/// Состояние НЕ сбрасывается при обрыве: отказ, из-за которого подключение не
/// состоялось, обязан остаться на экране — иначе объяснение исчезает вместе с
/// попыткой, и человек снова видит голую ошибку.
class AccessGuard extends StateNotifier<AccessVerdict> {
  final Future<AccessVerdict> Function(bool live) _check;
  final Duration _first;
  final Duration _every;
  Timer? _timer;
  bool _busy = false;

  /// Туннель поднят прямо сейчас. Отдельно от таймера: запрос живёт секунды, и
  /// за это время стадия успевает смениться — перезаводить сторожа на уже
  /// оборванном туннеле нельзя.
  bool _connected = false;

  AccessGuard({
    required Future<AccessVerdict> Function(bool live) check,
    Duration first = kAccessWatchFirst,
    Duration every = kAccessWatchEvery,
    AccessVerdict initial = AccessVerdict.unknown,
  }) : _check = check,
       _first = first,
       _every = every,
       super(initial);

  /// Вопрос перед подключением. Возвращает вердикт вызывающему — решение
  /// «подключаться или нет» принимает он, а не сторож.
  Future<AccessVerdict> checkBeforeConnect() async {
    // Свежий отказ переживает эту попытку. Всё остальное — нет: показывать
    // вчерашний отказ рядом с сегодняшним подключением нельзя.
    final remembered = _freshRefusal();
    state = AccessVerdict.unknown;
    final verdict = await _check(false);
    // Ответ не пришёл, а полминуты назад оператор отказывал — отказ и есть
    // ответ. Иначе выходит гонка: человек жмёт «подключиться» сразу после
    // отключения, сеть Android ещё переключается, бюджет в шесть секунд
    // истекает раньше ответа, и туннель поднимается на кэше зелёным. Один раз
    // из четырёх на устройстве — ровно та ложная защита, ради которой сторож
    // и написан.
    final answer = verdict.answer == AccessAnswer.unknown && remembered != null
        ? remembered
        : verdict;
    state = answer;
    return answer;
  }

  /// Последний отказ оператора, если он ещё не устарел.
  ///
  /// Срок короткий: человек, оплативший тариф, не должен упереться в память о
  /// вчерашнем отказе. За [kRefusalMemory] панель успевает ответить сама.
  AccessVerdict? _freshRefusal() {
    final prev = state;
    if (!prev.blocked) return null;
    final at = prev.checkedAt;
    if (at == null) return null;
    return DateTime.now().difference(at) <= kRefusalMemory ? prev : null;
  }

  /// Стадия туннеля сменилась: сторож просыпается на живом туннеле и засыпает
  /// на любом другом.
  void onStage(VpnStage stage) {
    final connected = stage == VpnStage.connected;
    if (connected == _connected) return;
    _connected = connected;
    if (connected) {
      _timer ??= Timer(_first, _tick);
      return;
    }
    _timer?.cancel();
    _timer = null;
  }

  Future<void> _tick() async {
    _timer = null;
    if (_busy) return;
    _busy = true;
    try {
      final verdict = await _check(true);
      if (!mounted) return;
      // Ответа не было — молчим. Телефон в кармане на слабой сети выглядит
      // ровно так же, как отказ, и объявлять по этому поводу поломку значит
      // ронять щит на исправном туннеле.
      if (verdict.answer != AccessAnswer.unknown) state = verdict;
    } finally {
      _busy = false;
      // Перезаводим, только если туннель ещё жив: стадия могла смениться прямо
      // во время запроса.
      if (mounted && _timer == null && _connected) {
        _timer = Timer(_every, _tick);
      }
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }
}

/// Выборка тела подписки. Отдельным провайдером, чтобы тест мог ответить
/// вместо сети.
final subscriptionBodyFetchProvider = Provider<Future<String> Function(String)>(
  (ref) => fetchSubscriptionBody,
);

/// Спрашивает того, у кого есть ответ: свою подписку — по ссылке, панельную —
/// её же эндпоинтом.
Future<AccessVerdict> checkSubscriptionAccess(Ref ref, {required bool live}) async {
  final profile = ref.read(activeConnectionProfileProvider);
  if (profile != null && profile.isRaw) {
    final url = profile.source.trim();
    // Подписку, вставленную текстом, перезапросить неоткуда: источника нет.
    // Это единственный режим, в котором приложение о доступе узнать не может,
    // и врать про него оно тоже не будет.
    if (!url.startsWith('http')) return AccessVerdict.unknown;
    return probeSubscriptionUrl(
      url,
      fetch: ref.read(subscriptionBodyFetchProvider),
      live: live,
    );
  }
  // Панельный путь. Без сессии спрашивать не у кого — запрос ушёл бы в 401 и
  // вернулся бы «отказом», которого нет.
  if (ref.read(authProvider).stage != AuthStage.authenticated) {
    return AccessVerdict.unknown;
  }
  try {
    final sub = await ref
        .refresh(subscriptionProvider.future)
        .timeout(kAccessCheckBudget);
    return sub.access.isBlocked
        ? AccessVerdict.refused(sub.access)
        : AccessVerdict.allowed();
  } catch (_) {
    // Панель не ответила — это не отказ. Ядро на панельном пути перезапрашивает
    // подписку на каждом Up и своё 403 поймает само.
    return AccessVerdict.unknown;
  }
}

final accessGuardProvider = StateNotifierProvider<AccessGuard, AccessVerdict>((
  ref,
) {
  return AccessGuard(check: (live) => checkSubscriptionAccess(ref, live: live));
});

/// Отказ, известный ПРЯМО СЕЙЧАС; `null` — отказа нет.
///
/// Это ответ живой проверки, а не снимок логина: именно он имеет право
/// перекрашивать дайл и показывать карточку в гостевом режиме, где панельного
/// состояния подписки не существует вовсе.
final liveAccessRefusalProvider = Provider<AccessState?>(
  (ref) => ref.watch(accessGuardProvider).refusal,
);

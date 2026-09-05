/// Перевод отказа — ядра, панели или сети — в предложение, которое можно
/// прочитать и что-то сделать.
///
/// ЗДЕСЬ БЫЛО ОБРАТНОЕ ПРАВИЛО. Файл прямо предписывал показывать ИМЕННО текст
/// ядра, «потому что он объясняет проблему лучше любой нашей формулировки».
/// Владелец добавлял подписку и получил на экран вот это:
///
///   api: загрузка узлов подписки для замера: api: загрузка серверов подписки:
///   subscription: запрос подписки: transport: ни одна включённая ступень не
///   вернула ответ: transport: код состояния 403
///
/// Пять честных обёрток вокруг одного числа, которое никто не прочитал. Реальная
/// причина — израсходованная дневная норма бесплатного тарифа — в этой строке не
/// названа ни разу, и диагностика заняла часы.
///
/// Текст ядра поэтому больше не витрина, а УЛИКА: он сохраняется целиком и
/// достаётся из-под «Подробности» ([technicalDetailFor]), а на экран идёт
/// перевод. Прятать его насовсем нельзя — именно скрытая деталь и превращает
/// баг в многочасовой; показывать его первым — тоже, это и была ошибка.
library;

import 'package:caramba_vpn/caramba_vpn.dart' show CarambaCoreException;
import 'package:flutter/services.dart' show PlatformException;

import 'package:caramba_client/data/api_client.dart' show ApiException;
import 'package:caramba_client/data/models/subscription.dart'
    show AccessKind, AccessState;
import 'package:caramba_client/data/subscription_fetch.dart'
    show SubscriptionFetchException;

/// Разобранный отказ: что сказать человеку и что спрятать под «Подробности».
class CarambaFailure {
  /// Одно-два предложения по-русски: что случилось и что с этим делать.
  final String text;

  /// Исходный текст ядра/панели целиком. Показывается только по явному
  /// запросу. `null` — показывать нечего.
  final String? technical;

  /// HTTP-код, если он вообще был назван.
  final int? statusCode;

  /// Отказ по доступу к подписке (а не сеть и не сбой). `null` — не он.
  final AccessKind? access;

  /// Имеет ли смысл предлагать оплату.
  final bool payable;

  /// Имеет ли смысл кнопка «Повторить». У исчерпанного лимита — нет: повтор
  /// вернёт тот же 403 и научит человека, что кнопка врёт.
  final bool retryable;

  const CarambaFailure({
    required this.text,
    this.technical,
    this.statusCode,
    this.access,
    this.payable = false,
    this.retryable = true,
  });

  bool get hasTechnical => (technical?.trim().isNotEmpty ?? false);
}

/// Человеческий текст ошибки ядра/панели.
///
/// Сигнатура и контракт прежние — `null` означает «эту ошибку я не узнаю,
/// подставь своё слово», и все вызывающие продолжают работать. Изменилось
/// содержимое: вместо цепочки Go-обёрток возвращается перевод, а сама цепочка
/// остаётся доступной через [technicalDetailFor].
String? coreErrorText(Object error) => describeFailure(error)?.text;

/// Исходный технический текст, соответствующий уже показанной строке.
///
/// Нужен ровно там, где до экрана доезжает только `String`: замер складывает
/// текст ошибки в своё состояние (`ProbeRun.error`), а не исключение, и без
/// этого моста «Подробности» на экране серверов показать было бы нечего.
/// Таблица маленькая и кольцевая: это подсказка для UI, а не журнал.
String? technicalDetailFor(String? shown) {
  if (shown == null) return null;
  final raw = _details[shown];
  return (raw == null || raw == shown) ? null : raw;
}

/// Кольцо последних переводов: показанный текст -> исходный.
final Map<String, String> _details = <String, String>{};
const int _detailsMax = 24;

void _remember(String shown, String? technical) {
  if (technical == null || technical.isEmpty || technical == shown) return;
  if (_details.length >= _detailsMax) {
    _details.remove(_details.keys.first);
  }
  _details[shown] = technical;
}

/// Разбирает отказ. `null` — исключение не наше (вызывающий подставит свою
/// формулировку, она в его контексте точнее).
///
/// [access] — состояние подписки, если экран его уже знает: тогда вместо общих
/// слов про «403» показываются настоящие числа и срок.
CarambaFailure? describeFailure(Object error, {AccessState? access}) {
  final raw = _rawTextOf(error);
  final code = error is ApiException
      ? error.statusCode ?? _statusIn(raw)
      : _statusIn(raw);
  if (raw == null && code == null) return null;

  final failure = _classify(raw ?? '', code, access);
  if (failure == null) return null;
  _remember(failure.text, raw);
  return failure;
}

/// То же самое для голого текста.
///
/// Нужен там, где до экрана доезжает строка, а не исключение: `VpnStatus.detail`
/// несёт причину ошибки туннеля именно так, и до этой правки её не читал никто.
CarambaFailure? describeText(String? raw, {AccessState? access}) {
  final text = raw?.trim();
  if (text == null || text.isEmpty) return null;
  final failure = _classify(text, _statusIn(text), access);
  if (failure == null) return null;
  _remember(failure.text, text);
  return failure;
}

/// Сырой текст исключения — ровно то, что раньше уходило на экран.
String? _rawTextOf(Object error) {
  if (error is CarambaCoreException) {
    final m = error.message.trim();
    return m.isEmpty ? null : m;
  }
  if (error is ApiException) {
    final m = error.message.trim();
    return m.isEmpty ? null : m;
  }
  // Импорт подписки ходит своим Dio, а не ядром, и отказ панели доезжает сюда
  // строкой «ответ сервера 403» — тем же 403, что и на пути ядра. Разбирать
  // его вторым набором правил значило бы получить два разных ответа на одну
  // причину.
  if (error is SubscriptionFetchException) {
    final m = error.message.trim();
    return m.isEmpty ? null : m;
  }
  if (error is PlatformException) {
    final message = error.message?.trim();
    if (message != null && message.isNotEmpty) return message;
    final details = error.details?.toString().trim();
    if (details != null && details.isNotEmpty) return details;
    return error.code.isEmpty ? null : error.code;
  }
  return null;
}

/// Код состояния, вытащенный из текста обёрток ядра.
///
/// Ядро роняет и заголовки, и тело ответа (`transport/ladder.go`), и число в
/// строке — единственное, что от отказа доезжает. Пока ядро не научится
/// присылать причину отдельным полем, читаем то, что есть.
int? _statusIn(String? raw) {
  if (raw == null) return null;
  for (final re in _statusPatterns) {
    final m = re.firstMatch(raw);
    if (m != null) return int.tryParse(m.group(1)!);
  }
  return null;
}

final List<RegExp> _statusPatterns = <RegExp>[
  RegExp(r'код состояния\s+(\d{3})'),
  RegExp(r'ответ сервера\s+(\d{3})'),
  RegExp(r'вернула статус\s+(\d{3})'),
  RegExp(r'статус\s+(\d{3})'),
  RegExp(r'\bHTTP[ /]?\d?(?:\.\d)?\s+(\d{3})'),
  RegExp(r'status(?: code)?\s+(\d{3})'),
];

bool _has(String haystack, List<String> needles) {
  final low = haystack.toLowerCase();
  for (final n in needles) {
    if (low.contains(n)) return true;
  }
  return false;
}

CarambaFailure? _classify(String raw, int? code, AccessState? access) {
  // Панель называет отказ словами в теле ответа; при 403 они и есть причина.
  // Тексты сверены с `apps/caramba-panel/src/subscription.rs` и объявлены там
  // неизменными, так что сравнение по ним — контракт, а не догадка.
  final quota = _has(raw, ['traffic limit reached', 'quota']);
  final device = _has(raw, ['device limit']);
  final inactive = _has(raw, ['subscription inactive', 'inactive or expired']);
  final noServers = _has(raw, ['no servers available']);
  final notFound = _has(raw, ['subscription not found']);
  final rate = _has(raw, ['rate limit']);

  // Состояние подписки, если оно уже известно, точнее любого разбора текста:
  // в нём есть числа и срок, а в 403 нет ничего.
  if (access != null && access.isBlocked && (code == 403 || quota || device)) {
    return CarambaFailure(
      text: '${access.title}. ${access.body}',
      technical: raw,
      statusCode: code,
      access: access.kind,
      payable: access.kind != AccessKind.fleetUnavailable,
      retryable: false,
    );
  }

  if (device) {
    return CarambaFailure(
      text:
          'Занято максимум устройств для этой подписки. Отключите VPN на '
          'другом устройстве или удалите его в разделе «Устройства», либо '
          'выберите тариф, где устройств больше.',
      technical: raw,
      statusCode: code,
      access: AccessKind.deviceLimit,
      payable: true,
      retryable: false,
    );
  }

  if (quota) {
    return CarambaFailure(
      text:
          'Трафик по подписке закончился, и оператор больше не выдаёт '
          'конфигурацию. На бесплатном тарифе дневная норма вернётся после '
          'полуночи UTC; на платном нужно продлить тариф.',
      technical: raw,
      statusCode: code,
      access: AccessKind.planQuota,
      payable: true,
      retryable: false,
    );
  }

  if (inactive) {
    return CarambaFailure(
      text:
          'Подписка сейчас неактивна: закончился срок или дневная норма '
          'трафика. Проверьте подписку у оператора и продлите её.',
      technical: raw,
      statusCode: code,
      access: AccessKind.unknown,
      payable: true,
      retryable: false,
    );
  }

  if (notFound || code == 404) {
    return CarambaFailure(
      text:
          'Оператор не знает этой подписки. Проверьте ссылку или импортируйте '
          'её заново.',
      technical: raw,
      statusCode: code,
      retryable: false,
    );
  }

  if (rate || code == 429) {
    return CarambaFailure(
      text: 'Слишком много запросов подряд. Подождите минуту и повторите.',
      technical: raw,
      statusCode: code,
    );
  }

  if (noServers || code == 503) {
    return CarambaFailure(
      text:
          'У оператора сейчас нет свободных узлов. Это временно, попробуйте '
          'через минуту.',
      technical: raw,
      statusCode: code,
      access: AccessKind.fleetUnavailable,
    );
  }

  if (code == 401) {
    return CarambaFailure(
      text: 'Сессия истекла. Войдите заново, чтобы продолжить.',
      technical: raw,
      statusCode: code,
      retryable: false,
    );
  }

  // Голый 403 старой панели: причины она не называет вовсе. Сказать «ошибка
  // 403» значит вернуть человека туда же, откуда он пришёл, поэтому здесь
  // перечислены ровно те три ситуации, которые панель прячет за этим кодом.
  if (code == 403) {
    return CarambaFailure(
      text:
          'Оператор не выдал конфигурацию по этой подписке. Так отвечают в '
          'трёх случаях: закончился трафик, закончился срок подписки или занято '
          'максимальное число устройств. Откройте подписку у оператора: там '
          'видно, что именно.',
      technical: raw,
      statusCode: code,
      access: AccessKind.unknown,
      payable: true,
      retryable: false,
    );
  }

  if (code != null && code >= 500) {
    return CarambaFailure(
      text: 'Сервер оператора не отвечает. Повторите через минуту.',
      technical: raw,
      statusCode: code,
    );
  }

  // Ни одна ступень не ответила и кода нет: до сервера подписки не дошло
  // вообще ничего. Это сеть или блокировка, а не подписка.
  if (_has(raw, [
    'ни одна включённая ступень',
    'нет доступных ступеней',
    'timeout',
    'таймаут',
    'connection refused',
    'network',
    'socket',
    'dns',
  ])) {
    return CarambaFailure(
      text:
          'Не удалось достучаться до сервера подписки. Проверьте интернет и '
          'повторите; если сеть с ограничениями, включите обходные ступени в '
          'настройках.',
      technical: raw,
    );
  }

  if (raw.trim().isEmpty) return null;
  return CarambaFailure(
    text: code == null
        ? 'Не удалось выполнить запрос. Повторите попытку.'
        : 'Запрос не прошёл (код $code). Повторите попытку.',
    technical: raw,
    statusCode: code,
  );
}

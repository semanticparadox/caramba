/// Погашение кода из ссылки `caramba://connect`: `POST /api/v2/app/enroll/redeem`.
///
/// ПОЧЕМУ ОТДЕЛЬНЫЙ КЛИЕНТ, А НЕ `ApiClient`. Погашение идёт ДО того, как у
/// приложения появился профиль этой панели и хоть какая-то сессия для неё.
/// `ApiClient` строится вокруг активного профиля: он подставляет `Authorization`
/// из общего хранилища токенов, умеет ротацию refresh и отказывает, когда
/// панель не подключена. Здесь всё это лишнее и вредное: единственная
/// аутентификация это сам одноразовый код, а адрес берётся из ссылки, а не из
/// того, что сейчас активно. Тот же приём, что у `panel_probe.dart`.
///
/// Ответ панели (`app_enroll::RedeemResponse`) намеренно нуллабелен по
/// подписке: аккаунт без пригодной подписки это РЕАЛЬНОЕ состояние, и рядом
/// лежит машинная причина. Приложение обязано показать её как есть, а не
/// подставить выдуманный адрес.
library;

import 'package:dio/dio.dart';

import 'package:caramba_client/data/models/auth_tokens.dart';

/// Что вернуло погашение.
class ConnectRedeemResult {
  const ConnectRedeemResult({
    required this.tokens,
    required this.panelName,
    this.subscriptionUrl,
    this.subscriptionUuid,
    this.subscriptionStatus,
    this.subscriptionReason,
  });

  /// Пара JWT: с этого момента человек панельный клиент, а не гость.
  final AuthTokens tokens;

  /// Имя оператора, как его отдала САМА панель по TLS с того origin, который
  /// назвала ссылка. Сильнее, чем имя внутри ссылки: ссылка не подписана.
  final String panelName;

  /// Ссылка подписки, если она у аккаунта есть.
  final String? subscriptionUrl;

  /// UUID подписки: он же ключ подписки в остальном API панели.
  final String? subscriptionUuid;

  /// Статус подписки как он есть в панели (active / pending / expired ...).
  final String? subscriptionStatus;

  /// Машинная причина, по которой [subscriptionUrl] пуст. Непуста ТОЛЬКО
  /// тогда, когда подписки нет.
  final String? subscriptionReason;

  bool get hasSubscription => (subscriptionUuid ?? '').isNotEmpty;

  /// Человеческий текст причины отсутствия подписки. Неизвестная причина
  /// показывается как есть, а не прячется: панель может добавить новую, и
  /// «непонятно что» лучше, чем «ничего».
  String? get subscriptionReasonText {
    final reason = subscriptionReason;
    if (reason == null || reason.isEmpty) return null;
    return switch (reason) {
      'no_subscription_on_account' =>
        'У аккаунта нет подписки: оператор не настроил бесплатный план.',
      'subscription_has_no_uuid' =>
        'Подписка у аккаунта есть, но она не попадает в конфигурацию узлов. '
            'Подключиться по ней нельзя, пока оператор это не поправит.',
      'subscription_lookup_failed' =>
        'Панель не смогла прочитать подписку. Войти удалось, подписка '
            'подтянется позже.',
      'subscription_domain_not_configured' =>
        'Оператор не настроил домен подписок, поэтому адрес подписки панель '
            'выдать не может.',
      _ => 'Панель не выдала подписку. Причина: $reason.',
    };
  }
}

/// Погашение не состоялось.
class ConnectRedeemException implements Exception {
  const ConnectRedeemException(this.message, {this.statusCode});

  final String message;
  final int? statusCode;

  /// Код не подошёл: не существует, просрочен или уже погашен. Панель не
  /// различает эти три случая намеренно, и приложение не имеет права
  /// придумывать различие, которого в ответе нет.
  bool get isInvalidCode => statusCode == 400;

  @override
  String toString() => 'ConnectRedeemException($statusCode): $message';
}

/// Гасит код на панели [origin] и возвращает сессию.
///
/// [code] это 32 hex-символа, поле 2 ссылки в проводной форме. Сырые байты по
/// сети не ходят.
Future<ConnectRedeemResult> redeemConnectCode({
  required String origin,
  required String code,
  Dio? client,
  Duration timeout = const Duration(seconds: 20),
}) async {
  final dio =
      client ??
      Dio(
        BaseOptions(
          connectTimeout: timeout,
          receiveTimeout: timeout,
          contentType: Headers.jsonContentType,
          // Решаем по коду сами: 400 от панели это осмысленный ответ
          // «приглашение не подошло», а не сетевой сбой.
          validateStatus: (s) => s != null && s < 500,
        ),
      );

  Response<dynamic>? res;
  try {
    res = await dio.post<dynamic>(
      '$origin/api/v2/app/enroll/redeem',
      data: {'code': code},
    );
  } on DioException catch (e) {
    // Ответ ЕСТЬ, но Dio всё равно бросил: панель отдаёт ошибки простым
    // текстом (`(StatusCode, "msg")`), и разбор такого тела как JSON падает
    // уже после того, как статус получен. Без этой ветки честное «приглашение
    // не подошло» превращалось бы в «нет связи», то есть человека отправляли бы
    // чинить сеть вместо того, чтобы просить новую ссылку.
    res = e.response;
    if (res == null) {
      throw ConnectRedeemException(
        'Не удалось связаться с панелью ${Uri.parse(origin).host}. '
        'Проверьте связь и повторите. (${e.type.name})',
      );
    }
  }

  final status = res.statusCode ?? 0;
  if (status != 200) {
    throw ConnectRedeemException(
      status == 400
          ? 'Приглашение не подошло: оно уже использовано, просрочено или '
                'отозвано. Запросите новую ссылку у бота панели.'
          : 'Панель ответила ошибкой ${status == 0 ? 'без кода' : status}.',
      statusCode: status,
    );
  }

  final data = res.data;
  if (data is! Map) {
    throw const ConnectRedeemException(
      'Панель ответила не тем форматом. Обновите приложение или сообщите '
      'оператору.',
    );
  }
  final map = data.map((k, v) => MapEntry(k.toString(), v));
  if (map['access_token'] is! String || map['refresh_token'] is! String) {
    throw const ConnectRedeemException(
      'Панель не выдала токены сессии. Погашение не состоялось.',
    );
  }

  return ConnectRedeemResult(
    tokens: AuthTokens.fromJson(map),
    panelName: _text(map['panel_name']) ?? '',
    subscriptionUrl: _text(map['subscription_url']),
    subscriptionUuid: _text(map['subscription_uuid']),
    subscriptionStatus: _text(map['subscription_status']),
    subscriptionReason: _text(map['subscription_reason']),
  );
}

/// Непустая строка или `null`. Панель шлёт `null` там, где значения нет, и
/// пустая строка должна читаться так же, а не превращаться в пустую подпись.
String? _text(Object? value) {
  if (value is! String) return null;
  final trimmed = value.trim();
  return trimmed.isEmpty ? null : trimmed;
}

/// Загрузка тела подписки по внешней ссылке.
///
/// Ядро (`subimport.Import`) умеет только парсить байты — своего HTTP-клиента у
/// него нет, поэтому ссылку тянет приложение и передаёт вниз уже текст. Клиент
/// здесь отдельный от [ApiClient]: подписка это произвольный внешний URL, ей не
/// нужны ни baseUrl панели, ни заголовок Authorization (и отправлять туда JWT
/// было бы утечкой).
library;

import 'package:dio/dio.dart';

import 'package:caramba_client/data/safe_url.dart';

/// Сколько переходов подряд принимается при выборке подписки.
///
/// Предел существует не ради вежливости: цепочка переходов, за которой никто
/// не следит, это способ увести выборку куда угодно и заодно бесплатный цикл
/// для того, кто ей управляет.
const int kSubscriptionMaxRedirects = 5;

/// Ошибка загрузки подписки с текстом для inline-показа.
///
/// Кроме текста несёт САМ ОТВЕТ. Раньше от отказа оставалось одно число в
/// строке, и «оператор больше не выдаёт конфигурацию по исчерпанной дневной
/// норме» было неотличимо от «сеть отвалилась»: причину панель печатает
/// заголовками `x-caramba-*` и `subscription-userinfo`, а их выбрасывали
/// вместе с ответом. Разбирает их [refusalFromResponse].
class SubscriptionFetchException implements Exception {
  final String message;

  /// Код ответа. `null` — ответа не было вовсе (сеть, таймаут, схема).
  final int? statusCode;

  /// Тело ответа как пришло.
  final String body;

  /// Заголовки ответа; имена приведены к нижнему регистру.
  final Map<String, String> headers;

  const SubscriptionFetchException(
    this.message, {
    this.statusCode,
    this.body = '',
    this.headers = const <String, String>{},
  });

  @override
  String toString() => 'SubscriptionFetchException: $message';
}

/// Скачивает тело подписки. Бросает [SubscriptionFetchException] на сетевой
/// ошибке, не-2xx ответе или пустом теле.
Future<String> fetchSubscriptionBody(String url, {Dio? client}) async {
  // INV-8 действует и здесь. Generic-режим ходит своим Dio, а не лестницей в
  // Go, и это записано как факт в 02-SPEC.md 8.9: SPKI пинов, бюджета
  // соединения и истории попыток у этого пути нет. Единственное правило,
  // которое НЕ имеет права разъезжаться между двумя путями, это схема:
  // http:// отвергается для любой выборки конфигурации, и `.onion` это
  // единственное исключение без TLS (02-SPEC.md 8.10).
  final safe = csmSafeExternalUri(url);
  if (safe == null) {
    throw const SubscriptionFetchException(
      'нужен https-адрес подписки (http допустим только для .onion)',
    );
  }
  final dio =
      client ??
      Dio(
        BaseOptions(
          connectTimeout: const Duration(seconds: 15),
          receiveTimeout: const Duration(seconds: 20),
          responseType: ResponseType.plain,
          // Переходы НЕ следуются автоматически. csmSafeExternalUri проверил
          // ровно тот адрес, который ввёл пользователь, а HttpClient идёт по
          // 30x с https на http молча: INV-8 держался бы для введённого
          // адреса и не держался для того, который на самом деле выбирается.
          // Каждый переход проверяется тем же правилом, ниже.
          followRedirects: false,
          validateStatus: (code) => code != null && code > 0,
        ),
      );
  try {
    var target = safe;
    Response<String>? res;
    for (var hop = 0; hop <= kSubscriptionMaxRedirects; hop++) {
      res = await dio.get<String>(target.toString());
      final status = res.statusCode ?? 0;
      if (status < 300 || status > 399) {
        break;
      }
      if (hop == kSubscriptionMaxRedirects) {
        throw const SubscriptionFetchException('слишком много переходов');
      }
      final location = res.headers.value('location');
      if (location == null || location.isEmpty) {
        throw const SubscriptionFetchException('переход без адреса');
      }
      final next = csmSafeExternalUri(target.resolve(location).toString());
      if (next == null) {
        // Точно то же правило, что и для введённого адреса: http:// не
        // принимается ни на первом шаге, ни на пятом.
        throw const SubscriptionFetchException(
          'переход ведёт на адрес без https (http допустим только для .onion)',
        );
      }
      target = next;
    }
    final body = res?.data?.trim() ?? '';
    final code = res?.statusCode;
    if (code == null || code < 200 || code >= 300) {
      throw SubscriptionFetchException(
        'ответ сервера $code',
        statusCode: code,
        body: res?.data ?? '',
        headers: _headersOf(res),
      );
    }
    if (body.isEmpty) {
      throw const SubscriptionFetchException('пустой ответ');
    }
    return body;
  } on DioException catch (e) {
    throw SubscriptionFetchException(e.message ?? 'сеть недоступна');
  } finally {
    if (client == null) dio.close();
  }
}

/// Заголовки ответа плоской картой с именами в нижнем регистре. Повторяющиеся
/// имена склеиваются через запятую — по правилу HTTP для списочных значений.
Map<String, String> _headersOf(Response<String>? res) {
  final map = res?.headers.map;
  if (map == null || map.isEmpty) return const <String, String>{};
  return <String, String>{
    for (final e in map.entries) e.key.toLowerCase(): e.value.join(', '),
  };
}

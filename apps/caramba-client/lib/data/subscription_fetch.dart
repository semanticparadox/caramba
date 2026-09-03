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

/// Ошибка загрузки подписки с текстом для inline-показа.
class SubscriptionFetchException implements Exception {
  final String message;
  const SubscriptionFetchException(this.message);
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
          followRedirects: true,
        ),
      );
  try {
    final res = await dio.get<String>(safe.toString());
    final body = res.data?.trim() ?? '';
    final code = res.statusCode;
    if (code == null || code < 200 || code >= 300) {
      throw SubscriptionFetchException('ответ сервера $code');
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

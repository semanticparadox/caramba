import 'package:dio/dio.dart';

import 'package:caramba_client/data/models/branding.dart';

/// Распознавание панели Caramba по ссылке подписки.
///
/// Пользователь приносит обычную ссылку подписки, как в любой другой клиент.
/// Если её отдаёт панель Caramba, приложение может предложить подключить эту
/// панель и получить то, чего в самой подписке нет: смену страны, релэя и
/// протокола, тариф, устройства. Без этого шага человек вручную ищет код
/// приглашения, хотя ссылка уже говорит, где панель.
///
/// Проверка идёт по ПУБЛИЧНОМУ `/api/v2/app/branding`: он есть на каждой
/// панели, не требует токена и не раскрывает ничего о владельце подписки.
/// Ошибка, таймаут и любой не-Caramba хост означают просто «панели нет»:
/// импорт обычной подписки не должен зависеть от этого запроса.
class PanelProbeResult {
  /// Origin панели (`https://host[:port]`).
  final String origin;

  /// Брендинг, как его отдала панель. Имя может быть пустым.
  final Branding branding;

  const PanelProbeResult({required this.origin, required this.branding});

  /// Имя для показа пользователю: бренд оператора либо просто хост.
  String get displayName {
    final name = branding.brandName.trim();
    if (name.isNotEmpty) return name;
    return Uri.parse(origin).host;
  }
}

/// Origin ссылки подписки, пригодный для запроса к панели. `null`, если схема
/// не http(s) или хоста нет (сырой конфиг, файл, произвольный текст).
String? panelOriginOf(String subscriptionUrl) {
  final uri = Uri.tryParse(subscriptionUrl.trim());
  if (uri == null || !uri.hasScheme || uri.host.isEmpty) return null;
  final scheme = uri.scheme.toLowerCase();
  if (scheme != 'https' && scheme != 'http') return null;
  final port = uri.hasPort ? ':${uri.port}' : '';
  return '$scheme://${uri.host}$port';
}

/// Проверяет, стоит ли за ссылкой подписки панель Caramba.
///
/// Возвращает `null`, если нет. Никогда не бросает: это подсказка, а не путь.
Future<PanelProbeResult?> probeCarambaPanel(
  String subscriptionUrl, {
  Dio? client,
  Duration timeout = const Duration(seconds: 6),
}) async {
  final origin = panelOriginOf(subscriptionUrl);
  if (origin == null) return null;

  final dio =
      client ??
      Dio(
        BaseOptions(
          connectTimeout: timeout,
          receiveTimeout: timeout,
          validateStatus: (s) => s != null && s < 500,
        ),
      );
  try {
    final res = await dio.get<dynamic>('$origin/api/v2/app/branding');
    if (res.statusCode != 200) return null;
    final data = res.data;
    if (data is! Map) return null;
    final map = data.map((k, v) => MapEntry(k.toString(), v));
    // Панель отвечает объектом брендинга. Чужой сервер может вернуть 200 с
    // произвольным JSON, поэтому требуем хотя бы одно поле контракта.
    const marker = ['brand_name', 'bot_url', 'support_url', 'upstream_ads'];
    if (!marker.any(map.containsKey)) return null;
    return PanelProbeResult(origin: origin, branding: Branding.fromJson(map));
  } catch (_) {
    return null;
  }
}

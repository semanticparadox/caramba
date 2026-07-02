import 'dart:async';

import 'package:app_links/app_links.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/enrollment.dart';
import 'package:caramba_client/router/routes.dart';

/// Intake входящих deeplink'ов (P2, contract A).
///
/// Слушает custom-scheme URI `carambaconnect://enroll?panel=<https>&code=<code>`
/// через [AppLinks]: и холодный старт (приложение запущено ссылкой), и тёплый
/// (уже работает). На совпадение схемы/действия — навигация на [AppRoute.enroll]
/// с проброшенными query-параметрами; экран энроллмента заводит профиль панели,
/// валидирует код и ведёт в register/login. Account ALWAYS required.
///
/// Регистрация схемы — платформенная (Android intent-filter, iOS/macOS
/// CFBundleURLTypes, Windows/Linux протокол). Без неё OS не доставит ссылку;
/// сам разбор и навигация — здесь.
class DeepLinkHandler {
  final GoRouter _router;
  final AppLinks _appLinks;
  StreamSubscription<Uri>? _sub;

  DeepLinkHandler(this._router, {AppLinks? appLinks})
    : _appLinks = appLinks ?? AppLinks();

  /// Подписывается на поток ссылок и обрабатывает ту, что запустила приложение.
  Future<void> start() async {
    _sub = _appLinks.uriLinkStream.listen(
      _handle,
      onError: (_) {
        // Сбойный URI игнорируем: deeplink — best-effort вход, не критичный путь.
      },
    );
    try {
      final initial = await _appLinks.getInitialLink();
      if (initial != null) _handle(initial);
    } catch (_) {
      // Нет начальной ссылки или платформа не поддерживает — не падаем.
    }
  }

  void _handle(Uri uri) {
    final link = EnrollLink.tryParse(uri.toString());
    if (link == null) return;
    _router.go(
      Uri(
        path: AppRoute.enroll,
        queryParameters: {'panel': link.panelUrl, 'code': link.code},
      ).toString(),
    );
  }

  void dispose() {
    _sub?.cancel();
    _sub = null;
  }
}

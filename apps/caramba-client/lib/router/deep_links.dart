import 'dart:async';

import 'package:app_links/app_links.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/enrollment.dart';
import 'package:caramba_client/router/routes.dart';

/// Intake входящих deeplink'ов (P2, contract A).
///
/// Слушает custom-scheme URI через [AppLinks]: и холодный старт (приложение
/// запущено ссылкой), и тёплый (уже работает). Поддерживаются два действия:
///   * `carambaconnect://enroll?panel=<https>&code=<code>` — навигация на
///     [AppRoute.enroll]; экран заводит профиль панели, валидирует код и ведёт
///     в register/login (аккаунт обязателен);
///   * `carambaconnect://import?url=<encoded sub url>` — навигация на
///     [AppRoute.connectionImport] с подставленной ссылкой подписки
///     (generic-режим, панель не нужна).
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
    final raw = uri.toString();
    final enroll = EnrollLink.tryParse(raw);
    if (enroll != null) {
      _router.go(
        Uri(
          path: AppRoute.enroll,
          queryParameters: {'panel': enroll.panelUrl, 'code': enroll.code},
        ).toString(),
      );
      return;
    }
    final import = ImportLink.tryParse(raw);
    if (import != null) {
      _router.go(
        Uri(
          path: AppRoute.connectionImport,
          queryParameters: {'url': import.url},
        ).toString(),
      );
    }
  }

  void dispose() {
    _sub?.cancel();
    _sub = null;
  }
}

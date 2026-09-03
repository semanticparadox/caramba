import 'dart:async';

import 'package:app_links/app_links.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_enrollment.dart';
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
///
/// ГОНКА ХОЛОДНОГО СТАРТА. Ссылка приходит раньше, чем роутер готов её принять:
/// пока локальные настройки и профили не прочитаны, гейт навигации держит сплеш
/// и уводит на `/login`, и переход по ссылке просто теряется — на эмуляторе это
/// выглядело как «`carambaconnect://import` открывает /login». Поэтому цель
/// последней ссылки запоминается ([pendingLocation]) и повторяется вызовом
/// [replayPending], когда роутер сообщает, что гейт открыт.
class DeepLinkHandler {
  final GoRouter _router;
  final AppLinks _appLinks;

  /// Вызывается, когда пришла ссылка импорта подписки: приложение обязано
  /// включить generic-режим, иначе пользователь без аккаунта панели упрётся в
  /// `/login` вместо экрана импорта.
  final void Function()? _onImport;

  /// Вызывается, когда ссылка НАША по схеме и действию, но отвергнута: без TLS
  /// (INV-8), без кода, с неразбираемым адресом. Чужие ссылки сюда не попадают.
  final void Function(LinkRefusal refusal)? _onRefused;

  StreamSubscription<Uri>? _sub;
  String? _pending;

  DeepLinkHandler(
    this._router, {
    AppLinks? appLinks,
    void Function()? onImport,
    void Function(LinkRefusal refusal)? onRefused,
  }) : _appLinks = appLinks ?? AppLinks(),
       _onImport = onImport,
       _onRefused = onRefused;

  /// Локация роутера, на которую ведёт ссылка, или `null`, если это не наш
  /// deeplink. Чистая функция: разбор ссылки проверяется без роутера.
  static String? targetOf(String raw) {
    // Разбор CSM идёт ПЕРВЫМ: он единственный читает параметр k, то есть
    // link_pin. Разобрав ту же ссылку старым парсером, приложение молча теряет
    // пин и превращает закреплённый энроллмент в незакреплённый, а это ровно
    // та разница между продиктованным вне полосы и пришедшим в приложение,
    // которую экран личности оператора показывает как свойство безопасности.
    final csm = CsmEnrollLink.tryParse(raw);
    if (csm != null) {
      final pin = csm.linkPin;
      return Uri(
        path: AppRoute.enroll,
        queryParameters: {
          'panel': csm.origin,
          'code': csm.code,
          if (pin != null) 'k': pin,
        },
      ).toString();
    }
    final enroll = EnrollLink.tryParse(raw);
    if (enroll != null) {
      return Uri(
        path: AppRoute.enroll,
        queryParameters: {'panel': enroll.panelUrl, 'code': enroll.code},
      ).toString();
    }
    final import = ImportLink.tryParse(raw);
    if (import != null) {
      return Uri(
        path: AppRoute.connectionImport,
        queryParameters: {'url': import.url},
      ).toString();
    }
    return null;
  }

  /// Подписывается на поток ссылок и обрабатывает ту, что запустила приложение.
  Future<void> start() async {
    _sub = _appLinks.uriLinkStream.listen(
      handle,
      onError: (_) {
        // Сбойный URI игнорируем: deeplink — best-effort вход, не критичный путь.
      },
    );
    try {
      final initial = await _appLinks.getInitialLink();
      if (initial != null) handle(initial);
    } catch (_) {
      // Нет начальной ссылки или платформа не поддерживает — не падаем.
    }
  }

  /// Обрабатывает одну ссылку: запоминает цель и ведёт роутер.
  void handle(Uri uri) {
    final raw = uri.toString();
    final target = targetOf(raw);
    if (target == null) {
      final refusal = refusalOf(raw);
      if (refusal != null) _onRefused?.call(refusal);
      return;
    }
    if (target.startsWith(AppRoute.connectionImport)) _onImport?.call();
    _pending = target;
    _router.go(target);
  }

  /// Причина отказа для ссылки, которую мы узнали по схеме и действию, но не
  /// приняли. `null`, если ссылка вообще не наша: о чужих ссылках пользователю
  /// сообщать нечего, их приложению доставлять и не должны были.
  static LinkRefusal? refusalOf(String raw) {
    for (final r in <LinkRefusal?>[
      EnrollLink.parse(raw).refusal,
      ImportLink.parse(raw).refusal,
    ]) {
      if (r != null && r != LinkRefusal.notOurLink) return r;
    }
    return null;
  }

  /// Куда ведёт последняя принятая ссылка, если её ещё не повторяли.
  String? get pendingLocation => _pending;

  /// Повторяет последнюю ссылку, если гейт роутера успел увести с неё.
  /// Одноразово: цель гасится, чтобы поздний вызов не выдёргивал пользователя
  /// из экрана, куда он ушёл сам.
  void replayPending() {
    final target = _pending;
    _pending = null;
    if (target == null) return;
    if (_currentPath() == Uri.parse(target).path) return;
    _router.go(target);
  }

  /// Путь, на котором роутер стоит сейчас. До первой навигации конфигурация
  /// пуста — тогда путь пустой, и повтор точно нужен.
  String _currentPath() {
    final matches = _router.routerDelegate.currentConfiguration;
    return matches.isEmpty ? '' : matches.uri.path;
  }

  void dispose() {
    _sub?.cancel();
    _sub = null;
  }
}

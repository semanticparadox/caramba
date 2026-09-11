/// Накладные маршруты на десктопе: панель справа или диалог по центру
/// (DESKTOP-SPEC.md, раздел 3).
///
/// ЗАЧЕМ. Таблица маршрутов одна на все платформы, и трогать её ради десктопа
/// нельзя: `AppRoute.overlays`, `CarambaRouter.go/opensOverStack` и стек
/// «Назад» уже проверены тестами. Меняется только ПРЕДСТАВЛЕНИЕ: тот же
/// маршрут на мобильном остаётся полноэкранной страницей, а на десктопе
/// ложится поверх шелла непрозрачной страницей, из-под которой видно
/// приложение. Поэтому развилка живёт в одной функции `desktopOverlayPage`, и
/// на мобильном она обязана отдавать РОВНО то же, что отдал бы `builder:` у
/// `GoRoute` — `MaterialPage` с тем же ключом и именем.
library;

import 'package:flutter/material.dart';

import 'package:caramba_client/desktop/desktop_panel.dart';
import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/theme/colors.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';

/// Как накладной маршрут показан на десктопе.
enum DesktopPresentation {
  /// Панель 720 справа: таблицы, списки, экраны панели оператора.
  panelWide,

  /// Панель 560 справа: формы в одну колонку.
  panelNarrow,

  /// Центрированный диалог: вход, энроллмент, ссылка, автонастройка.
  dialog,
}

/// Страница накладного маршрута.
///
/// На мобильном — обычная `MaterialPage`: ни анимации, ни скрима, ни рамки,
/// чтобы поведение мобильного стека осталось ровно прежним. На десктопе —
/// непрозрачная страница со скримом, панелью или диалогом.
Page<T> desktopOverlayPage<T>({
  required LocalKey key,
  required Widget child,
  DesktopPresentation presentation = DesktopPresentation.panelNarrow,
  bool dismissible = true,
  String? name,
}) {
  if (!isDesktopPlatform) {
    return MaterialPage<T>(key: key, name: name, child: child);
  }
  return _DesktopOverlayPage<T>(
    key: key,
    name: name,
    presentation: presentation,
    dismissible: dismissible,
    child: child,
  );
}

/// Каким представлением открывается [path].
///
/// Таблица закрыта «вниз» так же, как `AppRoute.overlays`: подмаршрут
/// наследует представление родителя (`/connections/import` — та же широкая
/// панель, `/tickets/12` — тоже).
DesktopPresentation presentationFor(String path) {
  final normalized = _normalize(path);
  if (_matches(normalized, _dialogRoots)) return DesktopPresentation.dialog;
  if (_matches(normalized, _panelWideRoots)) {
    return DesktopPresentation.panelWide;
  }
  return DesktopPresentation.panelNarrow;
}

/// Закрывается ли [path] по скриму и Esc.
///
/// Три исключения — экраны, с которых уход по случайному клику стоит
/// пользователю данных: подтверждение ссылки подключения, энроллмент и
/// автонастройка, которая в этот момент правит конфиг ядра.
bool dismissibleFor(String path) => !_matches(_normalize(path), _modalRoots);

/// Широкая панель: всё, где внутри таблица, список или длинная форма.
const Set<String> _panelWideRoots = <String>{
  AppRoute.servers,
  AppRoute.connections,
  AppRoute.siteRules,
  AppRoute.appRules,
  AppRoute.protocol,
  AppRoute.relay,
  // Ветка проверки CSM целиком: /csm/operator, /csm/documents, /csm/transport,
  // /csm/disclosure.
  '/csm',
  AppRoute.tickets,
  AppRoute.notifications,
  AppRoute.plans,
  AppRoute.partner,
  AppRoute.referrals,
};

const Set<String> _dialogRoots = <String>{
  AppRoute.login,
  AppRoute.enroll,
  AppRoute.connect,
  AppRoute.settingsAutotune,
};

const Set<String> _modalRoots = <String>{
  AppRoute.connect,
  AppRoute.enroll,
  AppRoute.settingsAutotune,
};

/// Query и фрагмент несут параметры экрана (`?link=`, `?panel=`), а не другой
/// маршрут; хвостовой слэш тоже не меняет маршрут.
String _normalize(String path) {
  var out = path;
  final cut = out.indexOf(_tail);
  if (cut >= 0) out = out.substring(0, cut);
  if (out.length > 1 && out.endsWith('/')) {
    out = out.substring(0, out.length - 1);
  }
  return out;
}

/// Начало «хвоста» пути: query или фрагмент.
final RegExp _tail = RegExp(r'[?#]');

bool _matches(String path, Set<String> roots) =>
    roots.any((root) => path == root || path.startsWith('$root/'));

class _DesktopOverlayPage<T> extends Page<T> {
  const _DesktopOverlayPage({
    required LocalKey super.key,
    required this.child,
    required this.presentation,
    required this.dismissible,
    super.name,
  });

  final Widget child;
  final DesktopPresentation presentation;
  final bool dismissible;

  bool get _isDialog => presentation == DesktopPresentation.dialog;

  @override
  Route<T> createRoute(BuildContext context) {
    // Скрим берём из темы навигатора: маршрут строится вне дерева страницы, и
    // `context.c` внутри pageBuilder дал бы тот же цвет, но уже после первого
    // кадра барьера. Тема выше навигатора есть всегда, кроме голых тестов —
    // там падаем на тёмную палитру, а не на прозрачный барьер.
    final scrim =
        Theme.of(context).extension<AppTokens>()?.colors.overlayScrim ??
        AppColors.dark.overlayScrim;
    return PageRouteBuilder<T>(
      settings: this,
      // Главное свойство всей страницы: приложение под панелью остаётся
      // видимым и живым, а не подменяется полноэкранным листом.
      opaque: false,
      barrierColor: scrim,
      barrierDismissible: dismissible,
      barrierLabel: 'Закрыть',
      transitionDuration: _isDialog
          ? DesktopTokens.dialogFade
          : DesktopTokens.panelSlide,
      reverseTransitionDuration: AppMotion.micro,
      pageBuilder: (context, animation, secondaryAnimation) => _isDialog
          ? DesktopDialogFrame(child: child)
          : DesktopPanelFrame(
              width: presentation == DesktopPresentation.panelWide
                  ? DesktopTokens.panelWide
                  : DesktopTokens.panelNarrow,
              child: child,
            ),
      transitionsBuilder: (context, animation, secondaryAnimation, frame) {
        if (_isDialog) {
          // Только opacity: масштаб на диалоге входа дёргает поля ввода.
          return FadeTransition(
            opacity: animation.drive(CurveTween(curve: AppMotion.enter)),
            child: frame,
          );
        }
        // Только transform: слайд не трогает раскладку, поэтому панель не
        // пересобирается на каждом кадре.
        return SlideTransition(
          position: animation.drive(
            Tween<Offset>(
              begin: const Offset(1, 0),
              end: Offset.zero,
            ).chain(CurveTween(curve: AppMotion.enter)),
          ),
          child: frame,
        );
      },
    );
  }
}

// Гейт навигации при обязательном обновлении.
//
// Панель требует сборку новее установленной: экран «Нужно обновиться»
// перекрывает приложение, любая другая локация с него не уводит. Когда
// требование снято, с него уводят в приложение. Проверяется чистая
// [resolveRedirect] — без GoRouter и сети, как в router_redirect_test.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/router/app_router.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';

String? redirect({
  required String location,
  AuthStage stage = AuthStage.authenticated,
  bool updateRequired = false,
  bool bootReady = true,
}) => resolveRedirect(
  stage: stage,
  firstRun: false,
  bootReady: bootReady,
  profilesReady: true,
  guest: false,
  location: location,
  updateRequired: updateRequired,
);

void main() {
  test('требование обновиться уводит с любой локации на свой экран', () {
    for (final where in <String>[
      AppRoute.splash,
      AppRoute.home,
      AppRoute.settings,
      AppRoute.updates,
      AppRoute.login,
      '${AppRoute.connectionImport}?url=https://x',
    ]) {
      expect(
        redirect(location: where, updateRequired: true),
        AppRoute.updateRequired,
        reason: where,
      );
    }
    // Сильнее сплеша: даже пока настройки не прочитаны.
    expect(
      redirect(
        location: AppRoute.splash,
        updateRequired: true,
        bootReady: false,
        stage: AuthStage.unknown,
      ),
      AppRoute.updateRequired,
    );
  });

  test('на своём экране требование держит, а не крутит редирект', () {
    expect(
      redirect(location: AppRoute.updateRequired, updateRequired: true),
      isNull,
    );
    expect(
      redirect(
        location: AppRoute.updateRequired,
        updateRequired: true,
        stage: AuthStage.unauthenticated,
      ),
      isNull,
    );
  });

  test('требование снято: с экрана уводят в приложение', () {
    expect(redirect(location: AppRoute.updateRequired), AppRoute.home);
    expect(
      redirect(
        location: AppRoute.updateRequired,
        stage: AuthStage.unauthenticated,
      ),
      AppRoute.home,
    );
  });

  test('без требования гейт ведёт себя как раньше', () {
    expect(redirect(location: AppRoute.home), isNull);
    expect(redirect(location: AppRoute.splash), AppRoute.home);
    expect(redirect(location: AppRoute.updates), isNull);
  });

  test('«Обновления» накладной, «Нужно обновиться» нет', () {
    expect(AppRoute.isOverlay(AppRoute.updates), isTrue);
    expect(AppRoute.isOverlay(AppRoute.updateRequired), isFalse);
  });
}

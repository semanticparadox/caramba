// Гейт навигации: кого и куда пускать.
//
// Проверяется чистая [resolveRedirect] — без GoRouter, платформенных каналов и
// сети. Главное свойство: generic-режим (своя подписка) работает БЕЗ аккаунта
// панели, а пользователь без подписки и без сессии по-прежнему уходит на вход.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/router/app_router.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';

String? redirect({
  required AuthStage stage,
  required String location,
  bool firstRun = false,
  bool bootReady = true,
  bool profilesReady = true,
  bool guest = false,
}) => resolveRedirect(
  stage: stage,
  firstRun: firstRun,
  bootReady: bootReady,
  profilesReady: profilesReady,
  guest: guest,
  location: location,
);

void main() {
  group('generic-режим (своя подписка, аккаунта панели нет)', () {
    test('со сплеша ведёт сразу на Home, не дожидаясь auth-пробы', () {
      expect(
        redirect(
          stage: AuthStage.unknown,
          location: AppRoute.splash,
          guest: true,
        ),
        AppRoute.home,
      );
      expect(
        redirect(
          stage: AuthStage.unauthenticated,
          location: AppRoute.splash,
          guest: true,
        ),
        AppRoute.home,
      );
    });

    test('шелл и его экраны открыты без входа', () {
      for (final loc in [
        AppRoute.home,
        AppRoute.servers,
        AppRoute.settings,
        AppRoute.connections,
        AppRoute.connectionImport,
        AppRoute.splitTunnel,
        AppRoute.protocol,
        AppRoute.profile,
      ]) {
        expect(
          redirect(
            stage: AuthStage.unauthenticated,
            location: loc,
            guest: true,
          ),
          isNull,
          reason: 'гость должен попадать на $loc',
        );
      }
    });

    test('вход и энроллмент остаются доступны по своей воле', () {
      expect(
        redirect(
          stage: AuthStage.unauthenticated,
          location: AppRoute.login,
          guest: true,
        ),
        isNull,
      );
      expect(
        redirect(
          stage: AuthStage.unauthenticated,
          location: AppRoute.enroll,
          guest: true,
        ),
        isNull,
      );
    });

    test('онбординг гостю не навязывается', () {
      expect(
        redirect(
          stage: AuthStage.unauthenticated,
          location: AppRoute.autotune,
          guest: true,
          firstRun: true,
        ),
        AppRoute.home,
      );
    });
  });

  group('без подписки и без сессии', () {
    test('любой экран уводит на вход', () {
      expect(
        redirect(stage: AuthStage.unauthenticated, location: AppRoute.home),
        AppRoute.login,
      );
      expect(
        redirect(stage: AuthStage.unauthenticated, location: AppRoute.splash),
        AppRoute.login,
      );
      expect(
        redirect(
          stage: AuthStage.unauthenticated,
          location: AppRoute.connections,
        ),
        AppRoute.login,
      );
    });

    test('на самом входе редиректа нет', () {
      expect(
        redirect(stage: AuthStage.unauthenticated, location: AppRoute.login),
        isNull,
      );
    });

    test('энроллмент по deeplink доступен до входа', () {
      expect(
        redirect(stage: AuthStage.unauthenticated, location: AppRoute.enroll),
        isNull,
      );
      expect(
        redirect(stage: AuthStage.unknown, location: AppRoute.enroll),
        isNull,
      );
    });

    test('пока сессия резолвится, держим сплеш', () {
      expect(
        redirect(stage: AuthStage.unknown, location: AppRoute.splash),
        isNull,
      );
      expect(
        redirect(stage: AuthStage.unknown, location: AppRoute.home),
        AppRoute.splash,
      );
    });
  });

  group('локальное состояние ещё грузится', () {
    test('решение откладывается на сплеше, а не принимается по дефолтам', () {
      // Профили лежат в secure storage: без них гость выглядел бы как чужой и
      // отскакивал бы на /login прямо на холодном старте.
      expect(
        redirect(
          stage: AuthStage.unauthenticated,
          location: AppRoute.home,
          profilesReady: false,
        ),
        AppRoute.splash,
      );
      expect(
        redirect(
          stage: AuthStage.unauthenticated,
          location: AppRoute.splash,
          bootReady: false,
        ),
        isNull,
      );
    });

    test('залогиненного не уводим в онбординг до чтения настроек', () {
      expect(
        redirect(
          stage: AuthStage.authenticated,
          location: AppRoute.home,
          firstRun: true,
          bootReady: false,
        ),
        AppRoute.splash,
      );
    });
  });

  group('залогиненный пользователь', () {
    test('первый вход ведёт в автоподбор', () {
      expect(
        redirect(
          stage: AuthStage.authenticated,
          location: AppRoute.home,
          firstRun: true,
        ),
        AppRoute.autotune,
      );
      expect(
        redirect(
          stage: AuthStage.authenticated,
          location: AppRoute.autotune,
          firstRun: true,
        ),
        isNull,
      );
    });

    test('уводится из pre-auth экранов, включая энроллмент', () {
      for (final loc in [
        AppRoute.splash,
        AppRoute.login,
        AppRoute.autotune,
        AppRoute.enroll,
      ]) {
        expect(
          redirect(stage: AuthStage.authenticated, location: loc),
          AppRoute.home,
          reason: 'после входа $loc должен уводить на Home',
        );
      }
    });

    test('внутри приложения не трогаем', () {
      expect(
        redirect(stage: AuthStage.authenticated, location: AppRoute.servers),
        isNull,
      );
    });
  });
}

// Вкладки шелла и то, что лежит поверх него.
//
// Подписи нижней навигации и ветки [StatefulShellRoute] — два конца одного
// списка, но живут в разных файлах: подписи в шелле, ветки в таблице
// маршрутов. Разъедутся они молча — человек нажмёт «Настройки» и попадёт в
// профиль. Тест сверяет оба конца по числу и порядку, а заодно фиксирует, что
// серверы, подтверждение ссылки и энроллмент открываются НАД шеллом, а не
// вместо него.

import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/router/app_router.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/shell/app_shell.dart';

/// Пути веток шелла в порядке объявления.
List<String> _branchPaths() {
  final shell = appRoutes().whereType<StatefulShellRoute>().single;
  return shell.branches
      .map((b) => (b.routes.first as GoRoute).path)
      .toList(growable: false);
}

void main() {
  test('в шелле три ветки: Подключение, Профиль, Настройки', () {
    expect(_branchPaths(), <String>[
      AppRoute.home,
      AppRoute.profile,
      AppRoute.settings,
    ]);
    // Серверы вкладкой быть перестали: экран открывают строкой «Сервер».
    expect(_branchPaths(), isNot(contains(AppRoute.servers)));
  });

  test('подписи вкладок совпадают с ветками по числу и порядку', () {
    expect(kShellDestinations.map((d) => d.label).toList(), <String>[
      'Подключение',
      'Профиль',
      'Настройки',
    ]);
    expect(kShellDestinations.length, _branchPaths().length);
  });

  test('серверы, подтверждение ссылки и энроллмент ложатся поверх шелла', () {
    expect(AppRoute.isOverlay(AppRoute.servers), isTrue);
    expect(AppRoute.isOverlay(AppRoute.connect), isTrue);
    // Ссылка приезжает query-строкой, и это тот же маршрут.
    expect(AppRoute.isOverlay('${AppRoute.connect}?link=x'), isTrue);
    expect(AppRoute.isOverlay(AppRoute.enroll), isTrue);

    // Сплеш и первый автоподбор заменяют приложение, а не лежат на нём.
    expect(AppRoute.isOverlay(AppRoute.splash), isFalse);
    expect(AppRoute.isOverlay(AppRoute.autotune), isFalse);

    // `/connections` накладной сам по себе, а не потому, что начинается на
    // `/connect`: сравнение идёт по полному совпадению либо по границе
    // сегмента. Проверяем на пути, которого в списке нет вовсе.
    expect(AppRoute.isOverlay(AppRoute.connections), isTrue);
    expect(AppRoute.isOverlay('${AppRoute.connect}ions-of-mine'), isFalse);
  });
}

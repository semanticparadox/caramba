/// Раздельное туннелирование по приложениям на Android: инварианты моста,
/// которые нечем проверить из Dart иначе.
///
/// ЗАЧЕМ ЧТЕНИЕ ИСХОДНИКОВ. Решение «кого пустить в туннель» принимается на
/// устройстве, в `VpnService.Builder`, внутри сервиса, который из теста не
/// запустить: в модуле плагина нет инфраструктуры Kotlin-тестов (android/build.gradle
/// не подключает ни junit, ни robolectric, и заводить их ради одной функции
/// значило бы принести в сборку клиента вторую тестовую цепочку). Ошибка при
/// этом НЕ ВИДНА: список приложений так и уедет в ядро, где правила
/// `PROCESS-NAME` на Android намеренно выключены (движок гасит поиск процесса,
/// см. libs/caramba-core/engine/engine_mihomo.go), и снаружи это выглядит как
/// «правила не работают», а не как отсутствие кода.
///
/// Поэтому тест сторожит три стыка:
///   1. у метода канала `listInstalledApps` есть нативная ветка;
///   2. сервис ПОЛУЧАЕТ политику в buildInterface и применяет оба списка;
///   3. манифест приложения объявляет `<queries>` с MAIN/LAUNCHER — без него
///      PackageManager вернёт почти пустой список на Android 11+.
///
/// РУЧНАЯ ПРОВЕРКА, которую этот тест не заменяет (нужен телефон/эмулятор):
/// выбрать приложение в режиме «кроме списка», поднять туннель и убедиться,
/// что у выбранного приложения свой IP, а у остальных — узла; затем режим
/// «только список» и обратная картина; затем удалить выбранное приложение и
/// поднять туннель снова — он обязан подняться.
library;

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  final root = _packageRoot().path;

  String read(String rel) => File('$root/$rel').readAsStringSync();

  const pluginPath =
      'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnPlugin.kt';
  const servicePath =
      'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnService.kt';
  const contractPath =
      'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnContract.kt';

  test('у listInstalledApps есть ветка в Kotlin-диспетчере', () {
    expect(
      read(pluginPath),
      contains('"listInstalledApps" ->'),
      reason:
          'Dart зовёт listInstalledApps; без ветки платформа ответит '
          'notImplemented, а экран выбора покажет пустой список — то есть '
          'отсутствие моста будет неотличимо от телефона без приложений.',
    );
  });

  test('перечисление идёт по MAIN/LAUNCHER и без своего пакета', () {
    final source = read(pluginPath);
    expect(source, contains('Intent.CATEGORY_LAUNCHER'));
    expect(source, contains('queryIntentActivities'));
    expect(
      source,
      contains('if (pkg == selfPackage) continue'),
      reason: 'своё приложение и так всегда вне туннеля — выбирать его нечего',
    );
  });

  test('сервис получает политику и применяет оба списка', () {
    final source = read(servicePath);
    expect(
      source,
      contains('buildInterface(serverName, seam.policyJson)'),
      reason:
          'без политики на входе Builder нечем узнать выбор человека — именно '
          'так список приложений и оставался украшением',
    );
    expect(source, contains('CarambaSplitPlan.fromPolicyJson(policyJson)'));
    expect(source, contains('builder.addAllowedApplication('));
    expect(source, contains('builder.addDisallowedApplication('));
    expect(
      source,
      contains('PackageManager.NameNotFoundException'),
      reason:
          'удалённый после выбора пакет обязан быть пропущен поимённо: без '
          'своего try на каждый пакет один протухший пакет роняет весь туннель',
    );
  });

  test('решение о списках вынесено в чистую функцию', () {
    final source = read(contractPath);
    expect(source, contains('internal data class CarambaSplitPlan'));
    expect(source, contains('fun fromPolicyJson('));
    expect(
      source,
      contains('fun allowedApps('),
      reason:
          'allow и disallow нельзя смешивать на одном Builder — разделение '
          'режимов обязано жить в одном месте, а не растекаться по сервису',
    );
  });

  final manifest = File('$root/../../android/app/src/main/AndroidManifest.xml');
  test(
    'манифест приложения объявляет <queries> с MAIN/LAUNCHER',
    () {
      final source = manifest.readAsStringSync();
      final queries = source.substring(source.indexOf('<queries>'));
      expect(
        queries,
        contains('android.intent.category.LAUNCHER'),
        reason:
            'с Android 11 чужие пакеты невидимы без объявления, и экран выбора '
            'окажется пустым на любом современном телефоне',
      );
      expect(
        RegExp(r'<uses-permission[^>]*QUERY_ALL_PACKAGES').hasMatch(source),
        isFalse,
        reason:
            'разрешение на полный список пакетов требует отдельного '
            'обоснования в Google Play review; решение было — объявить '
            '<queries> с запускаемыми, а не просить разрешение',
      );
    },
    // Плагин может быть вычитан отдельно от приложения-хоста; тогда сторожить
    // тут нечего, и молчаливый пропуск честнее выдуманного провала.
    skip: manifest.existsSync()
        ? false
        : 'манифест приложения-хоста не найден рядом с пакетом',
  );
}

/// Корень пакета caramba_vpn, откуда бы ни запустили тест.
Directory _packageRoot() {
  const marker =
      'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnPlugin.kt';
  var dir = Directory.current;
  for (var i = 0; i < 8; i++) {
    if (File('${dir.path}/$marker').existsSync()) return dir;
    final nested = Directory('${dir.path}/packages/caramba_vpn');
    if (File('${nested.path}/$marker').existsSync()) return nested;
    final parent = dir.parent;
    if (parent.path == dir.path) break;
    dir = parent;
  }
  throw StateError(
    'не найден корень пакета caramba_vpn от ${Directory.current.path}',
  );
}

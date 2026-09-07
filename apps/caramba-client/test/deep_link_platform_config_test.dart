// Встроенный диплинкинг платформы обязан быть ВЫКЛЮЧЕН на каждой платформе,
// где приложение регистрирует свои схемы.
//
// ЗАЧЕМ ЭТОТ ТЕСТ. Ссылки разбирает Dart (lib/router/deep_links.dart): только
// он знает про `caramba://connect?d=<armor>` и две старые схемы, и только он
// умеет сказать человеку, почему ссылка не подошла. Встроенный диплинкинг
// фреймворка знает лишь одно — «отдай ссылку маршрутизатору как локацию», и
// с недавних пор он включён по умолчанию (нет meta-data = true). Тогда на
// каждый VIEW-интент приходит ВТОРАЯ навигация: go_router сводит
// `caramba://connect?d=...` к пустому пути, то есть к `/`, гейт уводит со
// сплеша на `/home`, и этот переход затирает тот, что уже сделал
// DeepLinkHandler.
//
// Ломается это молча и только на ТЁПЛОМ старте: при холодном ссылку приносит
// getInitialLink, и наш переход случается последним. Именно так и выглядел
// дефект на устройстве — ссылка из бота при уже открытом приложении не делала
// ровно ничего. Проверять такое руками каждый релиз нельзя, поэтому свойство
// закреплено здесь: правка платформенных файлов без этого ключа обязана падать
// в CI, а не всплывать на телефоне.

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

/// Ключ и его значение в plist пишутся двумя строками (`<key>` и `<false/>`),
/// поэтому ищем пару, а не одну строку.
bool _plistDisables(String xml, String key) {
  final match = RegExp(
    '<key>$key</key>\\s*<(true|false)\\s*/>',
  ).firstMatch(xml);
  return match?.group(1) == 'false';
}

void main() {
  test('Android: flutter_deeplinking_enabled объявлен и равен false', () {
    final manifest = File(
      'android/app/src/main/AndroidManifest.xml',
    ).readAsStringSync();

    // Схемы обязаны остаться зарегистрированными: без них OS вообще не
    // доставит ссылку, и «выключенный диплинкинг» стал бы отговоркой.
    expect(manifest, contains('android:scheme="caramba"'));
    expect(manifest, contains('android:scheme="carambaconnect"'));

    final meta = RegExp(
      r'<meta-data\s+android:name="flutter_deeplinking_enabled"\s+'
      r'android:value="(true|false)"',
    ).firstMatch(manifest);
    expect(
      meta,
      isNotNull,
      reason:
          'Без этого meta-data embedding включает свой диплинкинг по умолчанию '
          'и затирает навигацию DeepLinkHandler на тёплом старте.',
    );
    expect(meta!.group(1), 'false');
  });

  test('iOS: FlutterDeepLinkingEnabled объявлен и равен false', () {
    final plist = File('ios/Runner/Info.plist').readAsStringSync();
    expect(plist, contains('<string>caramba</string>'));
    expect(plist, contains('<string>carambaconnect</string>'));
    expect(
      _plistDisables(plist, 'FlutterDeepLinkingEnabled'),
      isTrue,
      reason: 'На iOS значение по умолчанию тоже true.',
    );
  });

  test('macOS: FlutterDeepLinkingEnabled объявлен и равен false', () {
    final plist = File('macos/Runner/Info.plist').readAsStringSync();
    expect(plist, contains('<string>caramba</string>'));
    expect(plist, contains('<string>carambaconnect</string>'));
    expect(_plistDisables(plist, 'FlutterDeepLinkingEnabled'), isTrue);
  });
}

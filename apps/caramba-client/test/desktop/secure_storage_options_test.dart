// Хранилище секретов на macOS: связка ключей, в которую нас пускают.
//
// ЧТО ЗДЕСЬ СТОРОЖИТСЯ И ПОЧЕМУ ЭТО НЕ МЕЛОЧЬ. `flutter_secure_storage` по
// умолчанию ходит в data protection keychain, а он требует entitlement,
// который выдаётся только подписью с командой разработчика. Наша сборка
// подписана ad-hoc и живёт в песочнице, поэтому каждая запись возвращала
// `errSecMissingEntitlement` — молча, без исключения в UI. Внешне всё
// работало: импортированный профиль подписки был виден до самого выхода и
// исчезал после перезапуска (дефект D-10 ручной проверки).
//
// Отсюда два конца, которые проверяются ниже: значение выключено в общих
// настройках хранилищ И доезжает до канала обоих хранилищ (профили и токены).
// Проверка канала идёт только на macOS: на другом хосте `flutter_secure_storage`
// выбирает опции своей платформы, и требовать от него macOS-значение
// бессмысленно.

import 'dart:io' show Platform;

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/secure_storage_options.dart';
import 'package:caramba_client/data/token_store.dart';

const MethodChannel _secure = MethodChannel(
  'plugins.it_nomads.com/flutter_secure_storage',
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('на macOS data protection keychain выключен', () {
    expect(
      kSecureMacOsOptions.toMap()['useDataProtectionKeyChain'],
      'false',
      reason: 'иначе запись падает с errSecMissingEntitlement (-34018)',
    );
  });

  // Группа целиком только для macOS-хоста: `flutter_secure_storage` выбирает
  // опции по платформе, на которой ИДЁТ ТЕСТ, и на Linux-раннере проверять
  // macOS-значение нечего.
  if (!Platform.isMacOS) return;

  group('опции доезжают до канала', () {
    late List<Map<Object?, Object?>> options;

    setUp(() {
      options = <Map<Object?, Object?>>[];
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(_secure, (call) async {
            final args = call.arguments as Map<Object?, Object?>;
            options.add(args['options']! as Map<Object?, Object?>);
            return null;
          });
      addTearDown(
        () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
            .setMockMethodCallHandler(_secure, null),
      );
    });

    test('профили подписки пишутся в обычную связку ключей', () async {
      await ConnectionProfilesStore().writeProfiles(const []);

      expect(options, hasLength(1));
      expect(options.single['useDataProtectionKeyChain'], 'false');
    });

    test('сессия панели пишется туда же', () async {
      await TokenStore().clear();

      expect(options, isNotEmpty);
      for (final o in options) {
        expect(o['useDataProtectionKeyChain'], 'false');
      }
    });
  });
}

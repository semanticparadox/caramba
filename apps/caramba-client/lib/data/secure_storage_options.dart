/// Настройки платформенного secure storage — одни на все наши хранилища.
///
/// ЗАЧЕМ ОТДЕЛЬНЫЙ ФАЙЛ. Хранилищ секретов два ([TokenStore] и
/// [ConnectionProfilesStore]), а решение, КАК именно ходить в связку ключей,
/// одно. Разъехавшись, они разъедутся тихо: сессия переживает перезапуск, а
/// профили подписки нет (или наоборот), и виноватого не видно ни в одном
/// из файлов.
///
/// ЗАЧЕМ НА macOS ВЫКЛЮЧЕН DATA PROTECTION KEYCHAIN. `flutter_secure_storage`
/// по умолчанию ходит в «айфонную» связку ключей
/// (`kSecUseDataProtectionKeychain = true`). Она требует у приложения
/// entitlement `application-identifier`/`keychain-access-groups`, который
/// выдаётся только подписью с командой разработчика. Наша macOS-сборка
/// подписана ad-hoc (`CODE_SIGN_IDENTITY = "-"`) и работает в App Sandbox,
/// поэтому КАЖДАЯ запись возвращает `errSecMissingEntitlement` (-34018) —
/// проверено отдельной пробой на этой же машине с этими же entitlements.
/// Отказ приходит молча: профиль подписки живёт в памяти, выглядит
/// сохранённым, и исчезает после ⌘Q (дефект D-10 ручной проверки).
///
/// Обычная связка ключей (`false`) в песочнице работает и на запись, и на
/// чтение после перезапуска — на ней и остаёмся. Значение осмысленно и для
/// подписанной сборки: элементы остаются в login.keychain, доступ к своим
/// записям приложение получает по собственной подписи.
///
/// Android и iOS не трогаем: там значения те же, что были у обоих хранилищ.
library;

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

const AndroidOptions kSecureAndroidOptions = AndroidOptions(
  encryptedSharedPreferences: true,
);

const IOSOptions kSecureIOSOptions = IOSOptions(
  accessibility: KeychainAccessibility.first_unlock,
);

const MacOsOptions kSecureMacOsOptions = MacOsOptions(
  accessibility: KeychainAccessibility.first_unlock,
  useDataProtectionKeyChain: false,
);

/// Хранилище секретов с нашими настройками. Ровно один конструктор на
/// приложение: см. заголовок файла.
FlutterSecureStorage createSecureStorage() => const FlutterSecureStorage(
  aOptions: kSecureAndroidOptions,
  iOptions: kSecureIOSOptions,
  mOptions: kSecureMacOsOptions,
);

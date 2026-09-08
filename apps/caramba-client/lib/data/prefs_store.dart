import 'dart:convert';

import 'package:shared_preferences/shared_preferences.dart';

/// Локальное хранилище НЕсекретных настроек поверх `shared_preferences`.
///
/// Здесь живут четыре среза, которые обязаны пережить перезапуск:
///   * [kCoreConfig]  — выбор пользователя по ядру (протокол/маршрут/DNS/...);
///   * [kSettings]    — настройки приложения (тема, тумблеры);
///   * [kFirstRun]    — показывать ли онбординг (autotune) после входа;
///   * [kTunnelMode]  — способ захвата трафика (`tun` / `proxy`);
///   * [kGuestMode]   — работа без аккаунта панели (своя подписка).
///
/// Секреты (JWT, raw-конфиги подписок) сюда НЕ попадают: они остаются в
/// `TokenStore` / `ConnectionProfilesStore` (secure storage).
///
/// Значения-объекты пишутся JSON-строкой: так новые поля добавляются без
/// миграции ключей, а старая запись без поля читается со значением по
/// умолчанию.
class PrefsStore {
  static const String kCoreConfig = 'caramba.core_config';
  static const String kSettings = 'caramba.app_settings';
  static const String kFirstRun = 'caramba.first_run';
  static const String kTunnelMode = 'caramba.tunnel_mode';

  /// Пользователь работает без аккаунта панели (generic-режим, своя подписка).
  static const String kGuestMode = 'caramba.guest_mode';

  SharedPreferences? _prefs;

  /// Загружен ли бэкенд (после [load]). До этого чтения отдают дефолты, а
  /// записи молча игнорируются.
  bool get isLoaded => _prefs != null;

  /// Открывает `SharedPreferences`. Идемпотентен. Не бросает: на платформе без
  /// плагина хранилище остаётся незагруженным, приложение работает на дефолтах.
  Future<void> load() async {
    if (_prefs != null) return;
    try {
      _prefs = await SharedPreferences.getInstance();
    } catch (_) {
      _prefs = null;
    }
  }

  /// Читает JSON-объект по ключу. Пустая карта, если ключа нет или JSON битый.
  Map<String, dynamic> readJson(String key) {
    final raw = _prefs?.getString(key);
    if (raw == null || raw.isEmpty) return const <String, dynamic>{};
    try {
      final decoded = jsonDecode(raw);
      return decoded is Map<String, dynamic>
          ? decoded
          : const <String, dynamic>{};
    } on FormatException {
      return const <String, dynamic>{};
    }
  }

  /// Пишет JSON-объект по ключу.
  Future<void> writeJson(String key, Map<String, dynamic> value) async {
    await _prefs?.setString(key, jsonEncode(value));
  }

  bool readBool(String key, {required bool fallback}) =>
      _prefs?.getBool(key) ?? fallback;

  Future<void> writeBool(String key, bool value) async {
    await _prefs?.setBool(key, value);
  }

  String? readString(String key) => _prefs?.getString(key);

  Future<void> writeString(String key, String value) async {
    await _prefs?.setString(key, value);
  }

  /// Полная очистка (логаут/сброс). Профили и токены чистятся своими сторами.
  Future<void> clear() async {
    for (final key in const [
      kCoreConfig,
      kSettings,
      kFirstRun,
      kTunnelMode,
      kGuestMode,
    ]) {
      await _prefs?.remove(key);
    }
  }
}

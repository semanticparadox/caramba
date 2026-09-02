import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'package:caramba_client/data/models/connection_profile.dart';

/// Локальное хранилище профилей подключения в платформенном secure storage.
///
/// Secure storage (а не prefs) оправдан: [ConnectionProfile.rawConfig] и
/// [ConnectionProfile.accessToken] чувствительны (прокси-креды и JWT). Весь
/// список сериализуется в одну JSON-строку под одним ключом плюс отдельный ключ
/// для id активного профиля. Зеркалит стиль `token_store.dart`.
class ConnectionProfilesStore {
  static const _kProfiles = 'caramba.connection_profiles';
  static const _kActiveId = 'caramba.active_profile_id';

  final FlutterSecureStorage _storage;

  ConnectionProfilesStore({FlutterSecureStorage? storage})
    : _storage =
          storage ??
          const FlutterSecureStorage(
            aOptions: AndroidOptions(encryptedSharedPreferences: true),
            iOptions: IOSOptions(
              accessibility: KeychainAccessibility.first_unlock,
            ),
          );

  /// Читает сохранённый список профилей. Пустой список, если ничего нет или
  /// JSON повреждён (не роняем UI на битой записи).
  Future<List<ConnectionProfile>> readProfiles() async {
    final raw = await _storage.read(key: _kProfiles);
    if (raw == null || raw.isEmpty) return const [];
    try {
      final decoded = jsonDecode(raw);
      if (decoded is! List) return const [];
      return decoded
          .whereType<Map<dynamic, dynamic>>()
          .map(
            (e) => ConnectionProfile.fromJson(
              e.map((k, v) => MapEntry(k.toString(), v)),
            ),
          )
          .toList(growable: false);
    } catch (_) {
      return const [];
    }
  }

  /// Читает id активного профиля (или `null`).
  Future<String?> readActiveId() => _storage.read(key: _kActiveId);

  /// Сохраняет весь список профилей одной записью.
  Future<void> writeProfiles(List<ConnectionProfile> profiles) async {
    final encoded = jsonEncode(
      profiles.map((p) => p.toJson()).toList(growable: false),
    );
    await _storage.write(key: _kProfiles, value: encoded);
  }

  /// Сохраняет id активного профиля. `null` очищает ключ.
  Future<void> writeActiveId(String? id) async {
    if (id == null) {
      await _storage.delete(key: _kActiveId);
    } else {
      await _storage.write(key: _kActiveId, value: id);
    }
  }

  Future<void> clear() async {
    await _storage.delete(key: _kProfiles);
    await _storage.delete(key: _kActiveId);
  }
}

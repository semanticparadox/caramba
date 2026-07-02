import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'package:caramba_client/data/models/auth_tokens.dart';

/// Хранилище JWT-пары в платформенном secure storage (Keychain/Keystore/
/// libsecret/DPAPI). Единственный источник истины по токенам — всё чтение/
/// запись токенов идёт через него.
class TokenStore {
  static const _kAccess = 'caramba.access_token';
  static const _kRefresh = 'caramba.refresh_token';
  static const _kUserId = 'caramba.user_id';

  final FlutterSecureStorage _storage;

  TokenStore({FlutterSecureStorage? storage})
    : _storage =
          storage ??
          const FlutterSecureStorage(
            aOptions: AndroidOptions(encryptedSharedPreferences: true),
            iOptions: IOSOptions(
              accessibility: KeychainAccessibility.first_unlock,
            ),
          );

  Future<void> save(AuthTokens tokens) async {
    await _storage.write(key: _kAccess, value: tokens.accessToken);
    await _storage.write(key: _kRefresh, value: tokens.refreshToken);
    await _storage.write(key: _kUserId, value: tokens.userId.toString());
  }

  Future<String?> readAccess() => _storage.read(key: _kAccess);

  Future<String?> readRefresh() => _storage.read(key: _kRefresh);

  Future<int?> readUserId() async {
    final v = await _storage.read(key: _kUserId);
    return v == null ? null : int.tryParse(v);
  }

  /// Есть ли вообще сохранённая сессия (refresh-токен).
  Future<bool> hasSession() async => (await readRefresh())?.isNotEmpty ?? false;

  Future<void> clear() async {
    await _storage.delete(key: _kAccess);
    await _storage.delete(key: _kRefresh);
    await _storage.delete(key: _kUserId);
  }
}

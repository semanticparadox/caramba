import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/subscription_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';

/// Корневые DI-провайдеры: secure storage, API-клиент и VPN-движок.
/// Остальные нотифаеры (`auth`, `servers`, `subscription`, `vpn`) зависят от них.

/// Платформенное secure storage для JWT.
final tokenStoreProvider = Provider<TokenStore>((ref) => TokenStore());

/// HTTP-клиент к панели. `onSessionExpired` пробрасывает auth-нотифаеру
/// принудительный логаут, когда refresh окончательно протух.
final apiClientProvider = Provider<ApiClient>((ref) {
  final client = ApiClient(tokens: ref.watch(tokenStoreProvider));
  ref.onDispose(() => client.onSessionExpired = null);
  return client;
});

/// VPN-движок. На всех 5 платформах (Android/iOS/macOS/Windows/Linux) —
/// нативное Go-ядро (mihomo) через каналы `com.caramba/vpn`, когда включён флаг
/// `USE_NATIVE_VPN`; иначе и на web — [MockVpnConnection], чтобы UI работал
/// end-to-end без нативного бэка.
final vpnConnectionProvider = Provider<VpnConnection>((ref) {
  final VpnConnection conn = _useNativeVpn()
      ? MethodChannelVpnConnection(configResolver: () => _resolveVpnConfig(ref))
      : MockVpnConnection();
  ref.onDispose(conn.dispose);
  return conn;
});

/// Лениво собирает конфиг для авторизации Go-ядра перед поднятием туннеля
/// (путь panelAccount). Источник зависит от активного профиля подключения:
///   * rawSub          → `null` (configure не нужен, ядро поднимает импорт);
///   * panelAccount со своими полями (panelUrl/subscriptionUuid/accessToken)
///                     → берём их из профиля (мульти-аккаунт, иной тенант);
///   * профиль не задан или поля пусты → дефолтный путь тенанта-1: JWT из
///     TokenStore + UUID подписки (`GET /subscription`) + [kApiBaseUrl].
/// Возвращает `null`, если данных ещё нет — тогда `configure` пропускается и
/// нативная сторона ответит стадией `error`.
Future<VpnConfig?> _resolveVpnConfig(Ref ref) async {
  final profile = ref.read(activeConnectionProfileProvider);

  // Импортированная подписка авторизации панели не требует.
  if (profile != null && profile.isRaw) return null;

  // panelAccount, несущий собственные креды (например другой тенант): берём их
  // напрямую из профиля, минуя TokenStore тенанта-1.
  if (profile != null && profile.isPanel) {
    final panelUrl = profile.panelUrl;
    final uuid = profile.subscriptionUuid;
    final token = profile.accessToken;
    if (panelUrl != null &&
        panelUrl.isNotEmpty &&
        uuid != null &&
        uuid.isNotEmpty &&
        token != null &&
        token.isNotEmpty) {
      return VpnConfig(
        panelUrl: panelUrl,
        subscriptionUuid: uuid,
        accessToken: token,
      );
    }
    // Поля профиля неполны — падаем на дефолтный путь тенанта-1 ниже.
  }

  // Дефолтный путь тенанта-1: текущая сессия TokenStore + активная подписка.
  final access = await ref.read(tokenStoreProvider).readAccess();
  if (access == null || access.isEmpty) return null;
  final Subscription sub;
  try {
    sub = await ref.read(subscriptionProvider.future);
  } catch (_) {
    return null;
  }
  if (sub.subscriptionUuid.isEmpty) return null;
  return VpnConfig(
    panelUrl: kApiBaseUrl,
    subscriptionUuid: sub.subscriptionUuid,
    accessToken: access,
  );
}

/// Включает нативное Go-ядро вместо [MockVpnConnection]. По умолчанию выключено,
/// чтобы голый `flutter run` (CI/dev без собранных нативных либ) показывал UI на
/// моке и не ловил MissingPluginException на незарегистрированном канале.
/// Поднимать флагом сборки: `flutter run --dart-define=USE_NATIVE_VPN=true`.
const bool _nativeVpnEnabled =
    bool.fromEnvironment('USE_NATIVE_VPN', defaultValue: false);

/// Нативный путь доступен на всех 5 десктоп/мобильных платформах (плагин
/// `caramba_vpn` регистрирует `com.caramba/vpn` на каждой). Web — всегда мок.
/// Гейтится флагом [_nativeVpnEnabled], иначе остаётся [MockVpnConnection].
bool _useNativeVpn() {
  if (kIsWeb) return false;
  if (!_nativeVpnEnabled) return false;
  return Platform.isAndroid ||
      Platform.isIOS ||
      Platform.isMacOS ||
      Platform.isWindows ||
      Platform.isLinux;
}

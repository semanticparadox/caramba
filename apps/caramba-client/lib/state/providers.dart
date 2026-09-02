import 'dart:async';
import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/data/prefs_store.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/subscription_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';

/// Корневые DI-провайдеры: secure storage, prefs, API-клиент и VPN-движок.
/// Остальные нотифаеры (`auth`, `servers`, `subscription`, `vpn`) зависят от них.

/// Платформенное secure storage для JWT.
final tokenStoreProvider = Provider<TokenStore>((ref) => TokenStore());

/// Локальные несекретные настройки (CoreConfig, AppSettings, first-run, режим
/// туннеля). Инстанс один на приложение; читается только после
/// `appBootProvider` (см. `bootstrap_state.dart`), до этого отдаёт дефолты.
final prefsStoreProvider = Provider<PrefsStore>((ref) => PrefsStore());

/// HTTP-клиент к панели. `onSessionExpired` пробрасывает auth-нотифаеру
/// принудительный логаут, когда refresh окончательно протух.
final apiClientProvider = Provider<ApiClient>((ref) {
  final client = ApiClient(tokens: ref.watch(tokenStoreProvider));
  ref.onDispose(() => client.onSessionExpired = null);
  return client;
});

/// VPN-движок. На всех 5 платформах (Android/iOS/macOS/Windows/Linux) —
/// нативное Go-ядро (mihomo), когда включён флаг `USE_NATIVE_VPN`: на macOS
/// внутрипроцессно через dart:ffi (`libcaramba_core.dylib`, без Xcode и
/// Network Extension), на остальных через каналы `com.caramba/vpn`. Иначе и на
/// web — [MockVpnConnection], чтобы UI работал end-to-end без нативного бэка.
final vpnConnectionProvider = Provider<VpnConnection>((ref) {
  final conn = createVpnConnection(
    native: _useNativeVpn(),
    configResolver: () => _resolveVpnConfig(ref),
  );
  ref.onDispose(conn.dispose);
  return conn;
});

/// Работает ли сейчас нативное ядро (а не мок). Экраны, у которых «настоящий»
/// и «имитационный» пути расходятся (autotune), ветвятся по нему.
final isNativeVpnProvider = Provider<bool>((ref) => _useNativeVpn());

/// Способ захвата трафика по умолчанию для текущей платформы.
///
/// Мобильные строят системный TUN через VpnService/NetworkExtension — там
/// [TunnelMode.tun]. Desktop так не может без прав администратора (а на macOS
/// без Xcode-расширения вовсе), поэтому там [TunnelMode.proxy]: локальный
/// mixed-инбаунд на 127.0.0.1.
TunnelMode defaultTunnelMode() {
  if (kIsWeb) return TunnelMode.proxy;
  if (Platform.isAndroid || Platform.isIOS) return TunnelMode.tun;
  return TunnelMode.proxy;
}

/// Порт локального mixed-инбаунда в [TunnelMode.proxy].
const int kMixedPort = 7890;

/// Выбранный способ захвата трафика. Персистится: пользователь, переключивший
/// desktop на TUN (запустив приложение с правами), не должен делать это заново
/// после каждого перезапуска. Применяется вызовом `setTunnelMode` перед connect.
class TunnelModeNotifier extends StateNotifier<TunnelMode> {
  final PrefsStore? _prefs;

  TunnelModeNotifier(this._prefs, TunnelMode initial) : super(initial);

  /// Ставит значение из [PrefsStore] на старте (не пишет обратно).
  void hydrate(TunnelMode mode) => super.state = mode;

  @override
  set state(TunnelMode value) {
    super.state = value;
    unawaited(_prefs?.writeString(PrefsStore.kTunnelMode, value.wire));
  }

  void set(TunnelMode mode) => state = mode;
}

final tunnelModeProvider =
    StateNotifierProvider<TunnelModeNotifier, TunnelMode>(
      (ref) => TunnelModeNotifier(
        ref.watch(prefsStoreProvider),
        defaultTunnelMode(),
      ),
    );

/// Лениво собирает конфиг для авторизации Go-ядра перед поднятием туннеля
/// (путь panelAccount). Источник зависит от активного профиля подключения:
///   * rawSub          → `null` (configure не нужен, ядро поднимает импорт);
///   * panelAccount со своими полями (panelUrl/subscriptionUuid) → берём их из
///     профиля (мульти-аккаунт, иной тенант), а токен предпочитаем СВЕЖИЙ из
///     [TokenStore]: ApiClient ротирует его при 401, и запись на профиле
///     устаревает уже через час;
///   * профиль не задан или поля пусты → дефолтный путь тенанта-1: JWT из
///     TokenStore + UUID подписки (`GET /subscription`) + [kApiBaseUrl].
/// Возвращает `null`, если данных ещё нет — тогда `configure` пропускается и
/// нативная сторона ответит стадией `error`.
Future<VpnConfig?> _resolveVpnConfig(Ref ref) async {
  final profile = ref.read(activeConnectionProfileProvider);

  // Импортированная подписка авторизации панели не требует.
  if (profile != null && profile.isRaw) return null;

  // panelAccount, несущий собственные креды (например другой тенант): берём их
  // напрямую из профиля, минуя дефолтный путь тенанта-1.
  if (profile != null && profile.isPanel) {
    final panelUrl = profile.panelUrl;
    final uuid = profile.subscriptionUuid;
    // Токен ротируется в общем TokenStore (в т.ч. энроллмент-сессией), поэтому
    // он приоритетнее снимка, лежащего на профиле.
    final live = await ref.read(tokenStoreProvider).readAccess();
    final token = (live != null && live.isNotEmpty)
        ? live
        : profile.accessToken;
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
const bool _nativeVpnEnabled = bool.fromEnvironment(
  'USE_NATIVE_VPN',
  defaultValue: false,
);

/// Нативный путь доступен на всех 5 десктоп/мобильных платформах (плагин
/// `caramba_vpn` регистрирует `com.caramba/vpn` на каждой, а macOS идёт по
/// dart:ffi). Web — всегда мок. Гейтится флагом [_nativeVpnEnabled].
bool _useNativeVpn() {
  if (kIsWeb) return false;
  if (!_nativeVpnEnabled) return false;
  return Platform.isAndroid ||
      Platform.isIOS ||
      Platform.isMacOS ||
      Platform.isWindows ||
      Platform.isLinux;
}

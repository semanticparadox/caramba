/// Публичная поверхность федеративного плагина `caramba_vpn`.
///
/// Плагин регистрирует на каждой платформе каналы `com.caramba/vpn`
/// (MethodChannel) + `com.caramba/vpn/status` и `com.caramba/vpn/traffic`
/// (EventChannel) и мостит их к Go-ядру caramba-core (mihomo): gomobile
/// aar/xcframework на мобильных, cgo c-shared `libcaramba_core` на desktop.
///
/// Здесь же живёт КОНТРАКТ движка ([VpnConnection] и модели статуса) и три его
/// реализации: канальная, dart:ffi (внутрипроцессная, для macOS без Xcode) и
/// мок. Контракт вынесен в плагин, чтобы FFI-реализация могла его реализовать
/// без обратной зависимости плагин -> приложение; модель сервера принадлежит
/// приложению и входит параметром типа `S`.
library;

import 'package:caramba_vpn/src/contract.dart';
import 'package:caramba_vpn/src/core_policy.dart';
import 'package:caramba_vpn/src/ffi_vpn_connection.dart';
import 'package:caramba_vpn/src/method_channel_vpn_connection.dart';
import 'package:caramba_vpn/src/mock_vpn_connection.dart';
import 'package:flutter/foundation.dart'
    show TargetPlatform, defaultTargetPlatform, kIsWeb;
import 'package:flutter/services.dart';

export 'package:caramba_vpn/src/contract.dart';
export 'package:caramba_vpn/src/core_models.dart'
    show ImportResult, ImportedServer, ProbeResult, ProbeVerdict;
export 'package:caramba_vpn/src/core_policy.dart';
export 'package:caramba_vpn/src/csm_device.dart'
    show CsmAgreement, CsmDeviceKey, csmDeviceSignatureFromJson;
export 'package:caramba_vpn/src/ffi/caramba_core_bindings.dart'
    show CarambaCoreException, CarambaCoreLibrary, CarambaCoreMissingSymbol;
export 'package:caramba_vpn/src/ffi/core_loader.dart'
    show
        CarambaCoreLibraryNotFound,
        currentProcessLibraryCandidates,
        kCarambaCoreLibEnv,
        openCarambaCoreLibrary,
        resolveCarambaCoreLibraryPath;
export 'package:caramba_vpn/src/ffi/library_lookup.dart'
    show carambaCoreLibFileName, carambaCoreLibraryCandidates;
export 'package:caramba_vpn/src/ffi_vpn_connection.dart' show FfiVpnConnection;
export 'package:caramba_vpn/src/method_channel_vpn_connection.dart'
    show MethodChannelVpnConnection;
export 'package:caramba_vpn/src/mock_vpn_connection.dart'
    show MockVpnConnection;

/// Какой транспорт до ядра использовать.
enum CarambaVpnBackend {
  /// Мок без нативного бэка (dev/CI, web).
  mock,

  /// Платформенные каналы `com.caramba/vpn` (Android/iOS/Linux/Windows).
  channel,

  /// Внутрипроцессное ядро через dart:ffi (macOS без Xcode-расширения).
  ffi,
}

/// Тонкий Dart-фасад плагина: seam-конфигурация ядра и фабрика соединений.
class CarambaVpn {
  CarambaVpn._();

  /// Единственный экземпляр фасада. Канал общий с приложением, поэтому
  /// состояние держать здесь нечего — фасад без полей.
  static final CarambaVpn instance = CarambaVpn._();

  /// Тот же MethodChannel, что использует [MethodChannelVpnConnection].
  /// Имя — часть контракта panel<->client, не менять.
  static const MethodChannel _channel = MethodChannel('com.caramba/vpn');

  /// Имя метода seam-конфигурации.
  static const String _configureMethod = 'configure';

  /// Передаёт нативному ядру auth/config-seam до подключения.
  ///
  /// * [panelUrl] — базовый URL панели (`https://panel.example`), обязателен.
  /// * [subscriptionId] — UUID подписки; нативная сторона вызывает им
  ///   `SetSubscriptionID`. Пусто — ядро тянет UUID у панели лениво.
  /// * [accessToken] — действующий JWT access-токен; кладётся в token-store
  ///   ядра, чтобы запросы конфига/подписки шли авторизованными.
  /// * [refreshToken] — долгоживущий refresh той же сессии. ПЕРЕДАВАТЬ, когда
  ///   он есть: access живёт ~15 минут, и без refresh ядро не сможет продлить
  ///   сессию, пока приложение выгружено (см. [VpnConfig]).
  /// * [accessExpiry] — когда истекает [accessToken]; `null` оставляет ядру
  ///   разобрать claim `exp` самого JWT.
  ///
  /// На провод уходит ключ `subscriptionUuid` — тот же, что шлёт
  /// `VpnConfig.toArgs()`. Нативные стороны принимают и устаревший
  /// `subscriptionId`.
  ///
  /// Идемпотентен: повторный вызов обновляет seam (например, после refresh
  /// токена). Вызывать до `connect`.
  Future<void> configure({
    required String panelUrl,
    required String subscriptionId,
    required String accessToken,
    String refreshToken = '',
    DateTime? accessExpiry,
  }) {
    return _channel.invokeMethod<void>(
      _configureMethod,
      VpnConfig(
        panelUrl: panelUrl,
        subscriptionUuid: subscriptionId,
        accessToken: accessToken,
        refreshToken: refreshToken,
        accessExpiry: accessExpiry,
      ).toArgs(),
    );
  }

  /// Какой бэкенд выбрать на текущей платформе.
  ///
  /// * `native == false` -> [CarambaVpnBackend.mock];
  /// * macOS + [preferFfiOnMacOS] -> [CarambaVpnBackend.ffi] (ядро в процессе,
  ///   proxy-режим, без Network Extension и без Xcode);
  /// * иначе -> [CarambaVpnBackend.channel].
  static CarambaVpnBackend selectBackend({
    required bool native,
    bool preferFfiOnMacOS = true,
    TargetPlatform? platform,
    bool isWeb = kIsWeb,
  }) {
    if (!native || isWeb) return CarambaVpnBackend.mock;
    final target = platform ?? defaultTargetPlatform;
    if (preferFfiOnMacOS && target == TargetPlatform.macOS) {
      return CarambaVpnBackend.ffi;
    }
    return CarambaVpnBackend.channel;
  }

  /// Создаёт соединение нужного типа.
  ///
  /// `S` — модель сервера приложения. Плагин её не знает, поэтому приложение
  /// отдаёт [describe] (свести модель к полям провода) и [rawTarget] (собрать
  /// плейсхолдер-«сервер» для rawSub-профиля).
  ///
  /// На macOS FFI-путь по умолчанию поднимается в [TunnelMode.proxy]: TUN там
  /// требует root или Network Extension, а mixed-инбаунд на 127.0.0.1 — нет.
  static VpnConnection<S> createConnection<S extends Object>({
    required bool native,
    required VpnServerDescriptor<S> describe,
    VpnRawTargetFactory<S>? rawTarget,
    VpnConfigResolver? configResolver,
    bool preferFfiOnMacOS = true,
    String? libraryPath,
    TunnelMode ffiTunnelMode = TunnelMode.proxy,
    int ffiMixedPort = 7890,
    TargetPlatform? platform,
  }) {
    switch (selectBackend(
      native: native,
      preferFfiOnMacOS: preferFfiOnMacOS,
      platform: platform,
    )) {
      case CarambaVpnBackend.mock:
        return MockVpnConnection<S>(rawTarget: rawTarget);
      case CarambaVpnBackend.ffi:
        return FfiVpnConnection<S>(
          describe: describe,
          rawTarget: rawTarget,
          configResolver: configResolver,
          libraryPath: libraryPath,
          defaultTunnelMode: ffiTunnelMode,
          mixedPort: ffiMixedPort,
        );
      case CarambaVpnBackend.channel:
        return MethodChannelVpnConnection<S>(
          describe: describe,
          rawTarget: rawTarget,
          configResolver: configResolver,
        );
    }
  }
}

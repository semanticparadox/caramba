import 'package:flutter/services.dart';

/// Тонкий Dart-фасад федеративного плагина `caramba_vpn`.
///
/// Плагин регистрирует на каждой платформе каналы `com.caramba/vpn`
/// (MethodChannel) + `com.caramba/vpn/status` и `com.caramba/vpn/traffic`
/// (EventChannel) и мостит их к Go-ядру caramba-core (mihomo): gomobile
/// aar/xcframework на мобильных, cgo c-shared `libcaramba_core` на desktop.
///
/// Жизненный цикл туннеля (`connect` / `disconnect` / `status` + потоки
/// статуса и трафика) уже принадлежит приложению — его держит
/// `MethodChannelVpnConnection` в `lib/vpn/vpn_service.dart`. Здесь намеренно
/// нет дублирующего API: единственное, что добавляет плагин поверх контракта
/// приложения, — метод `configure`, которым приложение передаёт нативной
/// стороне seam аутентификации/конфигурации (URL панели, UUID подписки и
/// access-токен) до первого `connect`.
///
/// Каналы оба раза одни и те же (`com.caramba/vpn`): нативная сторона
/// маршрутизирует `configure` к `mobile.NewClient(panelUrl, ...)` +
/// `SetSubscriptionID(subscriptionId)` и кладёт `accessToken` в token-store
/// ядра, после чего `connect` поднимает туннель уже аутентифицированным.
class CarambaVpn {
  CarambaVpn._();

  /// Единственный экземпляр фасада. Канал общий с приложением, поэтому
  /// состояние держать здесь нечего — фасад без полей.
  static final CarambaVpn instance = CarambaVpn._();

  /// Тот же MethodChannel, что использует `MethodChannelVpnConnection`
  /// приложения. Имя — часть контракта panel<->client, не менять.
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
  ///
  /// Идемпотентен: повторный вызов обновляет seam (например, после refresh
  /// токена). Вызывать до `connect`.
  Future<void> configure({
    required String panelUrl,
    required String subscriptionId,
    required String accessToken,
  }) {
    return _channel.invokeMethod<void>(_configureMethod, <String, String>{
      'panelUrl': panelUrl,
      'subscriptionId': subscriptionId,
      'accessToken': accessToken,
    });
  }
}

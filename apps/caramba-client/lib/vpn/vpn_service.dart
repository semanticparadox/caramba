/// VPN-движок приложения.
///
/// Контракт ([VpnConnection], статусы, модели ABI v2) живёт в плагине
/// `package:caramba_vpn` — так его FFI-реализация (`FfiVpnConnection`, ядро в
/// процессе приложения) реализует тот же интерфейс без обратной зависимости
/// плагин -> приложение. Плагинные типы обобщены по модели сервера; здесь они
/// закрепляются за [Server], а публичные имена остаются прежними, чтобы
/// остальное приложение не менялось.
library;

import 'package:caramba_vpn/caramba_vpn.dart' as vpn;

import 'package:caramba_client/data/models/server.dart';

export 'package:caramba_client/vpn/core_policy.dart';
export 'package:caramba_client/vpn/vpn_models.dart';
export 'package:caramba_vpn/caramba_vpn.dart'
    show CarambaVpnBackend, VpnConfig, VpnConfigResolver, VpnServerArgs;

/// Абстракция VPN-движка. За ней стоит Go-ядро caramba-core (mihomo) — через
/// gomobile/каналы на мобильных и desktop, через dart:ffi на macOS — либо
/// [MockVpnConnection] в dev-сборках.
///
/// Контракт намеренно узкий — UI и `vpnProvider` зависят только от него.
typedef VpnConnection = vpn.VpnConnection<Server>;

/// Синтетический [Server] для отображения rawSub-профиля в статусе туннеля.
///
/// У импортированной подписки нет узла из `GET /servers` (id/пинг/нагрузка), но
/// пайплайн статуса несёт `Server?` для подписи на орбе. Здесь собирается
/// плейсхолдер с именем профиля и нейтральными полями; id<0 помечает его как
/// не-панельный (никакой узел подписки за ним не стоит).
Server rawProfileServer(String label) =>
    Server(id: -1, name: label, status: 'online');

/// Сводит [Server] к полям, которые уходят на нативный провод.
///
/// serverId уходит СТРОКОЙ: нативный мост читает его как String и молча
/// коерсит не-строку в "" (теряя выбор узла) — отсюда `id.toString()`.
vpn.VpnServerArgs describeServer(Server s) => vpn.VpnServerArgs(
  id: s.id.toString(),
  name: s.name,
  countryCode: s.countryCode,
);

/// Реализация поверх платформенных каналов `com.caramba/vpn`.
///
/// Нативная сторона (Android/iOS через gomobile, Linux/Windows через
/// `libcaramba_core`) выставляет `configure`, `connect`, `connectRaw`,
/// `disconnect`, `status`, `importSubscription`, `probe`, `setPolicy`,
/// `setTunnelMode` плюс два EventChannel'а статуса и трафика.
class MethodChannelVpnConnection
    extends vpn.MethodChannelVpnConnection<Server> {
  MethodChannelVpnConnection({super.configResolver})
    : super(describe: describeServer, rawTarget: rawProfileServer);
}

/// Внутрипроцессное ядро через dart:ffi (`libcaramba_core.dylib`).
///
/// Путь macOS без Xcode: по умолчанию [vpn.TunnelMode.proxy] — локальный
/// mixed-инбаунд на 127.0.0.1:7890, никаких привилегий и Network Extension.
class FfiVpnConnection extends vpn.FfiVpnConnection<Server> {
  FfiVpnConnection({
    super.configResolver,
    super.libraryPath,
    super.defaultTunnelMode,
    super.mixedPort,
  }) : super(describe: describeServer, rawTarget: rawProfileServer);
}

/// Имитация ядра для desktop/dev: проходит реальный жизненный цикл состояний
/// и генерирует «дышащий» трафик, чтобы UI работал end-to-end без нативного
/// бэка.
class MockVpnConnection extends vpn.MockVpnConnection<Server> {
  MockVpnConnection() : super(rawTarget: rawProfileServer);
}

/// Выбирает реализацию движка для текущей платформы.
///
/// * `native == false` -> [MockVpnConnection];
/// * macOS (и `preferFfiOnMacOS`) -> [FfiVpnConnection]: ядро в процессе,
///   proxy-режим, без Network Extension;
/// * остальное -> [MethodChannelVpnConnection].
VpnConnection createVpnConnection({
  required bool native,
  vpn.VpnConfigResolver? configResolver,
  bool preferFfiOnMacOS = true,
  String? libraryPath,
}) => vpn.CarambaVpn.createConnection<Server>(
  native: native,
  describe: describeServer,
  rawTarget: rawProfileServer,
  configResolver: configResolver,
  preferFfiOnMacOS: preferFfiOnMacOS,
  libraryPath: libraryPath,
);

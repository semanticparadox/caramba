/// Снимок состояния туннеля для UI.
///
/// Сами типы живут в плагине (`package:caramba_vpn`), чтобы его FFI-реализация
/// могла реализовать контракт без обратной зависимости плагин -> приложение.
/// Здесь они закрепляются за моделью сервера приложения ([Server]): плагинный
/// [VpnStatus] обобщён по типу сервера, а приложение всюду работает с
/// `VpnStatus` = `caramba_vpn.VpnStatus<Server>`.
library;

import 'package:caramba_vpn/caramba_vpn.dart' as vpn;

import 'package:caramba_client/data/models/server.dart';

export 'package:caramba_vpn/caramba_vpn.dart'
    show TrafficStats, TunnelMode, TunnelWitness, VpnFailureReason, VpnStage;

/// Снимок состояния VPN-соединения, который эмитит нативное ядро (mihomo через
/// gomobile / libcaramba_core) и потребляет UI/`vpnProvider`.
///
/// Поля: `stage`, `server`, `detail`, `connectedSince`, плюс ABI v2
/// `mode` (tun/proxy), `mixedPort`, `activeProxy` — все три опциональны.
typedef VpnStatus = vpn.VpnStatus<Server>;

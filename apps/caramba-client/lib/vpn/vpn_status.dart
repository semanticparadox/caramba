import 'package:caramba_client/data/models/server.dart';

/// Фаза туннеля — отражает state-машину из DESIGN.md §4
/// (disconnected → connecting → connected → error, + reconnecting).
enum VpnStage { disconnected, connecting, connected, reconnecting, error }

/// Снимок состояния VPN-соединения, который эмитит нативное ядро (mihomo через
/// gomobile) и потребляет UI/`vpnProvider`.
class VpnStatus {
  final VpnStage stage;

  /// Сервер, к которому идёт/состоялось подключение (если применимо).
  final Server? server;

  /// Под-сообщение для орба («Securing tunnel», причина ошибки и т.п.).
  final String? detail;

  /// Момент перехода в `connected` — UI отсчитывает от него таймер аптайма.
  final DateTime? connectedSince;

  const VpnStatus({
    required this.stage,
    this.server,
    this.detail,
    this.connectedSince,
  });

  const VpnStatus.disconnected()
    : stage = VpnStage.disconnected,
      server = null,
      detail = null,
      connectedSince = null;

  bool get isConnected => stage == VpnStage.connected;
  bool get isBusy =>
      stage == VpnStage.connecting || stage == VpnStage.reconnecting;

  /// Парсинг события из нативного канала. `stage` приходит строкой, совпадающей
  /// с именами [VpnStage] (lowerCamel), что задаёт Go-фасад.
  factory VpnStatus.fromMap(Map<dynamic, dynamic> map, {Server? server}) {
    return VpnStatus(
      stage: _stageFrom(map['stage'] as String?),
      server: server,
      detail: map['detail'] as String?,
      connectedSince: () {
        final ms = map['connectedSinceMs'];
        if (ms is int && ms > 0) {
          return DateTime.fromMillisecondsSinceEpoch(ms);
        }
        return null;
      }(),
    );
  }

  static VpnStage _stageFrom(String? s) {
    switch (s) {
      case 'connecting':
        return VpnStage.connecting;
      case 'connected':
        return VpnStage.connected;
      case 'reconnecting':
        return VpnStage.reconnecting;
      case 'error':
        return VpnStage.error;
      case 'disconnected':
      default:
        return VpnStage.disconnected;
    }
  }

  VpnStatus copyWith({
    VpnStage? stage,
    Server? server,
    String? detail,
    DateTime? connectedSince,
  }) => VpnStatus(
    stage: stage ?? this.stage,
    server: server ?? this.server,
    detail: detail ?? this.detail,
    connectedSince: connectedSince ?? this.connectedSince,
  );
}

/// Мгновенная пропускная способность и накопленные счётчики туннеля.
class TrafficStats {
  /// Скорость скачивания, байт/с.
  final int downBps;

  /// Скорость отдачи, байт/с.
  final int upBps;

  /// Всего скачано за сессию, байт.
  final int downTotal;

  /// Всего отдано за сессию, байт.
  final int upTotal;

  const TrafficStats({
    this.downBps = 0,
    this.upBps = 0,
    this.downTotal = 0,
    this.upTotal = 0,
  });

  static const zero = TrafficStats();

  factory TrafficStats.fromMap(Map<dynamic, dynamic> map) => TrafficStats(
    downBps: (map['downBps'] as num?)?.toInt() ?? 0,
    upBps: (map['upBps'] as num?)?.toInt() ?? 0,
    downTotal: (map['downTotal'] as num?)?.toInt() ?? 0,
    upTotal: (map['upTotal'] as num?)?.toInt() ?? 0,
  );
}

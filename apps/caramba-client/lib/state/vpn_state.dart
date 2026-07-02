import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Нотифаер state-машины туннеля. Подписывается на [VpnConnection.status],
/// проксирует его в Riverpod, и предоставляет toggle/connect/disconnect для UI
/// (орб на Home). Реальные переходы стадий приходят от ядра.
///
/// Активный профиль подключения ([ConnectionProfile]) задаёт путь поднятия:
///   * panelAccount → `configure` + connect к узлу подписки;
///   * rawSub       → импорт сырой подписки + connect без узла.
/// Профиль резолвится лениво (на момент connect) через [_activeProfile].
class VpnNotifier extends StateNotifier<VpnStatus> {
  final VpnConnection _conn;
  final Server? Function() _recommended;
  final ConnectionProfile? Function() _activeProfile;
  StreamSubscription<VpnStatus>? _sub;

  VpnNotifier(this._conn, this._recommended, this._activeProfile)
    : super(_conn.currentStatus) {
    _sub = _conn.status.listen((s) => state = s);
  }

  /// Подключиться согласно активному профилю подключения.
  ///
  /// Если активен rawSub-профиль — поднимаем туннель из импортированной
  /// подписки. Иначе (panelAccount или профиль не задан) идём панельным путём:
  /// к [server], либо к рекомендованному узлу, если сервер не передан.
  /// Возвращает `false`, если подключаться не к чему.
  Future<bool> connect([Server? server]) async {
    final profile = _activeProfile();

    // rawSub: явный сервер для панельного пути не передан — поднимаем raw.
    if (server == null && profile != null && profile.isRaw) {
      final raw = profile.rawConfig ?? profile.source;
      if (raw.isEmpty) {
        state = const VpnStatus(stage: VpnStage.error, detail: 'Empty profile');
        return false;
      }
      await _conn.connectRaw(
        raw: raw,
        format: 'auto',
        label: profile.displayName,
      );
      return true;
    }

    final target = server ?? _recommended();
    if (target == null) {
      state = const VpnStatus(
        stage: VpnStage.error,
        detail: 'No server selected',
      );
      return false;
    }
    await _conn.connect(target);
    return true;
  }

  Future<void> disconnect() => _conn.disconnect();

  /// Тап по орбу: connected/connecting → отключение, иначе → подключение.
  Future<void> toggle([Server? server]) {
    if (state.isConnected || state.isBusy) return disconnect();
    return connect(server);
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }
}

final vpnProvider = StateNotifierProvider<VpnNotifier, VpnStatus>((ref) {
  return VpnNotifier(
    ref.watch(vpnConnectionProvider),
    () => ref.read(recommendedServerProvider),
    () => ref.read(activeConnectionProfileProvider),
  );
});

/// Поток статистики трафика для Home (тикает в connected). Эмитит нули вне сессии.
final trafficProvider = StreamProvider.autoDispose<TrafficStats>((ref) {
  return ref.watch(vpnConnectionProvider).traffic;
});

/// Удобный булев селектор «подключены ли мы» (для бейджей/иконок).
final isConnectedProvider = Provider<bool>(
  (ref) => ref.watch(vpnProvider).isConnected,
);

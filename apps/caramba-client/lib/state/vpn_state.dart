import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_policy_mapping.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
// Сериализатор политики живёт в плагине и не входит в узкий re-export
// `lib/vpn/core_policy.dart` (тот отдаёт только модели), поэтому берём его
// напрямую из пакета.
import 'package:caramba_vpn/caramba_vpn.dart' show jsonEncodePolicy;

/// Нотифаер state-машины туннеля. Подписывается на [VpnConnection.status],
/// проксирует его в Riverpod, и предоставляет toggle/connect/disconnect для UI
/// (орб на Home). Реальные переходы стадий приходят от ядра.
///
/// Активный профиль подключения ([ConnectionProfile]) задаёт путь поднятия:
///   * panelAccount → `configure` + connect к узлу подписки;
///   * rawSub       → импорт сырой подписки + connect к закреплённому узлу.
/// Профиль резолвится лениво (на момент connect) через [_activeProfile].
///
/// Перед КАЖДЫМ поднятием туннеля в ядро уходят политика ([CorePolicy], ABI v2
/// `setPolicy`) и способ захвата трафика (`setTunnelMode`): оба действуют со
/// следующего `Up`, поэтому применяются именно здесь, а не при правке настроек.
class VpnNotifier extends StateNotifier<VpnStatus> {
  final VpnConnection _conn;
  final Server? Function() _recommended;
  final ConnectionProfile? Function() _activeProfile;
  final CorePolicy Function() _policy;
  final TunnelMode Function() _tunnelMode;
  StreamSubscription<VpnStatus>? _sub;

  /// Политика, реально отданная ядру при последнем поднятии туннеля (JSON).
  /// `null` — туннель ещё не поднимали. По расхождению с текущей политикой UI
  /// показывает «Переподключитесь, чтобы применить» ([reconnectRequiredProvider]);
  /// авто-переподключения нет намеренно: обрыв туннеля решает пользователь.
  String? appliedPolicyJson;

  /// Способ захвата трафика, отданный ядру при последнем поднятии туннеля.
  /// Тоже действует со следующего `Up`, поэтому участвует в том же сравнении.
  TunnelMode? appliedTunnelMode;

  VpnNotifier(
    this._conn,
    this._recommended,
    this._activeProfile,
    this._policy,
    this._tunnelMode,
  ) : super(_conn.currentStatus) {
    _sub = _conn.status.listen((s) => state = s);
  }

  /// Подключиться согласно активному профилю подключения.
  ///
  /// Если активен rawSub-профиль — поднимаем туннель из импортированной
  /// подписки, передавая её формат и закреплённый узел. Иначе (panelAccount или
  /// профиль не задан) идём панельным путём: к [server], либо к рекомендованному
  /// узлу, если сервер не передан. Возвращает `false`, если подключаться не к чему.
  Future<bool> connect([Server? server]) async {
    final profile = _activeProfile();

    // rawSub: явный сервер для панельного пути не передан — поднимаем raw.
    if (server == null && profile != null && profile.isRaw) {
      final raw = profile.rawConfig ?? profile.source;
      if (raw.isEmpty) {
        state = const VpnStatus(stage: VpnStage.error, detail: 'Empty profile');
        return false;
      }
      await _applyCorePreferences();
      await _conn.connectRaw(
        raw: raw,
        format: profile.format,
        label: profile.displayName,
        serverId: profile.selectedServerId,
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
    await _applyCorePreferences();
    await _conn.connect(target);
    return true;
  }

  /// Отдаёт ядру политику и режим захвата трафика. Ошибку setPolicy не считаем
  /// фатальной для подключения: старое ядро (до ABI v2) её не знает, а туннель
  /// на дефолтной политике всё равно лучше, чем отказ подключаться.
  Future<void> _applyCorePreferences() async {
    final mode = _tunnelMode();
    await _conn.setTunnelMode(mode, mixedPort: kMixedPort);
    appliedTunnelMode = mode;
    final policy = _policy();
    try {
      await _conn.setPolicy(policy);
      appliedPolicyJson = jsonEncodePolicy(policy);
    } catch (_) {
      appliedPolicyJson = null;
    }
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
    () => ref.read(corePolicyProvider),
    () => ref.read(tunnelModeProvider),
  );
});

/// Текущая политика ядра, собранная из пользовательского выбора. Отдельный
/// провайдер, чтобы её видели и connect, и баннер «нужно переподключение».
final corePolicyProvider = Provider<CorePolicy>((ref) {
  return corePolicyFrom(
    ref.watch(coreConfigProvider),
    ref.watch(relaysProvider),
  );
});

/// `true`, когда настройки правились при поднятом туннеле и ещё не применены.
/// UI показывает баннер «Переподключитесь, чтобы применить»; переподключение
/// инициирует пользователь, приложение туннель само не рвёт.
final reconnectRequiredProvider = Provider<bool>((ref) {
  final status = ref.watch(vpnProvider);
  if (!status.isConnected) return false;
  final notifier = ref.read(vpnProvider.notifier);
  final applied = notifier.appliedPolicyJson;
  if (applied == null) return false;
  if (applied != jsonEncodePolicy(ref.watch(corePolicyProvider))) return true;
  return notifier.appliedTunnelMode != ref.watch(tunnelModeProvider);
});

/// Поток статистики трафика для Home (тикает в connected). Эмитит нули вне сессии.
final trafficProvider = StreamProvider.autoDispose<TrafficStats>((ref) {
  return ref.watch(vpnConnectionProvider).traffic;
});

/// Удобный булев селектор «подключены ли мы» (для бейджей/иконок).
final isConnectedProvider = Provider<bool>(
  (ref) => ref.watch(vpnProvider).isConnected,
);

/// Узел, на который ядро сейчас указывает селектором CARAMBA (ABI v2).
/// `null` вне сессии или когда ядро поле не прислало.
final activeProxyProvider = Provider<String?>(
  (ref) => ref.watch(vpnProvider).activeProxy,
);

/// Способ захвата трафика, о котором отчиталось ЯДРО (в отличие от
/// [tunnelModeProvider] — это выбор пользователя, применяемый со следующего Up).
final activeTunnelModeProvider = Provider<TunnelMode?>(
  (ref) => ref.watch(vpnProvider).mode,
);

/// Адрес локального прокси (`127.0.0.1:7890`), когда ядро работает в
/// proxy-режиме. `null` в tun-режиме и вне сессии — строку показывать нечего.
final proxyEndpointProvider = Provider<String?>((ref) {
  final status = ref.watch(vpnProvider);
  if (status.mode != TunnelMode.proxy) return null;
  final port = status.mixedPort;
  if (port == null || port <= 0) return null;
  return '127.0.0.1:$port';
});

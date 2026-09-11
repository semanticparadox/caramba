/// Автоподключение при запуске десктопного приложения.
///
/// ЗАЧЕМ ОТДЕЛЬНЫЙ СЕРВИС. Тумблер «Автоподключение» существовал в настройках
/// давно, но его не читал никто: ни один экран, ни один провайдер. С подопцией
/// «подключаться автоматически» у автозапуска это стало бы обещанием, которое
/// не выполняется ни разу. Здесь тумблер исполняется: один раз за процесс,
/// когда всё, что нужно для подъёма, готово.
///
/// Что значит «готово». Подключение зовёт тот же `VpnNotifier.connect()`, что
/// и дайл, а у дайла к моменту клика уже есть активный профиль и (для
/// панельного профиля) список серверов. На старте их ещё может не быть, и
/// `connect()` без сервера честно ответил бы «No server selected». Поэтому
/// сервис ждёт: настройки прочитаны, профили загружены, активный есть, для
/// панельного профиля есть рекомендованный сервер. Ждёт подпиской, а не
/// таймером: сколько идёт загрузка серверов, знает только сеть.
///
/// Причина запуска (вручную или автозапуск) не учитывается намеренно: тумблер
/// подписан «при каждом запуске», а на macOS причину запуска и не узнать.
library;

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/state/bootstrap_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Снимок того, что сервису нужно знать для решения.
@immutable
class AutoConnectInput {
  final bool bootReady;
  final bool wanted;
  final bool profilesLoading;

  /// Активный профиль есть.
  final bool hasProfile;

  /// Профиль сырой (подписка): сервер панели ему не нужен.
  final bool profileIsRaw;

  /// Есть сервер, который дайл выбрал бы сам.
  final bool hasRecommendedServer;

  final VpnStage stage;

  const AutoConnectInput({
    required this.bootReady,
    required this.wanted,
    required this.profilesLoading,
    required this.hasProfile,
    required this.profileIsRaw,
    required this.hasRecommendedServer,
    required this.stage,
  });
}

/// Пора ли подключаться. Чистое решение, его и проверяет тест.
bool autoConnectReady(AutoConnectInput i) {
  if (!i.bootReady || !i.wanted) return false;
  if (i.profilesLoading || !i.hasProfile) return false;
  if (!i.profileIsRaw && !i.hasRecommendedServer) return false;
  return i.stage == VpnStage.disconnected;
}

/// Снимает подписку, выданную `listenInput`.
typedef AutoConnectCanceller = void Function();

class AutoConnectService {
  final AutoConnectInput Function() _readInput;
  final AutoConnectCanceller Function(VoidCallback onChange) _listenInput;
  final Future<void> Function() _connect;

  AutoConnectCanceller? _cancel;

  /// Подключались ли уже. Один раз за процесс: человек, который сам отключил
  /// туннель после автоподключения, не должен получить его обратно от
  /// очередного движения провайдеров.
  bool _fired = false;

  bool get fired => _fired;

  AutoConnectService({
    required AutoConnectInput Function() readInput,
    required AutoConnectCanceller Function(VoidCallback onChange) listenInput,
    required Future<void> Function() connect,
  }) : _readInput = readInput,
       _listenInput = listenInput,
       _connect = connect;

  /// Проверяет сразу и дальше на каждое изменение входа, пока не выстрелит.
  void start() {
    if (_cancel != null || _fired) return;
    _cancel = _listenInput(_check);
    _check();
  }

  void _check() {
    if (_fired) return;
    if (!autoConnectReady(_readInput())) return;
    _fired = true;
    dispose();
    unawaited(_connect());
  }

  void dispose() {
    _cancel?.call();
    _cancel = null;
  }
}

/// Вход сервиса одним провайдером: Riverpod пересчитает его при движении
/// любого источника, и сервису достаётся одна подписка.
final autoConnectInputProvider = Provider<AutoConnectInput>((ref) {
  final profiles = ref.watch(connectionProfilesProvider);
  final active = profiles.active;
  return AutoConnectInput(
    bootReady: ref.watch(appBootReadyProvider),
    wanted: ref.watch(coreConfigProvider.select((c) => c.autoConnect)),
    profilesLoading: profiles.loading,
    hasProfile: active != null,
    profileIsRaw: active?.isRaw ?? false,
    hasRecommendedServer: ref.watch(recommendedServerProvider) != null,
    stage: ref.watch(vpnProvider.select((s) => s.stage)),
  );
});

/// Сервис автоподключения. `start()` зовёт десктопный хост сервисов, подписку
/// снимает dispose провайдера.
final autoConnectServiceProvider = Provider<AutoConnectService>((ref) {
  final service = AutoConnectService(
    readInput: () => ref.read(autoConnectInputProvider),
    listenInput: (onChange) {
      final sub = ref.container.listen<AutoConnectInput>(
        autoConnectInputProvider,
        (_, __) => onChange(),
      );
      return sub.close;
    },
    connect: () async {
      await ref.read(vpnProvider.notifier).connect();
    },
  );
  ref.onDispose(service.dispose);
  return service;
});

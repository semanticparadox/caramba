/// Статус-блок сайдбара: что сейчас с туннелем и что можно с ним сделать.
///
/// ЗАЧЕМ ОН ЕСТЬ, ЕСЛИ ЕСТЬ ДАЙЛ. Дайл живёт на «Подключении», а сайдбар виден
/// со всех веток. Человек, который правит настройки или смотрит профиль, обязан
/// видеть состояние защиты, не возвращаясь на первую вкладку: на десктопе окно
/// открыто часами, и «подключено ли я сейчас» — вопрос, который задают чаще
/// всех прочих.
///
/// ЦВЕТ ТОЛЬКО У ТОЧКИ. Ни фона, ни рамки статусного цвета: карточка в зелёном
/// читается как «всё хорошо» и тогда, когда доступ закрыт, а щит врёт. Слово
/// говорит правду, точка её подкрашивает.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/desktop/desktop_strings.dart';
import 'package:caramba_client/desktop/shell/desktop_shortcuts.dart';
import 'package:caramba_client/domain/autopilot/auto_pick.dart'
    show namingOfProxy;
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/access_guard.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Диаметр точки состояния.
const double kStatusDotSize = 8;

/// Высота кнопки действия. Ниже мобильных 50: десктопная плотность, но не
/// меньше комфортной цели мыши.
const double kStatusButtonHeight = 36;

class SidebarStatus extends ConsumerStatefulWidget {
  const SidebarStatus({super.key});

  @override
  ConsumerState<SidebarStatus> createState() => _SidebarStatusState();
}

class _SidebarStatusState extends ConsumerState<SidebarStatus> {
  /// Тикает раз в секунду и ТОЛЬКО в connected: таймер, который крутится в
  /// отключённом состоянии, перерисовывает сайдбар круглые сутки просто так.
  _SecondTicker? _ticker;

  @override
  void dispose() {
    _ticker?.stop();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final status = ref.watch(vpnProvider);
    final noConnections = ref.watch(desktopNoConnectionsProvider);
    // Доступ закрыт при поднятом туннеле: щит в этом состоянии — ложь, и
    // сайдбар обязан покрасить точку не в зелёный. Берём ЖИВОЙ отказ сторожа:
    // панельный снимок сюда не тащим, чтобы сайдбар не будил панельные
    // провайдеры на каждой ветке.
    final refusal = ref.watch(liveAccessRefusalProvider);
    final accessBlocked = status.stage == VpnStage.connected &&
        refusal != null &&
        refusal.isBlocked;

    _syncTicker(status.stage == VpnStage.connected);

    final label = DesktopStrings.stageLabel(
      status.stage,
      accessBlocked: accessBlocked,
      noConnections: noConnections,
    );
    final node = _nodeLabel(status);
    final session = status.stage == VpnStage.connected
        ? _session(status.connectedSince)
        : null;

    return Semantics(
      liveRegion: true,
      container: true,
      label: node == null ? label : '$label, $node',
      child: Container(
        padding: const EdgeInsets.all(AppSpace.s3),
        decoration: BoxDecoration(
          color: c.surface1,
          borderRadius: AppRadius.r14,
          border: Border.all(color: c.borderSubtle),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Container(
                  width: kStatusDotSize,
                  height: kStatusDotSize,
                  decoration: BoxDecoration(
                    color: _dotColor(
                      context,
                      stage: status.stage,
                      accessBlocked: accessBlocked,
                      noConnections: noConnections,
                    ),
                    shape: BoxShape.circle,
                  ),
                ),
                const SizedBox(width: AppSpace.s2),
                Expanded(
                  child: Text(
                    label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: AppType.bodyMd.copyWith(color: c.textHi),
                  ),
                ),
              ],
            ),
            if (node != null || session != null) ...[
              const SizedBox(height: AppSpace.s1),
              Row(
                children: [
                  if (node != null)
                    Expanded(
                      child: Text(
                        node,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: AppType.bodySm.copyWith(color: c.textMed),
                      ),
                    ),
                  if (session != null) ...[
                    if (node != null) const SizedBox(width: AppSpace.s2),
                    Text(
                      session,
                      style: AppType.monoSm.copyWith(color: c.textMed),
                    ),
                  ],
                ],
              ),
            ],
            const SizedBox(height: AppSpace.s3),
            OutlinedButton(
              style: OutlinedButton.styleFrom(
                minimumSize: const Size.fromHeight(kStatusButtonHeight),
                padding: const EdgeInsets.symmetric(
                  horizontal: AppSpace.s2,
                  vertical: AppSpace.s2,
                ),
              ),
              child: Text(
                _actionLabel(status.stage, noConnections: noConnections),
                textAlign: TextAlign.center,
              ),
              onPressed: () {
                if (noConnections) {
                  context.go(AppRoute.connectionImport);
                  return;
                }
                toggleConnection(context, ref);
              },
            ),
          ],
        ),
      ),
    );
  }

  /// Секундный тик заводится и гасится по стадии, а не по появлению виджета.
  void _syncTicker(bool connected) {
    if (connected) {
      _ticker ??= _SecondTicker(() {
        if (mounted) setState(() {});
      })
        ..start();
      return;
    }
    _ticker?.stop();
    _ticker = null;
  }

  /// Узел, через который идёт (или пойдёт) трафик.
  ///
  /// Порядок источников тот же, что на «Подключении»: сперва то, что ДОЛОЖИЛО
  /// ЯДРО (`activeProxy`), потом закреплённый в профиле узел, потом сервер
  /// панельной ветки, и лишь в конце имя самой подписки. Имя узла проходит
  /// через [namingOfProxy] — то же самое, что видно в списке серверов и в
  /// автоподборе; своё «красивое» имя здесь означало бы третье название одной
  /// машины.
  String? _nodeLabel(VpnStatus status) {
    final facts = ref.watch(fleetFactsProvider);
    final live = ref.watch(activeProxyProvider);
    if (status.stage == VpnStage.connected && live != null && live.isNotEmpty) {
      return namingOfProxy(live, facts).title;
    }

    final profile = ref.watch(activeConnectionProfileProvider);
    final pinnedId = profile?.selectedServerId;
    if (profile != null && pinnedId != null && pinnedId.isNotEmpty) {
      for (final s in profile.servers) {
        if (s.id != pinnedId) continue;
        final name = s.name.isEmpty ? s.id : s.name;
        return namingOfProxy(name, facts).title;
      }
    }

    final server = status.server?.name;
    if (server != null && server.isNotEmpty) return server;

    final display = profile?.displayName;
    if (display != null && display.isNotEmpty) return display;
    return null;
  }

  /// Длительность сессии, `чч:мм:сс`. В сайдбаре видно и многочасовые сессии,
  /// поэтому часы здесь есть, в отличие от подписи под дайлом.
  String _session(DateTime? since) {
    if (since == null) return '00:00:00';
    final d = DateTime.now().difference(since);
    final h = d.inHours.toString().padLeft(2, '0');
    final m = (d.inMinutes % 60).toString().padLeft(2, '0');
    final s = (d.inSeconds % 60).toString().padLeft(2, '0');
    return '$h:$m:$s';
  }

  static String _actionLabel(VpnStage stage, {required bool noConnections}) {
    if (noConnections) return DesktopStrings.actionAddConnection;
    return switch (stage) {
      VpnStage.connected => DesktopStrings.actionDisconnect,
      VpnStage.connecting ||
      VpnStage.reconnecting =>
        DesktopStrings.actionCancel,
      VpnStage.disconnected || VpnStage.error => DesktopStrings.actionConnect,
    };
  }

  static Color _dotColor(
    BuildContext context, {
    required VpnStage stage,
    required bool accessBlocked,
    required bool noConnections,
  }) {
    final c = context.c;
    if (noConnections) return c.borderStrong;
    return switch (stage) {
      VpnStage.disconnected => c.borderStrong,
      VpnStage.connecting || VpnStage.reconnecting => c.warning,
      // Туннель поднят, а доступа нет: цвет обязан перестать говорить «всё
      // хорошо» ровно там же, где это говорит слово.
      VpnStage.connected => accessBlocked ? c.warning : c.success,
      VpnStage.error => c.danger,
    };
  }
}

/// Секундный тик без `Timer.periodic` в билде.
///
/// Отдельным крошечным классом, чтобы завод и остановка были одной парой
/// вызовов: таймер, забытый в состоянии, продолжает будить сайдбар после
/// отключения туннеля.
class _SecondTicker {
  final void Function() onTick;

  _SecondTicker(this.onTick);

  Timer? _timer;

  void start() {
    _timer ??= Timer.periodic(const Duration(seconds: 1), (_) => onTick());
  }

  void stop() {
    _timer?.cancel();
    _timer = null;
  }
}

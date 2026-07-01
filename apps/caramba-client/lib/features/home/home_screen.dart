import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/features/notifications/notifications_screen.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/state/subscription_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/colors.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/connect_dial.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/traffic_chart.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Главная (демо §HOME): дисплей-дайл подключения + config-rows
/// (сервер/relay/протокол/маршрут) + 4 ячейки статистики с tabular-цифрами.
class HomeScreen extends ConsumerStatefulWidget {
  const HomeScreen({super.key});

  @override
  ConsumerState<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends ConsumerState<HomeScreen> {
  final ValueNotifier<int> _tick = ValueNotifier<int>(0);
  Timer? _ticker;

  @override
  void dispose() {
    _ticker?.cancel();
    _tick.dispose();
    super.dispose();
  }

  void _startTicker() =>
      _ticker ??= Timer.periodic(const Duration(seconds: 1), (_) => _tick.value++);

  void _stopTicker() {
    _ticker?.cancel();
    _ticker = null;
  }

  String _session(DateTime? since) {
    if (since == null) return '00:00';
    final d = DateTime.now().difference(since);
    final m = (d.inMinutes).toString().padLeft(2, '0');
    final s = (d.inSeconds % 60).toString().padLeft(2, '0');
    return '$m:$s';
  }

  String _fmtBytes(int bytes) {
    final mb = bytes / (1024 * 1024);
    if (mb >= 1024) {
      return '${(mb / 1024).toStringAsFixed(2).replaceAll('.', ',')} ГБ';
    }
    return '${mb.toStringAsFixed(1).replaceAll('.', ',')} МБ';
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;

    ref.listen<VpnStatus>(vpnProvider, (prev, next) {
      if (next.isConnected) {
        _startTicker();
      } else {
        _stopTicker();
      }
    });

    final status = ref.watch(vpnProvider);
    final user = ref.watch(currentUserProvider);
    final recommended = ref.watch(recommendedServerProvider);
    final traffic = ref.watch(trafficProvider).valueOrNull ?? TrafficStats.zero;
    final cfg = ref.watch(coreConfigProvider);
    final protocols = ref.watch(protocolsProvider);
    final modes = ref.watch(routingModesProvider);
    final relays = ref.watch(relaysProvider);
    // Индекс relay мог быть выбран на дефолтном списке; relay-список с панели
    // может быть короче — клампим, чтобы не выйти за границы.
    final relayIdx = relays.isEmpty ? 0 : cfg.relay.clamp(0, relays.length - 1);

    if (status.isConnected) {
      _startTicker();
    } else {
      _stopTicker();
    }

    final server = status.server ?? recommended;
    // План для чипа: активная подписка из /app/subscriptions, иначе /me, иначе
    // активная подписка из /app/subscription, иначе Free.
    final subsList = ref.watch(subscriptionsProvider).valueOrNull;
    String? subPlanName;
    if (subsList != null && subsList.isNotEmpty) {
      final active = subsList.where((s) => s.isActive);
      subPlanName = (active.isNotEmpty ? active.first : subsList.first).name;
    }
    final plan = subPlanName ??
        user?.planName ??
        ref.watch(subscriptionProvider).valueOrNull?.planName ??
        'Free';

    final csub = switch (status.stage) {
      VpnStage.connected => () {
          final relay = relays[relayIdx];
          if (!relay.isOff && !relay.isAuto) {
            return 'Вход: ${relay.name} -> ${server?.name ?? 'сервер'}';
          }
          return server?.name ?? 'Защищено';
        }(),
      VpnStage.connecting || VpnStage.reconnecting =>
        '${server?.name ?? 'Сервер'} · ${protocols[cfg.protocol].name}',
      VpnStage.error => 'Проверьте сеть и нажмите снова',
      VpnStage.disconnected => 'Нажмите, чтобы подключиться',
    };

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: ListView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s20 + AppSpace.s6,
          ),
          children: [
            Row(
              children: [
                Text(kBrandName,
                    style: AppType.titleMd.copyWith(color: c.textHi)),
                const Spacer(),
                _PlanChip(plan: plan),
                const SizedBox(width: AppSpace.s2),
                const NotificationBell(),
              ],
            ),
            const SizedBox(height: AppSpace.s4),
            Center(
              child: ValueListenableBuilder<int>(
                valueListenable: _tick,
                builder: (context, _, __) => ConnectDial(
                  stage: status.stage,
                  subLabel: status.stage == VpnStage.connected
                      ? _session(status.connectedSince)
                      : csub,
                  onTap: () {
                    unawaited(HapticFeedback.mediumImpact());
                    unawaited(ref.read(vpnProvider.notifier).toggle());
                  },
                ),
              ),
            ),
            const SizedBox(height: AppSpace.s5),
            RowsGroup(children: [
              CRow(
                icon: Lucide.globe,
                label: 'Сервер',
                value: server == null ? 'Авто' : server.name,
                chevron: true,
                trailing: server?.countryCode == null
                    ? null
                    : Padding(
                        padding: const EdgeInsets.only(right: AppSpace.s2),
                        child: CodeChip(server!.countryCode!),
                      ),
                onTap: () => context.go(AppRoute.servers),
              ),
              CRow(
                icon: Lucide.waypoints,
                label: 'Relay (вход)',
                value: relays[relayIdx].name,
                chevron: true,
                onTap: () => _pickRelay(),
              ),
              CRow(
                icon: protocols[cfg.protocol].icon,
                label: 'Протокол',
                value: protocols[cfg.protocol].name,
                chevron: true,
                onTap: () => context.go(AppRoute.protocol),
              ),
              CRow(
                icon: Lucide.route,
                label: 'Маршрут',
                value: modes[cfg.route].name,
                chevron: true,
                onTap: () => _pickRoute(),
              ),
            ]),
            const SizedBox(height: AppSpace.s4),
            ValueListenableBuilder<int>(
              valueListenable: _tick,
              builder: (context, _, __) {
                final connected = status.isConnected;
                return _StatsGrid(
                  down: connected ? _fmtBytes(traffic.downTotal) : '0,0 МБ',
                  up: connected ? _fmtBytes(traffic.upTotal) : '0,0 МБ',
                  latency: connected && server?.pingMs != null
                      ? '${server!.pingMs} мс'
                      : '·',
                  session: connected ? _session(status.connectedSince) : '00:00',
                );
              },
            ),
            const SizedBox(height: AppSpace.s4),
            SectionTitle('Трафик',
                padding: const EdgeInsets.only(bottom: AppSpace.s3)),
            ref.watch(trafficHistoryProvider).when(
                  data: (points) => TrafficChart(points: points),
                  loading: () => const TrafficChart(points: []),
                  error: (_, __) => const TrafficChart(points: []),
                ),
          ],
        ),
      ),
    );
  }

  Future<void> _pickRelay() async {
    final relays = ref.read(relaysProvider);
    final cfg = ref.read(coreConfigProvider);
    final sel =
        relays.isEmpty ? 0 : cfg.relay.clamp(0, relays.length - 1);
    final i = await showPickerSheet(
      context: context,
      title: 'Relay (вход)',
      subtitle:
          'Через какую страну идёт вход в цепочку. Удобно в России: вход через устойчивую страну, выход где нужно.',
      options: relays
          .map((r) => (name: r.name, desc: r.desc, icon: null as String?))
          .toList(),
      selected: sel,
    );
    if (i != null && mounted) {
      ref.read(coreConfigProvider.notifier).setRelay(i);
      showCarambaToast(context, 'Relay: ${relays[i].name}');
    }
  }

  Future<void> _pickRoute() async {
    final modes = ref.read(routingModesProvider);
    final cfg = ref.read(coreConfigProvider);
    final i = await showPickerSheet(
      context: context,
      title: 'Маршрутизация',
      subtitle: 'Что идёт через VPN, а что напрямую.',
      options: modes
          .map((m) => (name: m.name, desc: m.desc, icon: m.icon as String?))
          .toList(),
      selected: cfg.route,
    );
    if (i != null && mounted) {
      ref.read(coreConfigProvider.notifier).setRoute(i);
      showCarambaToast(context, 'Маршрут: ${modes[i].name}');
    }
  }
}

class _PlanChip extends StatelessWidget {
  final String plan;
  const _PlanChip({required this.plan});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: c.borderStrong),
      ),
      child: Text(plan, style: AppType.monoSm.copyWith(color: c.textMed)),
    );
  }
}

class _StatsGrid extends StatelessWidget {
  final String down;
  final String up;
  final String latency;
  final String session;
  const _StatsGrid({
    required this.down,
    required this.up,
    required this.latency,
    required this.session,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      decoration: BoxDecoration(
        color: c.borderSubtle,
        borderRadius: AppRadius.r14,
        border: Border.all(color: c.borderSubtle),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        children: [
          Row(children: [
            Expanded(child: _cell(c, down, 'Скачано')),
            Container(width: 1, height: 64, color: c.borderSubtle),
            Expanded(child: _cell(c, up, 'Отправлено')),
          ]),
          Container(height: 1, color: c.borderSubtle),
          Row(children: [
            Expanded(child: _cell(c, latency, 'Задержка')),
            Container(width: 1, height: 64, color: c.borderSubtle),
            Expanded(child: _cell(c, session, 'Сессия')),
          ]),
        ],
      ),
    );
  }

  Widget _cell(AppColors c, String value, String key) {
    return Container(
      color: c.surface1,
      padding: const EdgeInsets.symmetric(
          horizontal: AppSpace.s4, vertical: AppSpace.s4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            value,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: AppType.titleLg.copyWith(
              color: c.textHi,
              fontFeatures: AppType.monoMd.fontFeatures,
              fontFamily: 'SF Mono',
              fontFamilyFallback: AppType.monoFallback,
            ),
          ),
          const SizedBox(height: AppSpace.s1),
          Text(
            key.toUpperCase(),
            style: AppType.caption.copyWith(color: c.textLow, letterSpacing: 0.6),
          ),
        ],
      ),
    );
  }
}

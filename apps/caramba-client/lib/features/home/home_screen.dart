import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/atmosphere/atmosphere_layer.dart';
import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/features/notifications/notifications_screen.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/state/settings_state.dart';
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
/// (сервер/relay/протокол/маршрут) + ячейки статистики с tabular-цифрами.
///
/// Экран двухветочный, и ветка выбирается ОДИН раз на билд:
///   * аккаунт панели — сегодняшняя Home: план, колокол, relay, рекомендованный
///     сервер, история трафика;
///   * generic-режим (своя подписка, сессии панели нет) — те же дайл и
///     атмосфера, но данные берутся из активного [ConnectionProfile] и из
///     потока статистики ядра. Панельные провайдеры в этой ветке НЕ
///     подписываются вовсе: за ними нет данных, а запросы ушли бы в 401.
class HomeScreen extends ConsumerStatefulWidget {
  const HomeScreen({super.key});

  @override
  ConsumerState<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends ConsumerState<HomeScreen> {
  final ValueNotifier<int> _tick = ValueNotifier<int>(0);
  Timer? _ticker;

  // The atmosphere layer is registered to the real layout: the chart's home
  // station is the dial, and the boundary bottom plus the quiet lens come from
  // the laid-out connect block. Measured after layout, then only when the
  // result actually moves (text scale, rotation), so this does not loop.
  final GlobalKey _layerKey = GlobalKey();
  final GlobalKey _dialKey = GlobalKey();
  final GlobalKey _labelKey = GlobalKey();
  final GlobalKey _headerKey = GlobalKey();
  final ScrollController _scroll = ScrollController();
  AtmosphereAnchor _anchor = AtmosphereAnchor.unmeasured;

  @override
  void dispose() {
    _ticker?.cancel();
    _tick.dispose();
    _scroll.dispose();
    super.dispose();
  }

  /// Anchors the chart to where the dial sits at scroll offset zero. The
  /// atmosphere is a background layer and does not scroll with the list, so the
  /// current scroll offset is taken back out of the measurement.
  void _measureAnchor() {
    final layer = _layerKey.currentContext?.findRenderObject();
    final dial = _dialKey.currentContext?.findRenderObject();
    final label = _labelKey.currentContext?.findRenderObject();
    final header = _headerKey.currentContext?.findRenderObject();
    if (layer is! RenderBox ||
        dial is! RenderBox ||
        label is! RenderBox ||
        header is! RenderBox) {
      return;
    }
    if (!layer.hasSize || !dial.hasSize || !label.hasSize) return;
    final shift = Offset(0, _scroll.hasClients ? _scroll.offset : 0.0);
    final dialTopLeft = dial.localToGlobal(Offset.zero, ancestor: layer);
    final labelTopLeft = label.localToGlobal(Offset.zero, ancestor: layer);
    final headerTopLeft = header.localToGlobal(Offset.zero, ancestor: layer);
    final next = AtmosphereAnchor(
      dialCenter: dialTopLeft + dial.size.center(Offset.zero) + shift,
      labelRect: (labelTopLeft + shift) & label.size,
      headerBottom: headerTopLeft.dy + header.size.height + shift.dy,
    );
    if (next != _anchor && mounted) setState(() => _anchor = next);
  }

  void _startTicker() => _ticker ??= Timer.periodic(
    const Duration(seconds: 1),
    (_) => _tick.value++,
  );

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

  /// Мгновенная скорость. Отдельный формат: байты в секунду читаются в КБ/с,
  /// а не в МБ с одним знаком, иначе весь трафик выглядит как «0,0».
  String _fmtRate(int bytesPerSecond) {
    if (bytesPerSecond <= 0) return '0 КБ/с';
    final kb = bytesPerSecond / 1024;
    if (kb >= 1024) {
      return '${(kb / 1024).toStringAsFixed(1).replaceAll('.', ',')} МБ/с';
    }
    return '${kb.toStringAsFixed(0)} КБ/с';
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
    final traffic = ref.watch(trafficProvider).valueOrNull ?? TrafficStats.zero;
    final cfg = ref.watch(coreConfigProvider);
    final protocols = ref.watch(protocolsProvider);
    final modes = ref.watch(routingModesProvider);

    // Ветка экрана. Гость — это «пускаем без аккаунта панели» И «сессии панели
    // сейчас нет»: залогиненный пользователь со своими профилями остаётся на
    // панельной Home. Пока профили не прочитаны, выбор делается в пользу
    // generic-ветки: без сессии панельные запросы всё равно ушли бы в 401, а
    // мигать ими на холодном старте незачем.
    final panelSession =
        ref.watch(authProvider).stage == AuthStage.authenticated;
    final guest =
        !panelSession &&
        (ref.watch(guestAllowedProvider) ||
            !ref.watch(connectionProfilesReadyProvider));

    if (status.isConnected) {
      _startTicker();
    } else {
      _stopTicker();
    }

    WidgetsBinding.instance.addPostFrameCallback((_) => _measureAnchor());

    final String csub;
    final List<Widget> cards;
    final Widget headerTrailing;
    if (guest) {
      final profile = ref.watch(activeConnectionProfileProvider);
      final proxy = ref.watch(activeProxyProvider);
      final node = _pinnedNodeName(profile);
      csub = _guestSubLabel(
        status: status,
        profile: profile,
        proxy: proxy,
        node: node,
        protocol: protocols[cfg.protocol].name,
      );
      // Колокола и плана здесь нет, но высота шапки обязана остаться прежней:
      // атмосферный слой зарегистрирован на измеренную геометрию, и сдвиг дайла
      // вверх ломает порядок «зажигания» маршрутов (kAtmoOpenRank).
      headerTrailing = const SizedBox(height: 44);
      cards = _guestCards(
        status: status,
        traffic: traffic,
        profile: profile,
        proxy: proxy,
        node: node,
        cfg: cfg,
        protocols: protocols,
        modes: modes,
      );
    } else {
      final user = ref.watch(currentUserProvider);
      final recommended = ref.watch(recommendedServerProvider);
      final relays = ref.watch(relaysProvider);
      final server = status.server ?? recommended;
      // Индекс relay мог быть выбран на дефолтном списке; relay-список с панели
      // может быть короче — клампим, чтобы не выйти за границы.
      final relayIdx = relays.isEmpty
          ? 0
          : cfg.relay.clamp(0, relays.length - 1);
      // План для чипа: активная подписка из /app/subscriptions, иначе /me,
      // иначе активная подписка из /app/subscription, иначе Free.
      final subsList = ref.watch(subscriptionsProvider).valueOrNull;
      String? subPlanName;
      if (subsList != null && subsList.isNotEmpty) {
        final active = subsList.where((s) => s.isActive);
        subPlanName = (active.isNotEmpty ? active.first : subsList.first).name;
      }
      final plan =
          subPlanName ??
          user?.planName ??
          ref.watch(subscriptionProvider).valueOrNull?.planName ??
          'Free';

      csub = switch (status.stage) {
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
      headerTrailing = Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _PlanChip(plan: plan),
          const SizedBox(width: AppSpace.s2),
          const NotificationBell(),
        ],
      );
      cards = _panelCards(
        status: status,
        traffic: traffic,
        server: server,
        relays: relays,
        relayIdx: relayIdx,
        cfg: cfg,
        protocols: protocols,
        modes: modes,
      );
    }

    final needsReconnect = ref.watch(reconnectRequiredProvider);
    final proxyEndpoint = ref.watch(proxyEndpointProvider);

    return Scaffold(
      backgroundColor: c.bgBase,
      body: Stack(
        children: [
          // The chart sits full-bleed behind Home. It is decorative, excluded
          // from semantics, and reports nothing the dial does not already say.
          Positioned.fill(
            child: AtmosphereLayer(
              key: _layerKey,
              stage: status.stage,
              anchor: _anchor,
            ),
          ),
          Positioned.fill(
            child: SafeArea(
              bottom: false,
              child: ListView(
                controller: _scroll,
                padding: const EdgeInsets.fromLTRB(
                  AppSpace.s5,
                  AppSpace.s5,
                  AppSpace.s5,
                  AppSpace.s20 + AppSpace.s6,
                ),
                children: [
                  Row(
                    key: _headerKey,
                    children: [
                      // Operator brand names can be long; the row must not
                      // overflow on a narrow phone at a large text scale.
                      Flexible(
                        child: Text(
                          kBrandName,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: AppType.titleMd.copyWith(color: c.textHi),
                        ),
                      ),
                      const Spacer(),
                      headerTrailing,
                    ],
                  ),
                  // The chart needs its designed headroom above the dial: the
                  // upper stations and the boundary top edge sit in this gap.
                  const SizedBox(height: AppSpace.s8),
                  Center(
                    child: ValueListenableBuilder<int>(
                      valueListenable: _tick,
                      builder: (context, _, __) => ConnectDial(
                        dialKey: _dialKey,
                        labelKey: _labelKey,
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
                  // Proxy-режим не перехватывает трафик системы: адрес локального
                  // инбаунда нужно видеть, чтобы прописать его в браузере/системе.
                  if (proxyEndpoint != null) ...[
                    const SizedBox(height: AppSpace.s2),
                    Text(
                      'Прокси $proxyEndpoint',
                      textAlign: TextAlign.center,
                      style: AppType.monoSm.copyWith(color: c.textLow),
                    ),
                  ],
                  // The cards keep their own opaque surfaces; this is the seam
                  // where the chart slides under the content so it never fights
                  // the stats.
                  _CardsBackdrop(
                    children: [
                      // Правка настроек при поднятом туннеле применяется со
                      // следующего Up — баннер стоит первым в контенте, до
                      // самих настроек. Выше дайла его ставить нельзя: связка
                      // Home с атмосферой держится на измеренной геометрии
                      // шапки и дайла, и любой сдвиг ломает её инвариант.
                      if (needsReconnect) ...[
                        const ReconnectBanner(),
                        const SizedBox(height: AppSpace.s4),
                      ],
                      ...cards,
                    ],
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  /// Панельная Home: сервер/relay/протокол/маршрут, 4 ячейки и история трафика.
  List<Widget> _panelCards({
    required VpnStatus status,
    required TrafficStats traffic,
    required Server? server,
    required List<Relay> relays,
    required int relayIdx,
    required CoreConfig cfg,
    required List<ProtocolOption> protocols,
    required List<RoutingMode> modes,
  }) {
    return [
      RowsGroup(
        children: [
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
        ],
      ),
      const SizedBox(height: AppSpace.s4),
      ValueListenableBuilder<int>(
        valueListenable: _tick,
        builder: (context, _, __) {
          final connected = status.isConnected;
          return _StatsGrid(
            cells: [
              (
                value: connected ? _fmtBytes(traffic.downTotal) : '0,0 МБ',
                label: 'Скачано',
              ),
              (
                value: connected ? _fmtBytes(traffic.upTotal) : '0,0 МБ',
                label: 'Отправлено',
              ),
              (
                value: connected && server?.pingMs != null
                    ? '${server!.pingMs} мс'
                    : '·',
                label: 'Задержка',
              ),
              (
                value: connected ? _session(status.connectedSince) : '00:00',
                label: 'Сессия',
              ),
            ],
          );
        },
      ),
      const SizedBox(height: AppSpace.s4),
      const SectionTitle(
        'Трафик',
        padding: EdgeInsets.only(bottom: AppSpace.s3),
      ),
      ref
          .watch(trafficHistoryProvider)
          .when(
            data: (points) => TrafficChart(points: points),
            loading: () => const TrafficChart(points: []),
            error: (_, __) => const TrafficChart(points: []),
          ),
    ];
  }

  /// Generic-режим: подписка и узел берутся с активного профиля, статистика —
  /// из потока ядра. Ни квота-карты, ни плановых чипов здесь нет: тарифы живут
  /// у аккаунта панели, а его нет.
  List<Widget> _guestCards({
    required VpnStatus status,
    required TrafficStats traffic,
    required ConnectionProfile? profile,
    required String? proxy,
    required String? node,
    required CoreConfig cfg,
    required List<ProtocolOption> protocols,
    required List<RoutingMode> modes,
  }) {
    final connected = status.isConnected;
    final count = profile?.serverCount ?? 0;
    final base = node ?? 'Авто';
    // «Плюс активный узел ядра»: селектор мог встать не на закреплённый узел
    // (авто-выбор), и тогда важно показать оба.
    final serverValue = (connected && proxy != null && proxy.isNotEmpty)
        ? (proxy == base ? base : '$base · $proxy')
        : base;

    return [
      RowsGroup(
        children: [
          CRow(
            icon: Lucide.layers,
            label: 'Подписка',
            value: (profile == null || profile.displayName.isEmpty)
                ? 'Не выбрана'
                : profile.displayName,
            chevron: true,
            trailing: count == 0
                ? null
                : Padding(
                    padding: const EdgeInsets.only(right: AppSpace.s2),
                    child: Tag('узлов: $count'),
                  ),
            onTap: () => context.go(AppRoute.connections),
          ),
          CRow(
            icon: Lucide.globe,
            label: 'Сервер',
            value: serverValue,
            chevron: true,
            onTap: () => context.go(AppRoute.servers),
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
        ],
      ),
      const SizedBox(height: AppSpace.s4),
      ValueListenableBuilder<int>(
        valueListenable: _tick,
        builder: (context, _, __) {
          final mode = ref.watch(activeTunnelModeProvider);
          return _StatsGrid(
            cells: [
              (
                value: connected ? _fmtBytes(traffic.downTotal) : '0,0 МБ',
                label: 'Скачано',
              ),
              (
                value: connected ? _fmtBytes(traffic.upTotal) : '0,0 МБ',
                label: 'Отправлено',
              ),
              (
                value: connected ? _fmtRate(traffic.downBps) : '0 КБ/с',
                label: 'Приём',
              ),
              (
                value: connected ? _fmtRate(traffic.upBps) : '0 КБ/с',
                label: 'Отдача',
              ),
              (
                value: connected ? _session(status.connectedSince) : '00:00',
                label: 'Сессия',
              ),
              (
                value: switch (mode) {
                  TunnelMode.tun => 'TUN',
                  TunnelMode.proxy => 'Прокси',
                  null => '·',
                },
                label: 'Режим',
              ),
            ],
          );
        },
      ),
    ];
  }

  /// Подпись под дайлом в generic-режиме. Панельных имён серверов здесь нет:
  /// говорим узлом подписки, именем профиля и протоколом.
  String _guestSubLabel({
    required VpnStatus status,
    required ConnectionProfile? profile,
    required String? proxy,
    required String? node,
    required String protocol,
  }) {
    final name = (profile == null || profile.displayName.isEmpty)
        ? null
        : profile.displayName;
    return switch (status.stage) {
      VpnStage.connected => proxy ?? node ?? name ?? 'Защищено',
      VpnStage.connecting ||
      VpnStage.reconnecting => '${node ?? name ?? 'Узел'} · $protocol',
      VpnStage.error => 'Проверьте сеть и нажмите снова',
      VpnStage.disconnected =>
        profile == null
            ? 'Импортируйте подписку'
            : 'Нажмите, чтобы подключиться',
    };
  }

  /// Имя закреплённого узла подписки. `null` — пин не стоит (авто-выбор ядром)
  /// или узла с таким id в кэше больше нет.
  String? _pinnedNodeName(ConnectionProfile? profile) {
    final id = profile?.selectedServerId;
    if (profile == null || id == null || id.isEmpty) return null;
    for (final s in profile.servers) {
      if (s.id == id) return s.name.isEmpty ? s.id : s.name;
    }
    return id;
  }

  Future<void> _pickRelay() async {
    final relays = ref.read(relaysProvider);
    final cfg = ref.read(coreConfigProvider);
    final sel = relays.isEmpty ? 0 : cfg.relay.clamp(0, relays.length - 1);
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

/// The plane the config and stats cards sit on. The chart never deviates more
/// than about 7 percent from the base plane, but stacking it under a dense grid
/// of numbers still costs legibility, so the atmosphere is damped to under a
/// fifth from the first card down. The short top ramp keeps it from reading as
/// a box edge.
class _CardsBackdrop extends StatelessWidget {
  final List<Widget> children;
  const _CardsBackdrop({required this.children});

  @override
  Widget build(BuildContext context) {
    final base = context.c.bgBase;
    return Container(
      padding: const EdgeInsets.only(top: AppSpace.s5),
      decoration: BoxDecoration(
        gradient: LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [
            base.withValues(alpha: 0),
            base.withValues(alpha: 0.82),
            base.withValues(alpha: 0.82),
          ],
          stops: const [0, 0.055, 1],
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: children,
      ),
    );
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

/// Одна ячейка сетки статистики: крупное tabular-число и подпись капсом.
typedef StatCell = ({String value, String label});

/// Сетка статистики в две колонки. Число ячеек чётное: панельная Home даёт
/// четыре, generic — шесть (к объёмам добавляются мгновенные скорости и режим
/// захвата трафика, которых у панельной ветки нет).
class _StatsGrid extends StatelessWidget {
  final List<StatCell> cells;
  const _StatsGrid({required this.cells});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final rows = <Widget>[];
    for (var i = 0; i + 1 < cells.length; i += 2) {
      if (rows.isNotEmpty) {
        rows.add(Container(height: 1, color: c.borderSubtle));
      }
      rows.add(
        Row(
          children: [
            Expanded(child: _cell(c, cells[i])),
            Container(width: 1, height: 64, color: c.borderSubtle),
            Expanded(child: _cell(c, cells[i + 1])),
          ],
        ),
      );
    }
    return Container(
      decoration: BoxDecoration(
        color: c.borderSubtle,
        borderRadius: AppRadius.r14,
        border: Border.all(color: c.borderSubtle),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(children: rows),
    );
  }

  Widget _cell(AppColors c, StatCell cell) {
    return Container(
      color: c.surface1,
      padding: const EdgeInsets.symmetric(
        horizontal: AppSpace.s4,
        vertical: AppSpace.s4,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            cell.value,
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
            cell.label.toUpperCase(),
            style: AppType.caption.copyWith(
              color: c.textLow,
              letterSpacing: 0.6,
            ),
          ),
        ],
      ),
    );
  }
}

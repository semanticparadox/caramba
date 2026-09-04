import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/atmosphere/atmosphere_layer.dart';
import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/csm/config_age_card.dart';
import 'package:caramba_client/features/csm/keep_or_revert_card.dart';
import 'package:caramba_client/features/notifications/notifications_screen.dart';
import 'package:caramba_client/features/servers/relay_screen.dart'
    show effectiveRelayIndex;
import 'package:caramba_client/features/settings/applied_route_card.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/features/settings/route_picker.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
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
/// Подпись под дайлом на ПАНЕЛЬНОМ пути.
///
/// Вынесена из виджета намеренно: единственное нетривиальное решение здесь —
/// имеет ли приложение право утверждать цепочку, и проверять его наблюдением за
/// экраном нельзя. Дайл в состоянии [VpnStage.connected] подставляет таймер
/// сессии вместо этой строки, так что на самом экране она в этом состоянии
/// сейчас не видна вовсе — а вычислялась и была ложью.
///
/// «Вход: X -> Y» — это УТВЕРЖДЕНИЕ о цепочке. Тело clash, которое читает ядро,
/// цепочку выразить не может (`dialer-proxy` в нём нет), панель на живом флоте
/// говорит `chained_in_config: false`, и баннер двумя строками ниже это уже
/// сообщает — а заголовок утверждал обратное. Право на стрелку даёт только
/// подтверждённая возможность: «неизвестно» её не даёт, потому что утверждение,
/// которого никто не подтвердил, — та же ложь, что и опровергнутое.
String panelDialSubtitle({
  required VpnStage stage,
  required Relay? relay,
  required Availability chaining,
  required String? serverName,
  required String protocolName,
}) {
  switch (stage) {
    case VpnStage.connected:
      final chained =
          chaining.isAvailable &&
          relay != null &&
          !relay.isOff &&
          !relay.isAuto;
      if (chained) {
        return 'Вход: ${relay.name} -> ${serverName ?? 'сервер'}';
      }
      return serverName ?? 'Защищено';
    case VpnStage.connecting:
    case VpnStage.reconnecting:
      return '${serverName ?? 'Сервер'} · $protocolName';
    case VpnStage.error:
      return 'Проверьте сеть и нажмите снова';
    case VpnStage.disconnected:
      return 'Нажмите, чтобы подключиться';
  }
}

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
      // может быть короче. Приведение общее с кодировщиком провода, а не кламп:
      // кламп называл последнюю строку списка там, где ядру уходило «входа не
      // выбрано». См. [effectiveRelayIndex].
      final relayIdx = effectiveRelayIndex(cfg.relay, relays);
      final relay = relayIdx < 0 ? null : relays[relayIdx];
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

      csub = panelDialSubtitle(
        stage: status.stage,
        relay: relay,
        chaining: ref.watch(capabilitiesProvider).relayChaining.availability,
        serverName: server?.name,
        protocolName: protocols[cfg.protocol].name,
      );
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
        relay: relay,
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
                      // INV-21 и INV-22 живут здесь же, ниже дайла: возраст
                      // конфигурации, липкая ошибка и карточки «Оставить или
                      // Вернуть». Выше дайла их ставить нельзя, атмосферный
                      // слой зарегистрирован на измеренную геометрию шапки и
                      // дайла и любой сдвиг ломает его инвариант.
                      const CsmConfigAgeCard(),
                      const CsmPendingChangesSection(),
                      ...cards,
                      // Что ядро ФАКТИЧЕСКИ применило. Стоит после карточек
                      // выбора намеренно: сначала то, что пользователь
                      // попросил, следом — что из этого получилось. До моста
                      // отчёта второй половины не существовало вовсе, и
                      // «блок рекламы» оставался обещанием.
                      const AppliedRouteCard(),
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

  /// Ячейка задержки: число вместе с тем, кто его получил.
  ///
  /// Собственный замер приложения вытесняет число оператора, как только он
  /// появляется; до этого показывается операторское, но подписью «пинг узла»,
  /// а не «задержка» — узел меряет расстояние до СВОЕЙ цели, и выдавать это за
  /// расстояние пользователя нельзя. Замер, идущий прямо сейчас, — тоже
  /// состояние, а не пустота.
  StatCell _latencyCell({required bool connected, required Server? server}) {
    if (!connected || server == null) {
      return (value: '·', label: 'Задержка');
    }
    final node = ref
        .watch(exitInventoryProvider)
        .nodes
        .where((ExitNode n) => n.panelNodeId == server.id)
        .firstOrNull;
    final latency =
        node?.latency ??
        (server.pingMs == null
            ? Latency.none
            : Latency.fromOperator(server.pingMs!));
    return switch (latency.source) {
      LatencySource.client => (
        value: latency.isTimeout ? 'нет' : '${latency.ms} мс',
        label: 'Ваш пинг',
      ),
      LatencySource.operator => (
        value: latency.isTimeout ? 'нет' : '${latency.ms} мс',
        label: 'Пинг узла',
      ),
      LatencySource.measuring => (value: '…', label: 'Меряю пинг'),
      LatencySource.none => (value: '·', label: 'Задержка'),
    };
  }

  /// Панельная Home: сервер/relay/протокол/маршрут, 4 ячейки и история трафика.
  List<Widget> _panelCards({
    required VpnStatus status,
    required TrafficStats traffic,
    required Server? server,
    required Relay? relay,
    required CoreConfig cfg,
    required List<ProtocolOption> protocols,
    required List<RoutingMode> modes,
  }) {
    // Строка сервера говорит СТРАНОЙ: узел под ней меняется автоподбором, а
    // выбирает пользователь именно страну. Имя узла остаётся рядом вторичным —
    // без него не видно, куда автоподбор в итоге встал.
    //
    // Страну называет [exitHeadlineProvider], а не пин профиля: закреплённая
    // страна без свободных узлов connect не останавливает, и заголовок обязан
    // назвать ту страну, через которую трафик выходит, а не ту, которую
    // пользователь когда-то выбрал.
    final headline = ref.watch(exitHeadlineProvider);
    // Флаг берётся у страны инвентаря, а не выводится здесь из кода: там уже
    // решено, твёрдая ли это страна, и второе решение на том же коде поставило
    // бы на Home флаг, которого нет на экране серверов.
    final exitFlag = ref
        .watch(exitInventoryProvider)
        .locationOf(headline.countryCode)
        ?.flag;
    final country = (exitFlag == null || exitFlag == kNeutralFlag)
        ? headline.title
        : '$exitFlag ${headline.title}';
    return [
      RowsGroup(
        children: [
          CRow(
            icon: Lucide.globe,
            label: 'Сервер',
            value: country,
            chevron: true,
            trailing: server == null
                ? null
                : Padding(
                    padding: const EdgeInsets.only(right: AppSpace.s2),
                    child: ConstrainedBox(
                      constraints: const BoxConstraints(maxWidth: 132),
                      child: Text(
                        server.name,
                        overflow: TextOverflow.ellipsis,
                        textAlign: TextAlign.right,
                        style: AppType.bodySm.copyWith(
                          color: context.c.textLow,
                        ),
                      ),
                    ),
                  ),
            onTap: () => context.go(AppRoute.servers),
          ),
          _relayRow(relay: relay),
          CRow(
            icon: protocols[cfg.protocol].icon,
            label: 'Протокол',
            value: protocols[cfg.protocol].name,
            chevron: true,
            onTap: () => context.go(AppRoute.protocol),
          ),
          CRow(
            icon: Lucide.route,
            // Не «Маршрут»: владелец открыл эту строку в поисках выбора входа
            // и увидел название страны. Правила и страна входа — разные вещи,
            // и строка обязана называть свою.
            label: 'Маршрут (правила)',
            value: modes[cfg.route].name,
            chevron: true,
            onTap: () => _pickRoute(),
          ),
        ],
      ),
      if (!ref
          .watch(capabilitiesProvider)
          .relayChaining
          .availability
          .isAvailable) ...[
        const SizedBox(height: AppSpace.s3),
        InlineBanner(
          glyph: Lucide.waypoints,
          text: ref
              .watch(capabilitiesProvider)
              .relayChaining
              .availability
              .message,
        ),
      ],
      if (headline.diverged) ...[
        const SizedBox(height: AppSpace.s3),
        // Подмену выбора нельзя проводить молча: заголовок уже говорит правду о
        // стране, но без этой строки правда выглядела бы как «настройка сама
        // сбросилась». Причина названа тут же, рядом со строкой «Сервер».
        InlineBanner(glyph: Lucide.globe, text: headline.divergenceMessage),
      ],
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
              // Ячейка называет АВТОРА числа. Прежде здесь стояло
              // `server.pingMs` под подписью «Задержка» — то есть панельный
              // `nodes.last_latency`, RTT самого узла до его цели по heartbeat
              // раз в ~30 с, выданный за задержку пользователя. Числа разной
              // природы под одной подписью неотличимы, поэтому подпись теперь
              // говорит, чей это замер.
              _latencyCell(connected: connected, server: server),
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
    // МАШИНЫ, а не строки конфига. `profile.serverCount` — это длина списка
    // прокси, то есть по строке на каждый инбаунд каждой машины: у живой
    // подписки это 13 при двух машинах, и Home повторял ровно ту жалобу
    // владельца («восемь серверов»), от которой экран серверов уже избавлен.
    // Число берётся из того же слоя предложения, что и там: одна группа —
    // одна машина.
    final machines = ref.watch(offeringProvider).exits.length;
    final base = node ?? 'Авто';
    // «Плюс активный узел ядра»: селектор мог встать не на закреплённый узел
    // (авто-выбор), и тогда важно показать оба.
    final serverValue = (connected && proxy != null && proxy.isNotEmpty)
        ? (proxy == base ? base : '$base · $proxy')
        : base;
    // Цепочка через вход на сыром конфиге не собирается. Строку всё равно
    // показываем: спрятанная, она неотличима от «такой настройки не бывает», и
    // пользователь ищет её в обновлении приложения. Причина берётся из
    // возможности слоя предложения (`Capabilities.relayChaining`), а не
    // сочиняется здесь.
    final chaining = ref.watch(capabilitiesProvider).relayChaining.availability;

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
            trailing: machines == 0
                ? null
                : Padding(
                    padding: const EdgeInsets.only(right: AppSpace.s2),
                    // Слово то же, что на экране серверов, и считает то же
                    // самое: разойтись им нельзя.
                    child: Tag('узлов: $machines'),
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
          // Имя входа берём из [Relay.defaults], а не из [relaysProvider]:
          // тот тянет список у панели, а эта ветка в панель не ходит.
          _relayRow(
            relay:
                Relay.defaults[effectiveRelayIndex(cfg.relay, Relay.defaults)],
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
            label: 'Маршрут (правила)',
            value: modes[cfg.route].name,
            chevron: true,
            onTap: () => _pickRoute(),
          ),
        ],
      ),
      if (!chaining.isAvailable) ...[
        const SizedBox(height: AppSpace.s3),
        // Причина относится к ЦЕПОЧКЕ, а не к строке входа: сама строка живая
        // («Выкл» и «Авто» истинны при любом источнике). Места под подпись у
        // CRow нет, поэтому причина едет отдельной строкой под группой.
        InlineBanner(glyph: Lucide.waypoints, text: chaining.message),
      ],
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

  /// Строка входа, одинаковая в обеих ветках Home.
  ///
  /// Строка называет ЗНАЧЕНИЕ, которое сейчас в силе, и всегда ведёт на экран
  /// входа. Раньше при недоступной цепочке она говорила «Недоступно» и не
  /// нажималась — а это была неправда сразу дважды: «Выкл» и «Авто» доступны
  /// при любом флоте, и именно через эту строку лежит единственный путь к
  /// экрану, на котором чужой или устаревший вход можно снять. Причину
  /// недоступности цепочки несёт баннер под группой, а не подмена значения.
  ///
  /// Приглушение осталось ровно для одного случая: в силе настоящий вход
  /// оператора, а собрать цепочку источник не может — значит записанное
  /// значение не исполняется, и молчать об этом нельзя.
  Widget _relayRow({required Relay? relay}) {
    final a = ref.watch(capabilitiesProvider).relayChaining.availability;
    final ignored =
        a.isUnavailable && relay != null && !relay.isOff && !relay.isAuto;
    return Opacity(
      opacity: ignored ? 0.45 : 1,
      child: CRow(
        icon: Lucide.waypoints,
        label: 'Relay (вход)',
        value: relay?.name ?? '·',
        chevron: true,
        onTap: () => context.go(AppRoute.relay),
      ),
    );
  }

  /// Лист маршрутов общий с настройками. Своей копии здесь больше нет: она
  /// открывала список БЕЗ карты недоступного, то есть предлагала маршруты,
  /// которых оператор не предлагает.
  Future<void> _pickRoute() => showRoutePicker(context, ref);
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

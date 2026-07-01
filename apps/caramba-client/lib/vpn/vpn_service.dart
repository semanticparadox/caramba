import 'dart:async';

import 'package:flutter/services.dart';

import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Абстракция VPN-движка. За ней стоит Go-ядро (mihomo / Clash.Meta через
/// gomobile) на мобильных платформах и [MockVpnConnection] на desktop/dev.
///
/// Контракт намеренно узкий — UI и `vpnProvider` зависят только от него.
abstract interface class VpnConnection {
  /// Поток снимков состояния. Должен эмитить текущее состояние новому подписчику
  /// (broadcast + кэш последнего значения).
  Stream<VpnStatus> get status;

  /// Поток статистики трафика (~1 тик/с в connected, иначе нули/тишина).
  Stream<TrafficStats> get traffic;

  /// Последнее известное состояние (синхронно, для первичного рендера).
  VpnStatus get currentStatus;

  /// Поднять туннель к [server]. Ядро само тянет mihomo/clash-конфиг панели
  /// (`Subscription.clashUrl`) — конкретику конфигурации UI не знает.
  ///
  /// Это путь panelAccount-профиля: перед поднятием ядро авторизуется
  /// (`configure`) и поднимает туннель к выбранному узлу подписки.
  Future<void> connect(Server server);

  /// Поднять туннель из импортированной raw-подписки (профиль типа rawSub).
  ///
  /// Аддитивный путь (контракт F): сырые данные подписки [raw] в формате
  /// [format] (`auto`/`clash`/`singbox`/`v2ray`/`uri`) передаются ядру, которое
  /// парсит их в mihomo-конфиг (`mobile.ImportSubscription`) и поднимает туннель
  /// без обращения к панели. У rawSub нет узла подписки — отсюда [label] лишь
  /// для отображения (имя профиля), а connect идёт с пустым serverId.
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
  });

  /// Опустить туннель.
  Future<void> disconnect();

  /// Освободить ресурсы (каналы/контроллеры).
  Future<void> dispose();
}

/// Синтетический [Server] для отображения rawSub-профиля в статусе туннеля.
///
/// У импортированной подписки нет узла из `GET /servers` (id/пинг/нагрузка), но
/// пайплайн статуса несёт `Server?` для подписи на орбе. Здесь собирается
/// плейсхолдер с именем профиля и нейтральными полями; id<0 помечает его как
/// не-панельный (никакой узел подписки за ним не стоит).
Server _rawProfileServer(String label) => Server(
      id: -1,
      name: label,
      status: 'online',
    );

/// Данные для одноразовой авторизации Go-ядра перед поднятием туннеля.
///
/// Ядро (caramba-core) имеет собственный auth-store и само тянет mihomo-конфиг
/// по подписке, поэтому ему нужно передать JWT-доступ, UUID подписки и базовый
/// URL панели. Источник значений — TokenStore (JWT) и `GET /subscription`.
class VpnConfig {
  /// Базовый URL панели (`kApiBaseUrl`), без `/api/v2/app`.
  final String panelUrl;

  /// UUID подписки — основа config-URL, который ядро тянет с caramba-sub.
  final String subscriptionUuid;

  /// JWT access-токен текущей сессии (ядро переиспользует его, без ре-логина).
  final String accessToken;

  const VpnConfig({
    required this.panelUrl,
    required this.subscriptionUuid,
    required this.accessToken,
  });

  Map<String, dynamic> toArgs() => {
        'panelUrl': panelUrl,
        'subscriptionUuid': subscriptionUuid,
        'accessToken': accessToken,
      };

  /// Можно ли вообще конфигурировать ядро (все поля заполнены).
  bool get isComplete =>
      panelUrl.isNotEmpty &&
      subscriptionUuid.isNotEmpty &&
      accessToken.isNotEmpty;
}

/// Резолвер конфигурации ядра: лениво (на момент connect) достаёт свежие
/// JWT + UUID подписки + URL панели. Возвращает `null`, если данных пока нет
/// (не залогинены / подписка не загрузилась) — тогда configure пропускается.
typedef VpnConfigResolver = Future<VpnConfig?> Function();

/// Реализация поверх платформенных каналов `com.caramba/vpn`.
///
/// Нативная сторона (Android/iOS, бэк — caramba-core через gomobile) выставляет:
///   * MethodChannel `com.caramba/vpn` — методы `connect`, `disconnect`, `status`;
///   * EventChannel  `com.caramba/vpn/status`  — поток снимков [VpnStatus];
///   * EventChannel  `com.caramba/vpn/traffic` — поток [TrafficStats].
///
/// На desktop, где нативного бэка пока нет, использовать [MockVpnConnection].
class MethodChannelVpnConnection implements VpnConnection {
  static const _method = MethodChannel('com.caramba/vpn');
  static const _statusEvents = EventChannel('com.caramba/vpn/status');
  static const _trafficEvents = EventChannel('com.caramba/vpn/traffic');

  final _statusCtrl = StreamController<VpnStatus>.broadcast();
  VpnStatus _last = const VpnStatus.disconnected();
  Server? _target;

  StreamSubscription<dynamic>? _statusSub;

  /// Резолвер JWT + UUID подписки + URL панели для авторизации ядра.
  /// Если `null` — configure не вызывается (ядро уже сконфигурировано иначе
  /// либо работает в режиме без авторизации).
  final VpnConfigResolver? _resolveConfig;

  /// Последний успешно отправленный в ядро конфиг — чтобы не дёргать
  /// `configure` повторно с теми же значениями на каждый connect.
  VpnConfig? _appliedConfig;

  MethodChannelVpnConnection({VpnConfigResolver? configResolver})
      : _resolveConfig = configResolver {
    _statusSub = _statusEvents.receiveBroadcastStream().listen(
      (event) {
        if (event is Map) {
          _last = VpnStatus.fromMap(event, server: _target);
          _statusCtrl.add(_last);
        }
      },
      onError: (Object e) {
        _last = VpnStatus(
          stage: VpnStage.error,
          server: _target,
          detail: e.toString(),
        );
        _statusCtrl.add(_last);
      },
    );
  }

  @override
  Stream<VpnStatus> get status async* {
    yield _last;
    yield* _statusCtrl.stream;
  }

  @override
  Stream<TrafficStats> get traffic => _trafficEvents
      .receiveBroadcastStream()
      .where((e) => e is Map)
      .map((e) => TrafficStats.fromMap(e as Map));

  @override
  VpnStatus get currentStatus => _last;

  @override
  Future<void> connect(Server server) async {
    _target = server;
    _last = VpnStatus(stage: VpnStage.connecting, server: server);
    _statusCtrl.add(_last);
    // Авторизуем ядро (JWT + UUID подписки + URL панели) до поднятия туннеля.
    // Идемпотентно: повторяется только при смене значений (ротация токена и т.п.).
    await _ensureConfigured();
    await _method.invokeMethod<void>('connect', {
      'serverId': server.id,
      'serverName': server.name,
      'countryCode': server.countryCode,
    });
  }

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
  }) async {
    _target = _rawProfileServer(label);
    _last = VpnStatus(stage: VpnStage.connecting, server: _target);
    _statusCtrl.add(_last);
    // Сбрасываем кэш panelAccount-конфига: переключение на raw-источник означает,
    // что прежний configure() больше не относится к текущему туннелю.
    _appliedConfig = null;
    // Ядро парсит сырую подписку в mihomo-конфиг и хранит её как импортированный
    // источник (mobile.ImportSubscription -> SetImportedConfig). Это аддитивный
    // метод (контракт F); существующие connect/configure не затрагиваются.
    await _method.invokeMethod<void>('importRawProfile', {
      'raw': raw,
      'format': format,
    });
    // Поднимаем туннель с пустым serverId: для raw-источника узла подписки нет,
    // ядро поднимает импортированный конфиг (api.Core: raw-ветка Up).
    await _method.invokeMethod<void>('connect', {
      'serverId': '',
      'serverName': label,
      'countryCode': null,
    });
  }

  /// Передаёт ядру конфиг через `configure`, если резолвер задан и значения
  /// изменились. Тихо пропускает, если данных ещё нет (не залогинены/подписка
  /// не загрузилась) — нативная сторона в этом случае ответит стадией `error`.
  Future<void> _ensureConfigured() async {
    final resolver = _resolveConfig;
    if (resolver == null) return;
    final cfg = await resolver();
    if (cfg == null || !cfg.isComplete) return;
    final prev = _appliedConfig;
    if (prev != null &&
        prev.panelUrl == cfg.panelUrl &&
        prev.subscriptionUuid == cfg.subscriptionUuid &&
        prev.accessToken == cfg.accessToken) {
      return;
    }
    await _method.invokeMethod<void>('configure', cfg.toArgs());
    _appliedConfig = cfg;
  }

  @override
  Future<void> disconnect() async {
    await _method.invokeMethod<void>('disconnect');
  }

  @override
  Future<void> dispose() async {
    await _statusSub?.cancel();
    await _statusCtrl.close();
  }
}

/// Имитация ядра для desktop/dev: проходит реальный жизненный цикл состояний
/// и генерирует «дышащий» трафик, чтобы UI работал end-to-end без нативного бэка.
class MockVpnConnection implements VpnConnection {
  final _statusCtrl = StreamController<VpnStatus>.broadcast();
  final _trafficCtrl = StreamController<TrafficStats>.broadcast();

  VpnStatus _last = const VpnStatus.disconnected();
  Timer? _trafficTimer;
  Timer? _phaseTimer;
  int _seed = 0;

  @override
  Stream<VpnStatus> get status async* {
    yield _last;
    yield* _statusCtrl.stream;
  }

  @override
  Stream<TrafficStats> get traffic => _trafficCtrl.stream;

  @override
  VpnStatus get currentStatus => _last;

  void _emit(VpnStatus s) {
    _last = s;
    _statusCtrl.add(s);
  }

  @override
  Future<void> connect(Server server) async {
    _phaseTimer?.cancel();
    _emit(VpnStatus(
      stage: VpnStage.connecting,
      server: server,
      detail: 'Securing tunnel',
    ));
    _phaseTimer = Timer(const Duration(milliseconds: 1400), () {
      _emit(VpnStatus(
        stage: VpnStage.connected,
        server: server,
        connectedSince: DateTime.now(),
      ));
      _startTraffic();
    });
  }

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
  }) async {
    _phaseTimer?.cancel();
    final server = _rawProfileServer(label);
    _emit(VpnStatus(
      stage: VpnStage.connecting,
      server: server,
      detail: 'Importing profile',
    ));
    _phaseTimer = Timer(const Duration(milliseconds: 1400), () {
      _emit(VpnStatus(
        stage: VpnStage.connected,
        server: server,
        connectedSince: DateTime.now(),
      ));
      _startTraffic();
    });
  }

  @override
  Future<void> disconnect() async {
    _phaseTimer?.cancel();
    _stopTraffic();
    _emit(VpnStatus(stage: VpnStage.disconnected, server: _last.server));
  }

  void _startTraffic() {
    _stopTraffic();
    _trafficTimer = Timer.periodic(const Duration(seconds: 1), (_) {
      _seed++;
      // Псевдослучайный, но плавный профиль (без dart:math import — детерминирован).
      final wobble = (_seed * 2654435761) & 0x7fffffff;
      final down = 4 * 1024 * 1024 + (wobble % (10 * 1024 * 1024));
      final up = 256 * 1024 + (wobble % (1024 * 1024));
      _trafficCtrl.add(TrafficStats(
        downBps: down,
        upBps: up,
        downTotal: down * _seed,
        upTotal: up * _seed,
      ));
    });
  }

  void _stopTraffic() {
    _trafficTimer?.cancel();
    _trafficTimer = null;
    _trafficCtrl.add(TrafficStats.zero);
  }

  @override
  Future<void> dispose() async {
    _phaseTimer?.cancel();
    _trafficTimer?.cancel();
    await _statusCtrl.close();
    await _trafficCtrl.close();
  }
}

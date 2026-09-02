/// Реализация [VpnConnection] поверх платформенных каналов `com.caramba/vpn`.
library;

import 'dart:async';

import 'package:caramba_vpn/src/contract.dart';
import 'package:caramba_vpn/src/core_models.dart';
import 'package:caramba_vpn/src/core_policy.dart';
import 'package:flutter/services.dart';

/// Реализация поверх платформенных каналов `com.caramba/vpn`.
///
/// Нативная сторона (Android/iOS через gomobile, Linux/Windows через
/// `libcaramba_core`) выставляет:
///   * MethodChannel `com.caramba/vpn` — `configure`, `connect`, `connectRaw`,
///     `disconnect`, `status`, `importSubscription`, `probe`, `setPolicy`,
///     `setTunnelMode`;
///   * EventChannel  `com.caramba/vpn/status`  — поток снимков [VpnStatus];
///   * EventChannel  `com.caramba/vpn/traffic` — поток [TrafficStats].
///
/// `S` — модель сервера приложения; [describe] сводит её к [VpnServerArgs] для
/// провода, [rawTarget] строит плейсхолдер для rawSub-профиля.
class MethodChannelVpnConnection<S extends Object> implements VpnConnection<S> {
  static const MethodChannel _method = MethodChannel('com.caramba/vpn');
  static const EventChannel _statusEvents = EventChannel(
    'com.caramba/vpn/status',
  );
  static const EventChannel _trafficEvents = EventChannel(
    'com.caramba/vpn/traffic',
  );

  final _statusCtrl = StreamController<VpnStatus<S>>.broadcast();
  VpnStatus<S> _last = VpnStatus<S>.disconnected();
  S? _target;

  StreamSubscription<dynamic>? _statusSub;

  final VpnServerDescriptor<S> _describe;
  final VpnRawTargetFactory<S>? _rawTarget;

  /// Резолвер JWT + UUID подписки + URL панели для авторизации ядра.
  /// Если `null` — configure не вызывается (ядро уже сконфигурировано иначе
  /// либо работает в режиме без авторизации).
  final VpnConfigResolver? _resolveConfig;

  /// Последний успешно отправленный в ядро конфиг — чтобы не дёргать
  /// `configure` повторно с теми же значениями на каждый connect.
  VpnConfig? _appliedConfig;

  MethodChannelVpnConnection({
    required VpnServerDescriptor<S> describe,
    VpnRawTargetFactory<S>? rawTarget,
    VpnConfigResolver? configResolver,
  }) : _describe = describe,
       _rawTarget = rawTarget,
       _resolveConfig = configResolver {
    _statusSub = _statusEvents.receiveBroadcastStream().listen(
      (Object? event) {
        if (event is Map) {
          _last = VpnStatus<S>.fromMap(event, server: _target);
          _statusCtrl.add(_last);
        }
      },
      onError: (Object e) {
        _last = VpnStatus<S>(
          stage: VpnStage.error,
          server: _target,
          detail: e.toString(),
        );
        _statusCtrl.add(_last);
      },
    );
  }

  @override
  Stream<VpnStatus<S>> get status async* {
    yield _last;
    yield* _statusCtrl.stream;
  }

  @override
  Stream<TrafficStats> get traffic => _trafficEvents
      .receiveBroadcastStream()
      .where((Object? e) => e is Map)
      .map((Object? e) => TrafficStats.fromMap(e! as Map<Object?, Object?>));

  @override
  VpnStatus<S> get currentStatus => _last;

  @override
  Future<void> connect(S server) async {
    _target = server;
    _last = VpnStatus<S>(stage: VpnStage.connecting, server: server);
    _statusCtrl.add(_last);
    // Авторизуем ядро (JWT + UUID подписки + URL панели) до поднятия туннеля.
    // Идемпотентно: повторяется только при смене значений (ротация токена и т.п.).
    await _ensureConfigured();
    final args = _describe(server);
    // serverId уходит на провод СТРОКОЙ: нативный мост читает его как String и
    // молча коерсит не-строку в "" (теряя выбор узла).
    await _method.invokeMethod<void>('connect', <String, Object?>{
      'serverId': args.id,
      'serverName': args.name,
      'countryCode': args.countryCode,
    });
  }

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {
    _target = _rawTarget?.call(label);
    _last = VpnStatus<S>(stage: VpnStage.connecting, server: _target);
    _statusCtrl.add(_last);
    // Сбрасываем кэш panelAccount-конфига: переключение на raw-источник означает,
    // что прежний configure() больше не относится к текущему туннелю.
    _appliedConfig = null;
    // Единый канонический вызов `connectRaw`: нативный мост импортирует сырую
    // подписку [rawConfig] в формате [format] (mobile.ImportSubscription /
    // CarambaImportSubscription), затем поднимает туннель. serverId (ABI v2)
    // прикрепляет селектор CARAMBA к конкретному узлу импортированного конфига;
    // пусто — автоматический выбор. [label] лишь для отображения профиля.
    await _method.invokeMethod<void>('connectRaw', <String, Object?>{
      'rawConfig': raw,
      'format': format,
      'label': label,
      'serverId': serverId ?? '',
    });
  }

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async {
    final json = await _method.invokeMethod<String>(
      'importSubscription',
      <String, Object?>{'rawConfig': raw, 'format': format},
    );
    if (json == null || json.isEmpty) return ImportResult.empty;
    return ImportResult.fromJson(json);
  }

  @override
  Future<List<ProbeResult>> probe({
    Duration timeout = const Duration(seconds: 5),
  }) async {
    final json = await _method.invokeMethod<String>('probe', <String, Object?>{
      'timeoutMs': timeout.inMilliseconds,
    });
    if (json == null || json.isEmpty) return const <ProbeResult>[];
    return ProbeResult.listFromJson(json);
  }

  @override
  Future<void> setPolicy(CorePolicy policy) async {
    await _method.invokeMethod<void>('setPolicy', <String, Object?>{
      'json': jsonEncodePolicy(policy),
    });
  }

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 7890}) async {
    await _method.invokeMethod<void>('setTunnelMode', <String, Object?>{
      'mode': mode.wire,
      'port': mixedPort,
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

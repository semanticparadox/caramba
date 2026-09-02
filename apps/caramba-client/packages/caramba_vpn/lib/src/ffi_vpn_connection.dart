/// Внутрипроцессная реализация [VpnConnection] поверх dart:ffi.
///
/// Зачем: на macOS полноценный TUN требует Network Extension, а значит Xcode,
/// подписи и одобрения System Extension. Этот путь их обходит — ядро
/// (`libcaramba_core.dylib`) грузится прямо в процесс приложения и по умолчанию
/// поднимается в режиме [TunnelMode.proxy]: локальный mixed-инбаунд
/// (SOCKS5+HTTP) на 127.0.0.1:7890, никаких привилегий. Это даёт реальное
/// соединение по подписке без Xcode; системный TUN остаётся за путём
/// Network Extension (см. INTEGRATION.md, раздел macOS).
///
/// Потоки: блокирующие вызовы ядра (`CarambaUp` — до ~60 c, `CarambaProbe` — до
/// таймаута) выполняются через `Isolate.run`, чтобы не морозить UI-изолят.
/// Внутри изолята библиотека открывается ЗАНОВО по тому же пути: `dlopen` на
/// уже загруженный образ возвращает его же, Go-рантайм и таблица хэндлов в
/// процессе одни, поэтому целочисленный хэндл остаётся валидным. Через границу
/// изолята едут только сендабельные значения (путь, int-хэндл, строки).
/// Короткие вызовы (`Status`, `Traffic`, сеттеры, `Down`) идут синхронно в
/// UI-изоляте — они возвращаются мгновенно.
library;

import 'dart:async';
import 'dart:io';
import 'dart:isolate';

import 'package:caramba_vpn/src/contract.dart';
import 'package:caramba_vpn/src/core_models.dart';
import 'package:caramba_vpn/src/core_policy.dart';
import 'package:caramba_vpn/src/ffi/caramba_core_bindings.dart';
import 'package:caramba_vpn/src/ffi/core_loader.dart';

/// Реализация [VpnConnection], гоняющая ядро caramba-core в процессе приложения
/// через dart:ffi (без платформенных каналов и без Network Extension).
class FfiVpnConnection<S extends Object> implements VpnConnection<S> {
  /// Как часто опрашиваются `CarambaStatus` / `CarambaTraffic`, пока туннель
  /// поднят. Совпадает с 1 Гц остальных платформ.
  static const Duration pollInterval = Duration(seconds: 1);

  /// Заглушка URL панели для путей без панели (raw-подписка, generic-режим).
  ///
  /// `CarambaNew` требует непустой panelURL и возвращает 0 на пустом, а
  /// импорт/probe панель вообще не трогают. Домен `.invalid` зарезервирован
  /// RFC 2606 и никогда не резолвится, так что ядро физически не может по нему
  /// куда-то пойти.
  static const String placeholderPanelUrl = 'https://panel.invalid';

  final VpnServerDescriptor<S> _describe;
  final VpnRawTargetFactory<S>? _rawTarget;
  final VpnConfigResolver? _resolveConfig;

  /// Явный путь к библиотеке; null — обычный поиск (env, bundle, dev-путь).
  final String? _explicitLibraryPath;

  /// Рабочий каталог ядра (конфиг mihomo, geo-базы, кэш).
  final String _workDir;

  final _statusCtrl = StreamController<VpnStatus<S>>.broadcast();
  final _trafficCtrl = StreamController<TrafficStats>.broadcast();

  VpnStatus<S> _last = VpnStatus<S>.disconnected();
  TrafficStats _lastTraffic = TrafficStats.zero;
  S? _target;

  CarambaCoreLibrary? _lib;
  int _handle = 0;
  Timer? _pollTimer;
  VpnConfig? _appliedConfig;

  TunnelMode _mode;
  int _mixedPort;
  CorePolicy? _policy;

  FfiVpnConnection({
    required VpnServerDescriptor<S> describe,
    VpnRawTargetFactory<S>? rawTarget,
    VpnConfigResolver? configResolver,
    String? libraryPath,
    String? workDir,
    TunnelMode defaultTunnelMode = TunnelMode.proxy,
    int mixedPort = 7890,
  }) : _describe = describe,
       _rawTarget = rawTarget,
       _resolveConfig = configResolver,
       _explicitLibraryPath = libraryPath,
       _workDir = workDir ?? defaultCoreWorkDir(),
       _mode = defaultTunnelMode,
       _mixedPort = mixedPort;

  /// Каталог для конфига/кэша ядра. Без path_provider: на macOS/Linux это
  /// `~/.caramba-core`, иначе временный каталог системы.
  static String defaultCoreWorkDir() {
    final home =
        Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'];
    if (home != null && home.isNotEmpty) return '$home/.caramba-core';
    return '${Directory.systemTemp.path}/caramba-core';
  }

  // --- потоки ----------------------------------------------------------------

  @override
  Stream<VpnStatus<S>> get status async* {
    yield _last;
    yield* _statusCtrl.stream;
  }

  @override
  Stream<TrafficStats> get traffic async* {
    yield _lastTraffic;
    yield* _trafficCtrl.stream;
  }

  @override
  VpnStatus<S> get currentStatus => _last;

  void _emit(VpnStatus<S> s) {
    _last = s;
    if (!_statusCtrl.isClosed) _statusCtrl.add(s);
  }

  void _emitTraffic(TrafficStats t) {
    _lastTraffic = t;
    if (!_trafficCtrl.isClosed) _trafficCtrl.add(t);
  }

  void _emitError(Object error) {
    _stopPolling();
    _emit(
      VpnStatus<S>(
        stage: VpnStage.error,
        server: _target,
        detail: error is CarambaCoreException ? error.message : error.toString(),
      ),
    );
  }

  // --- ядро ------------------------------------------------------------------

  /// Открывает библиотеку и создаёт хэндл ядра (идемпотентно), применяя
  /// накопленные режим захвата и политику.
  CarambaCoreLibrary _ensureCore({String panelUrl = ''}) {
    final lib =
        _lib ??= openCarambaCoreLibrary(explicitPath: _explicitLibraryPath);
    if (_handle == 0) {
      Directory(_workDir).createSync(recursive: true);
      _handle = lib.create(
        panelUrl: panelUrl.isEmpty ? placeholderPanelUrl : panelUrl,
        workDir: _workDir,
        tokenPath: '$_workDir/tokens.json',
      );
      if (_handle == 0) {
        throw const CarambaCoreException('core init failed');
      }
      _applyTunnelMode(lib);
      _applyPolicy(lib);
    }
    return lib;
  }

  void _applyTunnelMode(CarambaCoreLibrary lib) {
    if (_handle == 0) return;
    final err = lib.setTunnelMode(_handle, _mode.wire, _mixedPort);
    _throwIfError(err);
  }

  void _applyPolicy(CarambaCoreLibrary lib) {
    final policy = _policy;
    if (policy == null || _handle == 0) return;
    final err = lib.setPolicy(_handle, jsonEncodePolicy(policy));
    _throwIfError(err);
  }

  /// Сеттеры ядра отдают '' при успехе и JSON `{"error":...}` при ошибке.
  static void _throwIfError(String raw) {
    final message = _errorOf(raw);
    if (message != null) throw CarambaCoreException(message);
  }

  /// Совпадают ли два seam-конфига поле в поле (у [VpnConfig] нет ==).
  static bool _sameConfig(VpnConfig? a, VpnConfig? b) =>
      a != null &&
      b != null &&
      a.panelUrl == b.panelUrl &&
      a.subscriptionUuid == b.subscriptionUuid &&
      a.accessToken == b.accessToken;

  /// Достаёт поле `error` из JSON-ответа ядра; null — ошибки нет.
  static String? _errorOf(String raw) {
    if (raw.isEmpty) return null;
    final map = decodeJsonMap(raw);
    final err = map['error'];
    if (err is String && err.isNotEmpty) return err;
    return null;
  }

  // --- жизненный цикл туннеля ------------------------------------------------

  @override
  Future<void> connect(S server) async {
    _target = server;
    _emit(
      VpnStatus<S>(
        stage: VpnStage.connecting,
        server: server,
        detail: 'Securing tunnel',
      ),
    );
    try {
      final cfg = await _resolveConfig?.call();
      final lib = _ensureCore(panelUrl: cfg?.panelUrl ?? '');
      if (cfg != null && cfg.isComplete && !_sameConfig(_appliedConfig, cfg)) {
        _throwIfError(
          lib.configure(
            _handle,
            panelUrl: cfg.panelUrl,
            subscriptionUuid: cfg.subscriptionUuid,
            accessToken: cfg.accessToken,
          ),
        );
        _appliedConfig = cfg;
      }
      await _raise(lib, _describe(server).id);
    } on Object catch (e) {
      _emitError(e);
    }
  }

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {
    _target = _rawTarget?.call(label);
    _emit(
      VpnStatus<S>(
        stage: VpnStage.connecting,
        server: _target,
        detail: 'Importing profile',
      ),
    );
    _appliedConfig = null;
    try {
      final lib = _ensureCore();
      _throwIfError(lib.importSubscription(_handle, raw, format));
      await _raise(lib, serverId ?? '');
    } on Object catch (e) {
      _emitError(e);
    }
  }

  /// Общий хвост обоих connect-путей: TUN fd не передаём (ядро само решает,
  /// а в proxy-режиме TUN и не поднимается), затем блокирующий Up в отдельном
  /// изоляте и запуск опроса.
  Future<void> _raise(CarambaCoreLibrary lib, String serverId) async {
    _throwIfError(lib.setTunFd(_handle, -1));
    final upJson = await _runOffUiIsolate(
      libPath: lib.path,
      handle: _handle,
      call: _FfiCall.up,
      arg: serverId,
    );
    final err = _errorOf(upJson);
    if (err != null) throw CarambaCoreException(err);
    _emit(
      VpnStatus<S>(
        stage: VpnStage.connected,
        server: _target,
        connectedSince: DateTime.now(),
        mode: _mode,
        mixedPort: _mode == TunnelMode.proxy ? _mixedPort : null,
      ),
    );
    _startPolling(lib);
  }

  @override
  Future<void> disconnect() async {
    final lib = _lib;
    _stopPolling();
    if (lib != null && _handle != 0) {
      lib.down(_handle); // best-effort: ошибку опускания не эскалируем
    }
    _emitTraffic(TrafficStats.zero);
    _emit(VpnStatus<S>(stage: VpnStage.disconnected, server: _last.server));
  }

  // --- generic-режим ---------------------------------------------------------

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async {
    final lib = _ensureCore();
    final json = lib.importSubscription(_handle, raw, format);
    final err = _errorOf(json);
    if (err != null) throw CarambaCoreException(err);
    return ImportResult.fromJson(json);
  }

  @override
  Future<List<ProbeResult>> probe({
    Duration timeout = const Duration(seconds: 5),
  }) async {
    final lib = _ensureCore();
    final json = await _runOffUiIsolate(
      libPath: lib.path,
      handle: _handle,
      call: _FfiCall.probe,
      arg: '${timeout.inMilliseconds}',
    );
    final err = _errorOf(json);
    if (err != null) throw CarambaCoreException(err);
    return ProbeResult.listFromJson(json);
  }

  @override
  Future<void> setPolicy(CorePolicy policy) async {
    _policy = policy;
    final lib = _lib;
    if (lib != null && _handle != 0) _applyPolicy(lib);
  }

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 7890}) async {
    _mode = mode;
    _mixedPort = mixedPort;
    final lib = _lib;
    if (lib != null && _handle != 0) _applyTunnelMode(lib);
  }

  // --- опрос -----------------------------------------------------------------

  void _startPolling(CarambaCoreLibrary lib) {
    _stopPolling();
    _pollTimer = Timer.periodic(pollInterval, (_) => _tick(lib));
  }

  void _stopPolling() {
    _pollTimer?.cancel();
    _pollTimer = null;
  }

  void _tick(CarambaCoreLibrary lib) {
    if (_handle == 0) return;
    final statusMap = decodeJsonMap(lib.status(_handle));
    if (statusMap.isNotEmpty) {
      final snapshot = VpnStatus<S>.fromMap(statusMap, server: _target);
      // Ядро не знает про режим, если бинарь старый — подставляем свой.
      _emit(
        snapshot.mode == null
            ? snapshot.copyWith(
                mode: _mode,
                mixedPort: _mode == TunnelMode.proxy ? _mixedPort : null,
              )
            : snapshot,
      );
      if (snapshot.stage == VpnStage.error ||
          snapshot.stage == VpnStage.disconnected) {
        _stopPolling();
        _emitTraffic(TrafficStats.zero);
        return;
      }
    }
    if (_last.stage == VpnStage.connected) {
      _emitTraffic(TrafficStats.fromMap(decodeJsonMap(lib.traffic(_handle))));
    } else {
      _emitTraffic(TrafficStats.zero);
    }
  }

  // --- завершение ------------------------------------------------------------

  @override
  Future<void> dispose() async {
    _stopPolling();
    final lib = _lib;
    if (lib != null && _handle != 0) {
      lib.free(_handle); // гасит туннель и освобождает ядро
    }
    _handle = 0;
    _lib = null;
    await _statusCtrl.close();
    await _trafficCtrl.close();
  }
}

/// Какой блокирующий вызов исполнить в отдельном изоляте.
enum _FfiCall { up, probe }

/// Выполняет блокирующий вызов ядра вне UI-изолята.
///
/// Через границу едут только сендабельные значения; библиотека открывается
/// внутри изолята заново по [libPath] (тот же образ в том же процессе), а
/// [handle] — обычный int из общей Go-таблицы.
Future<String> _runOffUiIsolate({
  required String libPath,
  required int handle,
  required _FfiCall call,
  required String arg,
}) {
  final isUp = call == _FfiCall.up;
  return Isolate.run<String>(() {
    final lib = CarambaCoreLibrary.open(libPath);
    if (isUp) return lib.up(handle, arg);
    return lib.probe(handle, int.tryParse(arg) ?? 5000);
  });
}

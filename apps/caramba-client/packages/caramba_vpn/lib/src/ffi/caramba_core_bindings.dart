/// dart:ffi-биндинги к C ABI ядра caramba-core
/// (`libs/caramba-core/ffi/caramba_core.h` + дополнения ABI v2).
///
/// Соглашения ABI (см. заголовок ядра):
///   * хэндл — `long` (на всех поддерживаемых платформах 64 бита; в Dart Int64);
///   * все возвращаемые `char*` выделены в Go и обязаны быть освобождены
///     ровно один раз через `CarambaFreeString`;
///   * сеттеры возвращают NULL при успехе и JSON `{"error":"..."}` при ошибке;
///   * `CarambaUp` / `CarambaStatus` / `CarambaTraffic` / `CarambaProbe` /
///     `CarambaImportSubscription` всегда отдают непустую JSON-строку.
///
/// Символы ABI v2 (`CarambaSetPolicy`, `CarambaProbe`) резолвятся ЛЕНИВО:
/// уже собранные dylib их не содержат, и жёсткий lookup при загрузке ронял бы
/// весь FFI-путь. Отсутствие символа превращается в понятную
/// [CarambaCoreMissingSymbol], а не в падение при `DynamicLibrary.open`.
library;

import 'dart:ffi';

import 'package:ffi/ffi.dart';

// --- C-сигнатуры ------------------------------------------------------------

typedef _CNew =
    Int64 Function(
      Pointer<Utf8> panelURL,
      Pointer<Utf8> subURL,
      Pointer<Utf8> workDir,
      Pointer<Utf8> tokenPath,
    );
typedef _DNew =
    int Function(
      Pointer<Utf8> panelURL,
      Pointer<Utf8> subURL,
      Pointer<Utf8> workDir,
      Pointer<Utf8> tokenPath,
    );

typedef _CConfigure =
    Pointer<Utf8> Function(
      Int64 h,
      Pointer<Utf8> panelURL,
      Pointer<Utf8> subscriptionID,
      Pointer<Utf8> accessToken,
    );
typedef _DConfigure =
    Pointer<Utf8> Function(
      int h,
      Pointer<Utf8> panelURL,
      Pointer<Utf8> subscriptionID,
      Pointer<Utf8> accessToken,
    );

typedef _CImport =
    Pointer<Utf8> Function(Int64 h, Pointer<Utf8> raw, Pointer<Utf8> format);
typedef _DImport =
    Pointer<Utf8> Function(int h, Pointer<Utf8> raw, Pointer<Utf8> format);

typedef _CSetTunnelMode =
    Pointer<Utf8> Function(Int64 h, Pointer<Utf8> mode, Int32 port);
typedef _DSetTunnelMode =
    Pointer<Utf8> Function(int h, Pointer<Utf8> mode, int port);

typedef _CSetTunFd = Pointer<Utf8> Function(Int64 h, Int32 fd);
typedef _DSetTunFd = Pointer<Utf8> Function(int h, int fd);

typedef _CSetPolicy = Pointer<Utf8> Function(Int64 h, Pointer<Utf8> json);
typedef _DSetPolicy = Pointer<Utf8> Function(int h, Pointer<Utf8> json);

typedef _CProbe = Pointer<Utf8> Function(Int64 h, Int32 timeoutMs);
typedef _DProbe = Pointer<Utf8> Function(int h, int timeoutMs);

typedef _CUp = Pointer<Utf8> Function(Int64 h, Pointer<Utf8> serverID);
typedef _DUp = Pointer<Utf8> Function(int h, Pointer<Utf8> serverID);

typedef _CHandleToString = Pointer<Utf8> Function(Int64 h);
typedef _DHandleToString = Pointer<Utf8> Function(int h);

typedef _CFree = Void Function(Int64 h);
typedef _DFree = void Function(int h);

typedef _CFreeString = Void Function(Pointer<Utf8> s);
typedef _DFreeString = void Function(Pointer<Utf8> s);

/// Символ ABI, которого нет в загруженной библиотеке.
///
/// Обычно означает dylib, собранный до ABI v2 (без `CarambaSetPolicy` /
/// `CarambaProbe`). Сообщение содержит имя символа и путь библиотеки.
class CarambaCoreMissingSymbol implements Exception {
  final String symbol;
  final String libraryPath;

  const CarambaCoreMissingSymbol(this.symbol, this.libraryPath);

  @override
  String toString() =>
      'caramba-core: symbol $symbol is missing in $libraryPath '
      '(rebuild the core with ABI v2: scripts/build-desktop-lib.sh)';
}

/// Ошибка, о которой ядро сообщило JSON-полем `error`.
class CarambaCoreException implements Exception {
  final String message;

  const CarambaCoreException(this.message);

  @override
  String toString() => 'caramba-core: $message';
}

/// Загруженная библиотека ядра + резолвленные функции ABI.
///
/// Экземпляр держит `DynamicLibrary`, поэтому его НЕЛЬЗЯ передавать между
/// изолятами. Для блокирующих вызовов в `Isolate.run` библиотека открывается
/// заново по [path] (dlopen на тот же файл возвращает уже загруженный образ,
/// так что хэндл ядра остаётся валидным).
class CarambaCoreLibrary {
  final String path;
  final DynamicLibrary _lib;

  CarambaCoreLibrary._(this.path, this._lib);

  /// Открывает библиотеку по абсолютному пути. Бросает, если файл не грузится.
  factory CarambaCoreLibrary.open(String path) =>
      CarambaCoreLibrary._(path, DynamicLibrary.open(path));

  late final _DNew _new = _lib.lookupFunction<_CNew, _DNew>('CarambaNew');
  late final _DConfigure _configure = _lib
      .lookupFunction<_CConfigure, _DConfigure>('CarambaConfigure');
  late final _DImport _import = _lib.lookupFunction<_CImport, _DImport>(
    'CarambaImportSubscription',
  );
  late final _DSetTunnelMode _setTunnelMode = _lib
      .lookupFunction<_CSetTunnelMode, _DSetTunnelMode>('CarambaSetTunnelMode');
  late final _DSetTunFd _setTunFd = _lib.lookupFunction<_CSetTunFd, _DSetTunFd>(
    'CarambaSetTunFd',
  );
  late final _DUp _up = _lib.lookupFunction<_CUp, _DUp>('CarambaUp');
  late final _DHandleToString _down = _lib
      .lookupFunction<_CHandleToString, _DHandleToString>('CarambaDown');
  late final _DHandleToString _status = _lib
      .lookupFunction<_CHandleToString, _DHandleToString>('CarambaStatus');
  late final _DHandleToString _traffic = _lib
      .lookupFunction<_CHandleToString, _DHandleToString>('CarambaTraffic');
  late final _DFree _free = _lib.lookupFunction<_CFree, _DFree>('CarambaFree');
  late final _DFreeString _freeString = _lib
      .lookupFunction<_CFreeString, _DFreeString>('CarambaFreeString');

  /// ABI v2, может отсутствовать в старом бинаре — резолвится лениво.
  _DSetPolicy _lookupSetPolicy() {
    try {
      return _lib.lookupFunction<_CSetPolicy, _DSetPolicy>('CarambaSetPolicy');
    } on ArgumentError {
      throw CarambaCoreMissingSymbol('CarambaSetPolicy', path);
    }
  }

  /// ABI v2, может отсутствовать в старом бинаре — резолвится лениво.
  _DProbe _lookupProbe() {
    try {
      return _lib.lookupFunction<_CProbe, _DProbe>('CarambaProbe');
    } on ArgumentError {
      throw CarambaCoreMissingSymbol('CarambaProbe', path);
    }
  }

  /// Есть ли символ в библиотеке (без вызова). Для диагностики/тестов.
  bool hasSymbol(String name) {
    try {
      _lib.lookup<Void>(name);
      return true;
    } on ArgumentError {
      return false;
    }
  }

  // --- обёртки над памятью ---------------------------------------------------

  /// Копирует строку ядра в Dart и освобождает оригинал. NULL -> ''.
  String _take(Pointer<Utf8> p) {
    if (p == nullptr) return '';
    final out = p.toDartString();
    _freeString(p);
    return out;
  }

  /// Освобождает строку, содержимое которой не нужно (результат сеттера).
  /// Возвращает её текст, чтобы вызывающий мог поднять ошибку.
  String _takeError(Pointer<Utf8> p) => _take(p);

  R _withStrings<R>(List<String> values, R Function(List<Pointer<Utf8>>) body) {
    final ptrs = values.map((s) => s.toNativeUtf8()).toList(growable: false);
    try {
      return body(ptrs);
    } finally {
      for (final p in ptrs) {
        malloc.free(p);
      }
    }
  }

  // --- ABI -------------------------------------------------------------------

  /// `CarambaNew`. Возвращает хэндл > 0 либо 0 при ошибке.
  int create({
    required String panelUrl,
    String subUrl = '',
    String workDir = '',
    String tokenPath = '',
  }) => _withStrings(
    <String>[panelUrl, subUrl, workDir, tokenPath],
    (p) => _new(p[0], p[1], p[2], p[3]),
  );

  /// `CarambaConfigure`. Возвращает текст ошибки или '' при успехе.
  String configure(
    int handle, {
    required String panelUrl,
    required String subscriptionUuid,
    required String accessToken,
  }) => _withStrings(
    <String>[panelUrl, subscriptionUuid, accessToken],
    (p) => _takeError(_configure(handle, p[0], p[1], p[2])),
  );

  /// `CarambaImportSubscription`. JSON метаданных либо `{"error":...}`.
  String importSubscription(int handle, String raw, String format) =>
      _withStrings(
        <String>[raw, format],
        (p) => _take(_import(handle, p[0], p[1])),
      );

  /// `CarambaSetTunnelMode`. Текст ошибки или '' при успехе.
  String setTunnelMode(int handle, String mode, int port) => _withStrings(
    <String>[mode],
    (p) => _takeError(_setTunnelMode(handle, p[0], port)),
  );

  /// `CarambaSetTunFd`. Текст ошибки или '' при успехе.
  String setTunFd(int handle, int fd) => _takeError(_setTunFd(handle, fd));

  /// `CarambaSetPolicy` (ABI v2). Текст ошибки или '' при успехе.
  String setPolicy(int handle, String json) {
    final fn = _lookupSetPolicy();
    return _withStrings(
      <String>[json],
      (p) => _takeError(fn(handle, p[0])),
    );
  }

  /// `CarambaProbe` (ABI v2). JSON `{"servers":[...]}` либо `{"error":...}`.
  String probe(int handle, int timeoutMs) {
    final fn = _lookupProbe();
    return _take(fn(handle, timeoutMs));
  }

  /// `CarambaUp`. JSON UpResult либо `{"error":...}`. Блокирующий вызов.
  String up(int handle, String serverId) =>
      _withStrings(<String>[serverId], (p) => _take(_up(handle, p[0])));

  /// `CarambaDown`. Текст ошибки или '' при успехе.
  String down(int handle) => _takeError(_down(handle));

  /// `CarambaStatus`. Плоский JSON статуса.
  String status(int handle) => _take(_status(handle));

  /// `CarambaTraffic`. Плоский JSON счётчиков.
  String traffic(int handle) => _take(_traffic(handle));

  /// `CarambaFree`. Гасит туннель и освобождает ядро.
  void free(int handle) => _free(handle);
}

import 'dart:convert';
import 'dart:io';

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter_test/flutter_test.dart';

/// Дымовой тест реального `libcaramba_core.dylib`.
///
/// Пропускается целиком, если `CARAMBA_CORE_LIB` не указывает на существующий
/// файл, — библиотека не коммитится и на CI её нет. Запуск:
///
/// ```bash
/// cd apps/caramba-client
/// CARAMBA_CORE_LIB=$PWD/../../libs/caramba-core/build/libcaramba_core.dylib \
///   flutter test test/ffi_smoke_test.dart
/// ```
void main() {
  final libPath = Platform.environment[kCarambaCoreLibEnv];
  final available = libPath != null && File(libPath).existsSync();
  final skipReason = available
      ? null
      : 'set $kCarambaCoreLibEnv to an existing libcaramba_core to run this';

  group(
    'libcaramba_core FFI smoke',
    () {
      late CarambaCoreLibrary lib;
      late int handle;

      setUpAll(() {
        lib = CarambaCoreLibrary.open(libPath!);
        // panelURL обязателен для CarambaNew; workDir — временный, чтобы тест
        // не трогал реальный каталог ядра.
        final workDir = Directory.systemTemp
            .createTempSync('caramba-ffi-smoke')
            .path;
        handle = lib.create(
          panelUrl: 'https://panel.invalid',
          workDir: workDir,
          tokenPath: '$workDir/tokens.json',
        );
      });

      tearDownAll(() {
        if (handle != 0) lib.free(handle);
      });

      test('CarambaNew returns a live handle', () {
        expect(handle, greaterThan(0));
      });

      test('CarambaStatus reports the disconnected stage', () {
        final status = VpnStatus<Object>.fromMap(_decode(lib.status(handle)));
        expect(status.stage, VpnStage.disconnected);
        expect(status.isConnected, isFalse);
      });

      test('CarambaTraffic reports zeros before any tunnel', () {
        final traffic = TrafficStats.fromMap(_decode(lib.traffic(handle)));
        expect(traffic.downBps, 0);
        expect(traffic.upBps, 0);
      });

      test('ABI v2 symbols are either present or reported clearly', () {
        // Библиотека, собранная до ABI v2, этих символов не несёт. Тогда вызов
        // обязан бросить понятную CarambaCoreMissingSymbol, а не уронить
        // процесс на этапе lookup.
        if (lib.hasSymbol('CarambaSetPolicy')) {
          expect(lib.setPolicy(handle, '{"preset":"global"}'), isA<String>());
        } else {
          expect(
            () => lib.setPolicy(handle, '{}'),
            throwsA(isA<CarambaCoreMissingSymbol>()),
          );
        }
        if (!lib.hasSymbol('CarambaProbe')) {
          expect(
            () => lib.probe(handle, 100),
            throwsA(isA<CarambaCoreMissingSymbol>()),
          );
        }
      });
    },
    skip: skipReason,
  );
}

/// Разбирает JSON-объект ядра; всё нераспознанное даёт пустую карту.
Map<Object?, Object?> _decode(String source) {
  if (source.isEmpty) return const <Object?, Object?>{};
  try {
    final decoded = jsonDecode(source);
    return decoded is Map<Object?, Object?>
        ? decoded
        : const <Object?, Object?>{};
  } on FormatException {
    return const <Object?, Object?>{};
  }
}

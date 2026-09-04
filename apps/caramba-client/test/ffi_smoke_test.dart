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
        // workDir — временный, чтобы тест не трогал реальный каталог ядра.
        final workDir = Directory.systemTemp
            .createTempSync('caramba-ffi-smoke')
            .path;
        handle = lib.create(
          panelUrl: 'https://panel.example',
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

      // Гейт под [FfiVpnConnection._ensureCore]: тот отдаёт ядру ПУСТОЙ адрес
      // панели на raw-пути, и вся честность отчёта о маршрутизации держится на
      // том, что ядро такой адрес принимает. Раньше здесь стояла заглушка
      // `https://panel.invalid` под комментарием «CarambaNew требует непустой
      // panelURL и возвращает 0 на пустом» — утверждение, которого никто не
      // проверял. Проверяем: заглушка стоила пользователям импортированной
      // подписки блокировки рекламы целиком.
      test('CarambaNew accepts an empty panel URL (import-only mode)', () {
        final workDir = Directory.systemTemp
            .createTempSync('caramba-ffi-nopanel')
            .path;
        final h = lib.create(
          panelUrl: '',
          workDir: workDir,
          tokenPath: '$workDir/tokens.json',
        );
        addTearDown(() {
          if (h != 0) lib.free(h);
        });
        expect(
          h,
          greaterThan(0),
          reason:
              'core must accept an empty PanelBaseURL: without that, the '
              'raw-import path has to invent a panel that does not exist',
        );
        // И хэндл обязан быть ЖИВЫМ, а не просто ненулевым.
        final status = VpnStatus<Object>.fromMap(_decode(lib.status(h)));
        expect(status.stage, VpnStage.disconnected);
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

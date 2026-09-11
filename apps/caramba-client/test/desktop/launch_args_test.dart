// Разбор argv и решение «показывать ли окно на старте».
//
// Сторожится развилка, ради которой флаг и появился: «запуск без окна» на
// Windows и Linux действует ТОЛЬКО при автозапуске, а ручной запуск показывает
// окно всегда. На macOS причина запуска неизвестна, и там настройка действует
// при каждом запуске, как раньше.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/desktop/launch_args.dart';

void main() {
  group('LaunchArgs.parse', () {
    test('пустой argv это ручной запуск', () {
      expect(LaunchArgs.parse(const <String>[]).autostart, isFalse);
    });

    test('флаг автозапуска узнаётся в любом месте', () {
      expect(
        LaunchArgs.parse(const <String>[kAutostartFlag]).autostart,
        isTrue,
      );
      expect(
        LaunchArgs.parse(const <String>[
          'caramba://connect?d=x',
          kAutostartFlag,
        ]).autostart,
        isTrue,
      );
    });

    test('диплинк и чужие аргументы флагом не считаются', () {
      expect(
        LaunchArgs.parse(const <String>['caramba://connect?d=x']).autostart,
        isFalse,
      );
      expect(
        LaunchArgs.parse(const <String>['--autostart=1']).autostart,
        isFalse,
      );
    });

    test('флаг один и тот же для регистрации и разбора', () {
      expect(kAutostartFlag, '--autostart');
    });
  });

  group('shouldShowWindowOnLaunch', () {
    test('без «запуска без окна» окно показывается всегда', () {
      for (final reason in const <bool?>[null, true, false]) {
        expect(
          shouldShowWindowOnLaunch(
            startInTray: false,
            launchedByAutostart: reason,
          ),
          isTrue,
        );
      }
    });

    test('Windows/Linux: без окна только при автозапуске', () {
      expect(
        shouldShowWindowOnLaunch(startInTray: true, launchedByAutostart: true),
        isFalse,
      );
      expect(
        shouldShowWindowOnLaunch(startInTray: true, launchedByAutostart: false),
        isTrue,
        reason: 'ручной запуск обязан показать окно',
      );
    });

    test('macOS: причина неизвестна, настройка действует всегда', () {
      expect(
        shouldShowWindowOnLaunch(startInTray: true, launchedByAutostart: null),
        isFalse,
      );
    });
  });
}

// Автозапуск на Windows через планировщик задач.
//
// Windows не запускает из реестра Run программы с requireAdministrator (наш
// манифест), поэтому путь пакета launch_at_startup там мёртв. Здесь
// проверяется всё, что можно проверить без Windows: текст скрипта регистрации,
// аргументы команд, разбор кодов выхода и то, что порт на Windows идёт в
// планировщик, а не в пакет.

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart' show PlatformException;
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/desktop/launch_args.dart';
import 'package:caramba_client/desktop/ports/autostart_port.dart';
import 'package:caramba_client/desktop/ports/windows_task_autostart.dart';

const String _exe = r'C:\Program Files\Caramba Connect\caramba_client.exe';
const String _task = 'Caramba Connect';

/// Запуск процессов, который ничего не запускает, а записывает и отвечает.
class _Runner {
  final List<(String, List<String>)> calls = <(String, List<String>)>[];
  int queryExit = 1;
  int registerExit = 0;
  int deleteExit = 0;
  String stderr = '';

  Future<ProcessResult> call(String exe, List<String> args) async {
    calls.add((exe, args));
    if (exe == 'schtasks' && args.first == '/Query') {
      return ProcessResult(1, queryExit, '', '');
    }
    if (exe == 'schtasks' && args.first == '/Delete') {
      return ProcessResult(1, deleteExit, '', stderr);
    }
    if (exe == 'powershell') {
      return ProcessResult(1, registerExit, '', stderr);
    }
    throw StateError('unexpected $exe $args');
  }
}

void main() {
  group('скрипт регистрации', () {
    final script = windowsTaskRegisterScript(taskName: _task, exePath: _exe);

    test('запускает наш exe с флагом автозапуска', () {
      expect(script, contains("-Execute '$_exe'"));
      expect(script, contains("-Argument '$kAutostartFlag'"));
      expect(
        script,
        contains(r"-WorkingDirectory 'C:\Program Files\Caramba Connect'"),
      );
    });

    test('задача с наивысшими правами и без лимита на выполнение', () {
      expect(script, contains('-RunLevel Highest'));
      expect(script, contains('-ExecutionTimeLimit (New-TimeSpan -Seconds 0)'));
      expect(script, contains('-AllowStartIfOnBatteries'));
      expect(script, contains('-DontStopIfGoingOnBatteries'));
      expect(script, contains('-AtLogOn'));
      expect(script, contains('-LogonType Interactive'));
      expect(script, contains("-TaskName '$_task'"));
      expect(script, contains('-Force'));
    });

    test('без двойных кавычек: скрипт уходит одним аргументом', () {
      expect(script.contains('"'), isFalse);
    });

    test('одинарная кавычка в пути экранируется удвоением', () {
      expect(psQuote("C:\\O'Brien\\app.exe"), "'C:\\O''Brien\\app.exe'");
    });

    test('каталог Windows-пути считается без dart:io', () {
      expect(windowsParentDir(_exe), r'C:\Program Files\Caramba Connect');
      expect(windowsParentDir(r'C:\app.exe'), r'C:');
      expect(windowsParentDir('app.exe'), 'app.exe');
    });

    test('PowerShell зовётся без профиля и без вопросов', () {
      final args = windowsPowershellArgs(script);
      expect(
        args,
        containsAllInOrder(<String>['-NoProfile', '-NonInteractive']),
      );
      expect(args, containsAllInOrder(<String>['-ExecutionPolicy', 'Bypass']));
      expect(args.last, script);
    });
  });

  group('WindowsTaskAutostart', () {
    late _Runner runner;
    late WindowsTaskAutostart task;

    setUp(() {
      runner = _Runner();
      task = WindowsTaskAutostart(
        taskName: _task,
        exePath: _exe,
        run: runner.call,
      );
    });

    test('isEnabled читает код выхода schtasks /Query', () async {
      runner.queryExit = 1;
      expect(await task.isEnabled(), isFalse);
      runner.queryExit = 0;
      expect(await task.isEnabled(), isTrue);
      expect(runner.calls.first.$2, <String>['/Query', '/TN', _task]);
    });

    test('enable регистрирует задачу через PowerShell', () async {
      await task.enable();
      final (exe, args) = runner.calls.single;
      expect(exe, 'powershell');
      expect(args.last, contains('Register-ScheduledTask'));
    });

    test('отказ PowerShell становится PlatformException с текстом', () async {
      runner
        ..registerExit = 1
        ..stderr = 'Access is denied';
      await expectLater(
        task.enable(),
        throwsA(
          isA<PlatformException>()
              .having((e) => e.code, 'code', kAutostartTaskFailedCode)
              .having((e) => e.message, 'message', 'Access is denied'),
        ),
      );
    });

    test('disable удаляет существующую задачу', () async {
      runner.queryExit = 0;
      await task.disable();
      expect(runner.calls.last.$2, <String>['/Delete', '/TN', _task, '/F']);
    });

    test('disable отсутствующей задачи это успех без удаления', () async {
      runner.queryExit = 1;
      await task.disable();
      expect(runner.calls.map((c) => c.$2.first), <String>['/Query']);
    });

    test('отказ удаления не молчит', () async {
      runner
        ..queryExit = 0
        ..deleteExit = 1
        ..stderr = 'ERROR: Access is denied.';
      await expectLater(task.disable(), throwsA(isA<PlatformException>()));
    });
  });

  group('LaunchAtStartupPort на Windows', () {
    tearDown(() => debugDefaultTargetPlatformOverride = null);

    test('идёт в планировщик, а не в реестр', () async {
      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      final runner = _Runner()..queryExit = 0;
      final port = LaunchAtStartupPort(run: runner.call);
      await port.setup(
        appName: _task,
        appPath: _exe,
        packageName: kAutostartPackageName,
        args: const <String>[kAutostartFlag],
      );

      expect(await port.isSupported(), isTrue);
      expect(await port.isEnabled(), isTrue);
      await port.enable();
      await port.disable();

      expect(runner.calls.map((c) => c.$1), <String>[
        'schtasks',
        'powershell',
        'schtasks',
        'schtasks',
      ]);
    });

    test('до setup порт честно падает, а не молчит', () async {
      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      await expectLater(
        LaunchAtStartupPort(run: _Runner().call).isEnabled(),
        throwsA(isA<StateError>()),
      );
    });
  });
}

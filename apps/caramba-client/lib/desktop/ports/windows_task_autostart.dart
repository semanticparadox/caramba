/// Автозапуск на Windows через планировщик задач, а не через реестр `Run`.
///
/// ЗАЧЕМ НЕ РЕЕСТР. `launch_at_startup` пишет `HKCU\...\CurrentVersion\Run`,
/// и это работает для обычных программ. Наша не обычная: манифест раннера
/// требует `requireAdministrator` (wintun создаёт адаптер только из процесса с
/// правами), а Windows со времён Vista НЕ запускает из `Run` программы,
/// которым нужно повышение: диалога UAC на входе в систему быть не может, и
/// такой запуск молча блокируется. Тумблер выглядел бы рабочим, а автозапуска
/// не было бы. Задача планировщика с уровнем «наивысшие права» и триггером
/// «при входе» запускается без диалога, и это штатный путь Microsoft для
/// таких программ.
///
/// Регистрация идёт через PowerShell (`Register-ScheduledTask`), а не через
/// `schtasks /Create`: у последнего нет ключей, чтобы снять лимит в 72 часа на
/// выполнение (после него планировщик убил бы VPN-клиент) и разрешить запуск
/// от батареи. Запрос и удаление задачи проще, они идут через `schtasks`.
///
/// Всё, что можно проверить без Windows, вынесено в чистые функции: текст
/// скрипта и аргументы команд. Сам запуск процессов подменяется в тестах.
library;

import 'dart:io' show Process, ProcessResult;

import 'package:flutter/services.dart' show PlatformException;

import 'package:caramba_client/desktop/launch_args.dart';

/// Запуск внешней команды. Подменяется в тестах.
typedef ProcessRunner =
    Future<ProcessResult> Function(String executable, List<String> arguments);

Future<ProcessResult> _runProcess(String executable, List<String> arguments) =>
    Process.run(executable, arguments);

/// Код отказа планировщика.
const String kAutostartTaskFailedCode = 'task_failed';

/// Экранирование для одинарных кавычек PowerShell: внутри них любой символ
/// буквален, кроме самой кавычки, которую удваивают.
String psQuote(String value) => "'${value.replaceAll("'", "''")}'";

/// Каталог Windows-пути, без `dart:io`: `File.parent` на другом хосте не
/// знает обратной косой черты, а функция проверяется тестом на macOS.
String windowsParentDir(String path) {
  final cut = path.lastIndexOf(RegExp(r'[\\/]'));
  return cut <= 0 ? path : path.substring(0, cut);
}

/// Скрипт регистрации задачи.
///
/// [exePath] — путь к исполняемому файлу приложения. Задача запускает его с
/// [kAutostartFlag], чтобы приложение отличило автозапуск от ручного.
///
/// Что в настройках и почему:
///   * `-RunLevel Highest` — без этого задача упрётся в тот же UAC;
///   * `-ExecutionTimeLimit 0` — планировщик по умолчанию убивает задачу через
///     72 часа, а VPN-клиент живёт, пока живёт сессия;
///   * `-AllowStartIfOnBatteries`/`-DontStopIfGoingOnBatteries` — иначе на
///     ноутбуке от батареи автозапуска не будет вовсе;
///   * `-MultipleInstances IgnoreNew` — второй экземпляр не нужен;
///   * `-LogonType Interactive` и триггер на пользователя — задача живёт в
///     сеансе того, кто вошёл, а не в фоне без окна и трея.
String windowsTaskRegisterScript({
  required String taskName,
  required String exePath,
}) {
  final workDir = windowsParentDir(exePath);
  return <String>[
    r"$ErrorActionPreference = 'Stop'",
    // Без двойных кавычек намеренно: скрипт целиком уходит одним аргументом
    // командной строки, и кавычки внутри пришлось бы экранировать дважды
    // (Dart и PowerShell), а инсталлятор Inno вставляет тот же текст третьим
    // способом.
    r"$user = $env:USERDOMAIN + '\' + $env:USERNAME",
    r'$action = New-ScheduledTaskAction -Execute '
        '${psQuote(exePath)} -Argument ${psQuote(kAutostartFlag)} '
        '-WorkingDirectory ${psQuote(workDir)}',
    r'$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user',
    r'$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries '
        r'-DontStopIfGoingOnBatteries -StartWhenAvailable '
        r'-MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Seconds 0)',
    r'$principal = New-ScheduledTaskPrincipal -UserId $user '
        r'-LogonType Interactive -RunLevel Highest',
    'Register-ScheduledTask -TaskName ${psQuote(taskName)} '
        r'-Action $action -Trigger $trigger -Settings $settings '
        r'-Principal $principal -Force | Out-Null',
  ].join('; ');
}

/// Аргументы PowerShell для [windowsTaskRegisterScript].
///
/// `-ExecutionPolicy Bypass` касается только этого вызова и нужен, потому что
/// на машине политика может запрещать скрипты вовсе; `-NonInteractive`, чтобы
/// сбой не повис на вопросе к пользователю.
List<String> windowsPowershellArgs(String script) => <String>[
  '-NoProfile',
  '-NonInteractive',
  '-ExecutionPolicy',
  'Bypass',
  '-Command',
  script,
];

/// `schtasks` умеет отвечать о наличии задачи кодом выхода: 0 есть, 1 нет.
List<String> windowsTaskQueryArgs(String taskName) => <String>[
  '/Query',
  '/TN',
  taskName,
];

List<String> windowsTaskDeleteArgs(String taskName) => <String>[
  '/Delete',
  '/TN',
  taskName,
  '/F',
];

/// Живая регистрация задачи планировщика.
class WindowsTaskAutostart {
  final String taskName;
  final String exePath;
  final ProcessRunner _run;

  WindowsTaskAutostart({
    required this.taskName,
    required this.exePath,
    ProcessRunner? run,
  }) : _run = run ?? _runProcess;

  Future<bool> isEnabled() async {
    final result = await _run('schtasks', windowsTaskQueryArgs(taskName));
    return result.exitCode == 0;
  }

  Future<void> enable() async {
    final result = await _run(
      'powershell',
      windowsPowershellArgs(
        windowsTaskRegisterScript(taskName: taskName, exePath: exePath),
      ),
    );
    if (result.exitCode != 0) {
      throw PlatformException(
        code: kAutostartTaskFailedCode,
        message: _trimOutput(result),
      );
    }
  }

  /// Удаление отсутствующей задачи считается успехом: цель «задачи нет»
  /// достигнута, а второй запрос ради различения «не было» и «удалили» стоил
  /// бы лишнего процесса.
  Future<void> disable() async {
    if (!await isEnabled()) return;
    final result = await _run('schtasks', windowsTaskDeleteArgs(taskName));
    if (result.exitCode != 0) {
      throw PlatformException(
        code: kAutostartTaskFailedCode,
        message: _trimOutput(result),
      );
    }
  }

  static String _trimOutput(ProcessResult result) {
    final err = '${result.stderr}'.trim();
    if (err.isNotEmpty) return err;
    return '${result.stdout}'.trim();
  }
}

/// Порт автозапуска при входе в систему: единственная дверь из десктопного
/// слоя в `launch_at_startup` и в наш macOS-канал.
///
/// ЗАЧЕМ порт, а не прямые вызовы плагина. Автозапуск живёт в трёх разных
/// местах системы: планировщик задач Windows (см. `windows_task_autostart.dart`,
/// почему не реестр), файл `~/.config/autostart` на Linux и
/// `SMAppService` на macOS (там канал реализован не плагином, а нашим
/// `AppDelegate.swift`). Любое обращение к ним из теста либо падает
/// `MissingPluginException`, либо, что хуже, реально правит систему хоста,
/// на котором гоняются тесты. Логика же проверять надо ту, что выше: чьё
/// значение считается истиной (настройка приложения), что делать, когда
/// система не умеет автозапуск вовсе, и что показывать при отказе.
///
/// Поэтому [AutostartService] знает только этот интерфейс, а живой
/// [LaunchAtStartupPort] остаётся тонким переходником: одна ветка на выбор
/// платформы и ни одного решения.
library;

import 'dart:io' show Platform;

import 'package:flutter/services.dart';
import 'package:launch_at_startup/launch_at_startup.dart';

import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/ports/windows_task_autostart.dart';

/// Идентификатор пакета MSIX на Windows.
///
/// Нужен плагину, чтобы отличить установку из Store (там автозапуск ставится
/// не в реестр `Run`, а через манифест пакета) от обычной. Значение совпадает
/// с идентификатором приложения; на macOS и Linux не используется.
const String kAutostartPackageName = 'com.caramba.carambaClient';

/// Код отказа, которым наш `AppDelegate.swift` отвечает на macOS 12.
///
/// `SMAppService` появился только в macOS 13, и на более старой системе канал
/// сознательно отвечает отказом вместо `false`: «выключено» и «не умею» это
/// разные ответы, и путать их значило бы показать человеку тумблер, который
/// молча ничего не делает.
const String kAutostartUnsupportedCode = 'unsupported';

/// Registration exists but still needs the user's approval in Login Items.
const String kAutostartApprovalCode = 'requires_approval';

/// То, что десктопному слою нужно от автозапуска, и ничего сверх этого.
abstract class AutostartPort {
  /// Сообщает системе, что именно регистрировать. Зовётся один раз до всего
  /// остального.
  ///
  /// [appPath] `null` означает «текущий исполняемый файл»: путь знает только
  /// живой порт (это `dart:io`), а сервису знать его незачем.
  ///
  /// [args] — аргументы, с которыми система запустит приложение при входе
  /// (Windows и Linux; macOS их не передаёт). По ним приложение отличает
  /// автозапуск от ручного запуска.
  Future<void> setup({
    required String appName,
    String? appPath,
    required String packageName,
    List<String> args = const <String>[],
  });

  /// Умеет ли ЭТА система автозапуск вообще (см. [kAutostartUnsupportedCode]).
  Future<bool> isSupported();

  /// Зарегистрировано ли приложение сейчас, по мнению самой системы.
  Future<bool> isEnabled();

  Future<void> enable();

  Future<void> disable();
}

/// Живая реализация поверх `launch_at_startup` 0.5.1 и планировщика Windows.
///
/// На Linux это файл `.desktop` (пакет), на macOS метод-канал
/// `launch_at_startup`, который у нас реализован своим Swift-кодом поверх
/// `SMAppService` (пакетный SPM-вариант мы не подключаем). На Windows пакет
/// НЕ используется: его реестровый путь для программы с `requireAdministrator`
/// заблокирован системой, поэтому там задача планировщика
/// ([WindowsTaskAutostart]).
class LaunchAtStartupPort implements AutostartPort {
  static const _channel = MethodChannel('launch_at_startup');

  /// Запуск процессов планировщика; подменяется в тестах.
  final ProcessRunner? _run;

  WindowsTaskAutostart? _task;

  LaunchAtStartupPort({ProcessRunner? run}) : _run = run;

  @override
  Future<void> setup({
    required String appName,
    String? appPath,
    required String packageName,
    List<String> args = const <String>[],
  }) async {
    final path = appPath ?? Platform.resolvedExecutable;
    if (isWindowsPlatform) {
      _task = WindowsTaskAutostart(taskName: appName, exePath: path, run: _run);
      return;
    }
    launchAtStartup.setup(
      appName: appName,
      // `resolvedExecutable` — путь к бинарнику внутри бандла; плагин сам
      // поднимается от него до `.app` там, где системе нужен бандл.
      appPath: path,
      packageName: packageName,
      args: args,
    );
  }

  /// Задача планировщика после [setup]. До него порт не знает ни имени, ни
  /// пути, и обращение к нему это ошибка порядка вызовов, а не системы.
  WindowsTaskAutostart get _windowsTask {
    final task = _task;
    if (task == null) throw StateError('setup() must run before use');
    return task;
  }

  @override
  Future<bool> isEnabled() async {
    if (isMacOSPlatform) {
      return await _channel.invokeMethod<bool>('launchAtStartupIsEnabled') ??
          false;
    }
    if (isWindowsPlatform) return _windowsTask.isEnabled();
    return launchAtStartup.isEnabled();
  }

  @override
  Future<void> enable() async {
    if (isMacOSPlatform) {
      await _channel.invokeMethod<void>('launchAtStartupSetEnabled', {
        'setEnabledValue': true,
      });
    } else if (isWindowsPlatform) {
      await _windowsTask.enable();
    } else {
      await launchAtStartup.enable();
    }
  }

  @override
  Future<void> disable() async {
    if (isMacOSPlatform) {
      // The package skips unregister when isEnabled is false. A pending
      // approval is not enabled, but must still be cancellable.
      await _channel.invokeMethod<void>('launchAtStartupSetEnabled', {
        'setEnabledValue': false,
      });
    } else if (isWindowsPlatform) {
      await _windowsTask.disable();
    } else {
      await launchAtStartup.disable();
    }
  }

  @override
  Future<bool> isSupported() async {
    // Планировщик Windows и `~/.config/autostart` есть всегда: спрашивать
    // систему не о чем.
    if (isWindowsPlatform || isLinuxPlatform) return true;
    if (!isMacOSPlatform) return false;
    // Only the explicit unsupported response establishes an old OS. Missing
    // channels and other failures propagate so the UI can report a probe error.
    try {
      await isEnabled();
      return true;
    } on PlatformException catch (error) {
      if (error.code == kAutostartApprovalCode) return true;
      if (error.code == kAutostartUnsupportedCode) return false;
      rethrow;
    }
  }
}

/// Порт-заглушка: помнит состояние вместо того, чтобы править систему.
///
/// Живёт в `lib/`, а не в тесте, сознательно: подменять автозапуск нужно и
/// тестам сборки десктопных сервисов, и тестам трея, и каждый из них иначе
/// тащил бы свою копию. В сборку класс не попадает — на него нет ни одной
/// ссылки из живого кода.
class FakeAutostartPort implements AutostartPort {
  /// Порядок вызовов: по нему тест видит, что систему трогали ровно тогда,
  /// когда надо.
  final List<String> calls = <String>[];

  /// Умеет ли «система» автозапуск (macOS 12 — не умеет).
  bool supported = true;

  /// Состояние «системы»: зарегистрировано или нет.
  bool enabled = false;

  bool approvalPending = false;

  /// Следующая правка провалится. Так проверяется, что отказ доходит до
  /// человека, а приложение остаётся живым.
  bool failNextWrite = false;

  /// Чем именно провалится следующая правка. По умолчанию обычное исключение;
  /// тест подставляет сюда `PlatformException`, когда проверяет разбор кода
  /// отказа (например, ожидание разрешения в «Объектах входа»).
  Object? nextWriteError;

  /// С какими аргументами «система» запустит приложение при входе.
  List<String> args = const <String>[];

  @override
  Future<void> setup({
    required String appName,
    String? appPath,
    required String packageName,
    List<String> args = const <String>[],
  }) async {
    this.args = args;
    calls.add('setup($appName)');
  }

  @override
  Future<bool> isSupported() async {
    calls.add('isSupported');
    return supported;
  }

  @override
  Future<bool> isEnabled() async {
    calls.add('isEnabled');
    if (approvalPending) {
      throw PlatformException(code: kAutostartApprovalCode);
    }
    return enabled;
  }

  @override
  Future<void> enable() async {
    calls.add('enable');
    if (failNextWrite) {
      failNextWrite = false;
      throw nextWriteError ?? Exception('enable failed');
    }
    approvalPending = false;
    enabled = true;
  }

  @override
  Future<void> disable() async {
    calls.add('disable');
    if (failNextWrite) {
      failNextWrite = false;
      throw nextWriteError ?? Exception('disable failed');
    }
    approvalPending = false;
    enabled = false;
  }
}

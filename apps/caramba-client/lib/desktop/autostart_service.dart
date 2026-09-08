/// Автозапуск при входе в систему: настройка приложения правит систему.
///
/// ЗАЧЕМ отдельный сервис. Здесь два источника состояния, и они умеют
/// разъезжаться: наш снимок настроек (`desktopPrefs.launchAtLogin`) и сама
/// система (Login Items, реестр, файл `.desktop`), где человек мог всё
/// поменять руками или другим приложением. Правило одно и оно принято здесь:
/// ИСТИНА — настройка приложения. При старте система приводится к ней, дальше
/// каждое переключение тумблера доезжает до системы.
///
/// Третье решение того же уровня: система, которая автозапуск не умеет
/// (macOS 12 без `SMAppService`), не должна выглядеть сломанной. Тогда сервис
/// один раз объявляет [autostartSupportedProvider] `false`, и настройки просто
/// не предлагают того, чего не будет.
///
/// Плагин сюда не заглядывает: всё через [AutostartPort], все чтения
/// провайдеров через замыкания. Поэтому каждое из решений проверяется тестом.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show PlatformException;
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/desktop/desktop_prefs.dart';
import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/desktop_strings.dart';
import 'package:caramba_client/desktop/ports/autostart_port.dart';
import 'package:caramba_client/main.dart' show rootMessengerKey;

/// Текст отказа. Подробности (код канала, отказ реестра) человеку ничего не
/// говорят, а действие у него ровно одно: попробовать ещё раз или поставить
/// автозапуск средствами системы.
const String kAutostartFailedMessage = DesktopStrings.launchAtLoginFailed;

/// Отдельный текст для одного отказа, у которого действие ЕСТЬ: macOS приняла
/// заявку, но ждёт разрешения в «Объектах входа». Показать здесь общее «не
/// удалось» значило бы отправить человека искать причину самому.
const String kAutostartApprovalMessage =
    DesktopStrings.launchAtLoginNeedsApproval;

/// Сообщение по отказу системы.
String autostartFailureMessage(Object error) {
  if (error is PlatformException && error.code == kAutostartApprovalCode) {
    return kAutostartApprovalMessage;
  }
  return kAutostartFailedMessage;
}

/// Снимает подписку, выданную `listenLaunchAtLogin`.
typedef AutostartSubscriptionCanceller = void Function();

class AutostartService {
  final AutostartPort port;

  final bool Function() _readWanted;
  final void Function(bool wanted) _writeWanted;
  final AutostartSubscriptionCanceller Function(void Function(bool wanted))
      _listenWanted;
  final void Function(bool supported) _setSupported;
  final void Function(String message) _report;
  final void Function(bool pending)? _setApprovalPending;
  final void Function(String message)? _setUnavailableMessage;

  AutostartSubscriptionCanceller? _cancel;

  bool _supported = false;
  Future<void> _pending = Future<void>.value();
  bool _disposed = false;

  /// Умеет ли эта система автозапуск. До [start] всегда `false`: обещать
  /// возможность, которой может не оказаться, дороже, чем показать её на кадр
  /// позже.
  bool get supported => _supported;

  /// Второй `start()` подписался бы вторым слушателем, и каждое переключение
  /// тумблера уходило бы в систему дважды.
  bool _started = false;

  AutostartService({
    required this.port,
    required bool Function() readWanted,
    required void Function(bool wanted) writeWanted,
    required AutostartSubscriptionCanceller Function(void Function(bool wanted))
        listenWanted,
    required void Function(bool supported) setSupported,
    void Function(String message)? report,
    void Function(bool pending)? setApprovalPending,
    void Function(String message)? setUnavailableMessage,
  })  : _readWanted = readWanted,
        _writeWanted = writeWanted,
        _listenWanted = listenWanted,
        _setSupported = setSupported,
        _report = report ?? showAutostartFailure,
        _setApprovalPending = setApprovalPending,
        _setUnavailableMessage = setUnavailableMessage;

  /// Знакомит систему с приложением, узнаёт, умеет ли она автозапуск, и
  /// приводит её к настройке.
  ///
  /// Отказ на любом шаге не имеет права уронить старт: приложение без
  /// автозапуска работает, приложение, упавшее из-за автозапуска, — нет.
  Future<void> start() async {
    if (_started) return;
    _started = true;
    try {
      await port.setup(
        appName: DesktopStrings.appName,
        packageName: kAutostartPackageName,
      );
      _supported = await port.isSupported();
      if (!_supported) {
        _setUnavailableMessage?.call(
          isMacOSPlatform
              ? DesktopStrings.launchAtLoginUnsupportedMac
              : DesktopStrings.launchAtLoginUnsupported,
        );
      }
    } catch (error) {
      _supported = false;
      debugPrint('Autostart capability probe failed: $error');
      _setUnavailableMessage
          ?.call('Не удалось проверить доступность автозапуска');
    }
    _setSupported(_supported);
    if (!_supported) return;

    // Подписка ДО сверки: пока сверка ходит в систему, человек уже может
    // щёлкнуть тумблером, и это переключение терять нельзя.
    _cancel = _listenWanted((_) {
      if (!_reverting) unawaited(_enqueueReconcile());
    });
    await _enqueueReconcile();
  }

  void dispose() {
    _disposed = true;
    _cancel?.call();
    _cancel = null;
  }

  // Serialize system writes; each queued reconciliation reads the latest
  // preference, so slow registration cannot overtake a later cancellation.
  Future<void> _enqueueReconcile() {
    return _pending = _pending.then((_) async {
      if (!_disposed) await _reconcile();
    });
  }

  /// Приводит систему к настройке приложения.
  ///
  /// Сначала читаем систему: лишняя запись в Login Items на каждом запуске
  /// это лишний диалог согласия на macOS и лишний повод для системы решить,
  /// что приложение ведёт себя странно.
  Future<void> _reconcile() async {
    final wanted = _readWanted();
    try {
      final enabled = await port.isEnabled();
      _setApprovalPending?.call(false);
      if (enabled == wanted) return;
    } catch (error) {
      if (error is PlatformException &&
          error.code == kAutostartApprovalCode &&
          wanted) {
        // Keep the request across restarts, without re-registering it.
        _setApprovalPending?.call(true);
        return;
      }
      // Систему не прочитать — всё равно попробуем записать желание: хуже
      // текущего «неизвестно» не станет.
    }
    await _apply(wanted);
  }

  /// Единственное место, где система действительно правится.
  ///
  /// A registration awaiting approval remains requested. Other failures
  /// restore the previous setting; the UI names the pending approval separately.
  Future<void> _apply(bool wanted) async {
    if (!_supported) return;
    if (_reverting) return;
    try {
      if (wanted) {
        await port.enable();
      } else {
        await port.disable();
      }
      _setApprovalPending?.call(false);
    } catch (error) {
      _report(autostartFailureMessage(error));
      if (wanted &&
          error is PlatformException &&
          error.code == kAutostartApprovalCode) {
        _setApprovalPending?.call(true);
        return;
      }
      _revert(wanted);
    }
  }

  /// Возврат настройки в прежнее положение.
  ///
  /// Флаг нужен, чтобы наша же запись не пришла обратно подписчиком и не
  /// отправила в систему вторую правку: система уже отказала, второй заход
  /// отказал бы так же, но с ещё одним сообщением поверх первого.
  bool _reverting = false;

  void _revert(bool attempted) {
    if (_disposed || _readWanted() != attempted) return;
    _reverting = true;
    try {
      _writeWanted(!attempted);
    } finally {
      _reverting = false;
    }
  }
}

/// Показывает отказ поверх любого экрана.
///
/// Через `rootMessengerKey`, а не через `showCarambaToast`: тому нужен
/// `BuildContext` ПОД `ScaffoldMessenger`, а у сервиса контекста нет вовсе
/// (он живёт рядом с окном, а не в дереве виджетов). Тем же ключом и по той
/// же причине показывает отказ разбор диплинков в роутере. Окна может не быть
/// (запуск только со значком в трее) — тогда сообщения просто нет.
void showAutostartFailure(String message) {
  final messenger = rootMessengerKey.currentState;
  if (messenger == null) return;
  messenger.clearSnackBars();
  messenger.showSnackBar(
    SnackBar(
      duration: const Duration(milliseconds: 2400),
      content: Text(message),
    ),
  );
}

/// Живой порт автозапуска. Отдельным провайдером, чтобы тест подменял его
/// фейком, не трогая проводку [autostartServiceProvider].
final autostartPortProvider = Provider<AutostartPort>(
  (ref) => LaunchAtStartupPort(),
);

/// Умеет ли система автозапуск. Ставится сервисом в [AutostartService.start];
/// до него `false`. Настройки прячут (или гасят) тумблер по этому значению.
final autostartSupportedProvider = StateProvider<bool>((ref) => false);

/// Unknown/channel failures must not be presented as an obsolete OS version.
final autostartUnavailableMessageProvider = StateProvider<String>(
  (ref) => 'Проверяем доступность автозапуска…',
);

/// A requested login item still needs approval in macOS System Settings.
final autostartApprovalPendingProvider = StateProvider<bool>((ref) => false);

/// Сервис автозапуска. Создаётся один на процесс; `start()` зовёт десктопный
/// хост сервисов, подписку снимает dispose провайдера.
final autostartServiceProvider = Provider<AutostartService>((ref) {
  final service = AutostartService(
    port: ref.watch(autostartPortProvider),
    readWanted: () => ref.read(desktopPrefsProvider).launchAtLogin,
    writeWanted: (wanted) =>
        ref.read(desktopPrefsProvider.notifier).setLaunchAtLogin(wanted),
    // Через контейнер, а не `ref.listen`: подписка нужна ПОСЛЕ сборки
    // провайдера и только когда система автозапуск умеет.
    listenWanted: (onWanted) {
      final sub = ref.container.listen<bool>(
        desktopPrefsProvider.select((p) => p.launchAtLogin),
        (_, next) => onWanted(next),
      );
      return sub.close;
    },
    setUnavailableMessage: (message) =>
        ref.read(autostartUnavailableMessageProvider.notifier).state = message,
    setApprovalPending: (pending) =>
        ref.read(autostartApprovalPendingProvider.notifier).state = pending,
    setSupported: (supported) =>
        ref.read(autostartSupportedProvider.notifier).state = supported,
  );
  ref.onDispose(service.dispose);
  return service;
});

// Автозапуск при входе: настройка приложения правит систему.
//
// Главное свойство — чьё слово последнее. Система (Login Items, реестр, файл
// `.desktop`) живёт своей жизнью: её меняют руками, чистилками, другой
// установкой того же приложения. Истиной остаётся наш снимок настроек, и тест
// фиксирует именно это: при старте систему приводят к настройке, дальше
// каждое переключение тумблера доезжает до неё, а система, которая автозапуск
// не умеет (macOS 12 без SMAppService), объявляется неумеющей один раз и
// больше не трогается.
//
// Плагин `launch_at_startup` в тестах не поднимается: всё идёт через
// AutostartPort, сюда подставлен FakeAutostartPort. Иначе тест правил бы
// автозапуск на машине, где сам и гоняется.

import 'dart:async';

import 'package:flutter/services.dart'
    show PlatformException, MissingPluginException;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/desktop/autostart_service.dart';
import 'package:caramba_client/desktop/desktop_prefs.dart';
import 'package:caramba_client/desktop/launch_args.dart';
import 'package:caramba_client/desktop/ports/autostart_port.dart';
import 'package:caramba_client/state/bootstrap_state.dart';

/// Прокрутить очередь: правка системы уходит через `unawaited`.
Future<void> pump() => Future<void>.delayed(Duration.zero);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late FakeAutostartPort port;
  late ProviderContainer container;
  late List<String> messages;

  /// Сколько раз принималось состояние системы.
  late int adopted;

  setUp(() async {
    SharedPreferences.setMockInitialValues(<String, Object>{});
    port = FakeAutostartPort();
    messages = <String>[];
    adopted = 0;
    container = ProviderContainer();
    addTearDown(container.dispose);
    // Как при старте: настройки прочитаны с диска до того, как кто-то их
    // спросит. Без этого сверка читала бы недогидратированный снимок.
    await container.read(appBootProvider.future);
  });

  /// Человек щёлкнул тумблером «Запускать при входе в систему».
  void wantLaunchAtLogin(bool value) =>
      container.read(desktopPrefsProvider.notifier).setLaunchAtLogin(value);

  /// Собирает сервис ровно теми замыканиями, которыми его собирает провайдер.
  AutostartService build() {
    final service = AutostartService(
      port: port,
      readChosen: () =>
          container.read(desktopPrefsProvider).launchAtLoginChosen,
      onAdopted: () => adopted++,
      readWanted: () => container.read(desktopPrefsProvider).launchAtLogin,
      writeWanted: (wanted) => container
          .read(desktopPrefsProvider.notifier)
          .setLaunchAtLogin(wanted),
      listenWanted: (onWanted) {
        final sub = container.listen<bool>(
          desktopPrefsProvider.select((p) => p.launchAtLogin),
          (_, next) => onWanted(next),
        );
        return sub.close;
      },
      setSupported: (supported) =>
          container.read(autostartSupportedProvider.notifier).state = supported,
      report: messages.add,
      setUnavailableMessage: (message) =>
          container.read(autostartUnavailableMessageProvider.notifier).state =
              message,
      setApprovalPending: (pending) =>
          container.read(autostartApprovalPendingProvider.notifier).state =
              pending,
    );
    addTearDown(service.dispose);
    return service;
  }

  test('старт объявляет систему умеющей автозапуск', () async {
    await build().start();

    expect(container.read(autostartSupportedProvider), isTrue);
    expect(port.calls.first, 'setup(Caramba Connect)');
  });

  test(
    'регистрация просит систему запускать нас с флагом автозапуска',
    () async {
      await build().start();

      expect(port.args, <String>[kAutostartFlag]);
    },
  );

  // Галочка инсталлятора Windows ставит автозапуск до первого запуска
  // приложения. Дефолт настройки `false` снял бы его на первом же старте,
  // поэтому, пока человек не решал сам, состояние системы принимается.
  group('первый запуск принимает состояние системы', () {
    test('включённый в системе автозапуск становится настройкой', () async {
      port.enabled = true;

      await build().start();

      expect(container.read(desktopPrefsProvider).launchAtLogin, isTrue);
      expect(container.read(desktopPrefsProvider).launchAtLoginChosen, isTrue);
      expect(adopted, 1, reason: 'подопции включаются вместе');
      expect(port.calls, isNot(contains('disable')));
      expect(port.enabled, isTrue);
    });

    test('выключенный в системе автозапуск решением не считается', () async {
      port.enabled = false;

      await build().start();

      expect(container.read(desktopPrefsProvider).launchAtLogin, isFalse);
      expect(
        container.read(desktopPrefsProvider).launchAtLoginChosen,
        isFalse,
        reason: 'следующая установка с галочкой ещё имеет право быть принятой',
      );
      expect(adopted, 0);
    });

    test(
      'после решения человека система снова приводится к настройке',
      () async {
        // Снятый руками тумблер: решение есть, система включена мимо нас.
        wantLaunchAtLogin(false);
        port.enabled = true;

        await build().start();

        expect(container.read(desktopPrefsProvider).launchAtLogin, isFalse);
        expect(port.enabled, isFalse);
        expect(adopted, 0);
      },
    );

    test('снимок прежней версии считается решением', () async {
      // Ключ launch_at_login есть, launch_at_login_chosen нет: так писала
      // прежняя версия. Человек уже выбирал, и система ему не указ.
      final prefs = DesktopPrefs.fromJson(const <String, dynamic>{
        'launch_at_login': false,
      });
      expect(prefs.launchAtLoginChosen, isTrue);
    });

    test('сбой чтения системы не принимает ничего', () async {
      port.approvalPending = true;

      await build().start();

      expect(container.read(desktopPrefsProvider).launchAtLogin, isFalse);
      expect(adopted, 0);
    });
  });

  test(
    'missing channel reports a capability error, not an OS requirement',
    () async {
      port = _MissingChannelAutostartPort();
      await build().start();
      expect(container.read(autostartSupportedProvider), isFalse);
      expect(
        container.read(autostartUnavailableMessageProvider),
        'Не удалось проверить доступность автозапуска',
      );
    },
  );

  test('система приводится к настройке приложения', () async {
    wantLaunchAtLogin(true);
    port.enabled = false;

    await build().start();

    expect(port.enabled, isTrue, reason: 'настройка сильнее системы');
    expect(port.calls, contains('enable'));
  });

  test('совпадающее состояние систему не трогает', () async {
    wantLaunchAtLogin(true);
    port.enabled = true;

    await build().start();

    expect(port.calls, isNot(contains('enable')));
    expect(port.calls, isNot(contains('disable')));
  });

  test('снятый руками автозапуск возвращается на место', () async {
    wantLaunchAtLogin(false);
    // Кто-то поставил приложение в Login Items мимо нас.
    port.enabled = true;

    await build().start();

    expect(port.enabled, isFalse);
    expect(port.calls, contains('disable'));
  });

  test('включение тумблера доезжает до системы', () async {
    await build().start();

    wantLaunchAtLogin(true);
    await pump();

    expect(port.enabled, isTrue);
    expect(port.calls, contains('enable'));
  });

  test('выключение тумблера доезжает до системы', () async {
    wantLaunchAtLogin(true);
    port.enabled = true;
    await build().start();

    wantLaunchAtLogin(false);
    await pump();

    expect(port.enabled, isFalse);
    expect(port.calls, contains('disable'));
  });

  test(
    'система без автозапуска объявляется неумеющей и не трогается',
    () async {
      port.supported = false;

      await build().start();
      wantLaunchAtLogin(true);
      await pump();

      expect(container.read(autostartSupportedProvider), isFalse);
      expect(port.calls, isNot(contains('enable')));
      expect(port.calls, isNot(contains('isEnabled')));
    },
  );

  test('отказ системы не роняет приложение, а сообщает человеку', () async {
    await build().start();
    port.failNextWrite = true;

    wantLaunchAtLogin(true);
    await pump();

    expect(messages, <String>[kAutostartFailedMessage]);
    expect(port.enabled, isFalse, reason: 'система осталась как была');
  });

  // Дефект D-04 ручной проверки: система отказывала (неподписанный бандл вне
  // /Applications), а тумблер оставался включённым и обещал автозапуск,
  // которого не будет.
  test('отказ возвращает тумблер в прежнее положение', () async {
    await build().start();
    port.failNextWrite = true;

    wantLaunchAtLogin(true);
    await pump();

    expect(
      container.read(desktopPrefsProvider).launchAtLogin,
      isFalse,
      reason: 'настройка не имеет права врать про систему',
    );
    expect(
      port.calls.where((c) => c == 'enable').length,
      1,
      reason: 'откат не уходит в систему второй правкой',
    );
  });

  test('ожидание разрешения называет, что делать', () async {
    await build().start();
    port
      ..failNextWrite = true
      ..nextWriteError = PlatformException(
        code: kAutostartApprovalCode,
        message: 'requires approval in Login Items',
      );

    wantLaunchAtLogin(true);
    await pump();

    expect(messages, <String>[kAutostartApprovalMessage]);
    expect(container.read(desktopPrefsProvider).launchAtLogin, isTrue);
    expect(container.read(autostartApprovalPendingProvider), isTrue);
  });

  test('pending approval survives restart and later approval', () async {
    wantLaunchAtLogin(true);
    port.approvalPending = true;
    final first = build();
    await first.start();
    expect(container.read(autostartApprovalPendingProvider), isTrue);
    expect(port.calls, isNot(contains('enable')));
    first.dispose();

    port
      ..approvalPending = false
      ..enabled = true;
    await build().start();
    expect(port.calls, isNot(contains('disable')));
    expect(container.read(desktopPrefsProvider).launchAtLogin, isTrue);
    expect(container.read(autostartApprovalPendingProvider), isFalse);
  });

  test('pending registration is cancelled when wanted is false', () async {
    port.approvalPending = true;
    await build().start();
    expect(port.calls, contains('disable'));
    expect(port.approvalPending, isFalse);
  });

  test('pending request can be cancelled from the switch', () async {
    wantLaunchAtLogin(true);
    port.approvalPending = true;
    await build().start();
    wantLaunchAtLogin(false);
    await pump();
    expect(port.calls, contains('disable'));
    expect(port.approvalPending, isFalse);
    expect(container.read(autostartApprovalPendingProvider), isFalse);
  });

  test(
    'failed slow enable does not overwrite a newer off preference',
    () async {
      final delayed = _DelayedAutostartPort()..failNextWrite = true;
      port = delayed;
      await build().start();
      wantLaunchAtLogin(true);
      await pump();
      wantLaunchAtLogin(false);
      delayed.release.complete();
      await pump();
      await pump();
      expect(container.read(desktopPrefsProvider).launchAtLogin, isFalse);
      expect(port.enabled, isFalse);
    },
  );

  test('rapid toggles serialize writes and preserve latest intent', () async {
    final delayed = _DelayedAutostartPort();
    port = delayed;
    await build().start();
    wantLaunchAtLogin(true);
    await pump();
    wantLaunchAtLogin(false);
    await pump();
    expect(port.calls, isNot(contains('disable')));
    delayed.release.complete();
    await pump();
    await pump();
    expect(port.enabled, isFalse);
    expect(container.read(desktopPrefsProvider).launchAtLogin, isFalse);
    expect(port.calls.where((c) => c == 'disable'), hasLength(1));
  });

  test('отказ при сверке на старте тоже не роняет старт', () async {
    wantLaunchAtLogin(true);
    port.failNextWrite = true;

    await build().start();

    expect(container.read(autostartSupportedProvider), isTrue);
    expect(messages, <String>[kAutostartFailedMessage]);
    expect(
      container.read(desktopPrefsProvider).launchAtLogin,
      isFalse,
      reason: 'сверка тоже откатывает настройку, если система отказала',
    );
  });

  test('повторный start не подписывается вторым слушателем', () async {
    final service = build();
    await service.start();
    await service.start();

    wantLaunchAtLogin(true);
    await pump();

    expect(
      port.calls.where((c) => c == 'enable').length,
      1,
      reason: 'вторая подписка правила бы систему дважды на одно нажатие',
    );
  });

  test('снятая подписка больше не трогает систему', () async {
    final service = build();
    await service.start();
    service.dispose();

    wantLaunchAtLogin(true);
    await pump();

    expect(port.calls, isNot(contains('enable')));
  });

  test('провайдер собирает сервис на подставленном порту', () async {
    final wired = ProviderContainer(
      overrides: <Override>[autostartPortProvider.overrideWithValue(port)],
    );
    addTearDown(wired.dispose);
    await wired.read(appBootProvider.future);

    await wired.read(autostartServiceProvider).start();
    wired.read(desktopPrefsProvider.notifier).setLaunchAtLogin(true);
    await pump();

    expect(wired.read(autostartSupportedProvider), isTrue);
    expect(port.enabled, isTrue);
  });
}

class _DelayedAutostartPort extends FakeAutostartPort {
  final release = Completer<void>();

  @override
  Future<void> enable() async {
    await release.future;
    await super.enable();
  }
}

class _MissingChannelAutostartPort extends FakeAutostartPort {
  @override
  Future<bool> isSupported() async => throw MissingPluginException();
}

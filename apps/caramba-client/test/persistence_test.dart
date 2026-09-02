// Локальные настройки переживают перезапуск.
//
// До этого рана CoreConfig, AppSettings, first-run и режим туннеля жили только
// в памяти: онбординг всплывал при каждом запуске, а выбранный протокол
// сбрасывался. Тест моделирует именно перезапуск: пишем через нотифаеры в
// одном ProviderContainer, читаем в НОВОМ поверх тех же prefs.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/data/prefs_store.dart';
import 'package:caramba_client/state/bootstrap_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/vpn/core_policy.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

  /// Поднимает контейнер и дожидается гидратации настроек — ровно то, что при
  /// старте приложения делает роутер, придерживая сплеш.
  Future<ProviderContainer> boot() async {
    final container = ProviderContainer();
    addTearDown(container.dispose);
    await container.read(appBootProvider.future);
    return container;
  }

  test('чистая установка даёт значения по умолчанию', () async {
    final c = await boot();

    expect(c.read(firstRunProvider), isTrue);
    expect(c.read(guestModeProvider), isFalse);
    expect(c.read(settingsProvider).themeMode, ThemeMode.dark);
    expect(c.read(coreConfigProvider).protocol, 0);
    expect(c.read(tunnelModeProvider), defaultTunnelMode());
  });

  test('CoreConfig переживает перезапуск', () async {
    final first = await boot();
    final cfg = first.read(coreConfigProvider.notifier);
    cfg.setProtocol(2);
    cfg.setRoute(3);
    cfg.setStack(2);
    cfg.setMtu(1);
    cfg.setKillSwitch(false);
    cfg.setSplitMode(SplitMode.bypassSelected);
    cfg.toggleSplitApp('org.telegram');
    cfg.setBypassDomains('bank.ru, gosuslugi.ru');
    // Записи идут через unawaited: даём микротаскам добежать до prefs.
    await Future<void>.delayed(Duration.zero);

    final second = await boot();
    final restored = second.read(coreConfigProvider);

    expect(restored.protocol, 2);
    expect(restored.route, 3);
    expect(restored.stack, 2);
    expect(restored.mtu, 1);
    expect(restored.killSwitch, isFalse);
    expect(restored.splitMode, SplitMode.bypassSelected);
    expect(restored.splitApps, {'org.telegram'});
    expect(restored.bypassDomainList, ['bank.ru', 'gosuslugi.ru']);
  });

  test(
    'AppSettings, first-run, guest-mode и режим туннеля переживают перезапуск',
    () async {
      final first = await boot();
      first.read(settingsProvider.notifier).setThemeMode(ThemeMode.light);
      first.read(settingsProvider.notifier).setConnectOnSelect(true);
      first.read(firstRunProvider.notifier).done();
      first.read(guestModeProvider.notifier).enable();
      first.read(tunnelModeProvider.notifier).set(TunnelMode.tun);
      await Future<void>.delayed(Duration.zero);

      final second = await boot();

      expect(second.read(settingsProvider).themeMode, ThemeMode.light);
      expect(second.read(settingsProvider).connectOnSelect, isTrue);
      // Главный смысл персиста: онбординг больше не всплывает.
      expect(second.read(firstRunProvider), isFalse);
      expect(second.read(guestModeProvider), isTrue);
      expect(second.read(tunnelModeProvider), TunnelMode.tun);
    },
  );

  test('битая или чужая запись читается как значения по умолчанию', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      PrefsStore.kCoreConfig: 'not json',
      PrefsStore.kSettings: '{"theme_mode":"neon","kill_switch":"yes"}',
      PrefsStore.kTunnelMode: 'carrier-pigeon',
    });

    final c = await boot();

    expect(c.read(coreConfigProvider).protocol, 0);
    expect(c.read(settingsProvider).themeMode, ThemeMode.dark);
    expect(c.read(settingsProvider).killSwitch, isTrue);
    expect(c.read(tunnelModeProvider), defaultTunnelMode());
  });

  test('индекс вне списка опций клампится на дефолт', () async {
    SharedPreferences.setMockInitialValues(<String, Object>{
      // Запись версии, где протоколов было больше: индекс 99 уронил бы экраны.
      PrefsStore.kCoreConfig: '{"protocol":99,"stack":-1,"dns":7}',
    });

    final c = await boot();
    final cfg = c.read(coreConfigProvider);

    expect(cfg.protocol, 0);
    expect(cfg.stack, 0);
    expect(cfg.dns, 0);
  });
}

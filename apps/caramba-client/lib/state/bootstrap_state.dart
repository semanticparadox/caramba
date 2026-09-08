/// Асинхронная инициализация приложения: единственное место, где локальные
/// настройки поднимаются с диска и раздаются нотифаерам.
///
/// Порядок важен. Роутер решает, вести ли пользователя в онбординг
/// (`/autotune`), по [firstRunProvider]; пока настройки не прочитаны, значение
/// там дефолтное (`true`), и онбординг всплывал бы при каждом запуске. Поэтому
/// роутер держит сплеш, пока [appBootProvider] не разрешится, а гидратация идёт
/// здесь ЯВНО (а не в конструкторах нотифаеров): так момент «настройки
/// применены» детерминирован и совпадает с завершением этого future.
library;

import 'package:caramba_client/data/prefs_store.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_catalog_guard.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/vpn/core_policy.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Разрешается, когда локальные настройки прочитаны и применены. Никогда не
/// падает: недоступное хранилище означает работу на дефолтах, а не сбой старта.
final appBootProvider = FutureProvider<void>((ref) async {
  final prefs = ref.watch(prefsStoreProvider);
  await prefs.load();

  // Разбор записи защищён: сломанный снимок настроек не имеет права стать
  // отказом старта. Хуже дефолтов быть не может, и роутер должен поехать
  // дальше в любом случае.
  final settings = prefs.readJson(PrefsStore.kSettings);
  if (settings.isNotEmpty) {
    try {
      ref
          .read(settingsProvider.notifier)
          .hydrate(AppSettings.fromJson(settings));
    } catch (_) {
      // Оставляем дефолтные настройки.
    }
  }

  final core = prefs.readJson(PrefsStore.kCoreConfig);
  if (core.isNotEmpty) {
    try {
      ref.read(coreConfigProvider.notifier).hydrate(CoreConfig.fromJson(core));
    } catch (_) {
      // Оставляем дефолтную конфигурацию ядра.
    }
  }

  // Страж каталога поднимается здесь по той же причине, что и всё остальное:
  // карточка 02-SPEC.md 7.7.1 обязана пережить перезапуск. Карточка, которую
  // закрыл перезапуск, отвечена молчанием, а молчание ответом не является.
  //
  // Корзина здесь СТАРАЯ, глобальная: на этот момент активный профиль ещё не
  // известен, а привязка к профилю (csmProfileBindingProvider) перечитает
  // корзину профиля, как только он определится. Установка, заведённая до
  // разделения по профилям, так не теряет свою единственную карточку.
  final guard = prefs.readJson(kCsmCatalogGuardKey);
  if (guard.isNotEmpty) {
    try {
      ref
          .read(csmCatalogGuardProvider.notifier)
          .hydrate(CsmCatalogGuardState.fromJson(guard));
    } catch (_) {
      // Сломанный снимок стража не имеет права стать отказом старта.
    }
  }

  ref
      .read(firstRunProvider.notifier)
      .hydrate(prefs.readBool(PrefsStore.kFirstRun, fallback: true));

  ref
      .read(guestModeProvider.notifier)
      .hydrate(prefs.readBool(PrefsStore.kGuestMode, fallback: false));

  final mode = TunnelMode.fromWire(prefs.readString(PrefsStore.kTunnelMode));
  ref.read(tunnelModeProvider.notifier).hydrate(mode ?? defaultTunnelMode());
});

/// Прочитаны ли уже настройки. Роутер гейтит на нём решение про онбординг:
/// до `true` любое значение [firstRunProvider] считается неизвестным.
final appBootReadyProvider = Provider<bool>(
  (ref) => ref.watch(appBootProvider).hasValue,
);

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/prefs_store.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';

/// Локальные пользовательские настройки приложения (DESIGN.md §6.7).
///
/// Персист — [PrefsStore] (JSON под одним ключом): запись сделана более старой
/// версией грузится без миграции, каждое поле независимо падает на дефолт.
class AppSettings {
  /// Тема: system / dark / light. Тёмная — герой и дефолт.
  final ThemeMode themeMode;

  /// Авто-подключение при запуске приложения.
  final bool autoConnect;

  /// Kill-switch: блокировать трафик при падении туннеля.
  final bool killSwitch;

  /// Подключаться сразу при выборе сервера в списке.
  final bool connectOnSelect;

  /// Уведомления.
  final bool notifications;

  const AppSettings({
    this.themeMode = ThemeMode.dark,
    this.autoConnect = false,
    this.killSwitch = true,
    this.connectOnSelect = false,
    this.notifications = true,
  });

  AppSettings copyWith({
    ThemeMode? themeMode,
    bool? autoConnect,
    bool? killSwitch,
    bool? connectOnSelect,
    bool? notifications,
  }) => AppSettings(
    themeMode: themeMode ?? this.themeMode,
    autoConnect: autoConnect ?? this.autoConnect,
    killSwitch: killSwitch ?? this.killSwitch,
    connectOnSelect: connectOnSelect ?? this.connectOnSelect,
    notifications: notifications ?? this.notifications,
  );

  Map<String, dynamic> toJson() => {
    'theme_mode': themeMode.name,
    'auto_connect': autoConnect,
    'kill_switch': killSwitch,
    'connect_on_select': connectOnSelect,
    'notifications': notifications,
  };

  factory AppSettings.fromJson(Map<String, dynamic> json) {
    const d = AppSettings();
    return AppSettings(
      themeMode: _themeMode(json['theme_mode']) ?? d.themeMode,
      autoConnect: _bool(json['auto_connect'], d.autoConnect),
      killSwitch: _bool(json['kill_switch'], d.killSwitch),
      connectOnSelect: _bool(json['connect_on_select'], d.connectOnSelect),
      notifications: _bool(json['notifications'], d.notifications),
    );
  }

  /// Не-булево значение (запись чужой версии) читается как дефолт, а не рушит
  /// весь снимок настроек.
  static bool _bool(Object? v, bool fallback) => v is bool ? v : fallback;

  static ThemeMode? _themeMode(Object? v) {
    for (final m in ThemeMode.values) {
      if (m.name == v) return m;
    }
    return null;
  }
}

class SettingsNotifier extends StateNotifier<AppSettings> {
  final PrefsStore? _prefs;

  SettingsNotifier([this._prefs]) : super(const AppSettings());

  /// Ставит снимок, прочитанный из [PrefsStore] на старте (не пишет обратно).
  void hydrate(AppSettings settings) => super.state = settings;

  @override
  set state(AppSettings value) {
    super.state = value;
    unawaited(_prefs?.writeJson(PrefsStore.kSettings, value.toJson()));
  }

  void setThemeMode(ThemeMode mode) => state = state.copyWith(themeMode: mode);
  void setAutoConnect(bool v) => state = state.copyWith(autoConnect: v);
  void setKillSwitch(bool v) => state = state.copyWith(killSwitch: v);
  void setConnectOnSelect(bool v) => state = state.copyWith(connectOnSelect: v);
  void setNotifications(bool v) => state = state.copyWith(notifications: v);
}

final settingsProvider = StateNotifierProvider<SettingsNotifier, AppSettings>(
  (ref) => SettingsNotifier(ref.watch(prefsStoreProvider)),
);

/// Удобный селектор темы для [MaterialApp].
final themeModeProvider = Provider<ThemeMode>(
  (ref) => ref.watch(settingsProvider).themeMode,
);

/// Флаг «первый вход»: при нём после логина показываем autotune. Сбрасывается
/// по завершении автоподбора (или ручной настройке). При логауте снова true.
class FirstRunNotifier extends StateNotifier<bool> {
  final PrefsStore? _prefs;

  FirstRunNotifier([this._prefs]) : super(true);

  /// Ставит значение, прочитанное из [PrefsStore] на старте (не пишет обратно).
  void hydrate(bool value) => super.state = value;

  @override
  set state(bool value) {
    super.state = value;
    unawaited(_prefs?.writeBool(PrefsStore.kFirstRun, value));
  }

  void done() => state = false;
  void reset() => state = true;
}

final firstRunProvider = StateNotifierProvider<FirstRunNotifier, bool>(
  (ref) => FirstRunNotifier(ref.watch(prefsStoreProvider)),
);

/// Generic-режим: пользователь работает по своей подписке, без аккаунта панели.
///
/// Флаг ставится явным выбором на экране входа («Импортировать подписку») и
/// переживает перезапуск: иначе следующий запуск снова упирался бы в /login,
/// хотя подключаться уже есть чем. Вход в панель флаг не снимает: аккаунт и
/// своя подписка сосуществуют как разные профили подключения.
class GuestModeNotifier extends StateNotifier<bool> {
  final PrefsStore? _prefs;

  GuestModeNotifier([this._prefs]) : super(false);

  /// Ставит значение из [PrefsStore] на старте (не пишет обратно).
  void hydrate(bool value) => super.state = value;

  @override
  set state(bool value) {
    super.state = value;
    unawaited(_prefs?.writeBool(PrefsStore.kGuestMode, value));
  }

  void enable() => state = true;
  void disable() => state = false;
}

final guestModeProvider = StateNotifierProvider<GuestModeNotifier, bool>(
  (ref) => GuestModeNotifier(ref.watch(prefsStoreProvider)),
);

/// Пускать ли в приложение без аккаунта панели.
///
/// Достаточно ЛЮБОГО из двух: явно выбранный generic-режим или хотя бы один
/// заведённый профиль подключения (импортированная подписка). Второе условие
/// важно для установок, где профиль появился раньше самого флага, и для
/// импорта по deeplink.
final guestAllowedProvider = Provider<bool>((ref) {
  if (ref.watch(guestModeProvider)) return true;
  return ref.watch(connectionProfilesProvider).profiles.isNotEmpty;
});

/// Прочитан ли уже список профилей из secure storage. Роутер обязан дождаться
/// его, иначе холодный старт с профилем успевал бы отскочить на /login.
final connectionProfilesReadyProvider = Provider<bool>(
  (ref) => !ref.watch(connectionProfilesProvider).loading,
);

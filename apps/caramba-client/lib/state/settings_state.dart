import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Локальные пользовательские настройки приложения (DESIGN.md §6.7).
///
/// Пока держится в памяти (in-process). Персист в secure storage / prefs
/// подключается отдельным раном — UI уже читает/пишет через нотифаер.
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
}

class SettingsNotifier extends StateNotifier<AppSettings> {
  SettingsNotifier() : super(const AppSettings());

  void setThemeMode(ThemeMode mode) => state = state.copyWith(themeMode: mode);
  void setAutoConnect(bool v) => state = state.copyWith(autoConnect: v);
  void setKillSwitch(bool v) => state = state.copyWith(killSwitch: v);
  void setConnectOnSelect(bool v) => state = state.copyWith(connectOnSelect: v);
  void setNotifications(bool v) => state = state.copyWith(notifications: v);
}

final settingsProvider = StateNotifierProvider<SettingsNotifier, AppSettings>(
  (ref) => SettingsNotifier(),
);

/// Удобный селектор темы для [MaterialApp].
final themeModeProvider = Provider<ThemeMode>(
  (ref) => ref.watch(settingsProvider).themeMode,
);

/// Флаг «первый вход»: при нём после логина показываем autotune. Сбрасывается
/// по завершении автоподбора (или ручной настройке). При логауте снова true.
class FirstRunNotifier extends StateNotifier<bool> {
  FirstRunNotifier() : super(true);
  void done() => state = false;
  void reset() => state = true;
}

final firstRunProvider = StateNotifierProvider<FirstRunNotifier, bool>(
  (ref) => FirstRunNotifier(),
);

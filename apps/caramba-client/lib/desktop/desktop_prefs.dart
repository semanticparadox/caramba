/// Настройки десктопного окна и присутствия в системе.
///
/// Отдельный ключ, а не поле в [AppSettings], по двум причинам. Первая: эти
/// значения читает `initDesktop()` ДО `runApp` и напрямую из
/// `SharedPreferences`, минуя провайдеры, а лезть на раннем старте в чужой
/// снимок настроек значит завязать порядок инициализации на его формат.
/// Вторая: мобильная сборка про эти поля не знает вовсе, и их отсутствие в её
/// снимке не должно выглядеть потерей настроек.
library;

import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/prefs_store.dart';
import 'package:caramba_client/state/bootstrap_state.dart';
import 'package:caramba_client/state/providers.dart';

/// Ключ в `SharedPreferences`. Тот же литерал читает нативный старт окна.
const String kDesktopPrefsKey = 'caramba.desktop';

/// Снимок десктопных настроек.
///
/// Разбор каждого поля независим и падает на дефолт: запись, сделанную другой
/// версией (или руками), нельзя превращать в отказ старта. Хуже дефолтов
/// ничего не случится, а окно обязано открыться.
@immutable
class DesktopPrefs {
  /// Красная кнопка прячет окно, туннель продолжает работать. Выключенное
  /// значение возвращает поведение обычного приложения: закрыл окно, вышел.
  final bool closeToTray;

  /// Запуск без окна, только значок в строке меню (для автозапуска при входе).
  final bool startInTray;

  /// Автозапуск при входе в систему. Здесь только желание пользователя;
  /// реальную регистрацию держит `AutostartService`, и они могут разойтись
  /// (например, человек снял приложение из Login Items руками).
  final bool launchAtLogin;

  /// Последняя позиция и размер окна. `null` до первого запоминания, а также
  /// когда сохранённый прямоугольник не пережил проверку по дисплеям.
  final Rect? windowBounds;

  /// Окно было развёрнуто на весь экран. Хранится отдельно от [windowBounds]:
  /// у развёрнутого окна нужно помнить размер, к которому оно вернётся.
  final bool maximized;

  const DesktopPrefs({
    this.closeToTray = true,
    this.startInTray = false,
    this.launchAtLogin = false,
    this.windowBounds,
    this.maximized = false,
  });

  DesktopPrefs copyWith({
    bool? closeToTray,
    bool? startInTray,
    bool? launchAtLogin,
    Rect? windowBounds,
    bool clearWindowBounds = false,
    bool? maximized,
  }) => DesktopPrefs(
    closeToTray: closeToTray ?? this.closeToTray,
    startInTray: startInTray ?? this.startInTray,
    launchAtLogin: launchAtLogin ?? this.launchAtLogin,
    windowBounds: clearWindowBounds
        ? null
        : (windowBounds ?? this.windowBounds),
    maximized: maximized ?? this.maximized,
  );

  Map<String, dynamic> toJson() => {
    'close_to_tray': closeToTray,
    'start_in_tray': startInTray,
    'launch_at_login': launchAtLogin,
    if (windowBounds != null) ...{
      'x': windowBounds!.left,
      'y': windowBounds!.top,
      'w': windowBounds!.width,
      'h': windowBounds!.height,
    },
    'maximized': maximized,
  };

  factory DesktopPrefs.fromJson(Map<String, dynamic> json) {
    const d = DesktopPrefs();
    return DesktopPrefs(
      closeToTray: _bool(json['close_to_tray'], d.closeToTray),
      startInTray: _bool(json['start_in_tray'], d.startInTray),
      launchAtLogin: _bool(json['launch_at_login'], d.launchAtLogin),
      windowBounds: _rect(json),
      maximized: _bool(json['maximized'], d.maximized),
    );
  }

  /// Не-булево значение (запись чужой версии) читается как дефолт.
  static bool _bool(Object? v, bool fallback) => v is bool ? v : fallback;

  /// Прямоугольник собирается только целиком: три числа из четырёх это не
  /// «частично известная геометрия», а мусор, из которого окно не построить.
  /// Нулевые и бесконечные размеры отбрасываются там же.
  static Rect? _rect(Map<String, dynamic> json) {
    final x = _double(json['x']);
    final y = _double(json['y']);
    final w = _double(json['w']);
    final h = _double(json['h']);
    if (x == null || y == null || w == null || h == null) return null;
    if (w <= 0 || h <= 0) return null;
    return Rect.fromLTWH(x, y, w, h);
  }

  static double? _double(Object? v) {
    final d = switch (v) {
      final double x => x,
      final int x => x.toDouble(),
      _ => null,
    };
    if (d == null || !d.isFinite) return null;
    return d;
  }

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is DesktopPrefs &&
          other.closeToTray == closeToTray &&
          other.startInTray == startInTray &&
          other.launchAtLogin == launchAtLogin &&
          other.windowBounds == windowBounds &&
          other.maximized == maximized;

  @override
  int get hashCode => Object.hash(
    closeToTray,
    startInTray,
    launchAtLogin,
    windowBounds,
    maximized,
  );
}

/// Пишет каждое изменение в [PrefsStore] и умеет принять снимок с диска.
///
/// Записи не ожидаются вызывающим: настройка применяется в UI сразу, диск
/// догоняет. Так же устроены [SettingsNotifier] и остальные локальные снимки.
/// Но начатую запись помним ([flush]): выход из приложения обязан её дождаться,
/// иначе ⌘Q сразу после переключения тумблера теряет само переключение.
class DesktopPrefsNotifier extends StateNotifier<DesktopPrefs> {
  final PrefsStore? _prefs;

  DesktopPrefsNotifier([this._prefs]) : super(const DesktopPrefs());

  Future<void> _pendingWrite = Future<void>.value();

  /// Ждёт, пока последняя начатая запись настроек доедет до диска.
  ///
  /// Отказ диска проглатывается: барьер выхода не имеет права уронить
  /// завершение приложения.
  Future<void> flush() => _pendingWrite;

  /// Ставит снимок, прочитанный с диска, БЕЗ обратной записи: иначе гидратация
  /// перезаписывала бы только что прочитанное и один битый ключ размножался бы
  /// при каждом запуске.
  void hydrate(DesktopPrefs prefs) => super.state = prefs;

  @override
  set state(DesktopPrefs value) {
    super.state = value;
    final write = _prefs?.writeJson(kDesktopPrefsKey, value.toJson());
    if (write == null) return;
    _pendingWrite = write.catchError((Object _) {});
  }

  void setCloseToTray(bool v) => state = state.copyWith(closeToTray: v);

  void setStartInTray(bool v) => state = state.copyWith(startInTray: v);

  void setLaunchAtLogin(bool v) => state = state.copyWith(launchAtLogin: v);

  /// `null` стирает запомненную геометрию (окно откроется по центру).
  void setWindowBounds(Rect? bounds, {bool maximized = false}) =>
      state = state.copyWith(
        windowBounds: bounds,
        clearWindowBounds: bounds == null,
        maximized: maximized,
      );
}

final desktopPrefsProvider =
    StateNotifierProvider<DesktopPrefsNotifier, DesktopPrefs>((ref) {
      final prefs = ref.watch(prefsStoreProvider);
      final notifier = DesktopPrefsNotifier(prefs);

      // Читаем ТОЛЬКО после appBoot: до него `PrefsStore.load()` не выполнен,
      // хранилище отдаёт пустоту, и гидратация тихо закрепила бы дефолты.
      void hydrateIfReady(bool ready) {
        if (!ready) return;
        final raw = prefs.readJson(kDesktopPrefsKey);
        if (raw.isEmpty) return;
        try {
          notifier.hydrate(DesktopPrefs.fromJson(raw));
        } catch (_) {
          // Сломанный снимок оставляет дефолты и не рушит старт.
        }
      }

      hydrateIfReady(ref.read(appBootReadyProvider));
      ref.listen<bool>(appBootReadyProvider, (_, next) => hydrateIfReady(next));
      return notifier;
    });

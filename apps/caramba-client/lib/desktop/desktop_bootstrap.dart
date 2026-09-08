/// Старт десктопного окна ДО `runApp`.
///
/// ЗАЧЕМ ЭТО ЖИВЁТ ВНЕ ДЕРЕВА ВИДЖЕТОВ. Размер, позиция и сам факт показа окна
/// решаются раньше, чем есть первый кадр: окно, которое сначала открылось по
/// центру дефолтным размером, а потом прыгнуло на запомненное место, человек
/// читает как сбой. Поэтому геометрия читается напрямую из
/// `SharedPreferences`, минуя провайдеры (их контейнера ещё нет), и ставится
/// внутри `waitUntilReadyToShow` — единственного окна времени, когда окно уже
/// создано, но ещё не показано.
///
/// Отсюда же берётся второй инвариант десктопа: `setPreventClose(true)`
/// ставится ДО показа. Успей человек нажать красную кнопку в первые
/// миллисекунды, окно бы просто уничтожилось, унеся с собой туннель, — а
/// прятать его в трей мы обещали с первого кадра.
library;

import 'dart:convert';

import 'package:flutter/widgets.dart';
import 'package:screen_retriever/screen_retriever.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:window_manager/window_manager.dart';

import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/desktop_prefs.dart';
import 'package:caramba_client/desktop/desktop_strings.dart';
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/desktop/window_bounds.dart';
import 'package:caramba_client/theme/colors.dart';

/// Готовит и показывает окно. Зовётся из `main()` ровно один раз и только на
/// десктопной платформе.
///
/// Ничем не бросает: отказавший плагин экранов или битая запись настроек — не
/// повод не открыть окно вовсе. Худшее, что может случиться, — окно первого
/// запуска по центру.
Future<void> initDesktop() async {
  await windowManager.ensureInitialized();

  final prefs = await _readDesktopPrefs();
  // Проверка по дисплеям обязательна: между запусками монитор отключают, и
  // сохранённый прямоугольник начинает указывать в никуда.
  final bounds = restoreBounds(prefs.windowBounds, await _visibleBounds());

  final options = WindowOptions(
    // `size` нужен и при восстановлении: `setBounds` внутри колбэка приедет
    // уже после первого замера, и без него окно мигнуло бы дефолтом.
    size: bounds?.size ?? DesktopTokens.windowDefault,
    minimumSize: DesktopTokens.windowMin,
    // Центрируем только когда ставить некуда: у восстановленного окна центр
    // перебил бы запомненную позицию.
    center: bounds == null,
    // На macOS и Windows заголовок рисуем сами (трафик-лайты и кнопки живут
    // внутри тулбара). На Linux оставляем системный: свои кнопки там пришлось
    // бы рисовать под каждое окружение рабочего стола.
    titleBarStyle: isLinuxPlatform
        ? TitleBarStyle.normal
        : TitleBarStyle.hidden,
    // Трафик-лайты macOS рисует система; мы освобождаем им место отступом
    // `DesktopTokens.macTrafficLightInset` в тулбаре.
    windowButtonVisibility: true,
    skipTaskbar: false,
    title: DesktopStrings.appName,
    // Тёмная база: окно открывается раньше темы приложения, и белая вспышка
    // на старте видна каждый запуск.
    backgroundColor: AppColors.dark.bgBase,
  );

  await windowManager.waitUntilReadyToShow(options, () async {
    if (bounds != null) await windowManager.setBounds(bounds);
    // Развёрнутое окно помнит и размер, к которому вернётся: сначала ставим
    // прямоугольник, потом разворачиваем.
    if (prefs.maximized) await windowManager.maximize();
    await windowManager.setPreventClose(true);
    // Запуск «только значок в строке меню»: окна не показываем вовсе.
    if (!prefs.startInTray) {
      await windowManager.show();
      await windowManager.focus();
    }
  });
}

/// Десктопные настройки прямо с диска.
///
/// Мимо [PrefsStore] и провайдеров намеренно: на этом шаге ни контейнера
/// Riverpod, ни дерева виджетов ещё нет. Ключ и формат те же, что у
/// [DesktopPrefs], поэтому запись из приложения читается здесь без миграций.
Future<DesktopPrefs> _readDesktopPrefs() async {
  try {
    final prefs = await SharedPreferences.getInstance();
    final raw = prefs.getString(kDesktopPrefsKey);
    if (raw == null || raw.isEmpty) return const DesktopPrefs();
    final decoded = jsonDecode(raw);
    if (decoded is! Map<String, dynamic>) return const DesktopPrefs();
    return DesktopPrefs.fromJson(decoded);
  } catch (_) {
    // Битый снимок (правка руками, запись чужой версии) стоит дефолтов, а не
    // несостоявшегося старта.
    return const DesktopPrefs();
  }
}

/// Видимые области всех дисплеев прямоугольниками.
///
/// Берём именно ВИДИМУЮ область (без строки меню и дока), потому что окно,
/// поставленное под системную панель, схватить мышью так же нечем, как окно за
/// краем экрана. Плагин отказал — отдаём пустой список: тогда
/// [restoreBounds] честно решит, что ставить некуда, и окно откроется по
/// центру.
Future<List<Rect>> _visibleBounds() async {
  try {
    final displays = await screenRetriever.getAllDisplays();
    return <Rect>[
      for (final d in displays)
        (d.visiblePosition ?? Offset.zero) & (d.visibleSize ?? d.size),
    ];
  } catch (_) {
    return const <Rect>[];
  }
}

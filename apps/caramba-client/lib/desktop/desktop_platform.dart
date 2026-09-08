/// Признак десктопной платформы для всего десктопного слоя.
///
/// ПОЧЕМУ `defaultTargetPlatform`, а НЕ `dart:io Platform`. Тесты клиента
/// (975 штук) гоняются на macOS-хосте, где `Platform.isMacOS` истинно всегда.
/// Возьми мы `dart:io`, каждый существующий виджет-тест внезапно поехал бы по
/// десктопной ветке и мобильный шелл перестал бы проверяться вовсе.
/// `defaultTargetPlatform` в тестах равен `TargetPlatform.android`, пока тест
/// сам не выставит `debugDefaultTargetPlatformOverride`, поэтому десктопные
/// ветки видят только те тесты, которые их и проверяют.
///
/// Второе следствие того же выбора: ветка выбирается ПЛАТФОРМОЙ, а не шириной
/// окна. Узкое окно на Маке остаётся десктопом (трей, тулбар, шорткаты никуда
/// не деваются), а планшет с широким экраном остаётся мобильным.
library;

import 'package:flutter/foundation.dart';

/// Работаем ли мы на десктопной ОС (macOS, Windows, Linux) и не в вебе.
bool get isDesktopPlatform =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.macOS ||
        defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.linux);

/// macOS: трафик-лайты в тулбаре, строка меню, `SMAppService` для автозапуска.
bool get isMacOSPlatform =>
    !kIsWeb && defaultTargetPlatform == TargetPlatform.macOS;

/// Windows: свои кнопки окна, трей с левым кликом, автозапуск через реестр.
bool get isWindowsPlatform =>
    !kIsWeb && defaultTargetPlatform == TargetPlatform.windows;

/// Linux: системный заголовок окна, меню трея без левого клика.
bool get isLinuxPlatform =>
    !kIsWeb && defaultTargetPlatform == TargetPlatform.linux;

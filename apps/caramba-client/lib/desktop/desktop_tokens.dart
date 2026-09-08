/// Геометрия десктопного окна: карта окна из DESKTOP-SPEC.md, раздел 2.
///
/// Здесь ТОЛЬКО размеры и длительности. Цвета и типографика не дублируются:
/// десктоп берёт их из `context.c` и `AppType`, иначе тема поедет на одной из
/// платформ и вернуть её будет некуда.
library;

import 'package:flutter/widgets.dart';

import 'package:caramba_client/theme/spacing.dart';

abstract final class DesktopTokens {
  /// Меньше окно не ужимается: при 960 две колонки Home ещё держат дайл 232
  /// и правую колонку 460, ниже 640 по высоте пропадает статус-блок сайдбара.
  static const Size windowMin = Size(960, 640);

  /// Размер первого запуска, пока в prefs нет сохранённых bounds.
  static const Size windowDefault = Size(1120, 720);

  /// Высота тулбара. Совпадает с высотой верхней полосы сайдбара, чтобы
  /// вордмарк и заголовок раздела стояли на одной линии.
  static const double toolbarHeight = 52;

  /// Отступ слева под системные трафик-лайты macOS (кнопки рисует система,
  /// мы обязаны освободить место).
  static const double macTrafficLightInset = 78;

  /// Ширина одной кнопки заголовка на Windows (min/max/close), высота = тулбар.
  static const double winCaptionButtonWidth = 46;

  static const double sidebarWidth = 240;

  /// Строка навигации сайдбара: десктопная плотность, мобильные 48 здесь
  /// выглядят разреженно.
  static const double navRowHeight = 40;

  static const double sidebarPad = 12;

  /// Отступы контентной области.
  static const double contentPad = AppSpace.screenPadDesktop;

  /// Шире 1120 колонки расползаются, и строки настроек читаются хуже.
  static const double contentMaxWidth = 1120;

  static const double columnGap = AppSpace.s6;

  /// Левая панель Home: дайл 232 плюс подписи и кнопки без переносов.
  static const double homeLeftPane = 420;

  /// Накладная панель справа: широкая под таблицы и списки.
  static const double panelWide = 720;

  /// Накладная панель справа: узкая под формы в одну колонку.
  static const double panelNarrow = 560;

  /// Центрированный диалог. Тот же предел, что у мобильного диалога, чтобы
  /// формы входа выглядели одинаково на всех платформах.
  static const double dialogMaxWidth = AppBreakpoints.dialogMaxWidth;

  /// Левый индекс разделов в настройках.
  static const double settingsIndexWidth = 200;

  /// Форма настроек: длиннее строки лейбл и контрол расходятся слишком далеко.
  static const double settingsFormMaxWidth = 720;

  /// Строка таблицы серверов.
  static const double tableRowHeight = 48;

  /// Выезд панели справа.
  static const Duration panelSlide = AppMotion.standard;

  /// Появление диалога: только opacity, без масштабирования.
  static const Duration dialogFade = AppMotion.micro;

  /// Версия для подписи внизу сайдбара и пункта «О программе».
  ///
  /// ВНИМАНИЕ: синхронизируется с `pubspec.yaml` (`version: 1.0.0+105`)
  /// вручную. Читать её из `package_info_plus` на десктопе нельзя без лишнего
  /// плагина, а расхождение видно глазом при первом же запуске сборки.
  static const String kAppVersion = '1.0.0 (105)';
}

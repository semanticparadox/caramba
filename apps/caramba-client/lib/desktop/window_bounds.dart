/// Проверка запомненной геометрии окна по нынешней конфигурации дисплеев.
///
/// ЗАЧЕМ отдельный файл и чистые функции. Между двумя запусками монитор
/// отключают, разрешение меняют, ноутбук уносят из дока — и сохранённый
/// прямоугольник начинает указывать в никуда. Окно, открытое за пределами
/// видимой области, для человека неотличимо от приложения, которое не
/// запустилось: процесс есть, показать нечего, вернуть нечем. Проверка не
/// имеет права зависеть от плагина, поэтому дисплеи приходят готовым списком
/// прямоугольников, а решение проверяется тестом без единого канала.
library;

import 'dart:math' as math;

import 'package:flutter/widgets.dart';

import 'package:caramba_client/desktop/desktop_tokens.dart';

/// Кусок окна, который обязан остаться на экране, чтобы окно можно было
/// схватить: заголовок целиком по ширине не нужен, но полоса 200×100 даёт и
/// видимый край, и место, за которое окно тащат мышью.
const Size kMinVisiblePatch = Size(200, 100);

/// Геометрия, с которой окно можно открыть, или `null` — «забудь, ставь по
/// центру».
///
/// Порядок проверок именно такой: сначала размер доводится до минимума, потом
/// проверяется видимость. Обратный порядок проверял бы не тот прямоугольник,
/// который в итоге увидит человек: подросшее до минимума окно занимает больше
/// места, чем сохранённое, и «уже видимое» после подгонки могло бы уехать.
Rect? restoreBounds(
  Rect? saved,
  List<Rect> displayVisibleBounds, {
  Size min = DesktopTokens.windowMin,
}) {
  if (saved == null) return null;
  // Мусор в снимке (NaN, бесконечность) до сюда доехать не должен, но цена
  // ошибки — окно неизвестно где, поэтому отсекаем ещё раз.
  if (!saved.left.isFinite ||
      !saved.top.isFinite ||
      !saved.width.isFinite ||
      !saved.height.isFinite) {
    return null;
  }

  final rect = Rect.fromLTWH(
    saved.left,
    saved.top,
    math.max(saved.width, min.width),
    math.max(saved.height, min.height),
  );

  for (final display in displayVisibleBounds) {
    final overlap = rect.intersect(display);
    // `Rect.intersect` у непересекающихся прямоугольников отдаёт
    // отрицательные размеры, поэтому сравнение с порогом заодно отсекает и их.
    if (overlap.width >= kMinVisiblePatch.width &&
        overlap.height >= kMinVisiblePatch.height) {
      return rect;
    }
  }
  return null;
}

/// Окно первого запуска: [DesktopTokens.windowDefault] по центру основного
/// дисплея.
///
/// На дисплее, который меньше желаемого размера, окно ужимается до дисплея, но
/// не ниже [min]: лучше выйти краями за маленький экран, чем показать шелл, в
/// который не помещается ни сайдбар, ни контент.
Rect centeredDefault(
  Rect primary, {
  Size size = DesktopTokens.windowDefault,
  Size min = DesktopTokens.windowMin,
}) {
  final width = math.max(min.width, math.min(size.width, primary.width));
  final height = math.max(min.height, math.min(size.height, primary.height));
  return Rect.fromLTWH(
    primary.left + (primary.width - width) / 2,
    primary.top + (primary.height - height) / 2,
    width,
    height,
  );
}

// Запомненная геометрия против нынешней конфигурации дисплеев.
//
// Проверяется одно свойство: после перезапуска окно обязано оказаться там, где
// его видно и где его можно схватить. Отключённый монитор, сменившееся
// разрешение, вынутый из дока ноутбук — всё это приводит к прямоугольнику,
// который указывает в никуда, а окно за пределами экрана для человека
// неотличимо от приложения, которое не запустилось.

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/desktop/window_bounds.dart';

/// Основной дисплей ноутбука: начало координат, вычтена строка меню.
const Rect _laptop = Rect.fromLTWH(0, 25, 1512, 957);

/// Внешний монитор СЛЕВА от основного: отрицательные координаты — обычное дело
/// на реальном столе, и попасть в «за экраном» они не должны.
const Rect _external = Rect.fromLTWH(-2560, 0, 2560, 1440);

void main() {
  group('restoreBounds', () {
    test('пустой снимок остаётся пустым', () {
      expect(restoreBounds(null, const <Rect>[_laptop]), isNull);
    });

    test('целиком видимое окно сохраняется как есть', () {
      const saved = Rect.fromLTWH(120, 80, 1120, 720);

      expect(restoreBounds(saved, const <Rect>[_laptop]), saved);
    });

    test('окно за пределами всех дисплеев отбрасывается', () {
      // Монитор, на котором окно жило, отключили: остался только ноутбук.
      const saved = Rect.fromLTWH(-2400, 200, 1120, 720);

      expect(restoreBounds(saved, const <Rect>[_laptop]), isNull);
    });

    test('частично видимое окно сохраняется', () {
      // Съехало вправо: на экране осталась полоса шире 200 и выше 100.
      const saved = Rect.fromLTWH(1112, 100, 1120, 720);

      expect(restoreBounds(saved, const <Rect>[_laptop]), saved);
    });

    test('видимого края меньше 200x100 не хватает', () {
      // На экране остались 112 пикселей по ширине: схватить окно нечем.
      const saved = Rect.fromLTWH(1400, 100, 1120, 720);

      expect(restoreBounds(saved, const <Rect>[_laptop]), isNull);
    });

    test('окно меньше минимума подрастает до минимума', () {
      const saved = Rect.fromLTWH(200, 200, 400, 300);

      final restored = restoreBounds(saved, const <Rect>[_laptop]);

      expect(restored, isNotNull);
      expect(restored!.size, DesktopTokens.windowMin);
      // Левый верхний угол не двигается: человек ставил окно именно туда.
      expect(restored.topLeft, saved.topLeft);
    });

    test('второй дисплей считается наравне с основным', () {
      const saved = Rect.fromLTWH(-2000, 300, 1120, 720);

      expect(restoreBounds(saved, const <Rect>[_laptop, _external]), saved);
    });

    test('без дисплеев вовсе решения нет', () {
      const saved = Rect.fromLTWH(0, 0, 1120, 720);

      expect(restoreBounds(saved, const <Rect>[]), isNull);
    });

    test('нечисловая геометрия отбрасывается, а не подгоняется', () {
      const saved = Rect.fromLTWH(double.nan, 0, 1120, 720);

      expect(restoreBounds(saved, const <Rect>[_laptop]), isNull);
    });
  });

  group('centeredDefault', () {
    test('окно по умолчанию встаёт по центру основного дисплея', () {
      final rect = centeredDefault(_laptop);

      expect(rect.size, DesktopTokens.windowDefault);
      expect(rect.center, _laptop.center);
    });

    test('на маленьком дисплее размер ужимается, но не ниже минимума', () {
      const small = Rect.fromLTWH(0, 0, 1024, 600);

      final rect = centeredDefault(small);

      expect(rect.width, 1024);
      // По высоте дисплей меньше минимума: окно остаётся минимальным и
      // выходит за край, иначе в него не помещается шелл.
      expect(rect.height, DesktopTokens.windowMin.height);
    });
  });
}

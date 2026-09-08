// Десктопный пикер: открыть, выбрать, не выбрать выключенное, закрыть Esc.
//
// Проверяется ровно то, ради чего он заменил нижний лист, и ровно то, что от
// такой замены ломается молча:
//   * недоступное значение ВИДНО и названо причиной (02-SPEC.md 7.2, 7.9) —
//     самый лёгкий способ «починить» меню это выкинуть из него строку, и
//     тогда человек ищет пропавшее значение в обновлении, которого нет;
//   * выключенная строка не отдаёт выбор наружу;
//   * Esc закрывает меню. Оно закрывается только если фокус УШЁЛ внутрь меню
//     при открытии — без этого `DismissIntent` до панели не доходит, и
//     клавиатура молча перестаёт работать, оставляя мышь рабочей.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/desktop/widgets/desktop_picker.dart';
import 'package:caramba_client/theme/app_theme.dart';

const _options = <({String name, String desc, String? icon})>[
  (name: 'Авто', desc: 'Выбирается под платформу.', icon: null),
  (name: 'System', desc: 'Стек ОС.', icon: null),
  (name: 'gVisor', desc: 'Изолированный стек.', icon: null),
];

/// Десктопная ветка выбирается ПЛАТФОРМОЙ, а не шириной окна, поэтому тест
/// обязан объявить себя маком.
///
/// Возврат стоит в `finally`, а не в `tearDown`: flutter_test проверяет
/// «отладочные переменные foundation вернули в исходное» ещё ДО tearDown, и
/// тест, честно прибравший за собой в tearDown, всё равно падал бы этой
/// проверкой — причём падал бы СЛЕДУЮЩИЙ тест, а не тот, что наследил.
Future<void> _desktop(WidgetTester tester, Future<void> Function() body) async {
  debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
  tester.view
    ..physicalSize = const Size(1280, 800)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  try {
    await body();
  } finally {
    debugDefaultTargetPlatformOverride = null;
  }
}

Future<void> _pump(
  WidgetTester tester, {
  required void Function(int) onSelected,
  int selected = 0,
  Map<int, String> disabled = const <int, String>{},
}) async {
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.dark(),
      home: Scaffold(
        body: Center(
          child: DesktopPicker(
            options: _options,
            selected: selected,
            disabled: disabled,
            onSelected: onSelected,
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('поле показывает текущее значение и открывает меню', (
    tester,
  ) async {
    await _desktop(tester, () async {
      await _pump(tester, selected: 2, onSelected: (_) {});

      expect(find.text('gVisor'), findsOneWidget);
      // До открытия описаний нет: поле показывает только имя.
      expect(find.text('Стек ОС.'), findsNothing);

      await tester.tap(find.byType(DesktopPicker));
      await tester.pumpAndSettle();

      expect(find.text('Авто'), findsOneWidget);
      expect(find.text('System'), findsOneWidget);
      // Текущее значение теперь и в поле, и строкой меню.
      expect(find.text('gVisor'), findsNWidgets(2));
      expect(find.text('Стек ОС.'), findsOneWidget);
    });
  });

  testWidgets('выбор отдаёт индекс и закрывает меню', (tester) async {
    await _desktop(tester, () async {
      final picked = <int>[];
      await _pump(tester, onSelected: picked.add);

      await tester.tap(find.byType(DesktopPicker));
      await tester.pumpAndSettle();
      await tester.tap(find.text('gVisor'));
      await tester.pumpAndSettle();

      expect(picked, <int>[2]);
      // Меню закрылось: описания снова нет на экране.
      expect(find.text('Изолированный стек.'), findsNothing);
    });
  });

  testWidgets('выключенное значение видно, названо причиной и не выбирается', (
    tester,
  ) async {
    await _desktop(tester, () async {
      final picked = <int>[];
      await _pump(
        tester,
        onSelected: picked.add,
        disabled: const <int, String>{1: 'Оператор не предлагает этот стек.'},
      );

      await tester.tap(find.byType(DesktopPicker));
      await tester.pumpAndSettle();

      // Строка на месте, а не выкинута из списка.
      expect(find.text('System'), findsOneWidget);
      // Причина ЗАМЕЩАЕТ описание.
      expect(find.text('Оператор не предлагает этот стек.'), findsOneWidget);
      expect(find.text('Стек ОС.'), findsNothing);

      await tester.tap(find.text('System'));
      await tester.pumpAndSettle();

      expect(picked, isEmpty, reason: 'выключенная строка отдала выбор наружу');
    });
  });

  testWidgets('Esc закрывает меню', (tester) async {
    await _desktop(tester, () async {
      await _pump(tester, onSelected: (_) {});

      await tester.tap(find.byType(DesktopPicker));
      await tester.pumpAndSettle();
      expect(find.text('Изолированный стек.'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();

      expect(
        find.text('Изолированный стек.'),
        findsNothing,
        reason: 'Esc не дошёл до меню: фокус не ушёл внутрь при открытии',
      );
    });
  });
}

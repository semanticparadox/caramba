// Листы на десктопе становятся диалогами.
//
// Проверяется ровно та подмена, которую нельзя увидеть глазами на телефоне:
// один и тот же вызов обязан дать `BottomSheet` на android и `Dialog` на macOS,
// а десктопный пикер — уметь то, чего лист не умел никогда: закрываться по Esc.
// Отдельно сторожится выключенная строка: она видима, но нажатие по ней не
// возвращает индекс (иначе человек молча выбрал бы то, чего оператор не даёт).

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/desktop/adaptive_sheet.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Три опции пикера: первая выбрана, третью тесты выключают.
const _options = <({String name, String desc, String? icon})>[
  (name: 'Первый', desc: 'описание первого', icon: null),
  (name: 'Второй', desc: 'описание второго', icon: null),
  (name: 'Третий', desc: 'описание третьего', icon: null),
];

/// Хост с одной кнопкой: по ней открывается проверяемая модалка.
Widget _host(void Function(BuildContext ctx) onTap) => MaterialApp(
  theme: AppTheme.dark(),
  home: Scaffold(
    body: Builder(
      builder: (ctx) => Center(
        child: TextButton(
          onPressed: () => onTap(ctx),
          child: const Text('открыть'),
        ),
      ),
    ),
  ),
);

/// Тест на десктопной платформе в окне 1280x800.
///
/// Сброс `debugDefaultTargetPlatformOverride` стоит в `finally` ВНУТРИ тела, а
/// не только в `tearDown`: `testWidgets` сверяет debug-переменные сразу после
/// тела теста, раньше любых tearDown, и оставленный override валит тест
/// сообщением «The value of a foundation debug variable was changed». tearDown
/// ниже остаётся страховкой на случай падения до `finally`.
void _desktopTest(
  String description,
  Future<void> Function(WidgetTester) body,
) {
  testWidgets(description, (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    try {
      await body(tester);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}

void main() {
  tearDown(() => debugDefaultTargetPlatformOverride = null);

  group('showAdaptiveSheet', () {
    testWidgets('на android это нижний лист', (tester) async {
      await tester.pumpWidget(
        _host(
          (ctx) => showAdaptiveSheet<void>(
            ctx,
            builder: (_) => const Text('содержимое'),
          ),
        ),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();

      expect(find.byType(BottomSheet), findsOneWidget);
      expect(find.byType(Dialog), findsNothing);
      expect(find.text('содержимое'), findsOneWidget);
    });

    _desktopTest('на macOS это диалог', (tester) async {
      await tester.pumpWidget(
        _host(
          (ctx) => showAdaptiveSheet<void>(
            ctx,
            builder: (_) => const Text('содержимое'),
          ),
        ),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();

      expect(find.byType(Dialog), findsOneWidget);
      expect(find.byType(BottomSheet), findsNothing);
      expect(find.text('содержимое'), findsOneWidget);
    });

    _desktopTest('диалог не шире dialogMaxWidth', (tester) async {
      await tester.pumpWidget(
        _host(
          (ctx) => showAdaptiveSheet<void>(
            ctx,
            builder: (_) => const Text('содержимое'),
          ),
        ),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();

      // Меряется поверхность диалога, а не сам виджет Dialog: его рендер-объект
      // это padding во весь экран, и 1280 у него были бы всегда.
      final surface = find
          .descendant(of: find.byType(Dialog), matching: find.byType(Material))
          .first;
      // 520, а не 1200: без ограничения диалог растянулся бы на всё окно, и
      // текст в две строки читался бы одной строкой через весь экран.
      expect(tester.getSize(surface).width, 520);
    });

    _desktopTest('pop внутри билдера возвращает значение', (tester) async {
      String? result;
      await tester.pumpWidget(
        _host((ctx) async {
          result = await showAdaptiveSheet<String>(
            ctx,
            builder: (sheetCtx) => TextButton(
              onPressed: () => Navigator.of(sheetCtx).pop('да'),
              child: const Text('согласиться'),
            ),
          );
        }),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('согласиться'));
      await tester.pumpAndSettle();

      expect(result, 'да');
      expect(find.byType(Dialog), findsNothing);
    });

    _desktopTest('Esc закрывает диалог', (tester) async {
      var closed = false;
      await tester.pumpWidget(
        _host((ctx) async {
          await showAdaptiveSheet<void>(
            ctx,
            builder: (_) => const Text('содержимое'),
          );
          closed = true;
        }),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();

      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();

      expect(closed, isTrue);
      expect(find.byType(Dialog), findsNothing);
    });
  });

  group('showPickerSheet', () {
    testWidgets('на android остаётся нижним листом', (tester) async {
      await tester.pumpWidget(
        _host(
          (ctx) => showPickerSheet(
            context: ctx,
            title: 'Заголовок',
            subtitle: 'Подпись',
            options: _options,
            selected: 0,
          ),
        ),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();

      expect(find.byType(BottomSheet), findsOneWidget);
      expect(find.byType(Dialog), findsNothing);
    });

    _desktopTest('на macOS открывается диалогом с тем же списком', (
      tester,
    ) async {
      await tester.pumpWidget(
        _host(
          (ctx) => showPickerSheet(
            context: ctx,
            title: 'Заголовок',
            subtitle: 'Подпись',
            options: _options,
            selected: 0,
          ),
        ),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();

      expect(find.byType(Dialog), findsOneWidget);
      expect(find.byType(BottomSheet), findsNothing);
      expect(find.text('Заголовок'), findsOneWidget);
      expect(find.text('Подпись'), findsOneWidget);
      expect(find.byType(ListItemCard), findsNWidgets(3));
    });

    _desktopTest('выбор строки возвращает её индекс', (tester) async {
      int? picked;
      await tester.pumpWidget(
        _host((ctx) async {
          picked = await showPickerSheet(
            context: ctx,
            title: 'Заголовок',
            subtitle: 'Подпись',
            options: _options,
            selected: 0,
          );
        }),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Второй'));
      await tester.pumpAndSettle();

      expect(picked, 1);
      expect(find.byType(Dialog), findsNothing);
    });

    _desktopTest('Esc закрывает и возвращает null', (tester) async {
      var closed = false;
      int? picked;
      await tester.pumpWidget(
        _host((ctx) async {
          picked = await showPickerSheet(
            context: ctx,
            title: 'Заголовок',
            subtitle: 'Подпись',
            options: _options,
            selected: 0,
          );
          closed = true;
        }),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();

      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();

      expect(closed, isTrue);
      expect(picked, isNull);
      expect(find.byType(Dialog), findsNothing);
    });

    _desktopTest('выключенная строка видна, но не нажимается', (tester) async {
      int? picked;
      await tester.pumpWidget(
        _host((ctx) async {
          picked = await showPickerSheet(
            context: ctx,
            title: 'Заголовок',
            subtitle: 'Подпись',
            options: _options,
            selected: 0,
            disabled: const {2: 'оператор не предлагает'},
          );
        }),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();

      // Строка на месте, и вместо описания у неё названа причина.
      expect(find.text('Третий'), findsOneWidget);
      expect(find.text('оператор не предлагает'), findsOneWidget);

      await tester.tap(find.text('Третий'));
      await tester.pumpAndSettle();

      expect(picked, isNull);
      expect(find.byType(Dialog), findsOneWidget);
    });

    _desktopTest('крестик закрывает диалог', (tester) async {
      int? picked;
      var closed = false;
      await tester.pumpWidget(
        _host((ctx) async {
          picked = await showPickerSheet(
            context: ctx,
            title: 'Заголовок',
            subtitle: 'Подпись',
            options: _options,
            selected: 0,
          );
          closed = true;
        }),
      );
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(IconBtn));
      await tester.pumpAndSettle();

      expect(closed, isTrue);
      expect(picked, isNull);
    });
  });
}

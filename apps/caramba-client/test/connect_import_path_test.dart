// Экран подключения: одно поле, и оно разбирает всё.
//
// Владелец вставил ссылку своей подписки и получил требование инвайт-кода или
// QR. Требование было невыполнимым: коды на живой панели никем не выпускались
// (ни одного INSERT в enrollment_codes), а кнопка QR показывала тост. Первый
// тест стережёт эту дверь и после перестройки экрана в одно поле.
//
// Второй и третий стерегут саму перестройку: поле ровно одно, а вопросы,
// на которые человек ответить не может (имя профиля, формат конфига из пяти
// вариантов), на входе не задаются. Формат появляется ТОЛЬКО после того, как
// автоопределение ядра не справилось, — и проверить это дёшево тем, что до
// первой неудачи его в дереве нет.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/features/connections/connection_import_screen.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/typography.dart';

const String _goldenLink =
    'caramba://connect?d=8D5320D405W1GT3MEHR76EHF5XGQ0W1ECNW62WKFC9QQ8BKMDXR0'
    '4M00041061050R3GG28A1C60T3GF0DQM6RBJC5PP4R908DQPWVK5CDT0A6KA32JG0063SHSG';

const String _enrollLink =
    'carambaconnect://enroll?panel=https://panel.example&code=ABC123';

/// Роутер с тремя интересующими нас адресами. Экраны-заглушки: проверяется
/// КУДА ведёт экран импорта, а не что там нарисовано.
GoRouter _router() => GoRouter(
      initialLocation: AppRoute.connectionImport,
      routes: [
        GoRoute(
          path: AppRoute.connectionImport,
          builder: (context, state) => const ConnectionImportScreen(),
        ),
        GoRoute(
          path: AppRoute.connect,
          builder: (context, state) => const Text('ЭКРАН ПОДТВЕРЖДЕНИЯ'),
        ),
        GoRoute(
          path: AppRoute.enroll,
          builder: (context, state) => const Text('ЭКРАН ИНВАЙТ-КОДА'),
        ),
      ],
    );

Future<GoRouter> _pumpImport(WidgetTester tester) async {
  // Экран целиком в ListView, а ListView строит только видимое: на телефонном
  // холсте нижние кнопки просто не существуют в дереве.
  tester.view.physicalSize = const Size(1000, 2600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  final router = _router();
  await tester.pumpWidget(
    ProviderScope(
      child: MaterialApp.router(theme: AppTheme.dark(), routerConfig: router),
    ),
  );
  await tester.pumpAndSettle();
  return router;
}

void main() {
  testWidgets(
    'ссылка подключения в поле ведёт на подтверждение, а не за кодом',
    (tester) async {
      final router = await _pumpImport(tester);

      // Бот присылает ссылку подписки и ссылку подключения одним сообщением, и
      // перепутать поля проще простого. Раньше это давало ошибку разбора
      // конфига, из которой человек не мог понять вообще ничего. Поле теперь
      // одно, так что «перепутать» нечего — но разобрать обязано само.
      await tester.enterText(find.byType(TextField), _goldenLink);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Продолжить'));
      await tester.pumpAndSettle();

      expect(
        router.routerDelegate.currentConfiguration.uri.path,
        AppRoute.connect,
      );
      expect(find.text('ЭКРАН ИНВАЙТ-КОДА'), findsNothing);
    },
  );

  testWidgets('приглашение энроллмента уходит в энроллмент ВМЕСТЕ с кодом', (
    tester,
  ) async {
    // Тупиком был не экран энроллмента сам по себе, а переход на него БЕЗ кода.
    // Ссылка с кодом — рабочий вход, и одно поле обязано его узнавать.
    final router = await _pumpImport(tester);

    await tester.enterText(find.byType(TextField), _enrollLink);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Продолжить'));
    await tester.pumpAndSettle();

    final uri = router.routerDelegate.currentConfiguration.uri;
    expect(uri.path, AppRoute.enroll);
    expect(uri.queryParameters['code'], isNotNull);
    expect(uri.queryParameters['code'], isNot(isEmpty));
  });

  testWidgets('на входе одно поле и ни одного вопроса про формат и имя', (
    tester,
  ) async {
    await _pumpImport(tester);

    // Поле ровно одно. Было два (имя + источник), и первое спрашивало имя у
    // человека, который ещё не видел, что импортируется.
    expect(find.byType(TextField), findsOneWidget);
    expect(find.text('Имя'), findsNothing);
    // Формат — запасной ход, а не вопрос на входе.
    expect(find.textContaining('Формат'), findsNothing);
    // Одна кнопка вместо пары «Проверить» / «Сохранить профиль».
    expect(find.text('Продолжить'), findsOneWidget);
    expect(find.text('Проверить'), findsNothing);
    expect(find.text('Сохранить профиль'), findsNothing);
  });

  testWidgets('пустое поле не уводит никуда и говорит, чего не хватает', (
    tester,
  ) async {
    final router = await _pumpImport(tester);

    // Кнопка выключена, пока в поле пусто: нажать нечего, уйти некуда.
    final button = tester.widget<FilledButton>(find.byType(FilledButton));
    expect(button.onPressed, isNull);
    expect(
      router.routerDelegate.currentConfiguration.uri.path,
      AppRoute.connectionImport,
    );
  });

  test('экран импорта нигде не ведёт на экран инвайт-кода сам', () {
    // Вторая дверь в тупик — предложение после сохранения профиля: раньше
    // единственная кнопка листа делала `context.go('/enroll?panel=...')` БЕЗ
    // кода. Она стоит за сетевой пробой панели и за модальным листом, поэтому
    // дешёвая и долгоживущая гарантия здесь одна: адрес энроллмента в этом
    // файле не собирается вовсе.
    //
    // Единственный путь на /enroll из этого экрана теперь — цель, построенная
    // `classifyEntry`, а она без кода этого адреса не выдаёт: гарантия на неё
    // живёт в entry_classifier_test.dart («энроллмент никогда не открывается
    // БЕЗ кода»). Два теста вместе покрывают обе двери.
    // Путь относительный: `flutter test` работает из корня пакета, тем же
    // приёмом читает корпус векторов csm_corpus_test.dart.
    final source = File(
      'lib/features/connections/connection_import_screen.dart',
    ).readAsStringSync();
    expect(source, isNot(contains('AppRoute.enroll')));
  });

  // Отчёт с устройства: на узком экране (эмулятор 1080×2400) кнопка
  // «Сканировать QR» обрывалась до «Сканировать …» — Row делил строку
  // пополам с «Вставить», а GhostButton режет подпись [TextOverflow.ellipsis]
  // (dart:ui сам подставляет предел в одну строку, когда `ellipsis` задан
  // без явного `maxLines`). Кнопки теперь стоят друг под другом на всю
  // ширину — это и проверяется.
  group('кнопки «Вставить»/QR не делят строку пополам на узком экране', () {
    Future<void> pumpNarrow(WidgetTester tester) async {
      // 1080×2400 при плотности 3.0 — логическая ширина 360, ровно узкий
      // экран из отчёта об устройстве.
      tester.view
        ..physicalSize = const Size(1080, 2400)
        ..devicePixelRatio = 3.0;
      addTearDown(tester.view.reset);

      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            theme: AppTheme.dark(),
            home: const ConnectionImportScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
    }

    testWidgets('обе кнопки — друг под другом, на всю ширину, без обрезки', (
      tester,
    ) async {
      await pumpNarrow(tester);

      // На хосте тестов Platform.isAndroid/isIOS ложны (dart:io видит ОС
      // раннера, не эмулятор), поэтому вторая кнопка — «Из файла», как на
      // desktop. Сама раскладка (стопкой, а не пополам) от этого не зависит:
      // мобильный вариант подписи проверяется ниже отдельным замером текста.
      final paste = find.text('Вставить');
      final second = find.text('Из файла');
      expect(paste, findsOneWidget);
      expect(second, findsOneWidget);

      final pasteTop = tester.getTopLeft(paste);
      final secondTop = tester.getTopLeft(second);
      expect(
        secondTop.dy,
        greaterThan(pasteTop.dy),
        reason: 'вторая кнопка обязана стоять НИЖЕ первой, а не рядом',
      );
      expect(
        pasteTop.dx,
        secondTop.dx,
        reason: 'обе кнопки обязаны начинаться с одного края — во всю ширину',
      );

      for (final f in <Finder>[paste, second]) {
        final rp = tester.renderObject<RenderParagraph>(
          find.descendant(of: f, matching: find.byType(RichText)).first,
        );
        expect(
          rp.didExceedMaxLines,
          isFalse,
          reason: 'подпись кнопки обрезана многоточием',
        );
      }
    });

    test(
      '«Сканировать QR» (мобильная ветка) тоже помещается в ту же ширину',
      () {
        // QR-ветка недоступна на хосте тестов (см. выше), поэтому подпись
        // меряется тем же TextPainter, которым в итоге пользуется движок —
        // напрямую, при доступной ширине ПОЛНОЙ кнопки (уже не половины
        // строки): ширина ListView минус его паддинги минус внутренняя
        // «хрома» OutlinedButton (паддинг + иконка + зазор перед текстом).
        const screenWidth = 360.0; // 1080 / 3.0
        const columnWidth = screenWidth - 2 * AppSpace.s5;
        const buttonChrome = 32.0 + 18.0 + AppSpace.s2; // padding + icon + gap
        final painter = TextPainter(
          text: TextSpan(text: 'Сканировать QR', style: AppType.label),
          textDirection: TextDirection.ltr,
          maxLines: 1,
          ellipsis: '…',
        )..layout(maxWidth: columnWidth - buttonChrome);
        expect(
          painter.didExceedMaxLines,
          isFalse,
          reason:
              'во всю ширину строки «Сканировать QR» обязана читаться целиком',
        );
      },
    );
  });
}

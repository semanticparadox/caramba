// Регрессия на «одно имя — два места назначения».
//
// Дефект уже случился дважды одной формы. В первый раз: вкладка настроек
// стала называться «Улучшения», заголовок самой вкладки — тоже, а строка,
// ведущая туда же, осталась «Маршрут (правила)» — вопреки собственному
// комментарию рядом («Не «Маршрут»»). Во второй раз чинили ИМЕННО первый
// случай: строку переименовали в «Улучшения», чтобы совпадала с вкладкой, —
// но по тапу она как открывала лист «Режим для страны» (showRoutePicker), так
// и открывает: это ОДНА ИЗ ТРЁХ частей вкладки «Улучшения» (там ещё блок
// рекламы и список сайтов), а не вкладка целиком. Строка снова называла место
// назначения не тем, что открывалось.
//
// Оба раза тест, который сравнивал только СТАТИЧНЫЙ текст на двух отдельно
// собранных экранах, дефект не поймал бы: имя строки совпадало со словом на
// вкладке, а то, что тап ведёт не туда, — нет. Поэтому здесь строка не
// читается, а НАЖИМАЕТСЯ: тест проверяет, что имя строки совпадает с
// заголовком того, что ФАКТИЧЕСКИ открывается по тапу.
//
// Пара теперь другая: строка живёт в Настройках (раздел «Правила трафика»),
// а не на «Подключении» — владелец забрал режим в Настройки, потому что
// переключают его редко. Свойство от переезда не изменилось ни на слово:
// строка обязана называться именем листа, который открывает.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/features/settings/route_picker.dart'
    show kRouteModeSheetTitle;
import 'package:caramba_client/features/settings/settings_screen.dart';
import 'package:caramba_client/features/settings/site_rules_screen.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';

import 'support/fake_core.dart';

/// Хост для обоих экранов: ядро подменено, больше ничего не требуется —
/// строка «Режим» рисуется из локальной конфигурации, а не из профиля.
Widget _host(Widget screen) => ProviderScope(
  overrides: <Override>[vpnConnectionProvider.overrideWithValue(FakeVpnCore())],
  child: MaterialApp(
    theme: AppTheme.dark(),
    home: Scaffold(body: screen),
  ),
);

void _phone(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 9000)
    ..devicePixelRatio = 2;
  addTearDown(tester.view.reset);
}

/// Имя РАСТВОРЁННОЙ вкладки настроек. Она держала три независимые вещи разом
/// (блок рекламы, списки сайтов, режим), и владелец разнёс их по одному месту
/// на каждую: реклама и режим — строками раздела «Правила трафика», списки —
/// своим экраном за одной строкой оттуда. Имя осталось здесь ровно как
/// сторож: строка не имеет права назваться вкладкой, которой больше нет.
const _kEnhancementsScreenTitle = 'Улучшения';

/// Заголовок и подпись листа берутся у САМОГО ЛИСТА, а не переписываются сюда
/// литералом. Дефект, который ловит файл, — расхождение двух концов одной пары;
/// собственная копия имени в тесте сделала бы третий конец, который расходится
/// с обоими молча. Владелец переименовал лист («просто переименуй в Режим») —
/// строка обязана была переехать вместе с ним, и проверяется это сравнением с
/// источником.
const _kRouteModeSheetTitle = kRouteModeSheetTitle;

/// Прежнее имя той же пары. Оно не имеет права остаться ни на строке, ни в
/// листе: два имени одного и того же листа на одном экране — это ровно тот
/// дефект, ради которого написан весь файл, только в третьей форме.
const _kRouteModeSheetTitleWas = 'Режим для страны';

/// Подпись листа `showRoutePicker`, которой нет больше нигде на экране —
/// по ней тест отличает «открылся тот самый лист» от простого совпадения
/// заголовков.
const _kRouteModeSheetSubtitle = 'а не страна входа';

void main() {
  testWidgets(
    'строка в Настройках называется тем же именем, что и лист, который открывает по тапу',
    (tester) async {
      _phone(tester);

      // Провайдеры настроек разрешаются асинхронно: до них раздел ещё пуст.
      await tester.pumpWidget(_host(const SettingsScreen()));
      await tester.pump();
      await tester.pump();
      await tester.pump();

      // До тапа: имя строки на экране ровно одно, и это имя листа, который
      // она открывает, — не имя вкладки «Улучшения», частью которой этот
      // лист является.
      expect(
        find.text(_kRouteModeSheetTitle),
        findsOneWidget,
        reason: 'строка Настроек обязана называть то, что откроется по тапу',
      );
      expect(
        find.text(_kEnhancementsScreenTitle),
        findsNothing,
        reason:
            'вкладки «Улучшения» больше нет — назвать её именем нечего и '
            'вести этим именем некуда',
      );
      // Старые половинчатые имена не должны были вернуться ни в каком виде.
      expect(find.text('Маршрут (правила)'), findsNothing);
      expect(find.text('Маршрут'), findsNothing);
      expect(find.text(_kRouteModeSheetTitleWas), findsNothing);

      // Тап — и лист обязан назвать себя тем же именем, что строка обещала.
      await tester.tap(find.text(_kRouteModeSheetTitle));
      await tester.pumpAndSettle();

      expect(
        find.text(_kRouteModeSheetTitle),
        findsNWidgets(2),
        reason:
            'на экране обязаны быть ровно два этих имени — строка Настроек '
            'под листом и заголовок самого листа; одно означает, что лист '
            'назвался иначе, чем обещала строка',
      );
      expect(
        find.textContaining(_kRouteModeSheetSubtitle),
        findsOneWidget,
        reason:
            'открылся не тот лист: подписи showRoutePicker на экране нет '
            '— значит тап привёл куда-то ещё',
      );

      // Закрываем лист: незакрытая модалка переживёт тест и уронит следующий.
      await tester.tapAt(const Offset(20, 20));
      await tester.pumpAndSettle();
    },
  );

  // Второй конец того же свойства: у величины ровно одно место. «Правила по
  // сайтам» — соседний экран из того же раздела, и строка «Режим» на нём была
  // бы вторым адресом для одного вопроса. AppliedRouteCard там строку «Режим»
  // не рисует: без отчёта ядра и без поднятого туннеля карточки нет вовсе.
  testWidgets('на «Правилах по сайтам» строки «Режим» нет', (tester) async {
    _phone(tester);

    await tester.pumpWidget(_host(const SiteRulesScreen()));
    await tester.pump();
    await tester.pump();

    expect(
      find.text(_kRouteModeSheetTitle),
      findsNothing,
      reason: 'режим снова получил второе место',
    );
  });
}

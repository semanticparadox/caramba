// Регрессия на «одно имя — два места назначения».
//
// Дефект уже случился дважды одной формы. В первый раз: вкладка настроек
// стала называться «Улучшения», заголовок самой вкладки — тоже, а строка на
// Home, ведущая туда же, осталась «Маршрут (правила)» — вопреки собственному
// комментарию рядом («Не «Маршрут»»). Во второй раз чинили ИМЕННО первый
// случай: строку на Home переименовали в «Улучшения», чтобы совпадала с
// вкладкой, — но по тапу она как открывала лист «Режим для страны»
// (showRoutePicker), так и открывает: это ОДНА ИЗ ТРЁХ частей вкладки
// «Улучшения» (там ещё блок рекламы и список сайтов), а не вкладка целиком.
// Строка снова называла место назначения не тем, что открывалось.
//
// Оба раза тест, который сравнивал только СТАТИЧНЫЙ текст на двух отдельно
// собранных экранах, дефект не поймал бы: имя на Home совпадало со словом на
// вкладке, а то, что тап ведёт не туда, — нет. Поэтому здесь строка не
// читается, а НАЖИМАЕТСЯ: тест проверяет, что имя строки на Home совпадает с
// заголовком того, что ФАКТИЧЕСКИ открывается по тапу, а не с именем вкладки,
// про часть которой эта строка на самом деле является.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/features/settings/route_picker.dart'
    show kRouteModeSheetTitle;
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart' show ImportedServer;

import 'support/fake_core.dart';

/// Профили из памяти: secure storage в тесте не поднимаем.
class _FakeProfilesStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles;
  String? activeId;

  _FakeProfilesStore(this.profiles, this.activeId);

  @override
  Future<List<ConnectionProfile>> readProfiles() async => profiles;

  @override
  Future<String?> readActiveId() async => activeId;

  @override
  Future<void> writeProfiles(List<ConnectionProfile> next) async {
    profiles = next;
  }

  @override
  Future<void> writeActiveId(String? id) async {
    activeId = id;
  }

  @override
  Future<void> clear() async {
    profiles = const [];
    activeId = null;
  }
}

final _profile = ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Моя подписка',
  source: 'https://sub.example/a',
  rawConfig: 'proxies: []',
  format: 'clash',
  servers: const <ImportedServer>[
    ImportedServer(
      id: 'nl-1',
      name: 'Amsterdam #1',
      type: 'vless',
      server: 'a.example',
      port: 443,
      country: 'NL',
    ),
  ],
  selectedServerId: 'nl-1',
  serversUpdatedMs: DateTime.now().millisecondsSinceEpoch,
);

Widget _guestHome() => ProviderScope(
  overrides: <Override>[
    vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
    connectionProfilesStoreProvider.overrideWithValue(
      _FakeProfilesStore(<ConnectionProfile>[_profile], _profile.id),
    ),
  ],
  child: MaterialApp(theme: AppTheme.dark(), home: const HomeScreen()),
);

void _phone(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 9000)
    ..devicePixelRatio = 2;
  addTearDown(tester.view.reset);
}

/// Имя РАСТВОРЁННОЙ вкладки настроек. Она держала три независимые вещи разом
/// (блок рекламы, списки сайтов, режим), и владелец разнёс их по одному месту
/// на каждую: реклама и списки — в Настройки, режим — на Главную. Имя
/// осталось здесь ровно как сторож: строка на Home не имеет права назваться
/// вкладкой, которой больше нет.
const _kEnhancementsScreenTitle = 'Улучшения';

/// Заголовок и подпись листа берутся у САМОГО ЛИСТА, а не переписываются сюда
/// литералом. Дефект, который ловит файл, — расхождение двух концов одной пары;
/// собственная копия имени в тесте сделала бы третий конец, который расходится
/// с обоими молча. Владелец переименовал лист («просто переименуй в Режим») —
/// строка на Home обязана была переехать вместе с ним, и проверяется это
/// сравнением с источником.
const _kRouteModeSheetTitle = kRouteModeSheetTitle;

/// Прежнее имя той же пары. Оно не имеет права остаться ни на Home, ни в
/// листе: два имени одного и того же листа на одном экране — это ровно тот
/// дефект, ради которого написан весь файл, только в третьей форме.
const _kRouteModeSheetTitleWas = 'Режим для страны';

/// Подпись листа `showRoutePicker`, которой нет больше нигде на экране —
/// по ней тест отличает «открылся тот самый лист» от простого совпадения
/// заголовков.
const _kRouteModeSheetSubtitle = 'а не страна входа';

void main() {
  testWidgets(
    'строка на Home называется тем же именем, что и лист, который открывает по тапу',
    (tester) async {
      _phone(tester);

      // Профили читаются асинхронно: до них экран ещё в панельной ветке.
      await tester.pumpWidget(_guestHome());
      await tester.pump();
      await tester.pump();
      await tester.pump();

      // До тапа: имя строки на экране ровно одно, и это имя листа, который
      // она открывает, — не имя вкладки «Улучшения», частью которой этот
      // лист является.
      expect(
        find.text(_kRouteModeSheetTitle),
        findsOneWidget,
        reason: 'строка Home обязана называть то, что откроется по тапу',
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
            'на экране обязаны быть ровно два этих имени — строка Home под '
            'листом и заголовок самого листа; одно означает, что лист '
            'назвался иначе, чем обещала строка',
      );
      expect(
        find.textContaining(_kRouteModeSheetSubtitle),
        findsOneWidget,
        reason:
            'открылся не тот лист: подписи showRoutePicker на экране нет '
            '— значит тап привёл куда-то ещё',
      );

      // Закрываем лист и гасим односекундный таймер сессии вместе с деревом
      // — иначе он переживёт тест и уронит следующий.
      await tester.tapAt(const Offset(20, 20));
      await tester.pumpAndSettle();
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  // Вторая проверка этого файла сравнивала строку Настроек «Улучшения» с
  // заголовком одноимённой вкладки. Пары больше нет: вкладка растворена, а
  // строка из Настроек убрана — сравнивать нечего, и половина, оставшаяся от
  // такой проверки, ловила бы только сама себя. Свойство, ради которого
  // вкладку и разобрали («реклама включается ровно в одном месте»),
  // проверяется в ads_and_site_rules_test.dart, где есть ОБА конца: есть в
  // Настройках и нет на «Правилах по сайтам».
}

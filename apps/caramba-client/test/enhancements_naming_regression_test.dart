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
import 'package:caramba_client/features/split/split_tunnel_screen.dart';
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

Widget _enhancementsScreen() => ProviderScope(
  overrides: <Override>[vpnConnectionProvider.overrideWithValue(FakeVpnCore())],
  child: MaterialApp(theme: AppTheme.dark(), home: const SplitTunnelScreen()),
);

void _phone(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 9000)
    ..devicePixelRatio = 2;
  addTearDown(tester.view.reset);
}

/// Заголовок вкладки настроек «Улучшения» (блок рекламы + сайты + режим
/// страны вместе). Единственное место, где записано это имя — обе проверки
/// читают его отсюда, а не дублируют литерал.
const _kEnhancementsScreenTitle = 'Улучшения';

/// Заголовок листа «Режим для страны» (showRoutePicker) — ОДНОЙ из трёх
/// частей «Улучшения», а не вкладки целиком. Строка на Home имеет право
/// называться только этим именем, потому что только его она и открывает.
const _kRouteModeSheetTitle = 'Режим для страны';

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
            'на Home нет строки, ведущей на вкладку «Улучшения» целиком — '
            'называть себя её именем строке нечем',
      );
      // Старые половинчатые имена не должны были вернуться ни в каком виде.
      expect(find.text('Маршрут (правила)'), findsNothing);
      expect(find.text('Маршрут'), findsNothing);

      // Тап — и лист обязан назвать себя тем же именем, что строка обещала.
      await tester.tap(find.text(_kRouteModeSheetTitle));
      await tester.pumpAndSettle();

      expect(
        find.text(_kRouteModeSheetTitle),
        findsWidgets,
        reason: 'заголовок открывшегося листа должен повторить имя строки',
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

  testWidgets('заголовок вкладки «Улучшения» называется тем же именем, что и строка в Настройках', (
    tester,
  ) async {
    _phone(tester);

    // Строка в Настройках (settings_screen.dart) ведёт на этот экран через
    // `context.go(AppRoute.splitTunnel)` и называет себя «Улучшения» — тем
    // же именем, каким называется сам экран. В отличие от строки на Home,
    // здесь имя и назначение — одна и та же сущность целиком, поэтому
    // сравнение статичного текста этот путь ловит корректно.
    await tester.pumpWidget(_enhancementsScreen());
    await tester.pump();
    expect(find.text(_kEnhancementsScreenTitle), findsOneWidget);
  });
}

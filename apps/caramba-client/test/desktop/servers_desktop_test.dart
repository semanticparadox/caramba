// Десктопные «Серверы»: таблица машин вместо списка карточек.
//
// Проверяется ровно то, что таблица обязана унаследовать у мобильного списка и
// не имеет права переизобрести:
//   * ПОРЯДОК строк (доступные раньше недоступных, внутри — по задержке) — он
//     здесь единственная подсказка, что выбирать, и разойдись он с телефоном,
//     один и тот же флот читался бы на двух платформах по-разному;
//   * недоступная машина ОСТАЁТСЯ видимой, приглушённой и названной причиной:
//     пропавшая строка неотличима от «такой машины у оператора нет»;
//   * клик закрепляет выбор и говорит об этом.
// Плюс то, что у таблицы своё: шапка колонок.
//
// Платформа переопределяется на macOS и возвращается в tearDown: без возврата
// следующий тест файла поехал бы по десктопной ветке, ничего об этом не зная.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_builder.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/servers/exit_node_list.dart';
import 'package:caramba_client/features/servers/exit_node_table.dart';
import 'package:caramba_client/features/servers/fleet_alignment.dart';
import 'package:caramba_client/features/servers/servers_desktop.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

import '../support/fake_core.dart';

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

// ─── фикстура флота ────────────────────────────────────────────────────────
//
// Задержки нарочно расходятся с порядком объявления: сортировка обязана быть
// видна, а не совпасть с порядком фикстуры случайно. У Франкфурта числа нет
// вовсе — «не мерили» это не «быстрее всех», и он уходит ВНИЗ доступных.

const _servers = <ImportedServer>[
  ImportedServer(
    id: 'p-se',
    name: 'Стокгольм',
    type: 'vless',
    server: 'se.example',
    port: 443,
    country: 'SE',
  ),
  ImportedServer(
    id: 'p-nl',
    name: 'Амстердам',
    type: 'vless',
    server: 'nl.example',
    port: 443,
    country: 'NL',
  ),
  ImportedServer(
    id: 'p-de',
    name: 'Франкфурт',
    type: 'hysteria2',
    server: 'de.example',
    port: 443,
    country: 'DE',
  ),
];

/// Узлы инвентаря под те же прокси. Ключ узла — имя прокси в теле: именно им
/// `connectRaw` закрепляет выбор, и по нему же машина предложения находит свой
/// узел.
const _nodes = <ExitNode>[
  ExitNode(
    key: 'p-se',
    name: 'Стокгольм',
    countryCode: 'SE',
    source: ExitInventorySource.importedSub,
    measuredMs: 120,
    protocol: 'vless',
  ),
  ExitNode(
    key: 'p-nl',
    name: 'Амстердам',
    countryCode: 'NL',
    source: ExitInventorySource.importedSub,
    measuredMs: 42,
    protocol: 'vless',
  ),
  ExitNode(
    key: 'p-de',
    name: 'Франкфурт',
    countryCode: 'DE',
    source: ExitInventorySource.importedSub,
    protocol: 'hysteria2',
  ),
  // Машины в списке выбора нет и быть не может: узел выключен оператором.
  ExitNode(
    key: 'p-ru',
    name: 'Москва',
    countryCode: 'RU',
    source: ExitInventorySource.importedSub,
    measuredMs: 8,
    protocol: 'vless',
    availability: ExitAvailability.unavailable(
      ExitUnavailableReason.nodeOffline,
    ),
  ),
];

/// Инвентарь с локациями: экран считает себя пустым по [ExitInventory.isEmpty],
/// а он смотрит на страны, а не на узлы.
ExitInventory _inventory({List<ExitNode> nodes = _nodes}) {
  final byCountry = <String, List<ExitNode>>{};
  for (final n in nodes) {
    byCountry.putIfAbsent(n.countryCode, () => <ExitNode>[]).add(n);
  }
  return ExitInventory(
    source: ExitInventorySource.importedSub,
    nodes: nodes,
    locations: byCountry.entries
        .map(
          (e) => ExitLocation.fromNodes(
            e.key,
            e.value,
            source: ExitInventorySource.importedSub,
          ),
        )
        .toList(growable: false),
  );
}

/// Предложение из того же тела: обе половины флота обязаны описывать ОДИН
/// источник, иначе таблица уйдёт на узлы инвентаря и строки предложения не
/// проверялись бы вовсе.
Offering _offering() => buildImportedOffering(servers: _servers);

ConnectionProfile _profile() => ConnectionProfile(
      id: 'cp_1',
      type: ProfileType.rawSub,
      displayName: 'Моя подписка',
      source: 'https://sub.example/a',
      rawConfig: 'proxies: []',
      format: 'clash',
      servers: _servers,
      serversUpdatedMs: DateTime.now().millisecondsSinceEpoch,
    );

Widget _app(
  Widget child, {
  ExitInventory? inventory,
  Offering? offering,
  _FakeProfilesStore? store,
}) =>
    ProviderScope(
      overrides: [
        vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
        connectionProfilesStoreProvider.overrideWithValue(
          store ?? _FakeProfilesStore(<ConnectionProfile>[_profile()], 'cp_1'),
        ),
        // Инвентарь и предложение подставляются целиком: тест проверяет ТАБЛИЦУ, а
        // не сборку инвентаря из тела подписки — у неё свои тесты.
        exitInventoryProvider.overrideWithValue(inventory ?? _inventory()),
        // По умолчанию предложение ПУСТО, и это не лень фикстуры: половины флота
        // разошлись (у предложения источника нет), и таблица обязана встать на
        // узлы инвентаря — путь, на котором только и видна недоступная машина.
        // Строки предложения проверяет отдельная группа ниже.
        offeringProvider.overrideWithValue(offering ?? buildEmptyOffering()),
        autoServerLabelProvider.overrideWithValue(AutoLabel.unknown),
      ],
      child: MaterialApp(theme: AppTheme.dark(), home: child),
    );

/// Тест на ДЕСКТОПНОЙ платформе: macOS и окно 1280×800 из карты окна.
///
/// Переопределение снимается внутри тела теста, а не в tearDown: проверку
/// «отладочные переменные foundation возвращены на место» flutter_test делает
/// сразу по выходу из тела, ДО всех tearDown, и оставленная платформа роняет
/// тест раньше, чем tearDown до неё доберётся. Групповой tearDown ниже остаётся
/// страховкой на случай падения посреди тела.
void _desktopTest(
  String description,
  Future<void> Function(WidgetTester) body,
) {
  testWidgets(description, (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    tester.view
      ..physicalSize = const Size(1280, 800)
      ..devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    try {
      await body(tester);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}

/// Профили читаются асинхронно: до них экран ещё «без профиля».
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump();
  }
}

/// Заголовки в порядке СВЕРХУ ВНИЗ — то, что человек и читает.
List<String> _rowOrder(WidgetTester tester, List<String> titles) {
  final placed = <(String, double)>[
    for (final t in titles) (t, tester.getTopLeft(find.text(t)).dy),
  ];
  placed.sort((a, b) => a.$2.compareTo(b.$2));
  return <String>[for (final p in placed) p.$1];
}

void main() {
  tearDown(() => debugDefaultTargetPlatformOverride = null);

  group('таблица выходов', () {
    _desktopTest(
        'порядок строк: доступные раньше недоступных, внутри — по '
        'задержке', (tester) async {
      await tester.pumpWidget(_app(const ServersDesktopScreen()));
      await _settle(tester);

      expect(
        _rowOrder(tester, <String>[
          'Амстердам',
          'Стокгольм',
          'Франкфурт',
          'Москва',
        ]),
        <String>[
          // 42 мс
          'Амстердам',
          // 120 мс
          'Стокгольм',
          // числа нет — вниз доступных, а не наверх
          'Франкфурт',
          // 8 мс, и это неважно: недоступная машина всегда последняя
          'Москва',
        ],
      );
    });

    _desktopTest('720 px panel keeps action labels and machine names readable',
        (tester) async {
      await tester.pumpWidget(
        _app(
          const Align(
            alignment: Alignment.centerRight,
            child: SizedBox(width: 720, child: ServersDesktopScreen()),
          ),
        ),
      );
      await _settle(tester);
      for (final label in [
        'Замерить свой пинг',
        'Подобрать лучший узел',
        'Амстердам',
      ]) {
        final paragraph = tester.renderObject<RenderParagraph>(
          find.descendant(
            of: find.text(label),
            matching: find.byType(RichText),
          ),
        );
        expect(paragraph.didExceedMaxLines, isFalse, reason: label);
      }
      expect(tester.takeException(), isNull);
    });

    _desktopTest('шапка колонок на месте', (tester) async {
      await tester.pumpWidget(_app(const ServersDesktopScreen()));
      await _settle(tester);

      for (final column in <String>[
        'КОД',
        'МАШИНА',
        'ТИП',
        'ЗАДЕРЖКА',
        'ЗАГРУЗКА',
      ]) {
        expect(find.text(column), findsOneWidget, reason: column);
      }
    });

    _desktopTest('недоступная машина видна, приглушена и названа причиной', (
      tester,
    ) async {
      await tester.pumpWidget(_app(const ServersDesktopScreen()));
      await _settle(tester);

      expect(find.text('Москва'), findsOneWidget);
      final opacities = tester.widgetList<Opacity>(
        find.ancestor(of: find.text('Москва'), matching: find.byType(Opacity)),
      );
      expect(
        opacities.any((o) => o.opacity == 0.45),
        isTrue,
        reason: 'строка «Москва» обязана быть приглушённой',
      );
      // Причина стоит В строке, а не только в подсказке: строка, у которой
      // отняли нажатие и не сказали почему, читается как поломка приложения.
      expect(find.textContaining('Узел не в сети'), findsWidgets);
    });

    _desktopTest('клик по строке закрепляет узел и говорит об этом', (
      tester,
    ) async {
      final store = _FakeProfilesStore(<ConnectionProfile>[_profile()], 'cp_1');
      await tester.pumpWidget(_app(const ServersDesktopScreen(), store: store));
      await _settle(tester);

      await tester.tap(find.text('Амстердам'));
      // Одиночный клик разрешается только когда истечёт окно двойного: строка
      // умеет оба, и без этой паузы `onTap` не сработал бы вовсе.
      await tester.pump(const Duration(milliseconds: 400));
      for (var i = 0; i < 6; i++) {
        await tester.pump(const Duration(milliseconds: 100));
      }

      expect(store.profiles.single.selectedServerId, 'p-nl');
      expect(find.text('Амстердам выбран'), findsOneWidget);
    });

    _desktopTest('пустой инвентарь не рисует таблицу', (tester) async {
      await tester.pumpWidget(
        _app(
          const ServersDesktopScreen(),
          inventory: _inventory(nodes: const <ExitNode>[]),
          offering: buildImportedOffering(servers: const <ImportedServer>[]),
        ),
      );
      await _settle(tester);

      expect(find.byType(ExitNodeTable), findsNothing);
      expect(find.text('Узлов в подписке нет'), findsOneWidget);
    });
  });

  group('порядок один на обе платформы', () {
    _desktopTest(
        'sortedExits публичная и даёт таблице тот же порядок, что и '
        'мобильному списку', (tester) async {
      // Москвы здесь нет намеренно: её узел в теле подписки не представлен,
      // и строкой предложения она не становится ни на одной платформе.
      const titles = <String>['Амстердам', 'Стокгольм', 'Франкфурт'];

      // Чистая функция: она и есть контракт, на который опираются оба виджета.
      final byFunction = <String>[
        for (final e in sortedExits(_offering().exits, _nodes))
          machineTitleOf(e),
      ];

      await tester.pumpWidget(
        _app(
          Scaffold(body: ExitNodeList(onSelect: (_) {})),
          offering: _offering(),
        ),
      );
      await _settle(tester);
      final listOrder = _rowOrder(tester, titles);

      await tester.pumpWidget(
        _app(
          Scaffold(
            body: ExitNodeTable(onSelect: (_) {}, onActivate: (_) {}),
          ),
          offering: _offering(),
        ),
      );
      await _settle(tester);
      final tableOrder = _rowOrder(tester, titles);

      expect(tableOrder, listOrder);
      expect(tableOrder, byFunction);
    });
  });
}

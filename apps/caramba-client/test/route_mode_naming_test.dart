// Лист «Режим»: имена, порядок и флаги.
//
// Владелец сказал прямо: «Режим для страны ты сделал совсем не то, что я
// думал». Он читал имя листа как четвёртый подряд выбор страны — после
// «Сервер», «Relay (вход)» и «Тип подключения», — и был прав: страна в имени
// пресета это страна СПИСКА БЛОКИРОВОК, а не страна узла. Правка сделана
// именами, порядком и флагами; механика не тронута.
//
// Здесь проверяется ровно то, что от такой правки ломается молча:
//   * порядок ПОКАЗА (владелец просил «Полный обход» первым) — он отвязан от
//     порядка ХРАНЕНИЯ, и связать их обратно легче всего случайно;
//   * состав: строка, выпавшая из таблицы показа, просто исчезает с экрана —
//     ни ошибки, ни следа;
//   * описания: имена мы придумываем, а описания обязаны совпадать с реестром
//     ядра посимвольно — это единственное утверждение экрана о том, что
//     пресет делает с трафиком;
//   * одно имя на одну величину: пикер, строка «Что применилось» и карточка
//     CSM называют пресет тремя разными местами кода.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/features/settings/route_picker.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_policy_mapping.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Идентификаторы режимов в том порядке, в каком владелец просил их видеть.
/// Литерал здесь намеренный: тест обязан ловить перестановку, а не повторять
/// её за исходником.
const _expectedOrder = <String>[
  'global',
  'ru-smart',
  'full',
  'by-smart',
  'ir-smart',
  'cn-smart',
  'telegram-only',
  'streaming',
];

/// Индекс режима в списке ХРАНЕНИЯ (`CoreConfig.route`).
int _storageIndex(String id) =>
    RoutingMode.defaults.indexWhere((m) => m.id == id);

List<String> _displayedIds({int selected = 0}) => routeDisplayIndexes(
  modes: RoutingMode.defaults,
  selected: selected,
).map((i) => RoutingMode.defaults[i].id).toList();

/// Все пресеты предложены и подтверждены: тест про имена и порядок, а не про
/// причины отказа.
List<RoutePresetOffer> _allOffered() => <RoutePresetOffer>[
  for (final p in kCoreRoutePresets)
    RoutePresetOffer(
      preset: p,
      legacyIndex: kLegacyRouteIndexByCoreId[p.id],
      availability: const Availability.available(CoreRoutePreset.origin),
    ),
];

Widget _sheetHost(WidgetRef Function(WidgetRef) capture, {int route = 0}) =>
    ProviderScope(
      overrides: <Override>[
        coreConfigProvider.overrideWith(
          (ref) => CoreConfigNotifier()..state = CoreConfig(route: route),
        ),
        routePresetOffersProvider.overrideWithValue(_allOffered()),
        csmDisabledRoutePresetsProvider.overrideWithValue(
          const <int, String>{},
        ),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(),
        home: Consumer(
          builder: (context, ref, __) {
            capture(ref);
            return Scaffold(
              body: Builder(
                builder: (inner) => TextButton(
                  onPressed: () => showRoutePicker(inner, ref),
                  child: const Text('открыть'),
                ),
              ),
            );
          },
        ),
      ),
    );

void main() {
  group('порядок и состав листа', () {
    test('первым стоит «Полный обход», дальше — порядок владельца', () {
      expect(_displayedIds(), _expectedOrder);
      expect(
        RoutingMode.defaults[_storageIndex('global')].name,
        'Полный обход',
      );
    });

    // Порядок показа отвязан от порядка хранения намеренно: `CoreConfig.route`
    // это сохранённый ИНДЕКС, и перестановка самого списка увела бы живого
    // пользователя на соседний маршрут. Тест фиксирует, что отвязка реальна, —
    // иначе «переставили показ» однажды починят перестановкой хранения.
    test('порядок ПОКАЗА не совпал с порядком ХРАНЕНИЯ', () {
      final storage = RoutingMode.defaults.map((m) => m.id).toList();
      expect(_displayedIds(), isNot(storage));
      expect(storage.first, 'ru-smart', reason: 'хранение трогать нельзя');
      expect(
        RoutingMode.defaults.length,
        kCoreRoutePresets.length,
        reason: 'состав списка хранения обязан совпадать с реестром ядра',
      );
    });

    // Строка, выпавшая из таблицы показа, просто исчезает с экрана: ни
    // исключения, ни пустого места — режима для человека больше нет.
    test('в таблице показа перечислены все режимы хранения, и только они', () {
      final display = kRouteDisplayOrder.toSet();
      final storage = RoutingMode.defaults.map((m) => m.id).toSet();
      expect(display.difference(storage), isEmpty, reason: 'выдуманный режим');
      expect(storage.difference(display), isEmpty, reason: 'режим потерян');
      expect(
        kRouteDisplayOrder.length,
        kRouteDisplayOrder.toSet().length,
        reason: 'режим перечислен дважды — он и покажется дважды',
      );
    });

    test('«Только блок рекламы» скрыт, пока не выбран', () {
      expect(_displayedIds(), isNot(contains(kAdBlockRouteId)));
      // Отнять у человека его текущий выбор нельзя: если он уже на нём стоит,
      // строка обязана быть на экране.
      final selected = _storageIndex(kAdBlockRouteId);
      expect(
        _displayedIds(selected: selected),
        contains(kAdBlockRouteId),
        reason: 'выбранный режим исчез с экрана — выбор нечем найти',
      );
    });
  });

  group('имена', () {
    test('страновые режимы названы по-человечески, без скобок реестра', () {
      String nameOf(String id) => RoutingMode.defaults[_storageIndex(id)].name;
      expect(nameOf('ru-smart'), 'Российский режим');
      expect(nameOf('full'), 'Российский полный обход');
      expect(nameOf('by-smart'), 'Белорусский режим');
      expect(nameOf('ir-smart'), 'Иранский режим');
      expect(nameOf('cn-smart'), 'Китайский режим');
      for (final m in RoutingMode.defaults) {
        expect(
          m.name,
          isNot(contains('(')),
          reason: 'скобка реестра «${m.name}» вернулась в UI',
        );
      }
    });

    // Два режима с одним именем — это выбор, который человек не может сделать
    // осознанно.
    test('имена уникальны', () {
      final names = RoutingMode.defaults.map((m) => m.name).toList();
      expect(names.toSet().length, names.length);
    });

    // Имя обязано не врать о том, что пресет делает: «Российский режим» это
    // УМНЫЙ пресет (по умолчанию напрямую), а «Российский полный обход» —
    // наоборот. Проверяем не слова имени, а то, что за ними стоят РАЗНЫЕ
    // описания реестра, и каждое говорит своё.
    test('два российских режима отличаются не только именем', () {
      final smart = RoutingMode.defaults[_storageIndex('ru-smart')];
      final full = RoutingMode.defaults[_storageIndex('full')];
      expect(smart.desc, isNot(full.desc));
      expect(smart.desc, contains('По умолчанию напрямую'));
      expect(full.desc, contains('Весь трафик через VPN'));
    });

    // Величина одна, мест, где её называют, три: лист выбора, карточка «Что
    // применилось» и карточка «Оставить или Вернуть» (CSM). Разъехавшись, они
    // рассказывают человеку про три разные настройки.
    test('лист, CSM-карточка и заголовок зовут пресет одним словом', () {
      expect(csmSettingTitle(CsmSettingKey.preset), kRouteModeSheetTitle);
      expect(kRouteModeSheetTitle, 'Режим');
    });
  });

  group('описания', () {
    // Имена мы придумываем, описания — нет. Это единственное утверждение
    // экрана о том, что пресет делает с трафиком, и расхождение с реестром
    // ядра — обещание, которого туннель не исполнит. Сверяется ID и ОПИСАНИЕ,
    // а не имя: имя UI и имя реестра разошлись намеренно (см. RoutingMode).
    test('описание каждого режима совпадает с реестром ядра посимвольно', () {
      for (final m in RoutingMode.defaults) {
        final preset = routePresetForMode(m.id);
        expect(preset, isNotNull, reason: 'режим ${m.id} не найден в реестре');
        expect(
          m.desc,
          preset!.description,
          reason: 'описание режима ${m.id} разошлось с ядром',
        );
      }
    });

    // Переименование `full` → `ru-full` живёт в одной карте; забыв её,
    // страновой пресет остался бы без флага и без описания.
    test('UI-идентификатор `full` доезжает до реестрового `ru-full`', () {
      expect(kRoutingPresetWire['full'], 'ru-full');
      expect(routePresetForMode('full')?.id, 'ru-full');
      expect(routePresetForMode('full')?.countryCode, 'RU');
    });
  });

  group('флаги', () {
    test('флаг есть ровно у страновых режимов', () {
      final withFlag = <String>[
        for (final m in RoutingMode.defaults)
          if ((routePresetForMode(m.id)?.countryCode ?? '').isNotEmpty) m.id,
      ];
      expect(withFlag, <String>[
        'ru-smart',
        'full',
        'ir-smart',
        'by-smart',
        'cn-smart',
      ]);
    });

    testWidgets('лист несёт заголовок «Режим», порядок и флаги стран', (
      tester,
    ) async {
      tester.view
        ..physicalSize = const Size(1080, 4200)
        ..devicePixelRatio = 2;
      addTearDown(tester.view.reset);

      await tester.pumpWidget(_sheetHost((ref) => ref));
      await tester.pump();
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();

      expect(find.text(kRouteModeSheetTitle), findsOneWidget);
      expect(find.textContaining('а не страна входа'), findsOneWidget);

      // Порядок на экране: сравниваем по вертикали, а не по индексу в дереве.
      final shown = <String, double>{};
      for (final id in _expectedOrder) {
        final name = RoutingMode.defaults[_storageIndex(id)].name;
        final finder = find.text(name);
        expect(finder, findsOneWidget, reason: 'режим $name не показан');
        shown[id] = tester.getTopLeft(finder).dy;
      }
      final byPosition = shown.keys.toList()
        ..sort((a, b) => shown[a]!.compareTo(shown[b]!));
      expect(byPosition, _expectedOrder);

      // «Только блок рекламы» не выбран — и его на экране нет.
      expect(
        find.text(RoutingMode.defaults[_storageIndex(kAdBlockRouteId)].name),
        findsNothing,
      );

      // Флаги: пять страновых строк, три кода стран (у двух российских
      // режимов он общий).
      expect(find.byType(FlagChip), findsNWidgets(5));
      final codes = tester
          .widgetList<FlagChip>(find.byType(FlagChip))
          .map((f) => f.code)
          .toList();
      expect(codes, <String>['RU', 'RU', 'BY', 'IR', 'CN']);
    });

    testWidgets('выбранный «Только блок рекламы» показан выключенным', (
      tester,
    ) async {
      tester.view
        ..physicalSize = const Size(1080, 4200)
        ..devicePixelRatio = 2;
      addTearDown(tester.view.reset);

      await tester.pumpWidget(
        _sheetHost((ref) => ref, route: _storageIndex(kAdBlockRouteId)),
      );
      await tester.pump();
      await tester.tap(find.text('открыть'));
      await tester.pumpAndSettle();

      final name = RoutingMode.defaults[_storageIndex(kAdBlockRouteId)].name;
      expect(find.text(name), findsOneWidget);
      // Вместо описания режима строка объясняет, где теперь этот выключатель.
      expect(find.textContaining('отдельный переключатель'), findsOneWidget);

      final card = tester.widget<ListItemCard>(
        find.ancestor(of: find.text(name), matching: find.byType(ListItemCard)),
      );
      expect(card.onTap, isNull, reason: 'строка обязана быть невыбираемой');
      expect(
        card.selected,
        isTrue,
        reason: 'без галочки человек не найдёт, что у него сейчас включено',
      );
    });

    // `ListItemCard` кладёт `leading` в `Row` внутри `IntrinsicHeight`: высота
    // строки подстраивается под самый высокий элемент, и им может оказаться
    // многострочное описание. `FlagChip` — `Container` с `alignment: center`
    // без своих width/height, а такой контейнер при loose-но-ограниченной
    // сверху высоте разворачивается во весь этот размер вместо своего
    // естественного — плашка превращалась в узкий «столбик» на всю строку.
    testWidgets(
      'плашка с флагом не растягивается по высоте длинного описания',
      (tester) async {
        tester.view
          ..physicalSize = const Size(1080, 4200)
          ..devicePixelRatio = 2;
        addTearDown(tester.view.reset);

        await tester.pumpWidget(_sheetHost((ref) => ref));
        await tester.pump();
        await tester.tap(find.text('открыть'));
        await tester.pumpAndSettle();

        Size flagSizeFor(String modeId) {
          final name = RoutingMode.defaults[_storageIndex(modeId)].name;
          final row = find.ancestor(
            of: find.text(name),
            matching: find.byType(ListItemCard),
          );
          final chip = find.descendant(
            of: row,
            matching: find.byType(FlagChip),
          );
          return tester.getSize(chip);
        }

        // «Российский режим» несёт самое длинное описание реестра (несколько
        // строк на этой ширине), у «Белорусского» — короткое, в одну строку.
        // Плашка со своим естественным размером даёт одинаковую высоту в обоих
        // случаях; растянутая — расползлась бы вместе с длинным описанием.
        final ru = flagSizeFor('ru-smart');
        final by = flagSizeFor('by-smart');
        expect(ru.height, closeTo(by.height, 0.5));
        expect(
          ru.height,
          lessThan(40),
          reason: 'плашка растянулась на всю высоту строки',
        );
      },
    );
  });
}

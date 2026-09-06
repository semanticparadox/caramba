// Вкладка «Улучшения»: блок рекламы, список сайтов, режим для страны.
//
// Проверяется ровно то, что легко сломать молча: подпись, которая говорит
// «работает» без подтверждения ядра, галочка, за которой нет правила, и
// переживание настроек между запусками. Ни один тест здесь не смотрит на
// картинку ради картинки — каждый закрывает случай, в котором пользователь
// увидел бы включённым то, чего нет.

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/features/settings/applied_route_card.dart';
import 'package:caramba_client/features/settings/enhancements_summary.dart';
import 'package:caramba_client/features/settings/route_report.dart';
import 'package:caramba_client/features/settings/settings_screen.dart';
import 'package:caramba_client/features/split/split_tunnel_screen.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';

import 'support/fake_core.dart';

// ------------------------------------------------------------- фикстуры

/// Отчёт ядра о поднятом туннеле, где список `ads` пришёл проверенным файлом.
const _reportAdsFromFile = '''
{"known":true,"tunnel_up":true,"source":"preset","rules":12,
 "preset":{"preset_id":"ru-smart","preset_name":"Россия (умный)","emoji":"🇷🇺",
   "country":"RU","rules":12,"dropped_rules":0,
   "sources":[{"name":"ads","state":"file","rules":148000,"kept_rules":148000}]},
 "geosite":{"required":true,"state":"verified"},
 "relay":{"state":"not_requested","dialer_proxy_seen":false}}
''';

/// Тот же подъём, но список выброшен и встроенная база GEOSITE тоже мертва.
const _reportAdsDeadEverywhere = '''
{"known":true,"tunnel_up":true,"source":"preset","rules":3,
 "preset":{"preset_id":"ru-smart","preset_name":"Россия (умный)","emoji":"🇷🇺",
   "country":"RU","rules":3,"dropped_rules":1,
   "sources":[{"name":"ads","state":"dropped","reason":"no_mirror","rules":1}]},
 "geosite":{"required":true,"state":"refused"},
 "relay":{"state":"not_requested","dialer_proxy_seen":false}}
''';

/// Подъёма не было.
const _reportNotRaised = '{"known":false,"reason":"not_raised"}';

/// Пресет `global`, который не режет рекламу и не уводит стриминг в туннель
/// (оба флага реестра по умолчанию `false`) — и не называет список `ads`
/// поимённо. Даёт для ОБЕИХ строк карточки один и тот же длинный ответ
/// «этот пресет его не включает», на котором ловилась обрезка.
const _reportNeitherCapability = '''
{"known":true,"tunnel_up":true,"source":"preset","rules":40,
 "preset":{"preset_id":"global","preset_name":"Полный обход","emoji":"🌐",
   "country":"","rules":40,"dropped_rules":0,"sources":[]},
 "geosite":{"required":false,"state":"not_required"},
 "relay":{"state":"not_requested","dialer_proxy_seen":false}}
''';

AppliedRoute _route(String raw) => AppliedRoute.parse(raw);

Widget _wrap(
  Widget screen, {
  String report = _reportNotRaised,
  List<Override> overrides = const <Override>[],
}) {
  final core = FakeVpnCore()..routeReportJson = report;
  return ProviderScope(
    overrides: <Override>[
      vpnConnectionProvider.overrideWithValue(core),
      ...overrides,
    ],
    child: MaterialApp(theme: AppTheme.dark(), home: screen),
  );
}

void _phone(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 9000)
    ..devicePixelRatio = 2;
  addTearDown(tester.view.reset);
}

/// Эмулятор из отчёта об устройстве: 1080×2400 при плотности 3.0 — узкий
/// экран (логическая ширина 360), на котором обрывались строки карточки.
void _narrowPhone(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(1080, 2400)
    ..devicePixelRatio = 3.0;
  addTearDown(tester.view.reset);
}

void main() {
  // ------------------------------------------------- блок рекламы: честность

  group('блок рекламы отвечает отчётом ядра, а не словом «включено»', () {
    test('выключен — так и сказано', () {
      final s = adBlockStatus(const CoreConfig(), _route(_reportNotRaised));
      expect(s.state, AdBlockState.off);
      expect(s.isWorking, isFalse);
    });

    // Просьба без подъёма — это «включится», а не «работает». Именно на этом
    // месте раньше стояла галочка, означавшая лишь «мы попросили».
    test('включён до первого подъёма — «подтвердить нечем»', () {
      final s = adBlockStatus(
        const CoreConfig(blockAds: true),
        _route(_reportNotRaised),
      );
      expect(s.state, AdBlockState.pending);
      expect(s.isWorking, isFalse);
      expect(s.message, contains('проверить нечем'));
    });

    test('отчёта нет вовсе — тот же ответ, а не молчание', () {
      final s = adBlockStatus(const CoreConfig(blockAds: true), null);
      expect(s.state, AdBlockState.pending);
    });

    test('список приехал файлом — «работает», с числом правил', () {
      final s = adBlockStatus(
        const CoreConfig(blockAds: true),
        _route(_reportAdsFromFile),
      );
      expect(s.state, AdBlockState.working);
      expect(s.message, contains('148000'));
    });

    test('список выброшен и база мертва — «не работает», с причиной', () {
      final s = adBlockStatus(
        const CoreConfig(blockAds: true),
        _route(_reportAdsDeadEverywhere),
      );
      expect(s.state, AdBlockState.broken);
      expect(s.isBad, isTrue);
      expect(s.message, contains('GEOSITE'));
    });

    // Пресет, который режет рекламу САМ, но переключателя не просили: ни одна
    // ветка не имеет права придумать «работает» из чужого признака.
    test('переключатель выключен — источник ads игнорируется', () {
      final s = adBlockStatus(const CoreConfig(), _route(_reportAdsFromFile));
      expect(s.state, AdBlockState.off);
    });
  });

  // ------------------------------------------------- строка «Улучшения»

  group('enhancementsSummary', () {
    test('без списков это просто режим страны', () {
      const cfg = CoreConfig(route: 0);
      expect(enhancementsSummary(cfg), RoutingMode.defaults[0].name);
    });

    test('блок рекламы добавляет свою часть', () {
      const cfg = CoreConfig(route: 0, blockAds: true);
      expect(enhancementsSummary(cfg), contains('без рекламы'));
    });

    // Сломанный блок рекламы не имеет права выглядеть работающим и в строке.
    test('сломанный блок рекламы назван сломанным', () {
      const cfg = CoreConfig(route: 0, blockAds: true);
      expect(
        enhancementsSummary(cfg, applied: _route(_reportAdsDeadEverywhere)),
        contains('не работает'),
      );
    });

    // Когда режим страны отменён списком сайтов, называть его в строке нельзя:
    // это приписало бы трафику правила, которых в туннеле нет.
    test('активный список сайтов вытесняет имя режима', () {
      const cfg = CoreConfig(
        route: 0,
        splitMode: SplitMode.onlySelected,
        allowDomains: 'youtube.com',
        allowSites: {'telegram'},
      );
      final line = enhancementsSummary(cfg);
      expect(line, contains('только 2 сайта'));
      expect(line, isNot(contains(RoutingMode.defaults[0].name)));
    });

    test('пустой список сайтов режим не отменяет', () {
      const cfg = CoreConfig(route: 0, splitMode: SplitMode.onlySelected);
      expect(enhancementsSummary(cfg), RoutingMode.defaults[0].name);
    });

    test('режим «кроме выбранных» считает свои домены', () {
      const cfg = CoreConfig(
        route: 0,
        splitMode: SplitMode.bypassSelected,
        bypassDomains: 'bank.ru, gosuslugi.ru',
      );
      expect(enhancementsSummary(cfg), contains('2 сайта напрямую'));
    });
  });

  // ------------------------------------------------- словарь наборов сайтов

  group('готовые наборы сайтов', () {
    // Зеркало `allowedSiteTags` из libs/caramba-core/api/policy_json.go.
    // Ядро отвергает незнакомый тег ЦЕЛИКОМ, поэтому расхождение здесь
    // выключило бы весь список сайтов у живого пользователя.
    test('состав совпадает с закрытым словарём ядра', () {
      final tags = kAllowSiteTags.map((t) => t.tag).toList()..sort();
      expect(tags, <String>[
        'discord',
        'disney',
        'facebook',
        'instagram',
        'netflix',
        'openai',
        'spotify',
        'telegram',
        'twitter',
        'youtube',
      ]);
    });

    test('незнакомый тег не попадает в состояние', () {
      final n = CoreConfigNotifier()..toggleAllowSite('vkontakte');
      expect(n.state.allowSites, isEmpty);
      n.toggleAllowSite('telegram');
      expect(n.state.allowSites, {'telegram'});
    });

    test('siteTagName переводит известный тег и не выдумывает чужой', () {
      expect(siteTagName('openai'), 'ChatGPT и OpenAI');
      expect(siteTagName('vkontakte'), 'vkontakte');
    });
  });

  // ------------------------------------------------- переживание настроек

  group('CoreConfig: новые поля переживают перезапуск', () {
    test('круговой обход JSON', () {
      const cfg = CoreConfig(
        blockAds: true,
        splitMode: SplitMode.onlySelected,
        allowDomains: 'youtube.com, a.example',
        allowSites: {'telegram', 'openai'},
      );
      final back = CoreConfig.fromJson(cfg.toJson());
      expect(back.blockAds, isTrue);
      expect(back.allowDomains, 'youtube.com, a.example');
      expect(back.allowSites, {'telegram', 'openai'});
      expect(back.allowSitesActive, isTrue);
    });

    // Запись более новой версии не имеет права выключить весь список: ядро
    // отвергло бы патч с незнакомым тегом целиком.
    test('чужой тег из будущей версии отбрасывается, остальные живут', () {
      final json = const CoreConfig(
        splitMode: SplitMode.onlySelected,
        allowSites: {'telegram'},
      ).toJson();
      json['allow_sites'] = <String>['telegram', 'tiktok'];
      expect(CoreConfig.fromJson(json).allowSites, {'telegram'});
    });

    test('запись старой версии читается без новых полей', () {
      final back = CoreConfig.fromJson(<String, dynamic>{
        'route': 0,
        'split_mode': 'off',
      });
      expect(back.blockAds, isFalse);
      expect(back.allowSites, isEmpty);
      expect(back.allowDomains, '');
    });

    test('siteRuleCount считает то, что реально уйдёт ядру', () {
      expect(
        const CoreConfig(
          splitMode: SplitMode.onlySelected,
          allowDomains: 'a.example',
          allowSites: {'telegram'},
        ).siteRuleCount,
        2,
      );
      // Выключенный режим — ноль: список, который никуда не уходит, ничего
      // не значит.
      expect(
        const CoreConfig(allowDomains: 'a.example').siteRuleCount,
        0,
      );
    });
  });

  // ------------------------------------------------- сам экран

  group('экран «Улучшения»', () {
    testWidgets('называется по-новому и несёт три блока', (tester) async {
      _phone(tester);
      await tester.pumpWidget(_wrap(const SplitTunnelScreen()));
      await tester.pump();

      expect(find.text('Улучшения'), findsOneWidget);
      expect(find.text('РЕКЛАМА И ТРЕКЕРЫ'), findsOneWidget);
      expect(find.text('ПРАВИЛА ПО САЙТАМ'), findsOneWidget);
      expect(find.text('РЕЖИМ ДЛЯ СТРАНЫ'), findsOneWidget);
      // Прежнее имя не должно остаться нигде: владелец его и не понял.
      expect(find.text('Раздельное туннелирование'), findsNothing);
    });

    // Ровно то, что просил владелец: галочка, которая ничего не включает, не
    // показывается вовсе — она показана выключенной с причиной.
    testWidgets('список приложений заменён выключенной строкой с причиной', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(_wrap(const SplitTunnelScreen()));
      await tester.pump();

      for (final demo in <String>['Telegram', 'Chrome', 'СберБанк']) {
        expect(
          find.text(demo),
          findsNothing,
          reason: 'демонстрационное приложение $demo снова показано тумблером',
        );
      }
      expect(find.text('Выбирать приложения'), findsOneWidget);
      final sw = tester.widget<Switch>(
        find.descendant(
          of: find
              .ancestor(
                of: find.text('Выбирать приложения'),
                matching: find.byType(Row),
              )
              .first,
          matching: find.byType(Switch),
        ),
      );
      expect(sw.onChanged, isNull, reason: 'строка обязана быть выключенной');
      expect(find.textContaining('демонстрационным'), findsOneWidget);
    });

    testWidgets('переключатель блока рекламы правит состояние ядра', (
      tester,
    ) async {
      _phone(tester);
      late WidgetRef captured;
      await tester.pumpWidget(
        _wrap(
          Consumer(
            builder: (_, ref, __) {
              captured = ref;
              return const SplitTunnelScreen();
            },
          ),
        ),
      );
      await tester.pump();

      expect(captured.read(coreConfigProvider).blockAds, isFalse);
      await tester.tap(find.text('Блокировать рекламу и трекеры'));
      await tester.pump();
      // Тап по подписи ничего не меняет — переключаем сам тумблер.
      final sw = find.descendant(
        of: find
            .ancestor(
              of: find.text('Блокировать рекламу и трекеры'),
              matching: find.byType(Row),
            )
            .first,
        matching: find.byType(Switch),
      );
      await tester.tap(sw);
      await tester.pump();
      expect(captured.read(coreConfigProvider).blockAds, isTrue);
    });

    testWidgets('режим «только выбранные» предупреждает о пустом списке', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(_wrap(const SplitTunnelScreen()));
      await tester.pump();

      await tester.tap(find.text('Только выбранные сайты'));
      await tester.pump();

      expect(find.text('СВОИ САЙТЫ'), findsOneWidget);
      expect(find.text('ГОТОВЫЕ НАБОРЫ'), findsOneWidget);
      expect(find.textContaining('Список пуст'), findsOneWidget);
    });

    // База GEOSITE мертва — правило `GEOSITE,telegram` не совпадёт никогда,
    // и тумблер обязан погаснуть, а не обещать несуществующее.
    testWidgets('мёртвая база GEOSITE гасит готовые наборы', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const SplitTunnelScreen(),
          report: _reportAdsDeadEverywhere,
        ),
      );
      await tester.pump();
      await tester.pump();

      await tester.tap(find.text('Только выбранные сайты'));
      await tester.pump();

      expect(find.text('база GEOSITE мертва'), findsWidgets);
      final sw = tester.widget<Switch>(
        find.descendant(
          of: find
              .ancestor(
                of: find.text('Telegram'),
                matching: find.byType(Row),
              )
              .first,
          matching: find.byType(Switch),
        ),
      );
      expect(sw.onChanged, isNull);
    });
  });

  // --------------------------------------------- фокус поля «Свои сайты»

  group('поле «Свои сайты» переживает фокус между символами', () {
    // Раньше исчезновение баннера «Список пуст» (взводится уже первым
    // непустым символом — allowSitesActive) пересобирало дерево виджетов
    // НАД полем. Без ключа Flutter не находил старый элемент поля на
    // сдвинутой позиции, пересоздавал его заново вместе с внутренним
    // FocusNode — и клавиатура закрывалась после первой же буквы.
    testWidgets('фокус и элемент поля не пересоздаются после первого символа', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(_wrap(const SplitTunnelScreen()));
      await tester.pump();

      await tester.tap(find.text('Только выбранные сайты'));
      await tester.pump();

      final field = find.byKey(
        const ValueKey('split-tunnel-allow-domains-field'),
      );
      expect(field, findsOneWidget);
      // Баннер обязан стоять ДО первого символа — иначе тест не проверяет то,
      // что должен: именно его исчезновение двигает поле по списку.
      expect(find.textContaining('Список пуст'), findsOneWidget);

      FocusNode nodeOf() => tester
          .widget<EditableText>(
            find.descendant(of: field, matching: find.byType(EditableText)),
          )
          .focusNode;

      // Фокусируем поле, ничего ещё не печатая — это база для сравнения.
      await tester.enterText(field, '');
      await tester.pump();
      final before = nodeOf();
      expect(
        before.hasFocus,
        isTrue,
        reason: 'поле обязано получить фокус перед вводом',
      );

      // Один символ — ровно то, что в отчёте закрывало клавиатуру.
      await tester.enterText(field, 'i');
      await tester.pump();

      // Баннер обязан пропасть — иначе перестройка дерева не произошла и
      // тест ничего не доказывает.
      expect(find.textContaining('Список пуст'), findsNothing);

      final after = nodeOf();
      expect(
        identical(before, after),
        isTrue,
        reason: 'элемент поля пересобран заново вместе с исчезнувшим баннером',
      );
      expect(
        after.hasFocus,
        isTrue,
        reason: 'фокус потерян после первого введённого символа',
      );
    });
  });

  // --------------------------------------------- обрезка строк на 1080

  group('карточка «Что применилось» не обрезает длинный ответ пресета', () {
    // «Блок рекламы» и «Стриминг через VPN» раньше показывали значение
    // строкой CRow с TextOverflow.ellipsis — а dart:ui включает предел в
    // одну строку уже тем, что `ellipsis` задан, ДАЖЕ без явного `maxLines`.
    // «этот пресет его не включает» резалось до «этот пресет его не …»
    // ровно на границе слова, где терялось отрицание.
    testWidgets('«этот пресет его не включает» переносится, а не обрывается', (
      tester,
    ) async {
      _narrowPhone(tester);
      await tester.pumpWidget(
        _wrap(
          const Scaffold(
            body: SingleChildScrollView(child: AppliedRouteCard()),
          ),
          report: _reportNeitherCapability,
        ),
      );
      await tester.pump();
      await tester.pump();

      expect(find.text('Блок рекламы'), findsOneWidget);
      expect(find.text('Стриминг через VPN'), findsOneWidget);

      final values = find.text('этот пресет его не включает');
      // Обе строки отвечают одним и тем же текстом реестра.
      expect(values, findsNWidgets(2));

      for (final element in values.evaluate()) {
        final rp = tester.renderObject<RenderParagraph>(
          find.descendant(
            of: find.byWidget(element.widget),
            matching: find.byType(RichText),
          ),
        );
        expect(
          rp.didExceedMaxLines,
          isFalse,
          reason: 'ответ пресета обрезан многоточием посреди фразы',
        );
      }
    });
  });

  group('строка «Улучшения» в Настройках не обрезает длинную сводку', () {
    // Настройки → «Улучшения» показывали значение строкой CRow с
    // TextOverflow.ellipsis — тот же дефект, что уже чинили в
    // AppliedRouteCard: обрезка у dart:ui включается самим фактом
    // `overflow`, ДАЖЕ без явного `maxLines`. Сводка «режим · N сайтов ·
    // блок рекламы» на узком экране резалась до «...· бе…» ровно на границе
    // слова «без рекламы».
    testWidgets('длинная сводка переносится на 1080, а не обрывается', (
      tester,
    ) async {
      _narrowPhone(tester);

      // Длинная сводка на всех трёх фронтах разом: полное имя режима,
      // список сайтов режима «кроме выбранных» и включённый блок рекламы.
      const cfg = CoreConfig(
        route: 2, // 'Россия (полный обход)' — одно из самых длинных имён реестра.
        splitMode: SplitMode.bypassSelected,
        bypassDomains: 'a.example, b.example, c.example, d.example, e.example',
        blockAds: true,
      );
      final expectedSummary = enhancementsSummary(cfg);
      // Сама сводка обязана быть длинной — иначе тест ничего не проверяет.
      expect(expectedSummary.length, greaterThan(40));

      await tester.pumpWidget(
        _wrap(
          const Scaffold(body: SettingsScreen()),
          overrides: [
            coreConfigProvider.overrideWith((ref) {
              return CoreConfigNotifier()..state = cfg;
            }),
          ],
        ),
      );
      await tester.pump();
      await tester.pump();

      final value = find.text(expectedSummary);
      expect(
        value,
        findsOneWidget,
        reason: 'сводка не найдена целиком — где-то обрезана или собрана иначе',
      );
      final rp = tester.renderObject<RenderParagraph>(
        find.descendant(of: value, matching: find.byType(RichText)),
      );
      expect(
        rp.didExceedMaxLines,
        isFalse,
        reason: 'сводка обрезана многоточием посреди слова',
      );
    });
  });
}

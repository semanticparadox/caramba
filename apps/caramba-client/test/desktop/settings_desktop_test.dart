// Настройки на десктопе: та же машина, другая раскладка.
//
// Экран переписан целиком (индекс разделов, форма, пикеры вместо листов), и
// самый дешёвый способ сломать его молча — отвязать контрол от того сеттера,
// в который он писал на телефоне. Тогда экран выглядит рабочим: пикер
// открывается, значение меняется в поле, а до ядра и до оператора не доезжает
// ничего. Поэтому здесь проверяются НЕ виджеты, а концы:
//
//   * DNS-пикер пишет и в [CoreConfig], и в состояние CSM — то есть идёт
//     через [CsmSettingsBridge], а не мимо него;
//   * Kill-switch правит конфигурацию ядра;
//   * «Правила по сайтам» ведут на свой экран, а не открывают лист;
//   * раздел «Приложение» пишет в десктопные настройки окна;
//   * все разделы названы и попали в индекс — раздел, выпавший из списка,
//     просто исчезает с экрана, не оставляя следа.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/desktop/autostart_service.dart';
import 'package:caramba_client/desktop/desktop_prefs.dart';
import 'package:caramba_client/desktop/desktop_strings.dart';
import 'package:caramba_client/desktop/widgets/desktop_picker.dart';
import 'package:caramba_client/desktop/widgets/form_row.dart';
import 'package:caramba_client/features/settings/settings_desktop.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';

import '../support/fake_core.dart';

const _pin = CsmPin(
  pid: '226e8a20f699b964',
  linkPin: '49Q8M87PK6WP9QXG3T30',
  origin: CsmPinOrigin.outOfBand,
  establishedMs: 1788300000000,
);

const _csm = CsmProfileState(pin: _pin, stage: CsmProfileStage.trusted);

class _Store implements ConnectionProfilesStore {
  _Store(this.profiles, this.activeId);

  List<ConnectionProfile> profiles;
  String? activeId;

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
    profiles = const <ConnectionProfile>[];
    activeId = null;
  }
}

ConnectionProfile _profile(CsmProfileState? csm) => ConnectionProfile(
      id: 'cp_1',
      type: ProfileType.rawSub,
      displayName: 'Моя подписка',
      source: 'https://sub.example/a',
      rawConfig: 'proxies: []',
      format: 'clash',
      csm: csm,
    );

/// Правка настройки проходит через нотифаер профилей и возвращается в
/// провайдеры следующим кадром. Восемь кадров с запасом.
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 8; i++) {
    await tester.pump();
  }
}

/// Десктопная ветка выбирается ПЛАТФОРМОЙ, а не шириной окна.
///
/// Возврат переопределения стоит в `finally`, а не в `tearDown`: flutter_test
/// проверяет «отладочные переменные foundation вернули в исходное» ещё ДО
/// tearDown, и наследивший тест уронил бы следующий, а не себя.
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

ProviderContainer _container({CsmProfileState? csm, bool autostart = false}) {
  final container = ProviderContainer(
    overrides: <Override>[
      vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
      connectionProfilesStoreProvider.overrideWithValue(
        _Store(<ConnectionProfile>[_profile(csm)], 'cp_1'),
      ),
      // Умеет ли система автозапуск, знает `AutostartService`, а он живёт
      // рядом с окном; в тесте его ответ подставляется сюда.
      autostartSupportedProvider.overrideWith((ref) => autostart),
      autostartUnavailableMessageProvider.overrideWith(
        (ref) => DesktopStrings.launchAtLoginUnsupportedMac,
      ),
    ],
  );
  addTearDown(container.dispose);
  return container;
}

/// Хранилище токенов лежит в secure storage: без заглушки канала экран падает
/// на чтении сессии ещё до первой строки формы.
void _mockSecureStorage() {
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  const channel = MethodChannel('plugins.it_nomads.com/flutter_secure_storage');
  messenger.setMockMethodCallHandler(channel, (call) async => null);
  addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
}

Future<ProviderContainer> _pump(
  WidgetTester tester, {
  CsmProfileState? csm,
  bool autostart = false,
}) async {
  _mockSecureStorage();
  final container = _container(csm: csm, autostart: autostart);
  await tester.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        theme: AppTheme.dark(),
        home: const SettingsDesktopScreen(),
      ),
    ),
  );
  await _settle(tester);
  return container;
}

/// Контрол строки: у формы все контролы одного типа, поэтому искать их можно
/// только через строку, к которой они привязаны.
Finder _controlIn(String label, Type control) => find.descendant(
      of: find.ancestor(of: find.text(label), matching: find.byType(FormRow)),
      matching: find.byType(control),
    );

void main() {
  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

  testWidgets('settings form stays within 720 pixels on a wide window',
      (tester) async {
    await _desktop(tester, () async {
      tester.view.physicalSize = const Size(1800, 900);
      await _pump(tester);
      final form = find.byType(SingleChildScrollView).last;
      expect(tester.getSize(form).width, lessThanOrEqualTo(720));
      expect(tester.takeException(), isNull);
    });
  });

  testWidgets('last index section is selected when the form reaches its bottom',
      (tester) async {
    await _desktop(tester, () async {
      await _pump(tester);
      await tester.tap(find.text(DesktopStrings.settingsAppSection));
      await tester.pumpAndSettle();
      final indexSemantics = find.ancestor(
        of: find.text(DesktopStrings.settingsAppSection),
        matching: find.byWidgetPredicate(
          (w) => w is Semantics && w.properties.selected == true,
        ),
      );
      expect(indexSemantics, findsOneWidget);
      final form = tester.widget<SingleChildScrollView>(
        find.byType(SingleChildScrollView).last,
      );
      expect(form.controller!.position.extentAfter, lessThanOrEqualTo(1));
      form.controller!.jumpTo(0);
      await tester.pump();
      expect(
        find.ancestor(
          of: find.text('Подключение'),
          matching: find.byWidgetPredicate(
            (w) => w is Semantics && w.properties.selected == true,
          ),
        ),
        findsOneWidget,
      );
    });
  });

  for (final scale in <double>[1, 1.5, 2]) {
    testWidgets('desktop action label fits at text scale $scale',
        (tester) async {
      await _desktop(tester, () async {
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(),
            home: MediaQuery(
              data: MediaQueryData(textScaler: TextScaler.linear(scale)),
              child: Scaffold(
                body: Center(
                  child: FormOpenButton(label: 'Изменить', onPressed: () {}),
                ),
              ),
            ),
          ),
        );
        final paragraph =
            tester.renderObject<RenderParagraph>(find.text('Изменить'));
        final painter = TextPainter(
          text: paragraph.text,
          textDirection: TextDirection.ltr,
          textScaler: paragraph.textScaler,
          maxLines: 1,
        )..layout();
        expect(paragraph.size.width, greaterThanOrEqualTo(painter.width));
        expect(
          tester.getSize(find.byType(FormOpenButton)).width,
          greaterThanOrEqualTo(kFormButtonWidth),
        );
        painter.dispose();
        expect(tester.takeException(), isNull);
      });
    });
  }

  testWidgets('разделы названы и попали в индекс', (tester) async {
    await _desktop(tester, () async {
      await _pump(tester, csm: _csm);

      // Заголовки разделов (SectionTitle рисует их в верхнем регистре).
      for (final title in const <String>[
        'ПОДКЛЮЧЕНИЕ',
        'ПРАВИЛА ТРАФИКА',
        'СЕТЬ И ЯДРО',
        'ПРОВЕРКА И ПРОЗРАЧНОСТЬ',
        'АВТОНАСТРОЙКА',
        'АККАУНТ ПАНЕЛИ',
        'ВИД',
        'ПРИЛОЖЕНИЕ',
      ]) {
        expect(find.text(title), findsOneWidget, reason: 'нет раздела $title');
      }

      // Тот же список слева, обычным регистром: индекс и форма собираются из
      // ОДНОГО списка разделов, и расхождение здесь означало бы два списка.
      expect(find.text('Сеть и ядро'), findsOneWidget);
      expect(find.text(DesktopStrings.settingsAppSection), findsOneWidget);

      // Заголовка экрана нет: его рисует тулбар оболочки.
      expect(find.text('Настройки'), findsNothing);
    });
  });

  testWidgets('DNS-пикер пишет и в ядро, и оператору', (tester) async {
    await _desktop(tester, () async {
      final container = await _pump(tester, csm: _csm);

      expect(container.read(coreConfigProvider).dns, 0);

      await tester.tap(_controlIn('DNS-резолвер', DesktopPicker));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Cloudflare'));
      await _settle(tester);

      expect(container.read(coreConfigProvider).dns, 1);
      // Второй конец: значение ушло в состояние CSM, то есть правка прошла
      // через мост, а не мимо него.
      final v = container
          .read(csmSettingsProvider)
          .valueOf(CsmSettingKey.dnsNameservers);
      expect((v! as CsmTextList).value.first, startsWith('https://'));

      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  testWidgets('Kill-switch правит конфигурацию ядра', (tester) async {
    await _desktop(tester, () async {
      final container = await _pump(tester);

      expect(container.read(coreConfigProvider).killSwitch, isTrue);
      await tester.tap(_controlIn('Kill-switch', Switch));
      await _settle(tester);
      expect(container.read(coreConfigProvider).killSwitch, isFalse);

      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  testWidgets('«Правила по сайтам» ведут на свой экран', (tester) async {
    await _desktop(tester, () async {
      _mockSecureStorage();
      final container = _container();
      final router = GoRouter(
        routes: <RouteBase>[
          GoRoute(path: '/', builder: (_, __) => const SettingsDesktopScreen()),
          GoRoute(
            path: AppRoute.siteRules,
            builder: (_, __) =>
                const Scaffold(body: Center(child: Text('экран списков'))),
          ),
        ],
      );
      addTearDown(router.dispose);

      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp.router(
            theme: AppTheme.dark(),
            routerConfig: router,
          ),
        ),
      );
      await _settle(tester);

      await tester.tap(
        find.descendant(
          of: find.ancestor(
            of: find.text('Правила по сайтам'),
            matching: find.byType(FormRow),
          ),
          matching: find.byType(FormOpenButton),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('экран списков'), findsOneWidget);

      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  // Дефект D-03 ручной проверки: подпись «Нужна macOS 13 или новее» висела на
  // ЛЮБОЙ macOS, а переключатель при этом нажимался.
  testWidgets('автозапуск объясняется и гаснет, когда система его не умеет', (
    tester,
  ) async {
    await _desktop(tester, () async {
      final container = await _pump(tester);

      await tester.tap(find.text(DesktopStrings.settingsAppSection));
      await tester.pumpAndSettle();

      expect(
        find.text(DesktopStrings.launchAtLoginUnsupportedMac),
        findsOneWidget,
      );
      final sw = tester.widget<Switch>(
        _controlIn(DesktopStrings.launchAtLoginTitle, Switch),
      );
      expect(
        sw.onChanged,
        isNull,
        reason: 'тумблер, которого система не примет, не нажимается',
      );
      expect(container.read(desktopPrefsProvider).launchAtLogin, isFalse);

      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  testWidgets('pending approval explains why requested autostart is not active',
      (tester) async {
    await _desktop(tester, () async {
      final container = await _pump(tester, autostart: true);
      container.read(desktopPrefsProvider.notifier).setLaunchAtLogin(true);
      container.read(autostartApprovalPendingProvider.notifier).state = true;
      await tester.tap(find.text(DesktopStrings.settingsAppSection));
      await tester.pumpAndSettle();
      expect(
        find.text(DesktopStrings.launchAtLoginNeedsApproval),
        findsOneWidget,
      );
      final sw = tester.widget<Switch>(
        _controlIn(DesktopStrings.launchAtLoginTitle, Switch),
      );
      expect(sw.value, isTrue);
      expect(sw.onChanged, isNotNull);
      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  testWidgets('на системе с автозапуском тумблер работает и молчит', (
    tester,
  ) async {
    await _desktop(tester, () async {
      final container = await _pump(tester, autostart: true);

      await tester.tap(find.text(DesktopStrings.settingsAppSection));
      await tester.pumpAndSettle();

      expect(
        find.text(DesktopStrings.launchAtLoginUnsupportedMac),
        findsNothing,
        reason: 'подпись про macOS 13 на рабочей системе — ложь',
      );

      await tester.tap(
        _controlIn(DesktopStrings.launchAtLoginTitle, Switch),
      );
      await _settle(tester);
      expect(container.read(desktopPrefsProvider).launchAtLogin, isTrue);

      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  testWidgets(
    'раздел «Приложение» пишет закрытие окна в десктопные настройки',
    (tester) async {
      await _desktop(tester, () async {
        final container = await _pump(tester);

        // Дефолт: красная кнопка прячет окно, туннель живёт.
        expect(container.read(desktopPrefsProvider).closeToTray, isTrue);

        // Заодно проверяем индекс: он обязан доводить до раздела, иначе форма
        // длиной в шесть экранов листается только колесом.
        await tester.tap(find.text(DesktopStrings.settingsAppSection));
        await tester.pumpAndSettle();

        await tester.tap(
          _controlIn(DesktopStrings.onWindowCloseTitle, DesktopPicker),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.text(DesktopStrings.onWindowCloseQuit));
        await _settle(tester);

        expect(container.read(desktopPrefsProvider).closeToTray, isFalse);

        await tester.pumpWidget(const SizedBox.shrink());
      });
    },
  );
}

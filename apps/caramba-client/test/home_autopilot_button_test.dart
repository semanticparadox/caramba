// Автоподбор на Главной: отдельная кнопка, а не пятая строка списка.
//
// Жалоба владельца была про вид, но дефект — про смысл: «Сервер», «Relay»,
// «Тип подключения» и «Режим» это ВЫБОРЫ, а автоподбор — ДЕЙСТВИЕ, и пока он
// стоял [CRow]-строкой с шевроном в той же группе, форма обещала пятый список.
// Поэтому проверяется не цвет, а два факта: кнопка вне группы строк, и строки
// «Автоподбор» с шевроном в группе больше нет.
//
// Вторая половина файла — таблица состояний. Слова «в силе» и «устарело» ведут
// человека в разные места (переподключиться против перезамерить), и подстановка
// одного вместо другого — не опечатка, а неверный совет. Таблица проверяется
// чистой функцией: пять прогонов дерева виджетов ошибку в одной ветке скрыли бы
// легче, чем пять строк ожиданий.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/features/home/autopilot_button.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart' show ImportedServer;
import 'package:caramba_client/widgets/ui.dart';

import 'support/fake_core.dart';

/// Ход замера с заранее выставленным состоянием: сам [AutopilotController]
/// поднять в тесте нельзя (он пошёл бы в ядро), а состояние «идёт» с Главной
/// достижимо и обязано проверяться.
class _FakeAutopilot extends AutopilotController {
  _FakeAutopilot(super.ref, AutopilotRun run) {
    state = run;
  }
}

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

const _profile = ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Моя подписка',
  source: 'https://sub.example/a',
  rawConfig: 'proxies: []',
  format: 'clash',
  servers: <ImportedServer>[
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
  serversUpdatedMs: 1,
);

const _canada = AutoPickRecord(
  proxyName: '🇨🇦 Stream',
  latencyMs: 160,
  updatedMs: 1,
  exitKey: 'ca-1',
  countryCode: 'CA',
  machineTitle: 'Канада',
  protocolLabel: 'vless · tcp · reality',
  confirmed: true,
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

/// Кнопка одна, без Главной: состояния задаются провайдерами, а не сборкой
/// живого профиля под каждую причину устаревания.
Widget _button({
  AutoPickRecord? pick,
  AutoStaleReason stale = AutoStaleReason.none,
  bool running = false,
}) => ProviderScope(
  overrides: <Override>[
    autoPickRecordProvider.overrideWithValue(pick),
    autoStaleProvider.overrideWithValue(stale),
    autopilotProvider.overrideWith(
      (ref) => _FakeAutopilot(ref, AutopilotRun(running: running)),
    ),
  ],
  child: MaterialApp(
    theme: AppTheme.dark(),
    home: const Scaffold(body: AutopilotButton()),
  ),
);

void _phone(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 9000)
    ..devicePixelRatio = 2;
  addTearDown(tester.view.reset);
}

void main() {
  group('таблица состояний', () {
    test(
      'замер не запускали — кнопка зовёт запустить, слова состояния нет',
      () {
        final m = autopilotButtonModel(
          running: false,
          pick: null,
          stale: AutoStaleReason.none,
        );
        expect(m.state, AutopilotButtonState.neverRun);
        expect(m.text, 'Подобрать лучший узел');
        expect(m.badge, '');
      },
    );

    test('идущий замер перебивает прошлый выбор', () {
      // Пока идёт перерешение, объявлять прошлый выбор «в силе» значит обещать
      // то, что через секунду сменится.
      final m = autopilotButtonModel(
        running: true,
        pick: _canada,
        stale: AutoStaleReason.none,
      );
      expect(m.state, AutopilotButtonState.running);
      expect(m.text, 'Подбираю узел…');
      expect(m.badge, '');
    });

    test('выбор исполняется — «в силе»', () {
      final m = autopilotButtonModel(
        running: false,
        pick: _canada,
        stale: AutoStaleReason.none,
      );
      expect(m.state, AutopilotButtonState.inForce);
      expect(m.text, 'Автоподбор: CA · Канада');
      expect(m.badge, kAutopilotBadgeInForce);
    });

    // Ядро стоит на другом узле / «Сервер» закреплён на другом: замер при этом
    // СВЕЖИЙ, и слово «устарело» послало бы перезамерять там, где перезамер
    // ничего не изменит.
    for (final reason in const <AutoStaleReason>[
      AutoStaleReason.tunnelDisagrees,
      AutoStaleReason.pinDisagrees,
    ]) {
      test('$reason — «не в силе», а не «устарело»', () {
        final m = autopilotButtonModel(
          running: false,
          pick: _canada,
          stale: reason,
        );
        expect(m.state, AutopilotButtonState.notInForce);
        expect(m.badge, kAutopilotBadgeNotInForce);
        expect(m.badge, isNot(kAutopilotBadgeStale));
      });
    }

    // Устарел сам ЗАМЕР: узел, может, и тот же, но число про него — вчерашнее.
    for (final reason in const <AutoStaleReason>[
      AutoStaleReason.age,
      AutoStaleReason.fleetChanged,
    ]) {
      test('$reason — «устарело», а не «не в силе»', () {
        final m = autopilotButtonModel(
          running: false,
          pick: _canada,
          stale: reason,
        );
        expect(m.state, AutopilotButtonState.stale);
        expect(m.badge, kAutopilotBadgeStale);
        expect(m.badge, isNot(kAutopilotBadgeNotInForce));
      });
    }

    test('каждая причина устаревания получила своё слово', () {
      // Прогон по ВСЕМ значениям перечисления: новая причина, добавленная
      // исполнителем autopilot_state, не имеет права молча попасть в чужую
      // корзину — здесь она обязана хотя бы получить непустое слово.
      for (final reason in AutoStaleReason.values) {
        final m = autopilotButtonModel(
          running: false,
          pick: _canada,
          stale: reason,
        );
        expect(m.badge, isNotEmpty, reason: '$reason осталась без слова');
      }
    });
  });

  group('имя выбора', () {
    test('страна и машина — через точку, как в строке «Сервер»', () {
      expect(autopilotChoiceLabel(_canada), 'CA · Канада');
    });

    test('машины нет — остаётся код страны, и он не удваивается', () {
      // shortLabel в этом случае и ЕСТЬ код страны: «CA · CA» было бы двумя
      // именами одного и того же.
      const pick = AutoPickRecord(
        proxyName: 'ca-1',
        latencyMs: 90,
        updatedMs: 1,
        countryCode: 'CA',
      );
      expect(autopilotChoiceLabel(pick), 'CA');
    });

    test('страны нет — остаётся имя прокси', () {
      const pick = AutoPickRecord(
        proxyName: '🇨🇦 Stream',
        latencyMs: 90,
        updatedMs: 1,
      );
      expect(autopilotChoiceLabel(pick), '🇨🇦 Stream');
    });
  });

  group('вид кнопки', () {
    testWidgets('без замера — призыв запустить, без слова состояния', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(_button());
      await tester.pump();

      expect(find.text('Подобрать лучший узел'), findsOneWidget);
      expect(find.text('В СИЛЕ'), findsNothing);
      expect(find.text('НЕ В СИЛЕ'), findsNothing);
      expect(find.text('УСТАРЕЛО'), findsNothing);
    });

    testWidgets('идущий замер показывает спиннер и остаётся нажимаемым', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(_button(pick: _canada, running: true));
      await tester.pump();

      expect(find.text('Подбираю узел…'), findsOneWidget);
      expect(find.byType(CircularProgressIndicator), findsOneWidget);
      // Кнопка ведёт на единственный экран, где ход замера видно. Запереть вход
      // туда ровно тогда, когда кнопка объявляет «Подбираю узел…», значило бы
      // спрятать то, о чём она сообщает.
      expect(
        tester.widget<OutlinedButton>(find.byType(OutlinedButton)).onPressed,
        isNotNull,
      );
      await tester.pumpWidget(const SizedBox.shrink());
    });

    testWidgets('расхождение подписано «НЕ В СИЛЕ»', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _button(pick: _canada, stale: AutoStaleReason.tunnelDisagrees),
      );
      await tester.pump();

      expect(find.text('Автоподбор: CA · Канада'), findsOneWidget);
      expect(find.text('НЕ В СИЛЕ'), findsOneWidget);
      expect(find.text('УСТАРЕЛО'), findsNothing);
    });

    // Владелец просил не просто «другую кнопку», а РАЗЛИЧИМЫЕ состояния: пока
    // все пять выглядят одинаково, кнопка снова становится строкой, только с
    // рамкой. Цвет обводки — единственный носитель этого различия, поэтому он
    // и проверяется, а не подбирается на глаз.
    testWidgets('состояния различаются обводкой, а не только словом', (
      tester,
    ) async {
      _phone(tester);

      Future<Color> borderOf({
        AutoPickRecord? pick,
        AutoStaleReason stale = AutoStaleReason.none,
      }) async {
        await tester.pumpWidget(_button(pick: pick, stale: stale));
        await tester.pump();
        final style = tester
            .widget<OutlinedButton>(find.byType(OutlinedButton))
            .style!;
        return style.side!.resolve(<WidgetState>{})!.color;
      }

      final neverRun = await borderOf();
      final inForce = await borderOf(pick: _canada);
      final notInForce = await borderOf(
        pick: _canada,
        stale: AutoStaleReason.tunnelDisagrees,
      );
      final stale = await borderOf(pick: _canada, stale: AutoStaleReason.age);

      // «В силе» — акцент, «не в силе» — предупреждение, остальные — покой.
      expect(inForce, isNot(neverRun));
      expect(notInForce, isNot(neverRun));
      expect(notInForce, isNot(inForce));
      // Устаревший замер тревогой не является: узел, возможно, тот же самый,
      // просто число про него вчерашнее.
      expect(stale, neverRun);

      await tester.pumpWidget(const SizedBox.shrink());
    });

    testWidgets('старый замер подписан «УСТАРЕЛО»', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _button(pick: _canada, stale: AutoStaleReason.age),
      );
      await tester.pump();

      expect(find.text('УСТАРЕЛО'), findsOneWidget);
      expect(find.text('НЕ В СИЛЕ'), findsNothing);
    });
  });

  group('место на Главной', () {
    testWidgets('автоподбор — кнопка вне группы строк, а не строка в ней', (
      tester,
    ) async {
      _phone(tester);
      // Профили читаются асинхронно: до них экран ещё в панельной ветке.
      await tester.pumpWidget(_guestHome());
      await tester.pump();
      await tester.pump();
      await tester.pump();

      expect(find.byType(AutopilotButton), findsOneWidget);
      expect(
        find.ancestor(
          of: find.byType(AutopilotButton),
          matching: find.byType(RowsGroup),
        ),
        findsNothing,
        reason: 'кнопка внутри группы — это снова строка списка',
      );
      // Строки с шевроном и старых её значений на экране не осталось.
      expect(find.text('Автоподбор'), findsNothing);
      expect(find.text('Не запускали'), findsNothing);

      expect(tester.takeException(), isNull);
      // Гасим односекундный таймер сессии вместе с деревом.
      await tester.pumpWidget(const SizedBox.shrink());
    });

    testWidgets('строка режима называется «Режим»', (tester) async {
      _phone(tester);
      await tester.pumpWidget(_guestHome());
      await tester.pump();
      await tester.pump();
      await tester.pump();

      // Владелец дословно: «просто переименуй в Режим». Старое имя не имеет
      // права остаться нигде на Главной — иначе на экране два имени одного
      // листа.
      expect(find.text('Режим'), findsOneWidget);
      expect(find.text('Режим для страны'), findsNothing);

      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    });
  });
}

// Единственное утверждение Home — слово «Защищено», и оно обязано быть правдой.
//
// ВОСПРОИЗВЕДЁННАЯ ПОЛОМКА (эмулятор, 1м43с): подписка за лимитом, туннель
// поднят на кэше конфигурации, ядро сообщает `connected`. Экран показывал
// зелёный щит, «Защищено» и идущий таймер сессии, счётчики стояли на 0,0 МБ, и
// ни одна строка нигде не говорила, что трафик по подписке кончился.
//
// Обратная сторона проверяется здесь же и так же строго: человек, который
// подключился и убрал телефон в карман, обязан остаться защищённым. Ноль байт
// за первые секунды — это исправный туннель, а не поломка, и объявлять по
// этому поводу отказ нельзя.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/state/access_guard.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/connect_dial.dart';
import 'support/fake_csm_device.dart';

/// Ядро, которое честно подняло туннель. Про доступ оно не знает ничего — в
/// этом вся суть поломки: `connected` означает «конфиг разобран и TUN поднят».
class _ConnectedCore with FakeCsmDevice implements VpnConnection {
  @override
  final VpnStatus currentStatus;

  final TrafficStats _traffic;

  _ConnectedCore({TrafficStats traffic = TrafficStats.zero})
    : _traffic = traffic,
      currentStatus = VpnStatus(
        stage: VpnStage.connected,
        connectedSince: DateTime.now().subtract(
          const Duration(minutes: 1, seconds: 43),
        ),
        mode: TunnelMode.tun,
        activeProxy: 'DE Stealth',
      );

  @override
  Stream<VpnStatus> get status => Stream<VpnStatus>.value(currentStatus);

  @override
  Stream<TrafficStats> get traffic => Stream<TrafficStats>.value(_traffic);

  @override
  Future<void> connect(Server server) async {}

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {}

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async => const ImportResult(servers: <ImportedServer>[]);

  @override
  Future<List<ProbeResult>> probe({Duration timeout = Duration.zero}) async =>
      const <ProbeResult>[];

  @override
  Future<void> setPolicy(CorePolicy policy) async {}

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 0}) async {}

  @override
  Future<void> disconnect() async {}

  @override
  Future<void> dispose() async {}
}

class _FakeProfilesStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles;
  String? activeId;

  _FakeProfilesStore(this.profiles, this.activeId);

  @override
  Future<List<ConnectionProfile>> readProfiles() async => profiles;

  @override
  Future<String?> readActiveId() async => activeId;

  @override
  Future<void> writeProfiles(List<ConnectionProfile> next) async =>
      profiles = next;

  @override
  Future<void> writeActiveId(String? id) async => activeId = id;

  @override
  Future<void> clear() async {
    profiles = const [];
    activeId = null;
  }
}

/// Профиль владельца: своя подписка, импортированная по ссылке, когда она была
/// здорова. Панельной сессии за ним нет.
final _profile = ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Моя подписка',
  source: 'https://app.exarobot.top/sub/feb7e480',
  rawConfig: 'proxies: [{name: DE Stealth}]',
  format: 'clash',
  servers: const <ImportedServer>[
    ImportedServer(
      id: 'de-reality',
      name: 'DE Stealth',
      type: 'vless',
      server: 'de.example',
      port: 443,
      country: 'DE',
    ),
  ],
  serversUpdatedMs: DateTime.now().millisecondsSinceEpoch,
);

/// Дневная норма исчерпана — ровно то, что панель сообщает кодом 3003.
const _dailyQuota = AccessState(
  mayConnect: false,
  kind: AccessKind.dailyQuota,
  st: 7,
  rc: 3003,
  usedBytes: 263 * 1024 * 1024,
  limitBytes: 200 * 1024 * 1024,
  period: 'day',
);

Widget _home({AccessVerdict verdict = AccessVerdict.unknown}) {
  return ProviderScope(
    overrides: [
      vpnConnectionProvider.overrideWithValue(_ConnectedCore()),
      connectionProfilesStoreProvider.overrideWithValue(
        _FakeProfilesStore(<ConnectionProfile>[_profile], _profile.id),
      ),
      // Сеть в тесте не поднимаем: сторожу отдаётся готовый вердикт, ровно тот,
      // который он получил бы от подписки.
      accessGuardProvider.overrideWith(
        (ref) => AccessGuard(
          check: (_) async => verdict,
          initial: verdict,
          first: const Duration(days: 1),
          every: const Duration(days: 1),
        ),
      ),
    ],
    child: MaterialApp(theme: AppTheme.dark(), home: const HomeScreen()),
  );
}

void _usePhoneView(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 1688)
    ..devicePixelRatio = 2
    ..viewPadding = const FakeViewPadding(top: 94)
    ..padding = const FakeViewPadding(top: 94);
  addTearDown(tester.view.reset);
}

ConnectDial _dial(WidgetTester tester) =>
    tester.widget<ConnectDial>(find.byType(ConnectDial));

void main() {
  testWidgets('живой туннель без доступа не называется защитой', (
    tester,
  ) async {
    _usePhoneView(tester);
    await tester.pumpWidget(_home(verdict: AccessVerdict.refused(_dailyQuota)));
    await tester.pump();

    // Само слово, ради которого всё это.
    expect(find.text('Защищено'), findsNothing);
    expect(find.text('Подключено, но доступ закрыт'), findsOneWidget);

    final dial = _dial(tester);
    expect(dial.stage, VpnStage.connected, reason: 'туннель и правда поднят');
    expect(dial.accessBlocked, isTrue);
    // Вместо таймера сессии под дайлом — причина. Идущие «01:43» и были тем,
    // что делало ложную защиту убедительной.
    expect(dial.subLabel, 'Дневной лимит израсходован');
    expect(dial.subLabel, isNot(matches(RegExp(r'^\d{2}:\d{2}$'))));

    // И объяснение с числами прямо под дайлом, а не за двумя переходами.
    expect(find.text('Лимит на сегодня закончился'), findsOneWidget);
    expect(
      find.textContaining('Сегодня израсходовано 263 МБ'),
      findsOneWidget,
    );
  });

  testWidgets('исправный туннель на простое остаётся защитой', (tester) async {
    // Ноль скачанных байт, ответа о доступе нет (телефон в кармане, сеть
    // молчит). Это НЕ повод объявлять поломку.
    _usePhoneView(tester);
    await tester.pumpWidget(_home());
    await tester.pump();

    expect(find.text('Защищено'), findsOneWidget);
    expect(find.text('Подключено, но доступ закрыт'), findsNothing);

    final dial = _dial(tester);
    expect(dial.accessBlocked, isFalse);
    // Под дайлом снова таймер сессии.
    expect(dial.subLabel, matches(RegExp(r'^\d{2}:\d{2}$')));

    // Никаких карточек про лимиты: сказать нечего.
    expect(find.text('Лимит на сегодня закончился'), findsNothing);
  });

  testWidgets('доступ есть — экран молчит о нём', (tester) async {
    _usePhoneView(tester);
    await tester.pumpWidget(_home(verdict: AccessVerdict.allowed()));
    await tester.pump();

    expect(find.text('Защищено'), findsOneWidget);
    expect(_dial(tester).accessBlocked, isFalse);
  });
}

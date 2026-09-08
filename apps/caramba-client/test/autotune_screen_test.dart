/// Экран автоподбора: что он показывает по итогу прохода.
///
/// Три шага-имитации с таймерами тут были ровно до этой правки, и главный
/// смысл этих тестов — не дать им вернуться: на экране обязаны быть ЧИСЛА
/// прохода (сколько проверено, сколько работает) и причина выбора, а не
/// анимация ожидания.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/features/autotune/autotune_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

import 'support/fake_core.dart';

/// Ядро, отдающее заранее заданный ответ замера.
class _ProbeCore extends FakeVpnCore {
  final List<ProbeResult> results;
  _ProbeCore(this.results);

  @override
  Future<List<ProbeResult>> probe({Duration timeout = Duration.zero}) async =>
      results;
}

class _Store implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles;
  String? activeId;
  _Store(this.profiles, this.activeId);

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

ConnectionProfile _rawProfile() => const ConnectionProfile(
  id: 'cp_raw',
  type: ProfileType.rawSub,
  displayName: 'Подписка',
  source: 'https://sub.example/x',
  rawConfig: 'proxies: []',
  servers: <ImportedServer>[
    ImportedServer(
      id: 'DE Stealth',
      name: 'DE Stealth',
      type: 'vless',
      server: 'de.example',
      port: 443,
      country: 'DE',
    ),
    ImportedServer(
      id: 'CA Speed',
      name: 'CA Speed',
      type: 'vless',
      server: 'ca.example',
      port: 443,
      country: 'CA',
    ),
  ],
);

/// Профиль с ЖИВЫМИ именами: один инбаунд обещает вход через Россию, второй —
/// нет, и оба стоят на одной канадской машине с одним адресом.
ConnectionProfile _relayNamedProfile() => const ConnectionProfile(
  id: 'cp_raw',
  type: ProfileType.rawSub,
  displayName: 'Подписка',
  source: 'https://sub.example/x',
  rawConfig: 'proxies: []',
  servers: <ImportedServer>[
    ImportedServer(
      id: '🇨🇦 Secure via 🇷🇺',
      name: '🇨🇦 Secure via 🇷🇺',
      type: 'vless',
      server: '158.69.213.88',
      port: 8443,
      country: 'CA',
    ),
    ImportedServer(
      id: '🇨🇦 Stealth',
      name: '🇨🇦 Stealth',
      type: 'vless',
      server: '158.69.213.88',
      port: 443,
      country: 'CA',
    ),
  ],
);

/// Профиль с БЛИЗНЕЦАМИ: те же адрес и порт под двумя именами. Так живая
/// подписка `sub 34` и выглядит после импортёра ядра — `detour` теряется, и
/// каждый инбаунд приезжает дважды одним и тем же подключением.
ConnectionProfile _twinProfile() => const ConnectionProfile(
  id: 'cp_raw',
  type: ProfileType.rawSub,
  displayName: 'Подписка',
  source: 'https://sub.example/x',
  rawConfig: 'proxies: []',
  servers: <ImportedServer>[
    ImportedServer(
      id: '🇨🇦 Stream via 🇷🇺',
      name: '🇨🇦 Stream via 🇷🇺',
      type: 'vless',
      server: '158.69.213.88',
      port: 10400,
      country: 'CA',
    ),
    ImportedServer(
      id: '🇨🇦 Stream',
      name: '🇨🇦 Stream',
      type: 'vless',
      server: '158.69.213.88',
      port: 10400,
      country: 'CA',
    ),
  ],
);

/// Ярлык и плоский узел, различающиеся в пределах шума одного замера.
const _tieResults = <ProbeResult>[
  ProbeResult(
    id: '🇨🇦 Secure via 🇷🇺',
    country: 'CA',
    latencyMs: 100,
    verdict: ProbeVerdict.ok,
  ),
  ProbeResult(
    id: '🇨🇦 Stealth',
    country: 'CA',
    latencyMs: 110,
    verdict: ProbeVerdict.ok,
  ),
];

Widget _app(List<ProbeResult> results, {_Store? store}) => ProviderScope(
  overrides: [
    vpnConnectionProvider.overrideWithValue(_ProbeCore(results)),
    connectionProfilesStoreProvider.overrideWithValue(
      store ?? _Store(<ConnectionProfile>[_rawProfile()], 'cp_raw'),
    ),
  ],
  child: MaterialApp(theme: AppTheme.dark(), home: const AutotuneScreen()),
);

void _tallView(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(900, 3600)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 8; i++) {
    await tester.pump(const Duration(milliseconds: 20));
  }
}

void main() {
  testWidgets('итог называет выбор, числа прохода и причину', (tester) async {
    _tallView(tester);
    await tester.pumpWidget(
      _app(const <ProbeResult>[
        // Быстрый узел, отвергнувший ключ. До вердиктов ядра он и был бы
        // «выбором»: у него есть время TCP.
        ProbeResult(
          id: 'DE Stealth',
          name: 'DE Stealth',
          country: 'DE',
          latencyMs: -1,
          tcpMs: 118,
          verdict: ProbeVerdict.authRejected,
        ),
        ProbeResult(
          id: 'CA Speed',
          name: 'CA Speed',
          country: 'CA',
          latencyMs: 179,
          verdict: ProbeVerdict.ok,
        ),
      ]),
    );
    await _settle(tester);

    expect(find.text('Выбрано'), findsOneWidget);
    // Выбран рабочий, а не быстрый мёртвый.
    expect(find.textContaining('Canada'), findsNothing);
    expect(find.textContaining('CA Speed'), findsWidgets);
    expect(find.textContaining('179 мс'), findsWidgets);
    // Числа прохода на экране, а не в логах.
    expect(find.text('Работает'), findsOneWidget);
    expect(find.text('1'), findsOneWidget);
    // Причина выбора названа словами.
    expect(find.textContaining('Лучший счёт'), findsOneWidget);
    // Мёртвый узел из списка не исчез — он назван причиной.
    expect(find.textContaining('ключ не принят'), findsOneWidget);
  });

  testWidgets('выбор закрепляется на профиле, а не остаётся на экране', (
    tester,
  ) async {
    _tallView(tester);
    final store = _Store(<ConnectionProfile>[_rawProfile()], 'cp_raw');
    const results = <ProbeResult>[
      ProbeResult(
        id: 'CA Speed',
        name: 'CA Speed',
        country: 'CA',
        latencyMs: 179,
        verdict: ProbeVerdict.ok,
      ),
    ];
    await tester.pumpWidget(_app(results, store: store));
    await _settle(tester);

    final saved = store.profiles.single;
    // Пин — то, чем `connectRaw` реально пользуется.
    expect(saved.selectedServerId, 'CA Speed');
    // И отдельно — запись о том, что так решил автоподбор: без неё строка
    // «Авто» не отличит свой выбор от ручного.
    expect(saved.autoPick?.proxyName, 'CA Speed');
    expect(saved.autoPick?.confirmed, isTrue);
  });

  testWidgets('без рабочих узлов экран называет класс отказа', (tester) async {
    _tallView(tester);
    await tester.pumpWidget(
      _app(const <ProbeResult>[
        ProbeResult(
          id: 'DE Stealth',
          latencyMs: -1,
          tcpMs: 118,
          verdict: ProbeVerdict.authRejected,
        ),
        ProbeResult(
          id: 'CA Speed',
          latencyMs: -1,
          tcpMs: 120,
          verdict: ProbeVerdict.authRejected,
        ),
      ]),
    );
    await _settle(tester);

    expect(find.text('Рабочего узла не нашлось'), findsOneWidget);
    // Не «проверьте сеть»: адреса ответили, ключ не приняли.
    expect(find.textContaining('Ключ подписки не принят'), findsOneWidget);
    expect(find.textContaining('Сеть тут ни при чём'), findsOneWidget);
  });

  testWidgets('пустой ответ ядра не выдаётся за «узлов нет»', (tester) async {
    _tallView(tester);
    await tester.pumpWidget(_app(const <ProbeResult>[]));
    await _settle(tester);

    expect(find.textContaining('Узлов для проверки нет'), findsOneWidget);
  });

  // Имена ниже сняты с живой подписки: sing-box-тело называет relay-узлы
  // `🇨🇦 Secure via 🇷🇺`, clash-тело — `🇨🇦 Secure ↪`, и ни в одном из них до
  // ядра не доезжает цепочка (`dialer-proxy` clash-генератор не выпускает,
  // `detour` теряется при переводе sing-box → clash в ядре).
  testWidgets('узел, чьё имя обещает вход, назван честно', (tester) async {
    _tallView(tester);
    final store = _Store(<ConnectionProfile>[_relayNamedProfile()], 'cp_raw');
    // Ярлык выигрывает с запасом — значит он и будет выбран, и экран обязан
    // объяснить его имя, а не тихо его переписать.
    const results = <ProbeResult>[
      ProbeResult(
        id: '🇨🇦 Secure via 🇷🇺',
        country: 'CA',
        latencyMs: 50,
        verdict: ProbeVerdict.ok,
      ),
      ProbeResult(
        id: '🇨🇦 Stealth',
        country: 'CA',
        latencyMs: 400,
        verdict: ProbeVerdict.ok,
      ),
    ];
    await tester.pumpWidget(_app(results, store: store));
    await _settle(tester);

    expect(store.profiles.single.autoPick?.proxyName, '🇨🇦 Secure via 🇷🇺');
    // В строке узла — то, что на проводе.
    expect(find.text('🇨🇦 Secure'), findsWidgets);
    // Имя оператора не потеряно: оно названо отдельной строкой...
    expect(find.text('Имя у оператора'), findsOneWidget);
    expect(find.text('🇨🇦 Secure via 🇷🇺'), findsOneWidget);
    // ...и расхождение объяснено словами, а не оставлено человеку на догадку.
    expect(find.textContaining('трафик идёт прямо на выход'), findsOneWidget);
  });

  testWidgets('при равном качестве выбирается узел без ярлыка', (tester) async {
    _tallView(tester);
    final store = _Store(<ConnectionProfile>[_relayNamedProfile()], 'cp_raw');
    await tester.pumpWidget(_app(_tieResults, store: store));
    await _settle(tester);

    expect(store.profiles.single.autoPick?.proxyName, '🇨🇦 Stealth');
    expect(find.textContaining('обещал именем вход'), findsOneWidget);
    // Проигравший ярлык из списка не исчез, и его имя названо целиком.
    expect(
      find.textContaining('«🇨🇦 Secure via 🇷🇺» — вход только в имени'),
      findsOneWidget,
    );
    // Объяснять имя ПРОИГРАВШЕГО карточка итога не обязана — она про выбор.
    expect(find.text('Имя у оператора'), findsNothing);
  });

  // ЗАМЕР С УСТРОЙСТВА. Два имени ОДНОГО провода: `158.69.213.88:10400`
  // дважды. Обещание в имени опровергнуто не порогом, а близнецом, и экран
  // обязан объяснить это фактом — иначе строка с лучшим числом, которую подбор
  // молча пропустил, выглядит как ошибка подбора.
  testWidgets('второе имя того же провода названо вторым именем', (
    tester,
  ) async {
    _tallView(tester);
    final store = _Store(<ConnectionProfile>[_twinProfile()], 'cp_raw');
    const results = <ProbeResult>[
      ProbeResult(
        id: '🇨🇦 Stream via 🇷🇺',
        country: 'CA',
        latencyMs: 138,
        verdict: ProbeVerdict.ok,
      ),
      ProbeResult(
        id: '🇨🇦 Stream',
        country: 'CA',
        latencyMs: 154,
        verdict: ProbeVerdict.ok,
      ),
    ];
    await tester.pumpWidget(_app(results, store: store));
    await _settle(tester);

    expect(store.profiles.single.autoPick?.proxyName, '🇨🇦 Stream');
    // Причина названа проводом, а не «разницей в пределах шума».
    expect(
      find.textContaining('вторым именем этого же подключения'),
      findsOneWidget,
    );
    // Проигравший из списка не исчез, и приписка называет его близнеца.
    expect(
      find.textContaining('«🇨🇦 Stream via 🇷🇺» — вход только в имени'),
      findsOneWidget,
    );
    expect(find.textContaining('что у «🇨🇦 Stream»'), findsOneWidget);
  });

  testWidgets('строки списка различаются, а числа не режутся', (tester) async {
    _tallView(tester);
    final store = _Store(<ConnectionProfile>[_relayNamedProfile()], 'cp_raw');
    await tester.pumpWidget(_app(_tieResults, store: store));
    await _settle(tester);

    // Раньше обе строки назывались «Канада · vless»: заголовком стояла машина,
    // а машина у шести инбаундов одна. Теперь строку называет сам узел.
    expect(find.text('🇨🇦 Stealth'), findsWidgets);
    expect(find.text('🇨🇦 Secure'), findsWidgets);
    // Два числа — две строки: «29 из 29 · работает 12» не помещалось и
    // обрывалось на «работае…».
    expect(find.text('Проверено'), findsOneWidget);
    expect(find.text('2 из 2'), findsOneWidget);
    expect(find.text('Работает'), findsOneWidget);
    // «29 из 29 · работает 12» одной строкой не помещалось и обрывалось.
    expect(find.textContaining('из 2 · работает'), findsNothing);
  });
}

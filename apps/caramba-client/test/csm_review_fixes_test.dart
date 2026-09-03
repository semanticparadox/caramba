// Регрессии, найденные тремя ревью. Каждый тест здесь падал бы до починки.

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/enrollment.dart';
import 'package:caramba_client/data/safe_url.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/router/deep_links.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';

class _MemoryStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles = <ConnectionProfile>[];
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
    profiles = <ConnectionProfile>[];
    activeId = null;
  }
}

const _pin = CsmPin(
  pid: '226e8a20f699b964',
  linkPin: '49Q8M87PK6WP9QXG3T30',
  origin: CsmPinOrigin.outOfBand,
  establishedMs: 1000,
);

Future<ProviderContainer> _boot(CsmProfileState? csm) async {
  final store = _MemoryStore()
    ..profiles = <ConnectionProfile>[
      ConnectionProfile(
        id: 'cp_1',
        type: ProfileType.panelAccount,
        displayName: 'Оператор',
        source: 'https://panel.example.net',
        csm: csm,
      ),
    ]
    ..activeId = 'cp_1';
  final container = ProviderContainer(
    overrides: <Override>[
      connectionProfilesStoreProvider.overrideWithValue(store),
    ],
  );
  addTearDown(container.dispose);
  container.read(connectionProfilesProvider);
  for (var i = 0; i < 100; i++) {
    if (!container.read(connectionProfilesProvider).loading) break;
    await Future<void>.delayed(Duration.zero);
  }
  return container;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('карве-аут по содержимому доезжает до боевого провайдера', () {
    test('бит без массива в каталоге читается нулём', () async {
      final c = await _boot(
        const CsmProfileState(
          pin: _pin,
          stage: CsmProfileStage.trusted,
          // Оператор поднял зеркала (бит 4) и ресурсы (бит 6).
          directiveCapabilities: CsmCapabilitySet(0x50),
          catalogContent: CsmCatalogContent(known: true),
        ),
      );
      final caps = c.read(csmCapabilitiesProvider);
      expect(caps.has(CsmCapability.mirrorPool), isFalse);
      expect(caps.has(CsmCapability.resourceHashes), isFalse);

      // И ступень зеркал называет причину, а не предлагает пустой пул.
      final ladder = c.read(csmLadderProvider);
      final mirrors = ladder.rungs.firstWhere((r) => r.rung == CsmRung.mirrors);
      expect(mirrors.reason, CsmUnavailableReason.notOfferedByOperator);
    });

    test('бит с массивом остаётся поднятым', () async {
      final c = await _boot(
        const CsmProfileState(
          pin: _pin,
          stage: CsmProfileStage.trusted,
          directiveCapabilities: CsmCapabilitySet(0x50),
          catalogContent: CsmCatalogContent(
            known: true,
            hasMirrors: true,
            hasResources: true,
          ),
        ),
      );
      final caps = c.read(csmCapabilitiesProvider);
      expect(caps.has(CsmCapability.mirrorPool), isTrue);
      expect(caps.has(CsmCapability.resourceHashes), isTrue);
    });
  });

  test('«Принять новое» двигает и настройку CSM, и конфиг ядра', () async {
    final c = await _boot(
      const CsmProfileState(
        pin: _pin,
        stage: CsmProfileStage.trusted,
        pendingChanges: <CsmPendingChange>[
          CsmPendingChange(
            id: 'card-1',
            raisedMs: 1000,
            items: <CsmCardItem>[
              CsmCardItem(
                key: CsmSettingKey.killSwitch,
                current: CsmBoolean(true),
                proposed: CsmBoolean(false),
                src: CsmProvenance.operator,
                trigger: CsmCardTrigger.narrowing,
              ),
            ],
          ),
        ],
      ),
    );
    expect(c.read(coreConfigProvider).killSwitch, isTrue);

    await c.read(csmNotifierProvider).revertCard('card-1');

    // Настройка CSM переехала...
    final entry = c.read(csmProfileStateProvider)!.settings.entries[
      CsmSettingKey.killSwitch
    ];
    expect((entry!.value as CsmBoolean).value, isFalse);
    expect(entry.userSet, isFalse);
    // ...и локальная политика вместе с ней. Одно значение на настройку: иначе
    // тег происхождения говорит одно, а ядро применяет другое.
    expect(c.read(coreConfigProvider).killSwitch, isFalse);
  });

  test('маршруты, которых оператор не предлагает, видны и выключены', () async {
    final c = await _boot(
      const CsmProfileState(
        pin: _pin,
        stage: CsmProfileStage.trusted,
        catalogContent: CsmCatalogContent(
          known: true,
          offeredRoutes: <String>['ru-smart', 'unknown-preset-v2'],
        ),
      ),
    );
    final modes = c.read(routingModesProvider);
    final disabled = c.read(csmDisabledRoutePresetsProvider);
    final offeredIndex = modes.indexWhere((m) => m.id == 'ru-smart');
    expect(offeredIndex, isNonNegative);
    expect(disabled.containsKey(offeredIndex), isFalse);
    // Всё остальное из локальной девятки видно и выключено с причиной.
    expect(disabled.length, modes.length - 1);
    // А маршрут оператора, которого не знает эта версия, назван, а не спрятан.
    expect(c.read(csmUnimplementedRoutesProvider), <String>['unknown-preset-v2']);
  });

  test('без проверенного каталога пикер не сужается', () async {
    final c = await _boot(
      const CsmProfileState(pin: _pin, stage: CsmProfileStage.trusted),
    );
    expect(c.read(csmDisabledRoutePresetsProvider), isEmpty);
  });

  group('02-SPEC.md 7.4: выбор, которого в каталоге нет', () {
    test('нерешаемый exit и relay названы, а не подменены молча', () async {
      final c = await _boot(
        const CsmProfileState(
          pin: _pin,
          stage: CsmProfileStage.trusted,
          selection: CsmSelectionState(
            exit: 'n404i1',
            relay: 'r404i1',
            relayCountry: 'NL',
          ),
          catalogContent: CsmCatalogContent(
            known: true,
            offeredExits: <String>['n17i3'],
            offeredRelays: <String>['r1i1'],
          ),
        ),
      );
      final out = c.read(csmUnresolvableSelectionsProvider);
      expect(out.map((e) => e.field), <String>['sel.exit', 'sel.relay']);
      expect(out.first.value, 'n404i1');
    });

    test('до проверки каталога предикат не вычисляется', () async {
      final c = await _boot(
        const CsmProfileState(
          pin: _pin,
          stage: CsmProfileStage.trusted,
          selection: CsmSelectionState(exit: 'n404i1'),
        ),
      );
      expect(c.read(csmUnresolvableSelectionsProvider), isEmpty);
    });

    test('выбор, который каталог предлагает, уведомления не поднимает', () async {
      final c = await _boot(
        const CsmProfileState(
          pin: _pin,
          stage: CsmProfileStage.trusted,
          selection: CsmSelectionState(exit: 'n17i3'),
          catalogContent: CsmCatalogContent(
            known: true,
            offeredExits: <String>['n17i3'],
          ),
        ),
      );
      expect(c.read(csmUnresolvableSelectionsProvider), isEmpty);
    });
  });

  group('ссылка энроллмента с пином', () {
    test('deeplink доносит k до маршрута энроллмента', () {
      final target = DeepLinkHandler.targetOf(
        'carambaconnect://enroll?panel=https%3A%2F%2Fpanel.example.net'
        '&code=49Q8M87PK6WP9QXG3T30&k=49Q8M87PK6WP9QXG3T30',
      );
      expect(target, isNotNull);
      final uri = Uri.parse(target!);
      expect(uri.queryParameters['k'], '49Q8M87PK6WP9QXG3T30');
      expect(uri.queryParameters['panel'], 'https://panel.example.net');
    });

    test('ссылка без k разбирается по-старому и k не выдумывается', () {
      final target = DeepLinkHandler.targetOf(
        'carambaconnect://enroll?panel=https%3A%2F%2Fpanel.example.net'
        '&code=ABCD1234EFGH5678JKMN',
      );
      expect(target, isNotNull);
      expect(Uri.parse(target!).queryParameters.containsKey('k'), isFalse);
    });
  });

  group('внешние ссылки', () {
    test('пропускаются только https, mailto и onion по http', () {
      expect(csmSafeExternalUri('https://example.net/x'), isNotNull);
      expect(csmSafeExternalUri('mailto:a@example.net'), isNotNull);
      expect(csmSafeExternalUri('http://abc.onion/x'), isNotNull);
    });

    test('всё остальное отвергается', () {
      for (final bad in <String>[
        'javascript:alert(1)',
        'file:///etc/passwd',
        'intent://scan#Intent;scheme=zxing;end',
        'http://example.net/x',
        'example.net/x',
        '',
      ]) {
        expect(csmSafeExternalUri(bad), isNull, reason: bad);
      }
    });
  });

  test('INV-10: фильтр инертного текста не держится на списке доменов', () {
    for (final raw in <String>[
      'напишите на evil.co',
      'зайдите t.ly/abc',
      'откройте example.shop/promo',
      'адрес 203.0.113.10/pay',
      'admin@example.net',
      'https://panel.example.net/x',
    ]) {
      final out = csmInertText(raw);
      expect(out.contains('/'), isFalse, reason: raw);
      expect(out.contains('@'), isFalse, reason: raw);
      expect(RegExp(r'\w\.\w').hasMatch(out), isFalse, reason: raw);
    }
    // Обычный текст не калечится.
    expect(csmInertText('Профилактика в воскресенье'), 'Профилактика в воскресенье');
  });

  test('INV-8 в точке ввода: http-панель отвергается сразу', () {
    expect(EnrollLink.normalizePanelUrl('http://panel.example.net'), isNull);
    expect(
      EnrollLink.normalizePanelUrl('https://panel.example.net'),
      'https://panel.example.net',
    );
    // Луковые адреса самоаутентичны и остаются единственным исключением.
    expect(
      EnrollLink.normalizePanelUrl('http://abcdefgh.onion'),
      'http://abcdefgh.onion',
    );
  });
}

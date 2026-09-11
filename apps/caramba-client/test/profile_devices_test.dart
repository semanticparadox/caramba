// Секция устройств в профиле: имя, платформа, «это устройство», переименование
// и отвязка.
//
// ЗАЧЕМ ЭТИ ПРОВЕРКИ. Список устройств — единственное место, где человек
// управляет привязками, и раньше он умел ровно одно: удалить строку по
// корзине. Три вещи, без которых список бесполезен, зафиксированы здесь:
//
//   1. своё устройство отличимо от чужого (иначе «отвязать» это лотерея);
//   2. переименование доезжает и до панели, и до МЕСТНОГО имени — иначе
//      заголовок следующего запроса вернёт панели прежнее имя и отменит
//      переименование само собой;
//   3. отвязка своего устройства спрашивает подтверждение: она обрывает
//      туннель здесь и сейчас.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart' hide Family;
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/features/profile/profile_screen.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/device_identity.dart';
import 'package:caramba_client/theme/app_theme.dart';

const String _myId = 'a1b2c3d4-0000-4000-8000-000000000000';
const String _otherId = 'ffffffff-0000-4000-8000-000000000000';

/// Устройства без панели: `AsyncNotifier` строится своим `build()`, поэтому
/// подменяется он. Вызовы запоминаются — проверяем, ЧТО экран попросил сделать.
class _FakeDevices extends DevicesNotifier {
  _FakeDevices(this.devices);

  final List<Device> devices;
  final List<(int, String)> renamed = <(int, String)>[];
  final List<int> removed = <int>[];

  @override
  Future<List<Device>> build() async => devices;

  @override
  Future<void> rename(int id, String name) async => renamed.add((id, name));

  @override
  Future<void> remove(int id) async => removed.add(id);
}

Device _device({
  required int id,
  required String name,
  String platform = 'android',
  String clientDeviceId = _myId,
  bool isCurrent = false,
}) => Device.fromJson(<String, dynamic>{
  'id': id,
  'display_name': name,
  'platform': platform,
  'client_device_id': clientDeviceId,
  'last_seen_at': DateTime.now().toUtc().toIso8601String(),
  'is_current': isCurrent,
});

Widget _app({
  required List<Device> devices,
  required _FakeDevices notifier,
  required DeviceIdentityStore identity,
  required DeviceIdentity mine,
}) => ProviderScope(
  overrides: <Override>[
    devicesProvider.overrideWith(() => notifier),
    deviceIdentityStoreProvider.overrideWithValue(identity),
    deviceIdentityProvider.overrideWith((ref) async => mine),
  ],
  child: MaterialApp(
    theme: AppTheme.dark(),
    home: Scaffold(
      body: SingleChildScrollView(
        child: ProfileDevicesSection(devices: devices),
      ),
    ),
  ),
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late DeviceIdentityStore identity;
  late DeviceIdentity mine;

  setUp(() async {
    // Устройство «уже знакомо»: идентификатор лежит в хранилище, и ровно он
    // стоит в лизах, которые отдаёт панель.
    FlutterSecureStorage.setMockInitialValues(<String, String>{
      DeviceIdentityStore.idKey: _myId,
      DeviceIdentityStore.nameKey: 'Телефон Артёма',
    });
    identity = DeviceIdentityStore(platform: 'android', hostname: 'localhost');
    mine = await identity.ensure();
  });

  testWidgets('своё устройство помечено, чужое нет', (tester) async {
    final devices = <Device>[
      _device(id: 1, name: 'Телефон Артёма'),
      _device(id: 2, name: 'iMac', platform: 'macos', clientDeviceId: _otherId),
    ];
    await tester.pumpWidget(
      _app(
        devices: devices,
        notifier: _FakeDevices(devices),
        identity: identity,
        mine: mine,
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Телефон Артёма'), findsOneWidget);
    expect(find.text('iMac'), findsOneWidget);
    // Tag печатает капсом — ищем то, что видит человек, и ровно один раз.
    expect(find.text('ЭТО УСТРОЙСТВО'), findsOneWidget);
    // Платформа подписана словами, а не кодом панели.
    expect(find.textContaining('Android'), findsOneWidget);
    expect(find.textContaining('Mac'), findsWidgets);
    expect(find.textContaining('macos'), findsNothing);
  });

  testWidgets('переименование доезжает и до панели, и до местного имени', (
    tester,
  ) async {
    final devices = <Device>[_device(id: 1, name: 'Телефон Артёма')];
    final notifier = _FakeDevices(devices);
    await tester.pumpWidget(
      _app(
        devices: devices,
        notifier: notifier,
        identity: identity,
        mine: mine,
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Переименовать'));
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField), 'Рабочий телефон');
    await tester.tap(find.text('Сохранить'));
    await tester.pumpAndSettle();

    expect(notifier.renamed, <(int, String)>[(1, 'Рабочий телефон')]);
    // Местное имя обязано поехать следом: иначе заголовок следующего запроса
    // вернёт панели прежнее имя.
    expect(identity.cached?.displayName, 'Рабочий телефон');
    expect(find.text('Имя устройства изменено'), findsOneWidget);
  });

  testWidgets('чужое устройство отвязывается сразу', (tester) async {
    final devices = <Device>[
      _device(id: 2, name: 'iMac', platform: 'macos', clientDeviceId: _otherId),
    ];
    final notifier = _FakeDevices(devices);
    await tester.pumpWidget(
      _app(
        devices: devices,
        notifier: notifier,
        identity: identity,
        mine: mine,
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Отвязать'));
    await tester.pumpAndSettle();

    expect(notifier.removed, <int>[2]);
    expect(find.text('Устройство отвязано'), findsOneWidget);
  });

  testWidgets('своё устройство отвязывается только с подтверждением', (
    tester,
  ) async {
    final devices = <Device>[_device(id: 1, name: 'Телефон Артёма')];
    final notifier = _FakeDevices(devices);
    await tester.pumpWidget(
      _app(
        devices: devices,
        notifier: notifier,
        identity: identity,
        mine: mine,
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Отвязать'));
    await tester.pumpAndSettle();

    // Спрашиваем до того, как оборвать туннель на этом же устройстве.
    expect(find.text('Отвязать это устройство'), findsOneWidget);
    expect(notifier.removed, isEmpty);

    await tester.tap(find.text('Отмена'));
    await tester.pumpAndSettle();
    expect(notifier.removed, isEmpty);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Отвязать'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, 'Отвязать'));
    await tester.pumpAndSettle();

    expect(notifier.removed, <int>[1]);
  });

  testWidgets('пустой список говорит словами, а не пустотой', (tester) async {
    await tester.pumpWidget(
      _app(
        devices: const <Device>[],
        notifier: _FakeDevices(const <Device>[]),
        identity: identity,
        mine: mine,
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Подключённых устройств нет'), findsOneWidget);
  });
}

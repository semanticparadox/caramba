// Сцена «devices»: Профиль → Устройства: список с именами и платформами,
// переименование своего устройства, отвязка чужого.
//
// Только с --dart-define=CARAMBA_DEMO=1; см. support/demo_render.dart.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart' hide Family;
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/features/profile/profile_screen.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/device_identity.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/widgets/ui.dart';

import 'support/demo_render.dart';

const String _myId = 'a1b2c3d4-0000-4000-8000-000000000000';

/// Устройства без панели: переименование и отвязка правят список в памяти.
class _DemoDevices extends DevicesNotifier {
  _DemoDevices(this.devices);

  List<Device> devices;

  @override
  Future<List<Device>> build() async => devices;

  @override
  Future<void> rename(int id, String name) async {
    devices = <Device>[
      for (final d in devices)
        if (d.id == id)
          Device(
            id: d.id,
            subscriptionId: d.subscriptionId,
            name: name,
            icon: d.icon,
            lastIp: d.lastIp,
            userAgent: d.userAgent,
            lastSeenAt: d.lastSeenAt,
            online: d.online,
            platform: d.platform,
            clientDeviceId: d.clientDeviceId,
            isCurrent: d.isCurrent,
          )
        else
          d,
    ];
    state = AsyncData(devices);
  }

  @override
  Future<void> remove(int id) async {
    devices = devices.where((d) => d.id != id).toList(growable: false);
    state = AsyncData(devices);
  }
}

Device _device({
  required int id,
  required String name,
  required String platform,
  required Duration seenAgo,
  bool online = false,
  bool current = false,
}) =>
    Device.fromJson(<String, dynamic>{
      'id': id,
      'display_name': name,
      'platform': platform,
      'client_device_id': current ? _myId : 'dev-$id',
      'last_seen_at':
          DateTime.now().toUtc().subtract(seenAgo).toIso8601String(),
      'is_current': current,
      'online': online,
    });

class _Host extends ConsumerWidget {
  const _Host();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final devices = ref.watch(devicesProvider).valueOrNull ?? const <Device>[];
    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: ListView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s12,
          ),
          children: [
            const ScreenHead('Профиль'),
            ProfileDevicesSection(devices: devices),
          ],
        ),
      ),
    );
  }
}

void main() {
  testWidgets('devices', (tester) async {
    await loadDemoFonts();
    demoPhone(tester);
    final rec = DemoRecorder(tester, 'devices');

    FlutterSecureStorage.setMockInitialValues(<String, String>{
      DeviceIdentityStore.idKey: _myId,
      DeviceIdentityStore.nameKey: 'iPhone',
    });
    final identity = DeviceIdentityStore(platform: 'ios', hostname: 'iphone');
    final mine = await identity.ensure();

    final devices = <Device>[
      _device(
        id: 1,
        name: 'iPhone',
        platform: 'ios',
        seenAgo: Duration.zero,
        online: true,
        current: true,
      ),
      _device(
        id: 2,
        name: 'MacBook Pro',
        platform: 'macos',
        seenAgo: const Duration(minutes: 12),
      ),
      _device(
        id: 3,
        name: 'Рабочий ПК',
        platform: 'windows',
        seenAgo: const Duration(hours: 3),
      ),
      _device(
        id: 4,
        name: 'Старый планшет',
        platform: 'android',
        seenAgo: const Duration(days: 9),
      ),
    ];
    final notifier = _DemoDevices(devices);

    await tester.pumpWidget(
      rec.wrap(
        ProviderScope(
          overrides: <Override>[
            devicesProvider.overrideWith(() => notifier),
            deviceIdentityStoreProvider.overrideWithValue(identity),
            deviceIdentityProvider.overrideWith((ref) async => mine),
          ],
          child: MaterialApp(
            debugShowCheckedModeBanner: false,
            theme: demoTheme(),
            home: const _Host(),
          ),
        ),
      ),
    );
    await rec.settle();
    await rec.shot(1500);

    // Переименовать своё устройство.
    await rec.tapAnimated(find.text('Переименовать').first, upSteps: 1);
    await rec.animate(steps: 5, step: const Duration(milliseconds: 60));
    await rec.shot(900);
    await tester.enterText(find.byType(TextField), '');
    await tester.pump();
    await rec.typeAnimated(find.byType(TextField), 'iPhone Артёма',
        delayMs: 80);
    await rec.tapAnimated(find.text('Сохранить'), upSteps: 1);
    await rec.animate(steps: 5, step: const Duration(milliseconds: 60));
    await rec.settle(pumps: 3);
    await rec.shot(1600);

    // Отвязать старый планшет (чужое устройство — без подтверждения).
    final unbind = find.widgetWithText(GhostButton, 'Отвязать').last;
    await tester.ensureVisible(unbind);
    await tester.pump();
    await rec.shot(700);
    await rec.tapAnimated(unbind, upSteps: 3);
    await rec.settle(pumps: 3);
    await rec.shot(2600);

    rec.finish();
    await demoTearDownTree(tester);
  }, skip: !demoEnabled, timeout: const Timeout(Duration(minutes: 5)));
}

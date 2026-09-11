// Баннер «нет прав на TUN» на главном экране десктопа.
//
// Ядро отказ в правах на tun не возвращает и рапортует «подключено» над
// мёртвым туннелем. Баннер обязан появиться до подключения, когда права нет
// и выбран TUN, назвать выход и одним нажатием перевести на прокси.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/desktop/tun_privilege.dart';
import 'package:caramba_client/features/home/tun_permission_banner.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

import '../support/fake_core.dart';

Future<ProviderContainer> _pump(
  WidgetTester tester, {
  required TunPrivilege privilege,
  TunnelMode mode = TunnelMode.tun,
}) async {
  final container = ProviderContainer(
    overrides: <Override>[
      vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
      tunPrivilegeProvider.overrideWith((ref) async => privilege),
    ],
  );
  addTearDown(container.dispose);
  container.read(tunnelModeProvider.notifier).hydrate(mode);
  await tester.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        theme: AppTheme.dark(),
        home: const Scaffold(body: TunPermissionBanner()),
      ),
    ),
  );
  await tester.pump();
  await tester.pump();
  return container;
}

/// Платформа для виджета. Возврат в `finally`, а не в `tearDown`: flutter_test
/// проверяет отладочные переменные foundation ещё до tearDown.
Future<void> _on(TargetPlatform platform, Future<void> Function() body) async {
  debugDefaultTargetPlatformOverride = platform;
  try {
    await body();
  } finally {
    debugDefaultTargetPlatformOverride = null;
  }
}

void main() {
  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

  group('shouldShowTunPermissionBanner', () {
    const idle = VpnStatus.disconnected();

    test('только TUN без права', () {
      expect(
        shouldShowTunPermissionBanner(
          mode: TunnelMode.tun,
          privilege: TunPrivilege.missing,
          status: idle,
        ),
        isTrue,
      );
      expect(
        shouldShowTunPermissionBanner(
          mode: TunnelMode.proxy,
          privilege: TunPrivilege.missing,
          status: idle,
        ),
        isFalse,
        reason: 'прокси прав не требует',
      );
      expect(
        shouldShowTunPermissionBanner(
          mode: TunnelMode.tun,
          privilege: TunPrivilege.granted,
          status: idle,
        ),
        isFalse,
      );
      expect(
        shouldShowTunPermissionBanner(
          mode: TunnelMode.tun,
          privilege: TunPrivilege.unknown,
          status: idle,
        ),
        isFalse,
        reason: 'не зная, не пугаем',
      );
    });

    test('отказ ядра в правах тоже показывает баннер', () {
      expect(
        shouldShowTunPermissionBanner(
          mode: TunnelMode.tun,
          privilege: TunPrivilege.unknown,
          status: const VpnStatus(
            stage: VpnStage.error,
            detail: 'open /dev/net/tun: permission denied',
          ),
        ),
        isTrue,
      );
      expect(
        shouldShowTunPermissionBanner(
          mode: TunnelMode.tun,
          privilege: TunPrivilege.unknown,
          status: const VpnStatus(stage: VpnStage.error, detail: '403'),
        ),
        isFalse,
      );
    });
  });

  group('TunPermissionBanner', () {
    testWidgets(
      'на Linux без права называет install.sh и переводит на прокси',
      (tester) async {
        await _on(TargetPlatform.linux, () async {
          final container = await _pump(
            tester,
            privilege: TunPrivilege.missing,
          );

          expect(find.textContaining('install.sh'), findsOneWidget);
          await tester.tap(find.text(kTunPermissionSwitchLabel));
          await tester.pump();

          expect(container.read(tunnelModeProvider), TunnelMode.proxy);
          expect(find.textContaining('install.sh'), findsNothing);
        });
      },
    );

    testWidgets('на Windows текст про адаптер, без install.sh', (tester) async {
      await _on(TargetPlatform.windows, () async {
        await _pump(tester, privilege: TunPrivilege.missing);

        expect(find.textContaining('install.sh'), findsNothing);
        expect(find.textContaining('TUN-адаптер'), findsOneWidget);
      });
    });

    testWidgets('с правом баннера нет', (tester) async {
      await _on(TargetPlatform.linux, () async {
        await _pump(tester, privilege: TunPrivilege.granted);

        expect(find.text(kTunPermissionSwitchLabel), findsNothing);
      });
    });

    testWidgets('на мобильном виджет пуст', (tester) async {
      await _pump(tester, privilege: TunPrivilege.missing);

      expect(find.text(kTunPermissionSwitchLabel), findsNothing);
    });
  });
}

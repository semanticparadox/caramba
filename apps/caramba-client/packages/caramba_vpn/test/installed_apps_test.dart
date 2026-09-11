/// Перечисление установленных приложений: разбор ответа канала и платформенный
/// гейт.
///
/// ЗАЧЕМ ТЕСТ. Из этого списка человек собирает правила по приложениям, и
/// ошибка разбора здесь не видна ничем: экран просто оказывается пустым или
/// теряет часть приложений, а выглядит это как «на моём телефоне ничего нет»,
/// а не как поломка моста. Вторая половина — гейт платформы: на десктопе и iOS
/// метода на той стороне НЕТ ВОВСЕ, и вызов обязан вернуть пустой список, а не
/// уронить экран исключением канала.
library;

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel('com.caramba/vpn');
  late TestDefaultBinaryMessenger messenger;
  final calls = <String>[];
  Object? reply;

  setUp(() {
    calls.clear();
    reply = null;
    messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(channel, (call) async {
      calls.add(call.method);
      return reply;
    });
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(channel, null);
  });

  group('разбор ответа канала', () {
    test('полная запись доезжает целиком', () {
      final apps = installedAppsFromChannel(<Object?>[
        <Object?, Object?>{
          'packageName': 'org.telegram.messenger',
          'label': 'Telegram',
          'iconPng': Uint8List.fromList(<int>[1, 2, 3]),
        },
      ]);

      expect(apps, hasLength(1));
      expect(apps.single.packageName, 'org.telegram.messenger');
      expect(apps.single.label, 'Telegram');
      expect(apps.single.iconPng, isNotNull);
      expect(apps.single.iconPng!.toList(), <int>[1, 2, 3]);
    });

    test('порядок платформы сохраняется', () {
      // Сортирует Kotlin, один раз и без учёта регистра. Вторая сортировка в
      // Dart разошлась бы с первой на ровном месте.
      final apps = installedAppsFromChannel(<Object?>[
        <Object?, Object?>{'packageName': 'b.app', 'label': 'Aaa'},
        <Object?, Object?>{'packageName': 'a.app', 'label': 'Bbb'},
      ]);

      expect(apps.map((a) => a.packageName).toList(), <String>[
        'b.app',
        'a.app',
      ]);
    });

    test('без ярлыка показывается имя пакета', () {
      final apps = installedAppsFromChannel(<Object?>[
        <Object?, Object?>{'packageName': 'com.example.app', 'label': '  '},
      ]);

      expect(apps.single.label, 'com.example.app');
    });

    test('приложение без иконки остаётся в списке', () {
      final apps = installedAppsFromChannel(<Object?>[
        <Object?, Object?>{'packageName': 'com.example.app', 'label': 'App'},
      ]);

      expect(apps, hasLength(1));
      expect(apps.single.iconPng, isNull);
    });

    test('битая запись пропускается, остальные остаются', () {
      final apps = installedAppsFromChannel(<Object?>[
        'мусор',
        <Object?, Object?>{'label': 'без пакета'},
        <Object?, Object?>{'packageName': '   '},
        <Object?, Object?>{'packageName': 'com.example.app', 'label': 'App'},
      ]);

      expect(apps.map((a) => a.packageName).toList(), <String>[
        'com.example.app',
      ]);
    });

    test('ответ не списком — пустой список', () {
      expect(installedAppsFromChannel(null), isEmpty);
      expect(installedAppsFromChannel(<String, Object?>{}), isEmpty);
    });

    test('тождество — имя пакета, а не ярлык', () {
      // Смена языка системы меняет ярлык; выбор человека при этом тот же.
      const a = InstalledApp(packageName: 'com.example.app', label: 'App');
      const b = InstalledApp(
        packageName: 'com.example.app',
        label: 'Программа',
      );
      expect(a, equals(b));
      expect(<InstalledApp>{a, b}, hasLength(1));
    });
  });

  group('CarambaVpn.listInstalledApps', () {
    test('на Android спрашивает канал и разбирает ответ', () async {
      reply = <Object?>[
        <Object?, Object?>{'packageName': 'com.example.app', 'label': 'App'},
      ];

      final apps = await CarambaVpn.instance.listInstalledApps(
        platform: TargetPlatform.android,
        isWeb: false,
      );

      expect(calls, <String>['listInstalledApps']);
      expect(apps.single.packageName, 'com.example.app');
    });

    for (final platform in <TargetPlatform>[
      TargetPlatform.iOS,
      TargetPlatform.macOS,
      TargetPlatform.windows,
      TargetPlatform.linux,
    ]) {
      test('на $platform канал не трогается вовсе', () async {
        final apps = await CarambaVpn.instance.listInstalledApps(
          platform: platform,
          isWeb: false,
        );

        expect(apps, isEmpty);
        expect(
          calls,
          isEmpty,
          reason: 'нативной ветки на этой платформе нет — спрашивать нечего',
        );
      });
    }

    test('в вебе канал не трогается вовсе', () async {
      final apps = await CarambaVpn.instance.listInstalledApps(
        platform: TargetPlatform.android,
        isWeb: true,
      );

      expect(apps, isEmpty);
      expect(calls, isEmpty);
    });

    test(
      'сборка без ветки моста даёт пустой список, а не исключение',
      () async {
        messenger.setMockMethodCallHandler(channel, (call) async {
          calls.add(call.method);
          throw MissingPluginException('no such method');
        });

        final apps = await CarambaVpn.instance.listInstalledApps(
          platform: TargetPlatform.android,
          isWeb: false,
        );

        expect(apps, isEmpty);
        expect(calls, <String>['listInstalledApps']);
      },
    );
  });
}

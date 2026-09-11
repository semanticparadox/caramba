// Профиль на десктопе: две колонки и кнопка по содержимому.
//
// Проверяется ровно то, из-за чего десктопная раскладка вообще заводится:
//
//   1. Пустое состояние без аккаунта панели больше не растягивает кнопку на всю
//      ширину окна. На 1280 мобильный `FilledButton` в `ListView` занимал всю
//      строку и читался как баннер, а не как действие; здесь он ограничен
//      содержимым (min 200x44) и заведомо уже левой колонки в 400.
//   2. С сессией обе колонки живут ОДНОВРЕМЕННО: деньги слева (Баланс,
//      Подписки), использование справа (Устройства, Рефералы). Мобильная лента
//      показывала их на разных экранах прокрутки.
//
// Платформа задаётся через `TargetPlatformVariant.only(macOS)`: десктопные
// ветки выбираются платформой, а не шириной, и протечь в остальные 975 тестов
// они не должны. Вариант выставляет `debugDefaultTargetPlatformOverride` перед
// телом теста и снимает его после — руками этого делать нельзя: flutter_test
// проверяет обнуление СРАЗУ по выходу из тела, до `tearDown`. `tearDown` ниже
// оставлен страховкой на случай, если override выставит кто-то ещё.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart' hide Family;
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/branding.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/data/models/user.dart';
import 'package:caramba_client/features/profile/profile_desktop.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/notifications_state.dart';
import 'package:caramba_client/state/tickets_state.dart';
import 'package:caramba_client/theme/app_theme.dart';

/// Сессия подменяется целиком: настоящий [AuthNotifier] в конструкторе лезет в
/// secure storage и в панель, а тесту нужна только стадия и пользователь.
/// `noSuchMethod` закрывает остальной интерфейс: ни один из его методов на этом
/// экране не вызывается, а вызовись он — тест упадёт громко, а не тихо.
class _FakeAuth extends StateNotifier<AuthState> implements AuthNotifier {
  _FakeAuth(super.state);

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

/// Устройства без панели: `AsyncNotifier` строится своим `build()`, поэтому
/// подменяется он, а не результат.
class _FakeDevices extends DevicesNotifier {
  final List<Device> devices;
  _FakeDevices(this.devices);

  @override
  Future<List<Device>> build() async => devices;
}

const _user = User(
  id: 46,
  username: 'exarobot',
  email: 'owner@example.com',
  balanceCents: 1240,
);

const _referral = ReferralInfo(
  code: 'EXARO7K2',
  invited: 4,
  balanceEarnedCents: 3600,
  shareLink: 'https://exarobot.top/r/EXARO7K2',
);

const _device = Device(
  id: 7,
  name: 'MacBook Pro',
  icon: 'laptop',
  online: true,
);

/// Свежая подписка free-тарифа в том виде, в каком её отдаёт
/// `/app/subscriptions` (см. `profile_subscription_card_test.dart`).
SubPlan _freeSub() => SubPlan.fromJson(<String, dynamic>{
  'id': 27,
  'plan_name': 'Free',
  'kind': 'free',
  'status': 'active',
  'used_traffic_bytes': 0,
  'traffic_limit_bytes': 209715200,
  'quota_period': 'day',
  'is_free': true,
  'daily_traffic_mb': 200,
  'device_used': 1,
  'device_limit': 1,
});

Widget _app({required bool signedIn}) => ProviderScope(
  overrides: <Override>[
    authProvider.overrideWith(
      (ref) => _FakeAuth(
        signedIn
            ? const AuthState(stage: AuthStage.authenticated, user: _user)
            : const AuthState(stage: AuthStage.unauthenticated),
      ),
    ),
    subscriptionsProvider.overrideWith((ref) async => <SubPlan>[_freeSub()]),
    devicesProvider.overrideWith(() => _FakeDevices(<Device>[_device])),
    referralProvider.overrideWith((ref) async => _referral),
    isPartnerProvider.overrideWithValue(false),
    // Бейдж уведомлений и карточка отказа тянут свои ветки панели; экрану
    // профиля от них нужны только число и ссылка на оплату.
    unreadCountProvider.overrideWithValue(0),
    // Бейдж тикетов опрашивает панель таймером; в тесте ему нечего считать.
    unreadTicketsCountProvider.overrideWithValue(0),
    subscriptionAccessProvider.overrideWithValue(null),
    activeBrandingProvider.overrideWithValue(Branding.fallback),
  ],
  child: MaterialApp(
    theme: AppTheme.dark(),
    home: const ProfileDesktopScreen(),
  ),
);

/// Окно десктопа: раскладка выбирается платформой, но ширина всё равно должна
/// быть настоящей, иначе две колонки не поместятся и тест поймает переполнение
/// вместо раскладки.
void _useDesktopView(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(1280, 800)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

/// Панельные провайдеры асинхронные: даём им долететь.
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump();
  }
}

void main() {
  tearDown(() => debugDefaultTargetPlatformOverride = null);

  testWidgets(
    'без сессии: кнопка по содержимому, а не во всю ширину',
    (tester) async {
      _useDesktopView(tester);
      await tester.pumpWidget(_app(signedIn: false));
      await _settle(tester);

      expect(tester.takeException(), isNull);
      expect(find.text('Аккаунта панели пока нет'), findsOneWidget);

      final button = find.widgetWithText(FilledButton, 'Подключить панель');
      expect(button, findsOneWidget);

      // Заявленный минимум действия: 200x44. На экране он приезжает меньше на
      // адаптивную плотность macOS (VisualDensity.compact, -8 по обеим осям) —
      // ровно так же, как все остальные кнопки приложения на этой платформе,
      // поэтому фиксируем НАМЕРЕНИЕ в стиле, а на рендере проверяем главное:
      // кнопка уже колонки, а не во всю её ширину.
      final style = tester.widget<FilledButton>(button).style;
      expect(style?.minimumSize?.resolve(<WidgetState>{}), const Size(200, 44));

      final size = tester.getSize(button);
      expect(
        size.width,
        lessThan(400),
        reason: 'кнопка снова растянулась по ширине окна',
      );
      expect(size.width, greaterThanOrEqualTo(192));
      expect(size.height, greaterThanOrEqualTo(36));
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );

  testWidgets(
    'с сессией: обе колонки видны одновременно',
    (tester) async {
      _useDesktopView(tester);
      await tester.pumpWidget(_app(signedIn: true));
      await _settle(tester);

      expect(tester.takeException(), isNull);

      // `SectionTitle` печатает заголовок капсом — ищем то, что видит человек.
      for (final title in <String>[
        'БАЛАНС',
        'ПОДПИСКИ',
        'УСТРОЙСТВА',
        'РЕФЕРАЛЫ',
      ]) {
        expect(
          find.text(title),
          findsOneWidget,
          reason: 'раздел «$title» пропал',
        );
      }

      // Пустого состояния «нет аккаунта панели» на этом пути быть не должно.
      expect(find.text('Аккаунта панели пока нет'), findsNothing);
      // Данные аккаунта и секций доехали, а не остались в загрузке.
      expect(find.text('@exarobot'), findsOneWidget);
      expect(find.text('MacBook Pro'), findsOneWidget);
      expect(find.text('EXARO7K2'), findsOneWidget);

      // Колонки стоят рядом: деньги слева, использование справа.
      final balance = tester.getTopLeft(find.text('БАЛАНС'));
      final devices = tester.getTopLeft(find.text('УСТРОЙСТВА'));
      expect(
        devices.dx,
        greaterThan(balance.dx),
        reason: 'правая колонка съехала под левую',
      );
    },
    variant: TargetPlatformVariant.only(TargetPlatform.macOS),
  );
}

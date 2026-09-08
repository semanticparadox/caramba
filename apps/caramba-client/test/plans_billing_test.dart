// Экран тарифов и оплата: что показывается, что уезжает на панель и о чём
// приложение говорит честно вместо того, чтобы догадаться.
//
// Живой проверки покупки в этой сессии не было и быть не могло: панель на zeus
// не перезапускается, маршрутов `/plans`, `/payment-methods` и
// `/purchase/{id}` на развёрнутой версии ещё нет. Поэтому здесь проверяется
// ровно то, что можно проверить без живой панели, — контракт: какие байты
// уходят в `POST /purchase`, как разбирается ответ, и что рисуется на данных,
// снятых с БОЕВОЙ базы этого оператора.
//
// Данные фикстур не выдуманы. У оператора три плана: Gold (безлимит, 3
// устройства) с двумя сроками — 180 дней за 1500 и 360 дней за 2500 центов;
// Starter (100 ГБ, 1 устройство) БЕЗ ЕДИНОЙ строки в `plan_durations`; Free с
// суточной нормой 200 МБ. Тесты ниже фиксируют, что Starter показывается без
// цены и без кнопки, а не «чинится» на клиенте.

import 'dart:convert';

import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/plan_catalog.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/features/billing/payment_sheet.dart';
import 'package:caramba_client/features/billing/plans_screen.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/subscription_state.dart';
import 'package:caramba_client/theme/app_theme.dart';

// ---------------------------------------------------------------------------
// Фикстуры: ровно та витрина, что лежит в базе оператора.
// ---------------------------------------------------------------------------

Map<String, dynamic> _catalogJson({
  bool inAppPurchase = false,
  String currency = 'USD',
  Map<String, dynamic>? pay,
  bool withStarter = true,
}) => <String, dynamic>{
  'currency': currency,
  'in_app_purchase': inAppPurchase,
  'pay':
      pay ??
      <String, dynamic>{
        'miniapp_url': null,
        'miniapp_native': null,
        'bot_url': 'https://t.me/exa_robot',
      },
  'plans': <Map<String, dynamic>>[
    <String, dynamic>{
      'id': 1,
      'name': 'Gold',
      'description': 'Все узлы, без лимита',
      'traffic_limit_gb': 0,
      'device_limit': 3,
      'is_free': false,
      'is_trial': false,
      'daily_traffic_mb': 0,
      'durations': <Map<String, dynamic>>[
        {'id': 8, 'duration_days': 360, 'price': 25.0, 'price_cents': 2500},
        {'id': 7, 'duration_days': 180, 'price': 15.0, 'price_cents': 1500},
      ],
    },
    if (withStarter)
      <String, dynamic>{
        'id': 2,
        'name': 'Starter',
        'description': '',
        'traffic_limit_gb': 100,
        'device_limit': 1,
        'is_free': false,
        'is_trial': false,
        'daily_traffic_mb': 0,
        // Ни одной строки цены — тариф не продаётся. Это состояние базы, а не
        // сломанный ответ.
        'durations': <Map<String, dynamic>>[],
      },
    <String, dynamic>{
      'id': 3,
      'name': 'Free',
      'description': '',
      'traffic_limit_gb': 0,
      'device_limit': 1,
      'is_free': true,
      'is_trial': false,
      'daily_traffic_mb': 200,
      'durations': <Map<String, dynamic>>[],
    },
  ],
};

SubPlan _freeSub() => SubPlan.fromJson(<String, dynamic>{
  'id': 34,
  'plan_name': 'Free',
  'kind': 'free',
  'status': 'active',
  'used_traffic_bytes': 1024,
  'traffic_limit_bytes': 209715200,
  'quota_period': 'day',
  'is_free': true,
  'daily_traffic_mb': 200,
  'device_used': 1,
  'device_limit': 1,
});

// ---------------------------------------------------------------------------
// Сетевой стаб: путь, метод и тело запроса записываются, ответ задаётся тестом.
// ---------------------------------------------------------------------------

class _Route {
  final int status;
  final String body;
  final String contentType;
  const _Route(this.status, this.body, {this.contentType = 'application/json'});
}

class _StubAdapter implements HttpClientAdapter {
  final Map<String, _Route> routes;
  final List<String> paths = <String>[];
  final List<String> methods = <String>[];
  final List<String> bodies = <String>[];
  final List<Map<String, dynamic>> queries = <Map<String, dynamic>>[];

  _StubAdapter(this.routes);

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<List<int>>? requestStream,
    Future<void>? cancelFuture,
  ) async {
    paths.add(options.path);
    methods.add(options.method);
    bodies.add(jsonEncode(options.data));
    queries.add(Map<String, dynamic>.from(options.queryParameters));
    final route =
        routes[options.path] ?? const _Route(404, 'Not Found', contentType: 'text/plain');
    return ResponseBody.fromString(
      route.body,
      route.status,
      headers: <String, List<String>>{
        Headers.contentTypeHeader: <String>[route.contentType],
      },
    );
  }

  @override
  void close({bool force = false}) {}
}

ApiClient _api(_StubAdapter adapter) {
  final dio = Dio(
    BaseOptions(
      baseUrl: 'https://panel.example/api/v2/app',
      validateStatus: (s) => s != null && s < 500,
    ),
  )..httpClientAdapter = adapter;
  return ApiClient(tokens: TokenStore(), dio: dio);
}

// ---------------------------------------------------------------------------
// Обёртка виджетов.
// ---------------------------------------------------------------------------

/// Виджет внутри готового Scaffold (лист оплаты, карточка тарифа).
Widget _app(Widget child, {List<Override> overrides = const <Override>[]}) =>
    _wrap(Scaffold(body: SingleChildScrollView(child: child)), overrides);

/// Полноэкранный виджет, у которого свой Scaffold (экран тарифов).
Widget _screen(Widget child, {List<Override> overrides = const <Override>[]}) =>
    _wrap(child, overrides);

Widget _wrap(Widget home, List<Override> overrides) => ProviderScope(
  overrides: <Override>[
    subscriptionAccessProvider.overrideWithValue(null),
    ...overrides,
  ],
  child: MaterialApp(theme: AppTheme.dark(), home: home),
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const secureStorage = MethodChannel(
    'plugins.it_nomads.com/flutter_secure_storage',
  );
  late TestDefaultBinaryMessenger messenger;

  setUp(() {
    messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(secureStorage, (call) async => null);
  });

  tearDown(() => messenger.setMockMethodCallHandler(secureStorage, null));

  // =========================================================================
  group('каталог: цену придумывает оператор, а не приложение', () {
    test('Gold покупается, Starter и Free — нет', () {
      final catalog = PlanCatalog.fromJson(_catalogJson());

      final byName = <String, CatalogPlan>{
        for (final p in catalog.plans) p.name: p,
      };
      expect(byName['Gold']!.purchasable, isTrue);
      expect(
        byName['Starter']!.purchasable,
        isFalse,
        reason: 'у Starter нет ни одной строки в plan_durations',
      );
      expect(
        byName['Free']!.purchasable,
        isFalse,
        reason: 'бесплатный план не продаётся никогда',
      );
      expect(catalog.anyPurchasable, isTrue);
    });

    test('цена читается из price_cents, а не из дробного price', () {
      final catalog = PlanCatalog.fromJson(_catalogJson());
      final gold = catalog.plans.firstWhere((p) => p.name == 'Gold');
      final half = gold.durations.firstWhere((d) => d.days == 180);
      expect(half.id, 7, reason: 'в purchase уезжает plan_durations.id');
      expect(half.priceMinor, 1500);
      expect(gold.cheapest!.days, 180);
    });

    test('покупаемые идут первыми, внутри — от дешёвого', () {
      final sorted = PlanCatalog.fromJson(_catalogJson()).sorted;
      expect(sorted.first.name, 'Gold');
      expect(
        sorted.map((p) => p.name).toList(),
        ['Gold', 'Starter', 'Free'],
        reason: 'сначала покупаемое, затем снятое с продажи, бесплатное — вниз',
      );
    });

    test('голый массив планов (старый мини-апповый ответ) тоже разбирается', () {
      final catalog = PlanCatalog.fromJson(
        (_catalogJson()['plans'] as List<Map<String, dynamic>>),
      );
      expect(catalog.plans, hasLength(3));
      expect(
        catalog.currency,
        isEmpty,
        reason: 'валюты в таком ответе нет — и мы её не выдумываем',
      );
      expect(catalog.inAppPurchase, isFalse);
    });

    test('пустой каталог не считается покупаемым', () {
      final catalog = PlanCatalog.fromJson(<String, dynamic>{'plans': <dynamic>[]});
      expect(catalog.plans, isEmpty);
      expect(catalog.anyPurchasable, isFalse);
    });
  });

  // =========================================================================
  group('деньги на экране', () {
    test('известная валюта получает символ', () {
      expect(formatMoneyMinor(1500, 'USD'), r'15 $');
      expect(formatMoneyMinor(2500, 'usd'), r'25 $');
      expect(formatMoneyMinor(199000, 'RUB'), '1990 ₽');
    });

    test('копейки не теряются', () {
      expect(formatMoneyMinor(1550, 'USD'), r'15.50 $');
    });

    test('незнакомая валюта печатается кодом, пустая — без символа', () {
      expect(formatMoneyMinor(1500, 'XTS'), '15 XTS');
      expect(
        formatMoneyMinor(1500, ''),
        '15',
        reason: 'подставленный доллару оператору в рублях — это ложь о сумме',
      );
      expect(formatMoneyMinor(1500, '').contains(r'$'), isFalse);
    });
  });

  // =========================================================================
  group('что уезжает на панель', () {
    test('purchase шлёт duration_id и provider ровно теми именами', () async {
      final adapter = _StubAdapter(<String, _Route>{
        '/purchase': const _Route(
          200,
          '{"pay_url":"https://pay.example/i/42","pay_url_kind":"absolute_url",'
              '"session_id":"3f1a","amount":1500,"amount_decimal":15.0,'
              '"currency":"USD","provider":"wata","fulfilled":false}',
        ),
      });
      final checkout = await _api(
        adapter,
      ).purchase(durationId: 7, provider: 'wata');

      expect(adapter.paths.single, '/purchase');
      expect(adapter.methods.single, 'POST');
      expect(adapter.bodies.single, '{"duration_id":7,"provider":"wata"}');
      expect(checkout.kind, PayUrlKind.absoluteUrl);
      expect(checkout.sessionId, '3f1a');
      expect(checkout.absoluteUrl('https://panel.example'), 'https://pay.example/i/42');
    });

    test('без провайдера поле provider не отправляется вовсе', () async {
      final adapter = _StubAdapter(<String, _Route>{
        '/purchase': const _Route(200, '{"pay_url":"SUCCESS","fulfilled":true}'),
      });
      await _api(adapter).purchase(durationId: 7);
      expect(
        adapter.bodies.single,
        '{"duration_id":7}',
        reason: 'панель сама берёт первый доступный провайдер',
      );
    });

    test('payment-methods спрашивается по duration_id', () async {
      final adapter = _StubAdapter(<String, _Route>{
        '/payment-methods': const _Route(
          200,
          '{"providers":[{"id":"stars","label":"Telegram Stars","amount":1500,'
              '"currency":"USD","checkout":"telegram",'
              '"url":"https://t.me/exa_robot"}]}',
        ),
      });
      final methods = await _api(adapter).getPaymentMethods(durationId: 7);

      expect(adapter.paths.single, '/payment-methods');
      expect(adapter.queries.single['duration_id'], 7);
      expect(methods.single.checkout, PayCheckout.telegram);
      expect(methods.single.amountMinor, 1500);
    });

    test('Stars без поля checkout всё равно не идёт через /purchase', () {
      // Мини-апповый список провайдеров понятия `checkout` не знает. Отправить
      // Stars в `POST /purchase` — гарантированный отказ «provider not found»:
      // StarsProvider в MarketplaceService не зарегистрирован.
      final m = PaymentMethod.fromJson(<String, dynamic>{
        'id': 'stars',
        'label': '⭐ Stars',
      });
      expect(m.checkout, PayCheckout.telegram);

      final card = PaymentMethod.fromJson(<String, dynamic>{
        'id': 'wata',
        'label': 'Card',
      });
      expect(card.checkout, PayCheckout.inApp);
    });
  });

  // =========================================================================
  group('ответы панели разбираются без догадок', () {
    test('оплата с баланса не превращается в ссылку', () {
      final c = PurchaseCheckout.fromJson(<String, dynamic>{
        'pay_url': 'SUCCESS',
        'pay_url_kind': 'balance_success',
        'session_id': 'aa',
        'fulfilled': true,
      });
      expect(c.kind, PayUrlKind.balanceSuccess);
      expect(
        c.absoluteUrl('https://panel.example'),
        isNull,
        reason: 'сентинел SUCCESS нельзя открывать во внешнем браузере',
      );
    });

    test('относительный путь достраивается базовым URL панели', () {
      final c = PurchaseCheckout.fromJson(<String, dynamic>{
        'pay_url': '/manual-upload?session=aa',
        'pay_url_kind': 'relative_path',
      });
      expect(
        c.absoluteUrl('https://panel.example'),
        'https://panel.example/manual-upload?session=aa',
      );
      expect(
        c.absoluteUrl(''),
        isNull,
        reason: 'панели нет — достраивать нечем, и это не повод открыть путь',
      );
    });

    test('старая панель без pay_url_kind классифицируется по строке', () {
      expect(
        PurchaseCheckout.fromJson(<String, dynamic>{'pay_url': 'SUCCESS'}).kind,
        PayUrlKind.balanceSuccess,
      );
      expect(
        PurchaseCheckout.fromJson(<String, dynamic>{
          'pay_url': 'https://pay.example',
        }).kind,
        PayUrlKind.absoluteUrl,
      );
      expect(
        PurchaseCheckout.fromJson(<String, dynamic>{'pay_url': '/manual'}).kind,
        PayUrlKind.relativePath,
      );
    });

    test('статус сессии переводится, а не печатается сырым', () {
      const paid = PurchaseStatus(status: 'completed');
      const pending = PurchaseStatus(status: 'pending');
      const dead = PurchaseStatus(status: 'expired');
      expect(paid.isPaid, isTrue);
      expect(paid.label, 'Оплачено');
      expect(pending.label, 'Ожидает оплаты');
      expect(dead.isPaid, isFalse);
      expect(
        dead.label.toLowerCase().contains('expired'),
        isFalse,
        reason: 'внутреннее слово панели наружу не выходит',
      );
    });

    test('403 на purchase — исключение с кодом, а не тихий null', () async {
      final adapter = _StubAdapter(<String, _Route>{
        '/purchase': const _Route(
          403,
          'End-user billing is not enabled for this license tier',
          contentType: 'text/plain',
        ),
      });
      await expectLater(
        _api(adapter).purchase(durationId: 7),
        throwsA(
          isA<ApiException>().having((e) => e.statusCode, 'statusCode', 403),
        ),
      );
    });

    test('404 на /plans — исключение с кодом, а не пустой каталог', () async {
      final adapter = _StubAdapter(<String, _Route>{});
      await expectLater(
        _api(adapter).getPlans(),
        throwsA(
          isA<ApiException>().having((e) => e.statusCode, 'statusCode', 404),
        ),
      );
    });

    test('без панели вызовы отказывают названной причиной', () async {
      final api = ApiClient(tokens: TokenStore(), baseUrl: '');
      await expectLater(api.getPlans(), throwsA(isA<Exception>()));
    });
  });

  // =========================================================================
  group('нативная ссылка оплаты', () {
    test('miniapp_native доезжает до модели отдельно от https-формы', () {
      final pay = AccessPay.fromJson(<String, dynamic>{
        'miniapp_url': 'https://t.me/exa_robot/app?startapp=plans',
        'miniapp_native':
            'tg://resolve?domain=exa_robot&appname=app&startapp=plans',
        'bot_url': 'https://t.me/exa_robot',
      });
      expect(pay.native, startsWith('tg://resolve'));
      expect(pay.link, startsWith('https://'));
      expect(pay.isEmpty, isFalse);
    });

    test('пустой брендинг оператора остаётся пустым', () {
      final pay = AccessPay.fromJson(<String, dynamic>{
        'miniapp_url': null,
        'miniapp_native': null,
        'bot_url': null,
      });
      expect(pay.link, isNull);
      expect(pay.native, isNull);
      expect(pay.isEmpty, isTrue);
    });
  });

  // =========================================================================
  group('экран тарифов', () {
    Widget screen(
      PlanCatalog catalog, {
      List<SubPlan> subs = const <SubPlan>[],
    }) => _screen(
      const PlansScreen(),
      overrides: <Override>[
        planCatalogProvider.overrideWith((ref) async => catalog),
        subscriptionsProvider.overrideWith((ref) async => subs),
      ],
    );

    testWidgets('Starter показан, но без цены и без кнопки', (tester) async {
      await tester.pumpWidget(screen(PlanCatalog.fromJson(_catalogJson())));
      await tester.pumpAndSettle();

      expect(find.text('Starter'), findsOneWidget);
      expect(
        find.textContaining('не продаётся'),
        findsOneWidget,
        reason: 'причина названа словами, а не спрятана',
      );
      // Ни одной цены у Starter быть не может: сроков нет, брать её неоткуда.
      expect(find.textContaining(r'100 ГБ'), findsOneWidget);
      // Кнопка «Продолжить» есть ровно одна — у Gold.
      expect(find.textContaining('Продолжить'), findsOneWidget);
    });

    testWidgets('у Gold видны оба срока с ценами', (tester) async {
      await tester.pumpWidget(screen(PlanCatalog.fromJson(_catalogJson())));
      await tester.pumpAndSettle();

      expect(find.text('180 дней'), findsOneWidget);
      expect(find.text('360 дней'), findsOneWidget);
      expect(find.text(r'15 $'), findsOneWidget);
      expect(find.text(r'25 $'), findsOneWidget);
      // Предвыбран дешёвый: кнопка называет именно его цену.
      expect(find.textContaining(r'Продолжить · 15 $'), findsOneWidget);
    });

    testWidgets('выбор дорогого срока меняет цену на кнопке', (tester) async {
      await tester.pumpWidget(screen(PlanCatalog.fromJson(_catalogJson())));
      await tester.pumpAndSettle();

      await tester.tap(find.text('360 дней'));
      await tester.pump();
      expect(find.textContaining(r'Продолжить · 25 $'), findsOneWidget);
    });

    testWidgets('выключенная лицензией оплата названа, а не спрятана', (
      tester,
    ) async {
      await tester.pumpWidget(
        screen(PlanCatalog.fromJson(_catalogJson(inAppPurchase: false))),
      );
      await tester.pumpAndSettle();

      expect(find.textContaining('не включена'), findsOneWidget);
      expect(find.textContaining('Telegram'), findsWidgets);
    });

    testWidgets('оператор без адреса оплаты: говорим словами, бота не выдумываем', (
      tester,
    ) async {
      await tester.pumpWidget(
        screen(
          PlanCatalog.fromJson(
            _catalogJson(
              pay: <String, dynamic>{
                'miniapp_url': null,
                'miniapp_native': null,
                'bot_url': null,
              },
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.textContaining('не опубликовал'), findsOneWidget);
      expect(
        find.textContaining('exa_robot'),
        findsNothing,
        reason: 'чужой бот в коде запрещён',
      );
    });

    testWidgets('витрина без единого покупаемого тарифа объясняет почему', (
      tester,
    ) async {
      final catalog = PlanCatalog.fromJson(
        <String, dynamic>{
          ..._catalogJson(),
          'plans': (_catalogJson()['plans'] as List)
              .where((p) => (p as Map)['name'] != 'Gold')
              .toList(),
        },
      );
      await tester.pumpWidget(screen(catalog));
      await tester.pumpAndSettle();

      expect(find.textContaining('срока продажи'), findsOneWidget);
      expect(find.textContaining('Продолжить'), findsNothing);
    });

    testWidgets('текущая подписка названа сверху из живых данных', (
      tester,
    ) async {
      await tester.pumpWidget(
        screen(PlanCatalog.fromJson(_catalogJson()), subs: <SubPlan>[_freeSub()]),
      );
      await tester.pumpAndSettle();

      final head = tester
          .widgetList<Text>(find.textContaining('Сейчас: '))
          .first
          .data!;
      expect(head, contains('Free'));
      expect(head, contains('в день'));
      expect(head, contains('1 устройство'));
    });

    testWidgets('«Продолжить» открывает лист оплаты с выбранным сроком', (
      tester,
    ) async {
      final adapter = _StubAdapter(<String, _Route>{
        '/payment-methods': const _Route(
          200,
          '{"providers":[{"id":"stars","label":"Telegram Stars",'
              '"checkout":"telegram"}]}',
        ),
      });
      await tester.pumpWidget(
        _screen(
          const PlansScreen(),
          overrides: <Override>[
            planCatalogProvider.overrideWith(
              (ref) async => PlanCatalog.fromJson(_catalogJson()),
            ),
            subscriptionsProvider.overrideWith((ref) async => <SubPlan>[]),
            apiClientProvider.overrideWithValue(_api(adapter)),
          ],
        ),
      );
      await tester.pumpAndSettle();

      await tester.tap(find.text('360 дней'));
      await tester.pump();
      await tester.tap(find.textContaining('Продолжить'));
      await tester.pumpAndSettle();

      expect(find.text('Способ оплаты'), findsOneWidget);
      expect(
        find.textContaining(r'Gold · 360 дней · 25 $'),
        findsOneWidget,
        reason: 'лист покупает именно тот срок, который выбран на карточке',
      );
      expect(
        adapter.queries.single['duration_id'],
        8,
        reason: 'способы спрашиваются по plan_durations.id выбранного срока',
      );
    });

    testWidgets('старая панель (404) не притворяется пустой витриной', (
      tester,
    ) async {
      await tester.pumpWidget(
        _screen(
          const PlansScreen(),
          overrides: <Override>[
            planCatalogProvider.overrideWith(
              (ref) async =>
                  throw const ApiException('Not Found', statusCode: 404),
            ),
            subscriptionsProvider.overrideWith((ref) async => <SubPlan>[]),
          ],
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('Витрина недоступна'), findsOneWidget);
      expect(
        find.textContaining('Тарифов нет'),
        findsNothing,
        reason: '«панель старее» и «тарифов нет» — разные вещи',
      );
    });
  });

  // =========================================================================
  group('лист оплаты', () {
    Widget sheet(
      PlanCatalog catalog,
      _StubAdapter adapter, {
      CatalogPlan? plan,
    }) {
      final p =
          plan ?? catalog.plans.firstWhere((e) => e.name == 'Gold');
      return _app(
        PaymentSheet(
          plan: p,
          duration: p.durations.firstWhere((d) => d.days == 180),
          catalog: catalog,
        ),
        overrides: <Override>[
          apiClientProvider.overrideWithValue(_api(adapter)),
        ],
      );
    }

    testWidgets('способ показывается даже когда он один', (tester) async {
      final adapter = _StubAdapter(<String, _Route>{
        '/payment-methods': const _Route(
          200,
          '{"providers":[{"id":"stars","label":"Telegram Stars",'
              '"amount":1500,"currency":"USD","checkout":"telegram"}]}',
        ),
      });
      await tester.pumpWidget(
        sheet(PlanCatalog.fromJson(_catalogJson()), adapter),
      );
      await tester.pumpAndSettle();

      expect(find.text('Telegram Stars'), findsOneWidget);
      expect(find.textContaining('откроется Telegram'), findsOneWidget);
      // Заголовок называет, ЧТО именно покупается.
      expect(find.textContaining(r'Gold · 180 дней · 15 $'), findsOneWidget);
    });

    testWidgets('403 читается как настройка оператора, а не как поломка', (
      tester,
    ) async {
      final adapter = _StubAdapter(<String, _Route>{
        '/payment-methods': const _Route(
          200,
          '{"providers":[{"id":"wata","label":"Карта","checkout":"in_app"}]}',
        ),
        '/purchase': const _Route(
          403,
          'End-user billing is not enabled for this license tier',
          contentType: 'text/plain',
        ),
      });
      await tester.pumpWidget(
        sheet(PlanCatalog.fromJson(_catalogJson()), adapter),
      );
      await tester.pumpAndSettle();

      await tester.tap(find.text('Карта'));
      await tester.pumpAndSettle();

      expect(find.textContaining('не включил оплату внутри приложения'), findsOneWidget);
      expect(
        find.textContaining('license'),
        findsNothing,
        reason: 'английский текст ошибки панели на экран не долетает',
      );
      expect(
        find.textContaining('403'),
        findsNothing,
        reason: 'кода состояния человек видеть не должен',
      );
    });

    testWidgets('баланс списался — открывать нечего, говорим об этом', (
      tester,
    ) async {
      final adapter = _StubAdapter(<String, _Route>{
        '/payment-methods': const _Route(
          200,
          '{"providers":[{"id":"balance","label":"С баланса (15.00)",'
              '"amount":1500,"currency":"USD","checkout":"in_app"}]}',
        ),
        '/purchase': const _Route(
          200,
          '{"pay_url":"SUCCESS","pay_url_kind":"balance_success",'
              '"session_id":"bb","amount":1500,"currency":"USD",'
              '"provider":"balance","fulfilled":true}',
        ),
      });
      await tester.pumpWidget(
        sheet(PlanCatalog.fromJson(_catalogJson()), adapter),
      );
      await tester.pumpAndSettle();

      expect(find.textContaining('спишется с баланса сразу'), findsOneWidget);
      await tester.tap(find.textContaining('С баланса'));
      await tester.pumpAndSettle();

      expect(adapter.bodies.last, '{"duration_id":7,"provider":"balance"}');
      expect(find.textContaining('Подписка продлена'), findsOneWidget);
    });

    testWidgets('панель без ссылки: не молчим и не открываем пустоту', (
      tester,
    ) async {
      final adapter = _StubAdapter(<String, _Route>{
        '/payment-methods': const _Route(
          200,
          '{"providers":[{"id":"manual","label":"Вручную","checkout":"in_app"}]}',
        ),
        // Провайдер вернул чек-аут без адреса: панель это допускает, а мы обязаны
        // не отправить пустую строку в launchUrl.
        '/purchase': const _Route(
          200,
          '{"pay_url":"","pay_url_kind":"","session_id":"cc","fulfilled":false}',
        ),
      });
      await tester.pumpWidget(
        sheet(PlanCatalog.fromJson(_catalogJson()), adapter),
      );
      await tester.pumpAndSettle();

      await tester.tap(find.text('Вручную'));
      await tester.pumpAndSettle();

      expect(find.textContaining('не дала адрес оплаты'), findsOneWidget);
    });

    testWidgets('английский отказ панели на экран не долетает', (tester) async {
      final adapter = _StubAdapter(<String, _Route>{
        '/payment-methods': const _Route(
          200,
          '{"providers":[{"id":"wata","label":"Карта","checkout":"in_app"}]}',
        ),
        '/purchase': const _Route(
          400,
          'Unknown or disabled payment provider',
          contentType: 'text/plain',
        ),
      });
      await tester.pumpWidget(
        sheet(PlanCatalog.fromJson(_catalogJson()), adapter),
      );
      await tester.pumpAndSettle();

      await tester.tap(find.text('Карта'));
      await tester.pumpAndSettle();

      for (final word in <String>['Unknown', 'disabled', 'provider', '400']) {
        expect(
          find.textContaining(word),
          findsNothing,
          reason: 'на экране осталось внутреннее слово панели «$word»',
        );
      }
      expect(find.textContaining('Обновите витрину'), findsOneWidget);
    });

    testWidgets('после ухода в браузер состояние спрашивается, а не угадывается', (
      tester,
    ) async {
      // url_launcher в тестах живого канала не имеет: подменяем его, иначе
      // отказ открывалки не отличить от отказа панели.
      const launcher = MethodChannel('plugins.flutter.io/url_launcher');
      final launched = <String>[];
      messenger.setMockMethodCallHandler(launcher, (call) async {
        if (call.method == 'launch') {
          launched.add((call.arguments as Map)['url'] as String);
        }
        return true;
      });
      addTearDown(() => messenger.setMockMethodCallHandler(launcher, null));

      final adapter = _StubAdapter(<String, _Route>{
        '/payment-methods': const _Route(
          200,
          '{"providers":[{"id":"wata","label":"Карта","checkout":"in_app"}]}',
        ),
        '/purchase': const _Route(
          200,
          '{"pay_url":"https://pay.example/i/42",'
              '"pay_url_kind":"absolute_url","session_id":"3f1a",'
              '"amount":1500,"currency":"USD","provider":"wata",'
              '"fulfilled":false}',
        ),
        '/purchase/3f1a': const _Route(
          200,
          '{"status":"completed","provider":"wata","amount":1500,'
              '"currency":"USD"}',
        ),
      });
      await tester.pumpWidget(
        sheet(PlanCatalog.fromJson(_catalogJson()), adapter),
      );
      await tester.pumpAndSettle();

      await tester.tap(find.text('Карта'));
      await tester.pumpAndSettle();

      // Ушли в браузер и честно сказали, что об оплате приложение не узнает.
      expect(launched, ['https://pay.example/i/42']);
      expect(find.text('Ожидает оплаты'), findsOneWidget);
      expect(find.textContaining('вернитесь сюда'), findsOneWidget);

      await tester.tap(find.textContaining('Обновить состояние'));
      await tester.pumpAndSettle();

      expect(
        adapter.paths.last,
        '/purchase/3f1a',
        reason: 'статус спрашивается по номеру сессии из ответа панели',
      );
      expect(find.text('Оплачено'), findsOneWidget);
    });

    testWidgets('способов нет вовсе — путь к оплате всё равно назван', (
      tester,
    ) async {
      final adapter = _StubAdapter(<String, _Route>{
        '/payment-methods': const _Route(200, '{"providers":[]}'),
      });
      await tester.pumpWidget(
        sheet(PlanCatalog.fromJson(_catalogJson()), adapter),
      );
      await tester.pumpAndSettle();

      expect(find.textContaining('оформляется в Telegram'), findsOneWidget);
      expect(find.text('Открыть Telegram'), findsOneWidget);
    });
  });
}

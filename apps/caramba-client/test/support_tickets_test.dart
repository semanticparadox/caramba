// Тикеты поддержки в приложении: живая переписка, переход из уведомления,
// категория в форме, бейдж непрочитанного (раунд 6, зона C).
//
// ЗАЧЕМ ЭТИ ПРОВЕРКИ. Владелец описал раздел как «работает криво», и разведка
// свела это к четырём разрывам, каждый из которых молчалив: ответ поддержки
// не появлялся, пока экран не переоткроют; тап по уведомлению «Ответ по
// тикету» ничего не открывал; «новое» на карточке не гасло от прочтения;
// категория из формы не уходила вовсе. Здесь каждый разрыв закреплён тестом
// на поведении, а не на наличии виджета:
//
//   1. экран тикета сам подтягивает ответ поддержки по таймеру, отмечает его
//      плашкой и перестаёт опрашивать завершённый тикет;
//   2. свайп вниз перечитывает переписку, возврат на список перечитывает список;
//   3. уведомление с ticket_id ведёт на экран тикета и гасится;
//   4. форма отправляет выбранную категорию тем значением, что ждёт панель;
//   5. бейдж в профиле считает тикеты с непрочитанным ответом.
//
// Сеть подменяется на уровне Dio-адаптера: тест видит, какие пути и тела
// ушли, и сам решает, что вернуть на каждый вызов.

import 'dart:convert';

import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/notification.dart';
import 'package:caramba_client/data/models/ticket.dart';
import 'package:caramba_client/data/models/user.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/features/notifications/notifications_screen.dart';
import 'package:caramba_client/features/support/new_ticket_screen.dart';
import 'package:caramba_client/features/support/ticket_badge.dart';
import 'package:caramba_client/features/support/ticket_detail_screen.dart';
import 'package:caramba_client/features/support/tickets_screen.dart';
import 'package:caramba_client/state/app_update_state.dart'
    show InstalledVersion;
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/tickets_state.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/widgets/ui.dart';

// ---------------------------------------------------------------------------
// Сетевой стаб: на каждый путь — очередь ответов; последний повторяется.
// ---------------------------------------------------------------------------

class _Route {
  final int status;
  final Object body;
  const _Route(this.status, this.body);
}

class _StubAdapter implements HttpClientAdapter {
  final Map<String, List<_Route>> routes;
  final Map<String, int> hits = <String, int>{};
  final List<String> bodies = <String>[];
  final Map<String, String> lastBodyByKey = <String, String>{};
  _StubAdapter(this.routes);

  int count(String method, String path) => hits['$method $path'] ?? 0;

  /// Тело последнего запроса по этому пути (JSON-строка) или null.
  String? bodyOf(String method, String path) => lastBodyByKey['$method $path'];

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<List<int>>? requestStream,
    Future<void>? cancelFuture,
  ) async {
    final key = '${options.method} ${options.path}';
    final n = hits[key] ?? 0;
    hits[key] = n + 1;
    final encoded = jsonEncode(options.data);
    bodies.add(encoded);
    lastBodyByKey[key] = encoded;
    final queue = routes[key];
    final route = (queue == null || queue.isEmpty)
        ? const _Route(404, 'Not Found')
        : queue[n < queue.length ? n : queue.length - 1];
    final isJson = route.body is! String;
    return ResponseBody.fromString(
      isJson ? jsonEncode(route.body) : route.body as String,
      route.status,
      headers: <String, List<String>>{
        Headers.contentTypeHeader: <String>[
          isJson ? 'application/json' : 'text/plain',
        ],
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
  // Версия приложения подставляется: настоящая идёт через плагин
  // package_info, которого в тестовом процессе нет.
  return ApiClient(
    tokens: TokenStore(),
    dio: dio,
    installedVersion: () async => InstalledVersion.unknown,
  );
}

/// Сессия подменяется целиком, как в profile_desktop_test: настоящий
/// [AuthNotifier] лезет в связку ключей и панель, а экранам нужна стадия.
class _FakeAuth extends StateNotifier<AuthState> implements AuthNotifier {
  _FakeAuth(super.state);

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

const _user = User(
  id: 46,
  username: 'exarobot',
  email: 'owner@example.com',
  balanceCents: 0,
);

List<Override> _overrides(_StubAdapter adapter) => <Override>[
  authProvider.overrideWith(
    (ref) =>
        _FakeAuth(const AuthState(stage: AuthStage.authenticated, user: _user)),
  ),
  apiClientProvider.overrideWithValue(_api(adapter)),
];

// ---------------------------------------------------------------------------
// Фикстуры в форме ответов панели (`app_support.rs`).
// ---------------------------------------------------------------------------

Map<String, dynamic> _msg(String author, String body) => <String, dynamic>{
  'author': author,
  'body': body,
  'created_at': '2026-09-11T10:00:00Z',
};

Map<String, dynamic> _detail({
  String status = 'open',
  required List<Map<String, dynamic>> messages,
}) => <String, dynamic>{
  'id': 1,
  'subject': 'Не подключается',
  'category': 'connection',
  'status': status,
  'created_at': '2026-09-11T09:00:00Z',
  'updated_at': '2026-09-11T10:00:00Z',
  'messages': messages,
};

Map<String, dynamic> _summary({
  int id = 1,
  bool unread = false,
  String status = 'open',
}) => <String, dynamic>{
  'id': id,
  'subject': 'Тикет #$id',
  'category': 'general',
  'status': status,
  'created_at': '2026-09-11T09:00:00Z',
  'updated_at': '2026-09-11T10:00:00Z',
  'unread_for_user': unread ? 1 : 0,
  'has_unread': unread,
};

Map<String, dynamic> _notif({
  required int id,
  String kind = 'support_ticket',
  int? ticketId,
  bool read = false,
}) => <String, dynamic>{
  'id': id,
  'title': 'Ответ по тикету #$ticketId',
  'body': 'Проверьте, пожалуйста, версию',
  'kind': kind,
  'created_at': '2026-09-11T10:00:00Z',
  'read': read,
  if (ticketId != null) 'ticket_id': ticketId,
  if (ticketId != null)
    'payload': <String, dynamic>{
      'ticket_id': ticketId,
      'url': '/support/$ticketId',
    },
};

Widget _screen(Widget home, _StubAdapter adapter) => ProviderScope(
  overrides: _overrides(adapter),
  child: MaterialApp(theme: AppTheme.dark(), home: home),
);

/// Провайдеры асинхронные, а ответы платформенного канала (связка ключей под
/// токеном и идентичностью устройства) приходят не микротаском, а следующим
/// тиком: пустой pump их не дожидается. Шесть коротких pump'ов дают запросу
/// долететь, почти не сдвигая таймер опроса (суммарно 120 мс).
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump(const Duration(milliseconds: 20));
  }
}

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
  group('модели: контракт панели читается без потерь', () {
    test('уведомление о тикете несёт ticket_id и открывает тикет', () {
      final n = AppNotification.fromJson(_notif(id: 5, ticketId: 7));
      expect(n.ticketId, 7);
      expect(n.opensTicket, isTrue);
      expect(n.payload?['url'], '/support/7');
      expect(
        n.copyWith(read: true).ticketId,
        7,
        reason: 'copyWith не теряет id',
      );
    });

    test('ticket_id берётся из url payload, когда явного поля нет', () {
      final n = AppNotification.fromJson(<String, dynamic>{
        'id': 9,
        'title': 't',
        'kind': 'support_ticket',
        'payload': <String, dynamic>{'url': '/support/13'},
      });
      expect(n.ticketId, 13);
    });

    test('уведомление без тикета не ведёт никуда', () {
      final n = AppNotification.fromJson(_notif(id: 2, kind: 'billing'));
      expect(n.ticketId, isNull);
      expect(n.opensTicket, isFalse);
    });

    test('сводка тикета: has_unread и unread_for_user согласованы', () {
      expect(TicketSummary.fromJson(_summary(unread: true)).hasUnread, isTrue);
      expect(
        TicketSummary.fromJson(_summary(unread: false)).hasUnread,
        isFalse,
      );
      // Панель без счётчика (мини-аппный контракт или старый DTO).
      final flagOnly = TicketSummary.fromJson(<String, dynamic>{
        'id': 3,
        'subject': 's',
        'has_unread': true,
      });
      expect(flagOnly.hasUnread, isTrue);
      final legacy = TicketSummary.fromJson(<String, dynamic>{
        'id': 3,
        'subject': 's',
        'unread': true,
      });
      expect(legacy.hasUnread, isTrue);
    });

    test(
      'категория: значения панели, подписи RU/EN, неизвестное -> general',
      () {
        expect(
          TicketCategory.parse('feature_request'),
          TicketCategory.featureRequest,
        );
        expect(TicketCategory.parse('nonsense'), TicketCategory.general);
        expect(TicketCategory.parse(null), TicketCategory.general);
        expect(TicketCategory.featureRequest.labelFor('en'), 'Feature request');
        expect(TicketCategory.featureRequest.labelFor('ru'), 'Предложение');
        // Список обязан совпадать с ALLOWED_TICKET_CATEGORIES панели.
        expect(TicketCategory.values.map((c) => c.value).toSet(), <String>{
          'general',
          'billing',
          'connection',
          'device',
          'feature_request',
          'technical',
          'other',
        });
      },
    );
  });

  // =========================================================================
  group('API: форма отправляет категорию', () {
    test('createTicket кладёт category в тело POST /tickets', () async {
      final adapter = _StubAdapter(<String, List<_Route>>{
        'POST /tickets': const <_Route>[
          _Route(200, <String, dynamic>{
            'id': 21,
            'subject': 's',
            'status': 'open',
          }),
        ],
      });
      final id = await _api(adapter).createTicket(
        subject: 'Оплата',
        message: 'Не прошёл платёж',
        category: 'billing',
      );
      expect(id, 21);
      final body = jsonDecode(adapter.bodies.single) as Map<String, dynamic>;
      expect(body['category'], 'billing');
      expect(body['subject'], 'Оплата');
    });

    testWidgets('экран нового запроса: выбор категории уезжает на панель', (
      tester,
    ) async {
      final adapter = _StubAdapter(<String, List<_Route>>{
        'POST /tickets': const <_Route>[
          _Route(200, <String, dynamic>{
            'id': 22,
            'subject': 's',
            'status': 'open',
          }),
        ],
        'GET /tickets': const <_Route>[_Route(200, <dynamic>[])],
        'GET /tickets/22': <_Route>[
          _Route(
            200,
            _detail(messages: <Map<String, dynamic>>[_msg('user', 'x')]),
          ),
        ],
      });
      final router = GoRouter(
        initialLocation: '/tickets/new',
        routes: <RouteBase>[
          GoRoute(
            path: '/tickets',
            builder: (_, __) => const TicketsScreen(),
            routes: <RouteBase>[
              GoRoute(path: 'new', builder: (_, __) => const NewTicketScreen()),
              GoRoute(
                path: ':id',
                builder: (_, s) => TicketDetailScreen(
                  ticketId: int.parse(s.pathParameters['id']!),
                ),
              ),
            ],
          ),
        ],
      );
      await tester.pumpWidget(
        ProviderScope(
          overrides: _overrides(adapter),
          child: MaterialApp.router(
            theme: AppTheme.dark(),
            routerConfig: router,
          ),
        ),
      );
      await _settle(tester);
      await tester.pump(const Duration(milliseconds: 400));
      expect(find.byType(NewTicketScreen), findsOneWidget);

      // Тап по текущему значению попадает в кнопку списка наверняка.
      await tester.tap(find.text('Общий вопрос'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      await tester.tap(find.text('Оплата и подписка').last);
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));

      await tester.enterText(find.byType(TextField).at(0), 'Тема');
      await tester.enterText(find.byType(TextField).at(1), 'Текст');
      await tester.pump();
      await tester.tap(find.text('Отправить'));
      await _settle(tester);

      expect(adapter.count('POST', '/tickets'), 1);
      final body =
          jsonDecode(adapter.bodyOf('POST', '/tickets')!)
              as Map<String, dynamic>;
      expect(body['category'], 'billing');
      expect(body['subject'], 'Тема');
    });
  });

  // =========================================================================
  group('экран тикета: переписка живая', () {
    testWidgets('ответ поддержки появляется по таймеру с плашкой', (
      tester,
    ) async {
      final adapter = _StubAdapter(<String, List<_Route>>{
        'GET /tickets/1': <_Route>[
          _Route(
            200,
            _detail(
              messages: <Map<String, dynamic>>[_msg('user', 'Не работает')],
            ),
          ),
          _Route(
            200,
            _detail(
              messages: <Map<String, dynamic>>[
                _msg('user', 'Не работает'),
                _msg('support', 'Обновите приложение'),
              ],
            ),
          ),
        ],
      });
      await tester.pumpWidget(
        _screen(const TicketDetailScreen(ticketId: 1), adapter),
      );
      await _settle(tester);

      expect(find.text('Не работает'), findsOneWidget);
      expect(find.text('Обновите приложение'), findsNothing);
      expect(find.text('Новый ответ поддержки'), findsNothing);
      expect(adapter.count('GET', '/tickets/1'), 1);

      // До срока опроса ничего не происходит.
      await tester.pump(const Duration(seconds: 10));
      expect(adapter.count('GET', '/tickets/1'), 1);

      await tester.pump(const Duration(seconds: 5));
      await _settle(tester);
      expect(adapter.count('GET', '/tickets/1'), 2);
      expect(find.text('Обновите приложение'), findsOneWidget);
      expect(find.text('Новый ответ поддержки'), findsOneWidget);

      // Плашка гаснет сама (4 с от прихода ответа, с запасом на settle).
      await tester.pump(const Duration(seconds: 5));
      await tester.pump();
      expect(find.text('Новый ответ поддержки'), findsNothing);

      // Тот же ответ второй раз плашку не зажигает.
      await tester.pump(const Duration(seconds: 15));
      await _settle(tester);
      expect(adapter.count('GET', '/tickets/1'), 3);
      expect(find.text('Новый ответ поддержки'), findsNothing);
    });

    testWidgets('завершённый тикет не опрашивается', (tester) async {
      final adapter = _StubAdapter(<String, List<_Route>>{
        'GET /tickets/1': <_Route>[
          _Route(
            200,
            _detail(
              status: 'resolved',
              messages: <Map<String, dynamic>>[_msg('support', 'Готово')],
            ),
          ),
        ],
      });
      await tester.pumpWidget(
        _screen(const TicketDetailScreen(ticketId: 1), adapter),
      );
      await _settle(tester);
      expect(find.text('Готово'), findsOneWidget);
      expect(find.byType(TextField), findsNothing, reason: 'композер скрыт');

      await tester.pump(const Duration(seconds: 45));
      await _settle(tester);
      expect(adapter.count('GET', '/tickets/1'), 1);
    });

    testWidgets('свайп вниз перечитывает переписку', (tester) async {
      final adapter = _StubAdapter(<String, List<_Route>>{
        'GET /tickets/1': <_Route>[
          _Route(
            200,
            _detail(messages: <Map<String, dynamic>>[_msg('user', 'Привет')]),
          ),
        ],
      });
      await tester.pumpWidget(
        _screen(const TicketDetailScreen(ticketId: 1), adapter),
      );
      await _settle(tester);
      expect(adapter.count('GET', '/tickets/1'), 1);

      await tester.fling(find.text('Привет'), const Offset(0, 300), 1000);
      await tester.pump();
      await tester.pump(const Duration(seconds: 1));
      await _settle(tester);
      expect(adapter.count('GET', '/tickets/1'), 2);
    });

    testWidgets('возврат на список перечитывает список', (tester) async {
      final adapter = _StubAdapter(<String, List<_Route>>{
        'GET /tickets': <_Route>[
          _Route(200, <dynamic>[_summary(unread: true)]),
          _Route(200, <dynamic>[_summary(unread: false)]),
        ],
        'GET /tickets/1': <_Route>[
          _Route(
            200,
            _detail(messages: <Map<String, dynamic>>[_msg('support', 'Ответ')]),
          ),
        ],
      });
      final router = GoRouter(
        initialLocation: '/tickets',
        routes: <RouteBase>[
          GoRoute(
            path: '/tickets',
            builder: (_, __) => const TicketsScreen(),
            routes: <RouteBase>[
              GoRoute(
                path: ':id',
                builder: (_, s) => TicketDetailScreen(
                  ticketId: int.parse(s.pathParameters['id']!),
                ),
              ),
            ],
          ),
        ],
      );
      await tester.pumpWidget(
        ProviderScope(
          overrides: _overrides(adapter),
          child: MaterialApp.router(
            theme: AppTheme.dark(),
            routerConfig: router,
          ),
        ),
      );
      await _settle(tester);
      expect(find.text('новое'), findsOneWidget, reason: 'бейдж до прочтения');

      await tester.tap(find.text('Тикет #1'));
      await _settle(tester);
      expect(find.text('Ответ'), findsOneWidget);

      // Даём переходу между страницами закончиться: пока он идёт, список
      // под экраном тикета ещё на сцене, и его крестик тоже IconBtn.
      await tester.pump(const Duration(milliseconds: 400));
      await tester.tap(
        find.descendant(
          of: find.byType(TicketDetailScreen),
          matching: find.byType(IconBtn),
        ),
      );
      await _settle(tester);
      await tester.pump(const Duration(milliseconds: 400));
      expect(adapter.count('GET', '/tickets'), 2, reason: 'список перечитан');
      expect(
        find.text('новое'),
        findsNothing,
        reason: 'панель сняла has_unread',
      );
    });
  });

  // =========================================================================
  group('уведомление ведёт к тикету', () {
    Widget app(_StubAdapter adapter) {
      final router = GoRouter(
        initialLocation: '/notifications',
        routes: <RouteBase>[
          GoRoute(
            path: '/notifications',
            builder: (_, __) => const NotificationsScreen(),
          ),
          GoRoute(
            path: '/tickets/:id',
            builder: (_, s) =>
                Scaffold(body: Text('TICKET ${s.pathParameters['id']}')),
          ),
        ],
      );
      return ProviderScope(
        overrides: _overrides(adapter),
        child: MaterialApp.router(theme: AppTheme.dark(), routerConfig: router),
      );
    }

    testWidgets(
      'тап по «Ответ по тикету» открывает тикет и гасит уведомление',
      (tester) async {
        final adapter = _StubAdapter(<String, List<_Route>>{
          'GET /notifications': <_Route>[
            _Route(200, <String, dynamic>{
              'notifications': <dynamic>[_notif(id: 5, ticketId: 7)],
              'unread_count': 1,
            }),
          ],
          'POST /notifications/5/read': const <_Route>[
            _Route(200, <String, dynamic>{'ok': true, 'unread_count': 0}),
          ],
        });
        await tester.pumpWidget(app(adapter));
        await _settle(tester);
        expect(find.text('Ответ по тикету #7'), findsOneWidget);

        await tester.tap(find.text('Ответ по тикету #7'));
        await _settle(tester);
        expect(find.text('TICKET 7'), findsOneWidget);
        expect(adapter.count('POST', '/notifications/5/read'), 1);
      },
    );

    testWidgets('прочитанное уведомление о тикете тоже открывает тикет', (
      tester,
    ) async {
      final adapter = _StubAdapter(<String, List<_Route>>{
        'GET /notifications': <_Route>[
          _Route(200, <String, dynamic>{
            'notifications': <dynamic>[_notif(id: 5, ticketId: 7, read: true)],
            'unread_count': 0,
          }),
        ],
      });
      await tester.pumpWidget(app(adapter));
      await _settle(tester);
      await tester.tap(find.text('Ответ по тикету #7'));
      await _settle(tester);
      expect(find.text('TICKET 7'), findsOneWidget);
      expect(adapter.count('POST', '/notifications/5/read'), 0);
    });

    testWidgets('уведомление без тикета только помечается прочитанным', (
      tester,
    ) async {
      final adapter = _StubAdapter(<String, List<_Route>>{
        'GET /notifications': <_Route>[
          _Route(200, <String, dynamic>{
            'notifications': <dynamic>[_notif(id: 6, kind: 'billing')],
            'unread_count': 1,
          }),
        ],
        'POST /notifications/6/read': const <_Route>[
          _Route(200, <String, dynamic>{'ok': true, 'unread_count': 0}),
        ],
      });
      await tester.pumpWidget(app(adapter));
      await _settle(tester);
      await tester.tap(find.text('Ответ по тикету #null'));
      await _settle(tester);
      expect(find.byType(NotificationsScreen), findsOneWidget);
      expect(adapter.count('POST', '/notifications/6/read'), 1);
    });
  });

  // =========================================================================
  group('бейдж в профиле', () {
    Widget badge(List<TicketSummary> tickets) => ProviderScope(
      overrides: <Override>[
        ticketsProvider.overrideWith((ref) async => tickets),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(),
        home: const Scaffold(body: Center(child: TicketsUnreadBadge())),
      ),
    );

    testWidgets('считает тикеты с непрочитанным ответом', (tester) async {
      await tester.pumpWidget(
        badge(<TicketSummary>[
          TicketSummary.fromJson(_summary(id: 1, unread: true)),
          TicketSummary.fromJson(_summary(id: 2, unread: false)),
          TicketSummary.fromJson(_summary(id: 3, unread: true)),
        ]),
      );
      await _settle(tester);
      expect(find.text('2'), findsOneWidget);
    });

    testWidgets('без непрочитанного бейджа нет', (tester) async {
      await tester.pumpWidget(
        badge(<TicketSummary>[TicketSummary.fromJson(_summary(id: 1))]),
      );
      await _settle(tester);
      expect(find.byType(Text), findsNothing);
    });
  });
}

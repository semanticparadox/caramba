// Путь «ссылка -> подключённая панель» целиком, от диплинка до профиля.
//
// Он существует потому, что прежний путь был тупиком: единственным входом в
// панель был экран «введите инвайт-код», кодов на живой панели ноль, выпускать
// их было нечем, а кнопка QR показывала тост. Владелец вставил ссылку своей
// подписки и получил требование кода, которого не существует. Тесты здесь
// стерегут ровно это: ссылка ведёт на подтверждение, подтверждение выдаёт
// сессию и профиль, а импорт подписки больше никого не отправляет за кодом.

import 'package:dio/dio.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/auth_tokens.dart';
import 'package:caramba_client/features/enroll/connect_controller.dart';
import 'package:caramba_client/features/enroll/connect_link.dart';
import 'package:caramba_client/features/enroll/connect_redeem.dart';
import 'package:caramba_client/router/app_router.dart';
import 'package:caramba_client/router/deep_links.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';

/// Золотой вектор панели (тот же, что в connect_link_test.dart): origin
/// app.exarobot.top, код 000102..0f, оператор "Caramba Connect", срок
/// 1780000000.
const String _goldenLink =
    'caramba://connect?d=8D5320D405W1GT3MEHR76EHF5XGQ0W1ECNW62WKFC9QQ8BKMDXR0'
    '4M00041061050R3GG28A1C60T3GF0DQM6RBJC5PP4R908DQPWVK5CDT0A6KA32JG0063SHSG';

const int _goldenExpires = 1780000000;
const String _goldenOrigin = 'https://app.exarobot.top';
const String _goldenCode = '000102030405060708090a0b0c0d0e0f';

/// Отдаёт заданный ответ вместо сети. Тот же приём, что в enroll_controller_test.
class _StubAdapter implements HttpClientAdapter {
  _StubAdapter(this.status, this.body, {this.contentType});

  final int status;
  final String body;

  /// Панель отдаёт ошибки простым текстом, а успех — JSON. Подделываем это
  /// честно: с одним лишь JSON-заголовком тест не заметил бы, что разбор
  /// текстового тела роняет Dio и превращает 400 в «нет связи».
  final String? contentType;
  final List<RequestOptions> requests = <RequestOptions>[];

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<List<int>>? requestStream,
    Future<void>? cancelFuture,
  ) async {
    requests.add(options);
    return ResponseBody.fromString(
      body,
      status,
      headers: {
        Headers.contentTypeHeader: [contentType ?? Headers.jsonContentType],
      },
    );
  }

  @override
  void close({bool force = false}) {}
}

Dio _dio(_StubAdapter adapter) => Dio(
  BaseOptions(validateStatus: (s) => s != null && s < 500),
)..httpClientAdapter = adapter;

AuthTokens _tokens() => const AuthTokens(
  accessToken: 'acc-1',
  refreshToken: 'ref-1',
  tokenType: 'Bearer',
  expiresIn: 3600,
  userId: 46,
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late TestDefaultBinaryMessenger messenger;
  const secureStorage = MethodChannel(
    'plugins.it_nomads.com/flutter_secure_storage',
  );

  setUp(() {
    messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    // Хранилище держим в памяти, а не отвечаем null на всё: половина проверок
    // здесь именно о том, что сессия переживает появление профиля панели, а с
    // беспамятной заглушкой это невозможно увидеть.
    final storage = <String, String>{};
    messenger.setMockMethodCallHandler(secureStorage, (call) async {
      final args = (call.arguments as Map?)?.cast<String, Object?>() ?? {};
      final key = args['key'] as String?;
      switch (call.method) {
        case 'write':
          if (key != null) storage[key] = args['value'] as String? ?? '';
          return null;
        case 'read':
          return key == null ? null : storage[key];
        case 'containsKey':
          return key != null && storage.containsKey(key);
        case 'delete':
          storage.remove(key);
          return null;
        case 'deleteAll':
          storage.clear();
          return null;
        case 'readAll':
          return Map<String, String>.from(storage);
        default:
          return null;
      }
    });
  });

  tearDown(() => messenger.setMockMethodCallHandler(secureStorage, null));

  _loginByCodeTests();

  // --------------------------------------------------------------------------
  // Маршрутизация ссылки.
  // --------------------------------------------------------------------------

  group('диплинк', () {
    test('ссылка подключения ведёт на подтверждение, а не на инвайт-код', () {
      final target = DeepLinkHandler.targetOf(_goldenLink);
      expect(target, isNotNull);
      expect(Uri.parse(target!).path, AppRoute.connect);
      expect(Uri.parse(target).queryParameters['link'], _goldenLink);
      expect(
        target,
        isNot(contains(AppRoute.enroll)),
        reason: 'экран инвайт-кода это тупик, туда ссылка вести не должна',
      );
    });

    test('просроченная ссылка всё равно доезжает до экрана', () {
      // Срок здесь не проверяется намеренно: сказать «приглашение просрочено,
      // запросите новое» может только экран. Молчаливый отказ навигации
      // неотличим от зависшего приложения.
      final target = DeepLinkHandler.targetOf(_goldenLink);
      expect(target, isNotNull);
    });

    test('старые схемы продолжают работать', () {
      expect(
        Uri.parse(
          DeepLinkHandler.targetOf(
            'carambaconnect://enroll?panel=https://p.example&code=ABC',
          )!,
        ).path,
        AppRoute.enroll,
      );
      expect(
        Uri.parse(
          DeepLinkHandler.targetOf(
            'carambaconnect://import?url=https://sub.example/a',
          )!,
        ).path,
        AppRoute.connectionImport,
      );
    });

    test('чужая ссылка по-прежнему не наша', () {
      expect(DeepLinkHandler.targetOf('https://example.com/x'), isNull);
      expect(DeepLinkHandler.targetOf('caramba://something?d=X'), isNull);
    });
  });

  group('гейт роутера', () {
    test('подтверждение доступно без аккаунта', () {
      // Весь смысл ссылки в том, что аккаунт на панели у человека УЖЕ есть, а в
      // приложении сессии ещё нет. Уведи её гейт на /login, и ссылка снова
      // никуда не ведёт.
      for (final stage in <AuthStage>[
        AuthStage.unknown,
        AuthStage.unauthenticated,
        AuthStage.authenticating,
      ]) {
        expect(
          resolveRedirect(
            stage: stage,
            firstRun: true,
            bootReady: false,
            profilesReady: false,
            guest: false,
            location: '${AppRoute.connect}?link=$_goldenLink',
          ),
          isNull,
          reason: 'стадия $stage',
        );
      }
    });

    test('вошедшего с экрана подтверждения не выбрасывает', () {
      // Панелей бывает несколько: человек с сессией одной панели вправе
      // подключить вторую по ссылке, и гейт не должен его перехватывать.
      expect(
        resolveRedirect(
          stage: AuthStage.authenticated,
          firstRun: false,
          bootReady: true,
          profilesReady: true,
          guest: false,
          location: AppRoute.connect,
        ),
        isNull,
      );
    });
  });

  // --------------------------------------------------------------------------
  // Погашение.
  // --------------------------------------------------------------------------

  group('погашение кода', () {
    test('успех отдаёт сессию, подписку и имя оператора', () async {
      final adapter = _StubAdapter(200, '''
{"access_token":"acc-1","refresh_token":"ref-1","token_type":"Bearer",
 "expires_in":3600,"user_id":46,
 "subscription_url":"https://app.exarobot.top/sub/feb7e480",
 "subscription_uuid":"feb7e480","subscription_status":"active",
 "panel_name":"EXA ROBOT"}''');

      final result = await redeemConnectCode(
        origin: _goldenOrigin,
        code: _goldenCode,
        client: _dio(adapter),
      );

      expect(result.tokens.accessToken, 'acc-1');
      expect(result.tokens.userId, 46);
      expect(result.subscriptionUuid, 'feb7e480');
      expect(result.panelName, 'EXA ROBOT');
      expect(result.subscriptionReasonText, isNull);
      // Код уходит в проводной форме и ровно на тот origin, который назвала
      // ссылка, а не на активную панель.
      expect(
        adapter.requests.single.uri.toString(),
        '$_goldenOrigin/api/v2/app/enroll/redeem',
      );
      expect(adapter.requests.single.data, {'code': _goldenCode});
    });

    test('отсутствие подписки показывается причиной, а не выдуманным адресом', () async {
      final adapter = _StubAdapter(200, '''
{"access_token":"acc-1","refresh_token":"ref-1","token_type":"Bearer",
 "expires_in":3600,"user_id":46,
 "subscription_url":null,"subscription_uuid":null,"subscription_status":null,
 "subscription_reason":"subscription_domain_not_configured",
 "panel_name":"EXA ROBOT"}''');

      final result = await redeemConnectCode(
        origin: _goldenOrigin,
        code: _goldenCode,
        client: _dio(adapter),
      );

      expect(result.subscriptionUrl, isNull);
      expect(result.hasSubscription, isFalse);
      expect(result.subscriptionReasonText, contains('домен подписок'));
    });

    test('неизвестная причина не прячется', () async {
      final adapter = _StubAdapter(200, '''
{"access_token":"a","refresh_token":"r","token_type":"Bearer","expires_in":1,
 "user_id":1,"subscription_reason":"something_new","panel_name":"X"}''');

      final result = await redeemConnectCode(
        origin: _goldenOrigin,
        code: _goldenCode,
        client: _dio(adapter),
      );
      expect(result.subscriptionReasonText, contains('something_new'));
    });

    test('400 это одна причина на три случая, и приложение её не выдумывает', () async {
      final adapter = _StubAdapter(
        400,
        'Invalid or expired invite',
        contentType: 'text/plain; charset=utf-8',
      );
      await expectLater(
        redeemConnectCode(
          origin: _goldenOrigin,
          code: _goldenCode,
          client: _dio(adapter),
        ),
        throwsA(
          isA<ConnectRedeemException>().having(
            (e) => e.isInvalidCode,
            'isInvalidCode',
            isTrue,
          ),
        ),
      );
    });

    test('ответ без токенов это отказ, а не пустая сессия', () async {
      final adapter = _StubAdapter(200, '{"panel_name":"X"}');
      await expectLater(
        redeemConnectCode(
          origin: _goldenOrigin,
          code: _goldenCode,
          client: _dio(adapter),
        ),
        throwsA(isA<ConnectRedeemException>()),
      );
    });
  });

  // --------------------------------------------------------------------------
  // Поток целиком.
  // --------------------------------------------------------------------------

  group('поток подключения', () {
    ProviderContainer boot({
      required RedeemFn redeem,
      int nowSec = _goldenExpires - 600,
    }) {
      final container = ProviderContainer(
        overrides: [
          connectProvider.overrideWith(
            (ref) => ConnectNotifier(ref, now: () => nowSec, redeem: redeem),
          ),
        ],
      );
      addTearDown(container.dispose);
      // autoDispose живёт, пока на него подписан экран; в тесте — эта подписка.
      final sub = container.listen(connectProvider, (_, __) {});
      addTearDown(sub.close);
      return container;
    }

    RedeemFn okRedeem({String? uuid = 'feb7e480', String? reason}) =>
        ({required String origin, required String code}) async =>
            ConnectRedeemResult(
              tokens: _tokens(),
              panelName: 'EXA ROBOT',
              subscriptionUuid: uuid,
              subscriptionStatus: uuid == null ? null : 'active',
              subscriptionReason: reason,
            );

    test('ссылка ведёт на подтверждение с оператором и адресом', () {
      final container = boot(redeem: okRedeem());
      container.read(connectProvider.notifier).open(_goldenLink);

      final s = container.read(connectProvider);
      expect(s.stage, ConnectStage.confirm);
      expect(s.link!.origin, _goldenOrigin);
      expect(s.link!.operatorName, 'Caramba Connect');
      // Пока человек не подтвердил, профиля не появляется: подтверждение это
      // решение, а не формальность.
      expect(container.read(connectionProfilesProvider).profiles, isEmpty);
    });

    test('подтверждение заводит профиль панели и делает его активным', () async {
      final container = boot(redeem: okRedeem());
      final notifier = container.read(connectProvider.notifier);
      notifier.open(_goldenLink);
      await notifier.confirm();

      expect(container.read(connectProvider).stage, ConnectStage.done);
      final profiles = container.read(connectionProfilesProvider).profiles;
      expect(profiles, hasLength(1));
      final profile = profiles.single;
      expect(profile.isPanel, isTrue);
      expect(profile.panelUrl, _goldenOrigin);
      // Имя берём у панели по TLS, а не из неподписанной ссылки.
      expect(profile.displayName, 'EXA ROBOT');
      expect(profile.subscriptionUuid, 'feb7e480');
      expect(profile.accessToken, 'acc-1');
      expect(container.read(activeConnectionProfileProvider)?.id, profile.id);
    });

    test('аккаунт без подписки всё равно становится профилем панели', () async {
      final container = boot(
        redeem: okRedeem(uuid: null, reason: 'no_subscription_on_account'),
      );
      final notifier = container.read(connectProvider.notifier);
      notifier.open(_goldenLink);
      await notifier.confirm();

      final s = container.read(connectProvider);
      expect(s.stage, ConnectStage.done);
      expect(s.result!.subscriptionReasonText, isNotNull);
      final profile = container.read(connectionProfilesProvider).profiles.single;
      expect(profile.panelUrl, _goldenOrigin);
      expect(profile.subscriptionUuid, isNull);
    });

    test('просроченная ссылка отвергается и профиля не создаёт', () {
      final container = boot(
        redeem: okRedeem(),
        nowSec: _goldenExpires + 1,
      );
      container.read(connectProvider.notifier).open(_goldenLink);

      final s = container.read(connectProvider);
      expect(s.stage, ConnectStage.refused);
      expect(s.failure, ConnectLinkFailure.expired);
      expect(s.refusalText, contains('просрочено'));
      expect(container.read(connectionProfilesProvider).profiles, isEmpty);
    });

    test('испорченная ссылка называет сумму, а не «проверьте ссылку»', () {
      final container = boot(redeem: okRedeem());
      // Меняем один символ армора: сумма перестаёт сходиться.
      final broken = _goldenLink.replaceFirst('8D5320D405', '8D5320D406');
      container.read(connectProvider.notifier).open(broken);

      final s = container.read(connectProvider);
      expect(s.stage, ConnectStage.refused);
      expect(s.refusalText, contains('повреждена'));
      expect(s.detail, isNotNull);
    });

    test('отказ панели оставляет ссылку и не создаёт профиль', () async {
      final container = boot(
        redeem: ({required String origin, required String code}) async =>
            throw const ConnectRedeemException(
              'Приглашение не подошло',
              statusCode: 400,
            ),
      );
      final notifier = container.read(connectProvider.notifier);
      notifier.open(_goldenLink);
      await notifier.confirm();

      final s = container.read(connectProvider);
      expect(s.stage, ConnectStage.failed);
      expect(s.error, contains('Приглашение не подошло'));
      // Ссылка на месте: сетевой сбой мог и не потратить одноразовый код,
      // поэтому повтор осмыслен.
      expect(s.link, isNotNull);
      expect(container.read(connectionProfilesProvider).profiles, isEmpty);
    });
  });
}

// ============================================================================
// Вход по коду из бота: сессия без профиля панели была пустой вкладкой
// «Серверы».
// ============================================================================

/// Отдаёт ответы по пути запроса. Отдельный адаптер от [_StubAdapter]: там один
/// ответ на всё, а вход по коду дёргает три разных пути.
class _RouteAdapter implements HttpClientAdapter {
  _RouteAdapter(this.responses);

  final Map<String, (int, String)> responses;
  final List<String> requested = <String>[];

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<List<int>>? requestStream,
    Future<void>? cancelFuture,
  ) async {
    requested.add(options.path);
    final (status, body) = responses[options.path] ?? (404, '{}');
    return ResponseBody.fromString(
      body,
      status,
      headers: {
        Headers.contentTypeHeader: [Headers.jsonContentType],
      },
    );
  }

  @override
  void close({bool force = false}) {}
}

void _loginByCodeTests() {
  group('вход по коду из бота', () {
    const panel = 'https://panel.example';

    ProviderContainer boot(_RouteAdapter adapter) {
      final container = ProviderContainer(
        overrides: [
          apiClientProvider.overrideWith((ref) {
            final dio = Dio(
              BaseOptions(
                baseUrl: '$panel/api/v2/app',
                validateStatus: (s) => s != null && s < 500,
              ),
            )..httpClientAdapter = adapter;
            return ApiClient(tokens: ref.watch(tokenStoreProvider), dio: dio);
          }),
        ],
      );
      addTearDown(container.dispose);
      return container;
    }

    test('успешный вход заводит и активирует профиль панели', () async {
      // РЕГРЕССИЯ: сессия появлялась, а профиль подключения — нет, и вкладка
      // «Серверы» оставалась пустой ровно у тех, кто вошёл. Резолвер
      // конфигурации ядра читает panelUrl ИЗ ПРОФИЛЯ, а не из сессии.
      final adapter = _RouteAdapter({
        '/login/code': (
          200,
          '{"access_token":"acc-1","refresh_token":"ref-1","user_id":46}',
        ),
        '/me': (404, '{}'),
      });
      final container = boot(adapter);

      await container.read(authProvider.notifier).loginCode(code: '123456');

      final profiles = container.read(connectionProfilesProvider).profiles;
      expect(profiles, hasLength(1));
      expect(profiles.single.isPanel, isTrue);
      expect(profiles.single.panelUrl, panel);
      expect(profiles.single.accessToken, 'acc-1');
      expect(
        container.read(activeConnectionProfileProvider)?.id,
        profiles.single.id,
      );
      // Появление ПЕРВОГО профиля пересобирает граф (профили -> активный ->
      // хранилище токенов -> клиент -> auth), поэтому стадию сессии здесь не
      // проверяем: её перечитает уже новый нотифаер. Проверяем то, что при
      // этом обязано уцелеть, — саму сессию в хранилище.
      expect(await container.read(tokenStoreProvider).readRefresh(), 'ref-1');
    });

    test('повторный вход не плодит вторую запись и не переименовывает', () async {
      final adapter = _RouteAdapter({
        '/login/code': (
          200,
          '{"access_token":"acc-2","refresh_token":"ref-2","user_id":46}',
        ),
        '/me': (404, '{}'),
      });
      final container = boot(adapter);
      // Профиль уже есть и назван человеком.
      await container
          .read(connectionProfilesProvider.notifier)
          .addPanelAccount(panelUrl: panel, displayName: 'Моя панель');

      await container.read(authProvider.notifier).loginCode(code: '123456');

      final profiles = container.read(connectionProfilesProvider).profiles;
      expect(profiles, hasLength(1));
      expect(profiles.single.displayName, 'Моя панель');
      // Токен на профиле НЕ переписывается: он там только запасной снимок, а
      // рабочий берётся из общего хранилища, потому что клиент ротирует его при
      // 401 и запись на профиле устаревает за час. Заодно это и есть проверка
      // того, что повторный вход вообще ничего не пишет в список профилей: одна
      // такая запись пересобирала бы граф провайдеров на каждом входе.
      expect(await container.read(tokenStoreProvider).readAccess(), 'acc-2');
    });

    test('неверный код не заводит профиль', () async {
      final adapter = _RouteAdapter({'/login/code': (401, 'bad code')});
      final container = boot(adapter);

      await container.read(authProvider.notifier).loginCode(code: '000000');

      expect(container.read(authProvider).stage, AuthStage.unauthenticated);
      expect(container.read(connectionProfilesProvider).profiles, isEmpty);
    });
  });
}

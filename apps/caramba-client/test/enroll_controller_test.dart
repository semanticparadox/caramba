// Энроллмент не оставляет за собой мусора и доносит креды до профиля.
//
// Профиль панели заводится ДО валидации кода (аккаунт обязателен, профиль ведёт
// вход). Раньше на невалидном коде он оставался в списке подключений навсегда,
// а после успешного входа так и не получал subscription_uuid/access_token — из
// за чего `_resolveVpnConfig` конфигурировал ядро против тенанта-1, а не против
// панели из ссылки.

import 'package:dio/dio.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/enrollment.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/features/enroll/enroll_controller.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';

/// Отдаёт заранее заданные ответы вместо сети. Ключ — путь запроса.
class _StubAdapter implements HttpClientAdapter {
  final Map<String, (int, String)> responses;
  final List<String> requested = <String>[];

  _StubAdapter(this.responses);

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

ApiClient _client(_StubAdapter adapter) {
  final dio = Dio(
    BaseOptions(
      baseUrl: 'https://panel.example/api/v2/app',
      validateStatus: (s) => s != null && s < 500,
    ),
  )..httpClientAdapter = adapter;
  return ApiClient(tokens: TokenStore(), dio: dio);
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late TestDefaultBinaryMessenger messenger;
  const secureStorage = MethodChannel(
    'plugins.it_nomads.com/flutter_secure_storage',
  );

  setUp(() {
    messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    // Чистая установка: ни токенов, ни сохранённых профилей.
    messenger.setMockMethodCallHandler(secureStorage, (call) async => null);
  });

  tearDown(() => messenger.setMockMethodCallHandler(secureStorage, null));

  ProviderContainer containerWith(_StubAdapter adapter) {
    final container = ProviderContainer(
      overrides: [
        enrollApiClientProvider.overrideWith(
          (ref, panelUrl) => _client(adapter),
        ),
      ],
    );
    addTearDown(container.dispose);
    // enrollProvider автодиспозится без подписчиков: в приложении его держит
    // открытый экран, в тесте — эта подписка.
    final sub = container.listen(enrollProvider, (_, __) {});
    addTearDown(sub.close);
    return container;
  }

  const link = EnrollLink(panelUrl: 'https://panel.example', code: 'ABC123');

  test('невалидный код убирает созданный профиль-плейсхолдер', () async {
    final adapter = _StubAdapter({
      '/enroll/ABC123': (200, '{"valid":false,"reason":"expired"}'),
    });
    final container = containerWith(adapter);

    await container.read(enrollProvider.notifier).startWith(link);

    expect(container.read(enrollProvider).stage, EnrollStage.invalid);
    expect(
      container.read(connectionProfilesProvider).profiles,
      isEmpty,
      reason: 'плейсхолдер, созданный этим потоком, обязан быть удалён',
    );
  });

  test('панель недоступна — профиль тоже не остаётся', () async {
    final adapter = _StubAdapter({'/enroll/ABC123': (500, 'boom')});
    final container = containerWith(adapter);

    await container.read(enrollProvider.notifier).startWith(link);

    expect(container.read(enrollProvider).stage, EnrollStage.invalid);
    expect(container.read(connectionProfilesProvider).profiles, isEmpty);
  });

  test('чужой профиль с тем же URL не удаляется', () async {
    final adapter = _StubAdapter({
      '/enroll/ABC123': (200, '{"valid":false,"reason":"exhausted"}'),
    });
    final container = containerWith(adapter);
    // Профиль этой панели уже был заведён раньше и переименован пользователем.
    final profiles = container.read(connectionProfilesProvider.notifier);
    await profiles.addPanelAccount(
      panelUrl: 'https://panel.example',
      displayName: 'Моя панель',
    );

    await container.read(enrollProvider.notifier).startWith(link);

    final kept = container.read(connectionProfilesProvider).profiles;
    expect(kept, hasLength(1));
    expect(kept.single.displayName, 'Моя панель');
  });

  test('валидный код оставляет профиль и уточняет его имя', () async {
    final adapter = _StubAdapter({
      '/enroll/ABC123': (
        200,
        '{"valid":true,"panel_name":"Панель X","onboarding_traffic_mb":500}',
      ),
    });
    final container = containerWith(adapter);

    await container.read(enrollProvider.notifier).startWith(link);

    expect(container.read(enrollProvider).stage, EnrollStage.valid);
    final profiles = container.read(connectionProfilesProvider).profiles;
    expect(profiles, hasLength(1));
    expect(profiles.single.displayName, 'Панель X');
    expect(profiles.single.panelUrl, 'https://panel.example');
    // Энроллмент означает переход на эту панель: её профиль становится активным.
    expect(
      container.read(activeConnectionProfileProvider)?.id,
      profiles.single.id,
    );
  });

  test(
    'после входа профиль получает UUID подписки и токен своей панели',
    () async {
      final adapter = _StubAdapter({
        '/enroll/ABC123': (200, '{"valid":true,"panel_name":"Панель X"}'),
        '/login/code': (
          200,
          '{"access_token":"acc-1","refresh_token":"ref-1","user_id":7}',
        ),
        '/subscription': (
          200,
          '{"id":1,"subscription_uuid":"uuid-1","status":"active",'
              '"clash_url":"https://sub/x","config_url":"https://sub/x",'
              '"singbox_url":"","v2ray_url":"","subscription_url":""}',
        ),
      });
      final container = containerWith(adapter);
      final notifier = container.read(enrollProvider.notifier);

      await notifier.startWith(link);
      await notifier.loginCodeWithEnroll(botCode: '123456');

      final profile = container
          .read(connectionProfilesProvider)
          .profiles
          .single;
      expect(profile.panelUrl, 'https://panel.example');
      expect(profile.subscriptionUuid, 'uuid-1');
      expect(profile.accessToken, 'acc-1');
      expect(adapter.requested, contains('/subscription'));
    },
  );

  test('входа без подписки достаточно: сохраняются URL и токен', () async {
    final adapter = _StubAdapter({
      '/enroll/ABC123': (200, '{"valid":true}'),
      '/login/code': (
        200,
        '{"access_token":"acc-2","refresh_token":"ref-2","user_id":8}',
      ),
      // Новый неоплаченный аккаунт: подписки ещё нет.
      '/subscription': (404, '{"detail":"no subscription"}'),
    });
    final container = containerWith(adapter);
    final notifier = container.read(enrollProvider.notifier);

    await notifier.startWith(link);
    await notifier.loginCodeWithEnroll(botCode: '123456');

    final profile = container.read(connectionProfilesProvider).profiles.single;
    expect(profile.panelUrl, 'https://panel.example');
    expect(profile.accessToken, 'acc-2');
    expect(profile.subscriptionUuid, isNull);
  });
}

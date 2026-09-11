// Обновления приложения: сеть и состояние.
//
//   * ApiClient шлёт X-Caramba-App-Version на любой вызов, когда версия
//     известна, и не шлёт пустой заголовок, когда нет;
//   * getAppVersion: 404 старой панели — «версии нет», а не ошибка; 5xx —
//     ошибка;
//   * AppUpdateNotifier: проверка кладёт версию в состояние, «Позже»
//     запоминается в prefs, без панели проверять не у кого.

import 'dart:convert';

import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/features/updates/update_installer.dart';
import 'package:caramba_client/state/app_update_state.dart';
import 'package:caramba_client/state/bootstrap_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/device_identity.dart';
import 'package:caramba_client/state/providers.dart';

/// Отдаёт заданный ответ и запоминает заголовки запроса.
class _StubAdapter implements HttpClientAdapter {
  final String body;
  final int status;
  final List<RequestOptions> requests = <RequestOptions>[];

  _StubAdapter({this.body = '{}', this.status = 200});

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
      headers: <String, List<String>>{
        Headers.contentTypeHeader: <String>[Headers.jsonContentType],
      },
    );
  }

  @override
  void close({bool force = false}) {}
}

class _MemoryProfiles implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles;
  String? activeId;
  _MemoryProfiles(this.profiles, this.activeId);

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

class _NoopInstaller implements UpdateInstaller {
  final List<AppVersionInfo> installed = <AppVersionInfo>[];

  @override
  Future<String> install(AppVersionInfo info) async {
    installed.add(info);
    return 'ok';
  }
}

const _latestJson =
    '{"platform":"android","version":"1.0.0","build":110,'
    '"download_url":"https://app.example.com/downloads/a.apk",'
    '"size":3,"sha256":"ab","min_build":0,"notes":"Трей на Windows"}';

ApiClient _client(
  _StubAdapter adapter, {
  InstalledVersion version = const InstalledVersion(
    version: '1.0.0',
    build: 109,
  ),
  String baseUrl = 'https://panel.example',
}) {
  // Как в проде: 4xx не бросается Dio, решает клиент; пустая база — «панели
  // нет».
  final dio = Dio(
    BaseOptions(
      baseUrl: baseUrl.isEmpty ? '' : '$baseUrl/api/v2/app',
      validateStatus: (s) => s != null && s < 500,
    ),
  )..httpClientAdapter = adapter;
  return ApiClient(
    tokens: TokenStore(),
    dio: dio,
    deviceIdentity: DeviceIdentityStore(
      storage: const FlutterSecureStorage(),
      platform: 'android',
    ),
    installedVersion: () => version,
  );
}

ConnectionProfile _panel() => const ConnectionProfile(
  id: 'cp_panel',
  type: ProfileType.panelAccount,
  displayName: 'Оператор',
  source: 'https://panel.example',
  panelUrl: 'https://panel.example',
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() {
    FlutterSecureStorage.setMockInitialValues(<String, String>{});
    SharedPreferences.setMockInitialValues(<String, Object>{});
    InstalledVersion.setForTesting(null);
  });

  group('заголовок X-Caramba-App-Version', () {
    test('идёт на публичный вызов, когда версия известна', () async {
      final adapter = _StubAdapter(body: '{"enabled":false}');
      await _client(adapter).getBranding();
      expect(adapter.requests.single.headers[kAppVersionHeader], '1.0.0+109');
    });

    test('не отправляется пустым, когда версия неизвестна', () async {
      final adapter = _StubAdapter(body: '{"enabled":false}');
      await _client(adapter, version: InstalledVersion.unknown).getBranding();
      expect(
        adapter.requests.single.headers.containsKey(kAppVersionHeader),
        isFalse,
      );
    });
  });

  group('getAppVersion', () {
    test('разбирает ответ и спрашивает за свою платформу', () async {
      final adapter = _StubAdapter(body: _latestJson);
      final info = await _client(adapter).getAppVersion('android');
      expect(info, isNotNull);
      expect(info!.build, 110);
      expect(info.notes, 'Трей на Windows');
      final req = adapter.requests.single;
      expect(req.path, '/version');
      expect(req.queryParameters['platform'], 'android');
      expect(req.headers.containsKey('Authorization'), isFalse);
    });

    test('404 старой панели — версии нет, без ошибки', () async {
      final adapter = _StubAdapter(body: '{"error":"x"}', status: 404);
      expect(await _client(adapter).getAppVersion('android'), isNull);
    });

    test('пустой объект — тоже «версии нет»', () async {
      final adapter = _StubAdapter(body: '{}');
      expect(await _client(adapter).getAppVersion('android'), isNull);
    });

    test('5xx — ошибка, которую покажет экран «Обновления»', () async {
      final adapter = _StubAdapter(body: 'oops', status: 503);
      final dio = Dio(
        BaseOptions(
          baseUrl: 'https://panel.example/api/v2/app',
          // Как в проде: 5xx не бросается Dio, решает клиент.
          validateStatus: (s) => s != null && s < 500,
        ),
      )..httpClientAdapter = adapter;
      final client = ApiClient(
        tokens: TokenStore(),
        dio: dio,
        deviceIdentity: DeviceIdentityStore(
          storage: const FlutterSecureStorage(),
          platform: 'android',
        ),
        installedVersion: () => InstalledVersion.unknown,
      );
      expect(() => client.getAppVersion('android'), throwsA(anything));
    });
  });

  group('AppUpdateNotifier', () {
    ProviderContainer container({
      required _StubAdapter adapter,
      List<ConnectionProfile>? profiles,
      Map<String, Object> prefs = const <String, Object>{},
    }) {
      SharedPreferences.setMockInitialValues(prefs);
      final stored = profiles ?? <ConnectionProfile>[_panel()];
      final installer = _NoopInstaller();
      final c = ProviderContainer(
        overrides: [
          connectionProfilesStoreProvider.overrideWithValue(
            _MemoryProfiles(stored, stored.isEmpty ? null : stored.first.id),
          ),
          apiClientProvider.overrideWith(
            (ref) => _client(
              adapter,
              baseUrl: stored.isEmpty ? '' : 'https://panel.example',
            ),
          ),
          updateInstallerProvider.overrideWithValue(installer),
          updateCheckIntervalProvider.overrideWithValue(null),
          updateFirstCheckDelayProvider.overrideWithValue(Duration.zero),
          updatePlatformProvider.overrideWithValue('android'),
        ],
      );
      addTearDown(c.dispose);
      return c;
    }

    test('проверка кладёт последнюю версию, баннер показывается', () async {
      InstalledVersion.setForTesting(
        const InstalledVersion(version: '1.0.0', build: 109),
      );
      final adapter = _StubAdapter(body: _latestJson);
      final c = container(adapter: adapter);
      await c.read(appBootProvider.future);
      // Первая проверка стартует сама (задержка нулевая); повторный вызов
      // дожидается её, а не возвращается раньше.
      c.read(appUpdateProvider);
      await c.read(appUpdateProvider.notifier).check();
      final s = c.read(appUpdateProvider);
      expect(s.latest?.build, 110);
      expect(s.installed.build, 109);
      expect(s.verdict, UpdateVerdict.available);
      expect(s.error, isNull);
      expect(c.read(updateRequiredProvider), isFalse);
    });

    test('«Позже» пишется в prefs и переживает пересоздание', () async {
      InstalledVersion.setForTesting(
        const InstalledVersion(version: '1.0.0', build: 109),
      );
      final adapter = _StubAdapter(body: _latestJson);
      final c = container(adapter: adapter);
      await c.read(appBootProvider.future);
      await c.read(appUpdateProvider.notifier).check();
      await c.read(appUpdateProvider.notifier).dismiss();
      expect(c.read(appUpdateProvider).verdict, UpdateVerdict.none);
      expect(c.read(appUpdateProvider).hasNewer, isTrue);
      final prefs = c.read(prefsStoreProvider);
      expect(prefs.readString(kDismissedUpdateBuildKey), '110');

      // Новый контейнер с теми же prefs: отложенная сборка помнится.
      final c2 = container(
        adapter: _StubAdapter(body: _latestJson),
        prefs: <String, Object>{kDismissedUpdateBuildKey: '110'},
      );
      await c2.read(appBootProvider.future);
      await c2.read(appUpdateProvider.notifier).check();
      expect(c2.read(appUpdateProvider).verdict, UpdateVerdict.none);
    });

    test('min_build выше установленной — требование обновиться', () async {
      InstalledVersion.setForTesting(
        const InstalledVersion(version: '1.0.0', build: 100),
      );
      final adapter = _StubAdapter(
        body: _latestJson.replaceFirst('"min_build":0', '"min_build":105'),
      );
      final c = container(adapter: adapter);
      await c.read(appBootProvider.future);
      await c.read(appUpdateProvider.notifier).check();
      expect(c.read(appUpdateProvider).verdict, UpdateVerdict.required);
      expect(c.read(updateRequiredProvider), isTrue);
    });

    test('без панели проверять не у кого: ни запроса, ни ошибки', () async {
      InstalledVersion.setForTesting(
        const InstalledVersion(version: '1.0.0', build: 109),
      );
      final adapter = _StubAdapter(body: _latestJson);
      final c = container(adapter: adapter, profiles: <ConnectionProfile>[]);
      await c.read(appBootProvider.future);
      await c.read(appUpdateProvider.notifier).check();
      final s = c.read(appUpdateProvider);
      expect(adapter.requests, isEmpty);
      expect(s.latest, isNull);
      expect(s.error, isNull);
      expect(s.verdict, UpdateVerdict.none);
    });

    test('«Скачать» отдаёт версию установщику и сохраняет его ответ', () async {
      InstalledVersion.setForTesting(
        const InstalledVersion(version: '1.0.0', build: 109),
      );
      final adapter = _StubAdapter(body: _latestJson);
      final c = container(adapter: adapter);
      await c.read(appBootProvider.future);
      await c.read(appUpdateProvider.notifier).check();
      await c.read(appUpdateProvider.notifier).install();
      final s = c.read(appUpdateProvider);
      expect(s.installing, isFalse);
      expect(s.installMessage, 'ok');
    });
  });

  test('ответ панели о версии — тот же JSON, что в манифесте CI', () {
    // Контракт /api/v2/app/version: поля build/version обязательны, остальные
    // могут отсутствовать. Убеждаемся, что разбор терпит отсутствие.
    final json = jsonDecode(
      '{"platform":"linux","version":"1.0.0","build":110}',
    );
    final info = AppVersionInfo.fromJson((json as Map).cast<String, dynamic>());
    expect(info?.build, 110);
  });
}

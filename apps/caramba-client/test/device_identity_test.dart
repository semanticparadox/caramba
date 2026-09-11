// Идентичность устройства: стабильный `client_device_id`, имя по умолчанию и
// заголовки `X-Caramba-Device-*` на запросах к панели.
//
// ЗАЧЕМ ЭТИ ПРОВЕРКИ. Панель узнавала устройство по отпечатку
// `sha256(subscription_id + User-Agent)`: он менялся при смене тарифа и при
// обновлении приложения, поэтому лиза «слетала», лимит устройств считался
// неверно, а строки исчезали из списка отвязки. Стабильный идентификатор чинит
// это только при трёх условиях, каждое из которых здесь и зафиксировано:
//
//   1. идентификатор генерируется ОДИН раз и переживает перезапуск (иначе это
//      тот же плавающий отпечаток, только под новым именем);
//   2. он уезжает заголовком на КАЖДОМ авторизованном вызове и не уезжает на
//      публичных (панель, которую человек только рассматривает, не должна
//      получать стабильный след);
//   3. недоступная связка ключей не роняет запрос — устройство просто
//      остаётся неопознанным на этот запуск.

import 'dart:math';

import 'package:dio/dio.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/features/profile/profile_screen.dart';
import 'package:caramba_client/state/device_identity.dart';

/// Адаптер, запоминающий запросы: проверяем ровно заголовки, а не тело.
class _CapturingAdapter implements HttpClientAdapter {
  _CapturingAdapter(this.body);

  final String body;
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
      200,
      headers: <String, List<String>>{
        Headers.contentTypeHeader: <String>[Headers.jsonContentType],
      },
    );
  }

  @override
  void close({bool force = false}) {}
}

/// Хранилище, которое отказывает на любом обращении: так ведёт себя связка
/// ключей на заблокированном экране и в песочнице без entitlement.
class _BrokenStorage implements FlutterSecureStorage {
  @override
  dynamic noSuchMethod(Invocation invocation) =>
      Future<Never>.error(StateError('keychain unavailable'));
}

ApiClient _client(_CapturingAdapter adapter, DeviceIdentityStore identity) {
  final dio = Dio(
    BaseOptions(
      baseUrl: 'https://panel.example/api/v2/app',
      validateStatus: (s) => s != null && s < 500,
    ),
  );
  dio.httpClientAdapter = adapter;
  return ApiClient(tokens: TokenStore(), dio: dio, deviceIdentity: identity);
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late Map<String, String> backing;

  setUp(() {
    backing = <String, String>{};
    FlutterSecureStorage.setMockInitialValues(backing);
  });

  group('идентификатор', () {
    test('генерируется один раз и переживает перезапуск', () async {
      final first = await DeviceIdentityStore(
        platform: 'macos',
        hostname: 'MacBook-Pro.local',
      ).ensure();

      expect(first.clientDeviceId, isNotEmpty);
      expect(backing[DeviceIdentityStore.idKey], first.clientDeviceId);

      // Новый экземпляр — это следующий запуск приложения.
      final second = await DeviceIdentityStore(
        platform: 'macos',
        hostname: 'MacBook-Pro.local',
      ).ensure();
      expect(second.clientDeviceId, first.clientDeviceId);
      expect(second.displayName, first.displayName);
    });

    test('это настоящий UUID v4', () {
      final id = generateClientDeviceId(random: Random(7));
      expect(
        RegExp(
          r'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$',
        ).hasMatch(id),
        isTrue,
        reason: 'панель кладёт значение в колонку uuid-вида: $id',
      );
    });

    test('параллельные вызовы не плодят второй идентификатор', () async {
      final store = DeviceIdentityStore(platform: 'android');
      final results = await Future.wait<DeviceIdentity>(
        <Future<DeviceIdentity>>[
          store.ensure(),
          store.ensure(),
          store.ensure(),
        ],
      );
      expect(results[1].clientDeviceId, results[0].clientDeviceId);
      expect(results[2].clientDeviceId, results[0].clientDeviceId);
    });

    test(
      'недоступная связка ключей не отказ, а устройство на один запуск',
      () async {
        final store = DeviceIdentityStore(
          storage: _BrokenStorage(),
          platform: 'linux',
          hostname: 'thinkpad',
        );
        final identity = await store.ensure();
        expect(identity.isKnown, isTrue);
        expect(identity.displayName, 'thinkpad');
        // Второй вызов отдаёт то же самое из памяти процесса.
        expect((await store.ensure()).clientDeviceId, identity.clientDeviceId);
      },
    );
  });

  group('имя по умолчанию', () {
    test('десктоп берёт имя хоста без служебного суффикса', () {
      expect(
        defaultDeviceName(
          platform: 'macos',
          clientDeviceId: 'aabbccdd-0000-4000-8000-000000000000',
          hostname: 'MacBook-Pro.local',
        ),
        'MacBook-Pro',
      );
    });

    test('телефон получает платформу и хвост идентификатора', () {
      // Имя хоста на телефоне бесполезно («localhost»), но два Android одного
      // аккаунта обязаны отличаться в списке.
      expect(
        defaultDeviceName(
          platform: 'android',
          clientDeviceId: 'a1b2c3d4-0000-4000-8000-000000000000',
          hostname: 'localhost',
        ),
        'Android (a1b2)',
      );
    });
  });

  group('заголовки', () {
    test('едут на авторизованном вызове', () async {
      final identity = DeviceIdentityStore(
        platform: 'macos',
        hostname: 'MacBook-Pro.local',
      );
      final adapter = _CapturingAdapter('[]');
      await _client(adapter, identity).getDevices();

      expect(adapter.requests, hasLength(1));
      final headers = adapter.requests.single.headers;
      expect(headers[kDeviceIdHeader], identity.cached?.clientDeviceId);
      expect(headers[kDeviceNameHeader], 'MacBook-Pro');
      // Платформа отдельным заголовком: UA у нашего приложения один на все
      // пять платформ, и без него Windows и Linux в кабинете неотличимы.
      expect(headers[kDevicePlatformHeader], 'macos');
    });

    test('не едут на публичном вызове', () async {
      final identity = DeviceIdentityStore(platform: 'macos');
      final adapter = _CapturingAdapter('{}');
      // `/branding` публичный: аккаунта ещё нет, а стабильный след панель
      // получила бы уже за один просмотр.
      await _client(adapter, identity).getBranding();

      final headers = adapter.requests.single.headers;
      expect(headers.containsKey(kDeviceIdHeader), isFalse);
      expect(headers.containsKey(kDeviceNameHeader), isFalse);
      expect(headers.containsKey(kDevicePlatformHeader), isFalse);
    });

    test(
      'кириллическое имя не ломает запрос, а едет отдельным вызовом',
      () async {
        final identity = DeviceIdentityStore(platform: 'android');
        await identity.ensure();
        await identity.rename('Телефон Артёма');

        final adapter = _CapturingAdapter('[]');
        await _client(adapter, identity).getDevices();

        final headers = adapter.requests.single.headers;
        // Заголовки едут latin-1: кириллица в них либо ломает запрос, либо
        // приезжает мусором, поэтому имя просто не отправляется — панель
        // получит его телом PATCH /devices/{id}.
        expect(headers.containsKey(kDeviceNameHeader), isFalse);
        expect(headers[kDeviceIdHeader], isNotEmpty);
        // Локально имя всё равно сохранено: список показывает то, что задали.
        expect(identity.cached?.displayName, 'Телефон Артёма');
      },
    );

    test('ASCII-очистка режет управляющие символы и длину', () {
      expect(asciiHeaderValue('Mac\r\nX-Injected: 1'), 'MacX-Injected: 1');
      expect(asciiHeaderValue('   '), '');
      expect(asciiHeaderValue('a' * 200).length, kDeviceNameMaxLength);
    });
  });

  group('переименование', () {
    test('сохраняется и попадает в заголовок', () async {
      final store = DeviceIdentityStore(platform: 'windows', hostname: 'PC');
      await store.ensure();
      final renamed = await store.rename('Work laptop');

      expect(renamed.displayName, 'Work laptop');
      expect(backing[DeviceIdentityStore.nameKey], 'Work laptop');
      expect(renamed.headers[kDeviceNameHeader], 'Work laptop');

      // Следующий запуск читает заданное имя, а не собирает его заново.
      final next = await DeviceIdentityStore(
        platform: 'windows',
        hostname: 'PC',
      ).ensure();
      expect(next.displayName, 'Work laptop');
    });

    test('пустое имя возвращает авто-имя', () async {
      final store = DeviceIdentityStore(platform: 'macos', hostname: 'iMac');
      await store.ensure();
      await store.rename('Домашний');
      final back = await store.rename('   ');
      expect(back.displayName, 'iMac');
    });
  });

  group('контракт устройства', () {
    test('новые поля панели разбираются', () {
      final d = Device.fromJson(<String, dynamic>{
        'id': 12,
        'display_name': 'Телефон Артёма',
        'platform': 'android',
        'client_device_id': 'a1b2c3d4-0000-4000-8000-000000000000',
        'last_seen_at': '2026-09-11T05:38:00Z',
        'is_current': true,
      });

      expect(d.id, 12);
      expect(d.name, 'Телефон Артёма');
      expect(d.platform, 'android');
      expect(d.platformLabel, 'Android');
      expect(d.clientDeviceId, 'a1b2c3d4-0000-4000-8000-000000000000');
      expect(d.isCurrent, isTrue);
      expect(d.lastSeenAt, isNotNull);
    });

    test('старая панель и сторонний клиент не ломают список', () {
      // Полей нет вовсе: список обязан остаться читаемым, иначе обновление
      // приложения раньше панели показало бы пустые устройства.
      final legacy = Device.fromJson(<String, dynamic>{
        'id': 3,
        'name': 'Clash Verge',
        'user_agent': 'clash-verge/1.7',
        'online': false,
      });
      expect(legacy.name, 'Clash Verge');
      expect(legacy.platform, isEmpty);
      expect(legacy.platformLabel, isEmpty);
      expect(legacy.clientDeviceId, isEmpty);
      expect(legacy.isCurrent, isFalse);
      // Подпись без платформы — только время, без висящего разделителя.
      expect(legacy.metaLabel, legacy.lastSeenLabel);
    });

    test('своё устройство определяется и без поля панели', () {
      const mine = DeviceIdentity(
        clientDeviceId: 'a1b2c3d4-0000-4000-8000-000000000000',
        displayName: 'Android (a1b2)',
        platform: 'android',
      );
      final same = Device.fromJson(<String, dynamic>{
        'id': 1,
        'display_name': 'Android (a1b2)',
        'client_device_id': 'a1b2c3d4-0000-4000-8000-000000000000',
      });
      final other = Device.fromJson(<String, dynamic>{
        'id': 2,
        'display_name': 'iMac',
        'client_device_id': 'ffffffff-0000-4000-8000-000000000000',
      });
      expect(isCurrentDevice(same, mine), isTrue);
      expect(isCurrentDevice(other, mine), isFalse);
      // Устройство без идентификатора (сторонний клиент) своим не считается,
      // даже когда своего идентификатора нет тоже: пустое не равно пустому.
      expect(
        isCurrentDevice(Device.fromJson(<String, dynamic>{'id': 9}), mine),
        isFalse,
      );
    });
  });
}

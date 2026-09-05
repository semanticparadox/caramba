// Живая проверка права подключаться.
//
// Воспроизведённая поломка, ради которой этот слой существует: профиль
// импортирован, пока подписка была здорова; трафик кончился; тап по кнопке
// поднимает туннель НА КЭШЕ, и экран показывает зелёный щит, «Защищено» и
// идущий таймер, пока ни один сайт не открывается. Тесты фиксируют обе стороны
// контракта: явный отказ оператора обязан остановить подключение и назвать
// причину, а МОЛЧАНИЕ сети — не имеет права ни запретить подключение, ни
// погасить щит на живом туннеле.

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/data/subscription_fetch.dart';
import 'package:caramba_client/state/access_guard.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

const int _mib = 1024 * 1024;

/// Заголовки прямого отказа панели. Значения взяты из её собственного теста
/// `the_refusal_headers_carry_the_same_numbers_as_the_json`
/// (`apps/caramba-panel/src/subscription.rs`), а не придуманы здесь: если
/// панель их поменяет, разойтись мы обязаны заметно.
Map<String, String> _dailyQuotaHeaders({int resetsAt = 1788652800}) => {
  'x-caramba-state': 'quota_exceeded',
  'x-caramba-st': '7',
  'x-caramba-reason': '3003',
  'x-caramba-reason-name': 'daily_allowance_exhausted',
  'x-caramba-used': '${263 * _mib}',
  'x-caramba-limit': '${200 * _mib}',
  'x-caramba-period': 'day',
  'x-caramba-resets-at': '$resetsAt',
  'x-caramba-reset-lag': '1800',
  'x-caramba-bytes-after-reset': '${137 * _mib}',
  'subscription-userinfo':
      'upload=0; download=${263 * _mib}; total=${200 * _mib}; expire=253402300799',
};

void main() {
  group('разбор отказа подписки', () {
    test('прямой ответ панели даёт причину, числа и срок пополнения', () {
      final r = refusalFromResponse(
        statusCode: 403,
        body: 'Subscription inactive or expired',
        headers: _dailyQuotaHeaders(),
      );

      expect(r, isNotNull);
      expect(r!.fromOperator, isTrue);
      expect(r.access.isBlocked, isTrue);
      expect(r.access.kind, AccessKind.dailyQuota);
      expect(r.access.usedBytes, 263 * _mib);
      expect(r.access.limitBytes, 200 * _mib);
      expect(r.access.period, 'day');
      expect(r.access.bytesAfterReset, 137 * _mib);
      expect(r.access.resetLagSeconds, 1800);
      expect(
        r.access.resetsAt,
        DateTime.fromMillisecondsSinceEpoch(1788652800 * 1000, isUtc: true),
      );
      // Ни одного внутреннего слова на экране.
      expect(r.access.title, isNot(contains('throttl')));
      expect(r.access.shortReason, 'Дневной лимит израсходован');
    });

    test(
      'через зеркало (заголовки вырезаны) расход выдаёт трафик, а не срок',
      () {
        // caramba-sub копирует три заголовка, x-caramba-* среди них нет. Тело
        // при этом одно и то же и у троттлинга, и у истёкшего срока — сказать
        // «подписка закончилась» человеку, у которого просто кончилась дневная
        // норма, значит послать его продлевать то, что не истекло.
        final r = refusalFromResponse(
          statusCode: 403,
          body: 'Subscription inactive or expired',
          headers: {
            'subscription-userinfo':
                'upload=0; download=${263 * _mib}; total=${200 * _mib}; '
                'expire=253402300799',
            'profile-update-interval': '12',
          },
        );

        expect(r, isNotNull);
        expect(r!.fromOperator, isTrue);
        expect(r.access.kind, AccessKind.planQuota);
        expect(r.access.kind, isNot(AccessKind.expired));
        expect(r.access.usedBytes, 263 * _mib);
        expect(r.access.limitBytes, 200 * _mib);
      },
    );

    test('через зеркало истёкший срок остаётся истёкшим сроком', () {
      final past =
          DateTime.now().subtract(const Duration(days: 3)).millisecondsSinceEpoch ~/
          1000;
      final r = refusalFromResponse(
        statusCode: 403,
        body: 'Subscription inactive or expired',
        headers: {
          'subscription-userinfo':
              'upload=0; download=${5 * _mib}; total=${200 * _mib}; expire=$past',
        },
      );

      expect(r!.access.kind, AccessKind.expired);
    });

    test('лимит устройств узнаётся по телу', () {
      final r = refusalFromResponse(
        statusCode: 403,
        body: 'Device limit reached',
        headers: const {},
      );

      expect(r!.access.kind, AccessKind.deviceLimit);
      expect(r.fromOperator, isTrue);
    });

    test('чужой 403 без единой улики оператора не считается его ответом', () {
      // Портал публичного Wi-Fi отвечает так на любой адрес. Отказом подписки
      // это считать нельзя: на живом туннеле такой ответ погасил бы щит и
      // назвал причину, которой нет.
      final r = refusalFromResponse(
        statusCode: 403,
        body: '<html><body>Access denied by proxy</body></html>',
        headers: const {'content-type': 'text/html'},
      );

      expect(r, isNotNull);
      expect(r!.fromOperator, isFalse);
      expect(r.access.kind, AccessKind.unknown);
    });

    test('429 и 5xx отказом по доступу не являются', () {
      expect(refusalFromResponse(statusCode: 429, body: 'rate limit'), isNull);
      expect(refusalFromResponse(statusCode: 502, body: 'bad gateway'), isNull);
      expect(refusalFromResponse(statusCode: null, body: ''), isNull);
    });
  });

  group('опрос подписки по ссылке', () {
    test('успешный ответ — доступ есть', () async {
      final v = await probeSubscriptionUrl(
        'https://sub.example/x',
        fetch: (_) async => 'proxies: []',
        live: false,
      );

      expect(v.answer, AccessAnswer.allowed);
      expect(v.blocked, isFalse);
    });

    test('403 оператора — отказ с причиной и кодом', () async {
      final v = await probeSubscriptionUrl(
        'https://sub.example/x',
        fetch: (_) async => throw SubscriptionFetchException(
          'ответ сервера 403',
          statusCode: 403,
          body: 'Subscription inactive or expired',
          headers: _dailyQuotaHeaders(),
        ),
        live: false,
      );

      expect(v.blocked, isTrue);
      expect(v.refusal!.kind, AccessKind.dailyQuota);
      expect(v.statusCode, 403);
      // Улика для «Подробностей» несёт код, по которому причину узнали.
      expect(v.detail, contains('403'));
    });

    test('сеть молчит — это НЕ отказ', () async {
      final v = await probeSubscriptionUrl(
        'https://sub.example/x',
        fetch: (_) async =>
            throw const SubscriptionFetchException('сеть недоступна'),
        live: false,
      );

      expect(v.answer, AccessAnswer.unknown);
      expect(v.blocked, isFalse);
    });

    test('ответ не пришёл за бюджет — тоже не отказ', () async {
      final v = await probeSubscriptionUrl(
        'https://sub.example/x',
        fetch: (_) async {
          await Future<void>.delayed(const Duration(seconds: 5));
          return 'proxies: []';
        },
        live: false,
        budget: const Duration(milliseconds: 20),
      );

      expect(v.answer, AccessAnswer.unknown);
    });

    test('неопознанный 4xx: на подключении отказ, на живом туннеле — нет', () async {
      Future<String> portal(String _) async =>
          throw const SubscriptionFetchException(
            'ответ сервера 403',
            statusCode: 403,
            body: 'blocked by network policy',
          );

      final onConnect = await probeSubscriptionUrl(
        'https://sub.example/x',
        fetch: portal,
        live: false,
      );
      final onLiveTunnel = await probeSubscriptionUrl(
        'https://sub.example/x',
        fetch: portal,
        live: true,
      );

      // На подключении осторожность стоит дёшево: туннель ещё не поднят, и
      // подняться на кэше — это и есть ложная защита.
      expect(onConnect.blocked, isTrue);
      // На живом туннеле — наоборот: он работает, и гасить его по чужому
      // ответу нельзя.
      expect(onLiveTunnel.answer, AccessAnswer.unknown);
    });
  });

  group('сторож доступа', () {
    AccessGuard guardOf(Future<AccessVerdict> Function(bool) check) =>
        AccessGuard(
          check: check,
          first: const Duration(milliseconds: 5),
          every: const Duration(milliseconds: 5),
        );

    final blocked = AccessVerdict.refused(
      const AccessState(mayConnect: false, kind: AccessKind.dailyQuota),
      statusCode: 403,
    );

    // Оба случая ниже сняты на устройстве против РАЗВЁРНУТОЙ панели и оба
    // заканчивались зелёным щитом поверх мёртвого туннеля.

    test(
      'своих слов панели хватает, чтобы признать отказ отказом оператора',
      () {
        // Панель от 4 сентября при отказе не шлёт ни x-caramba-*, ни
        // subscription-userinfo — только это тело. Пока оно не считалось
        // уликой, живой сторож молчал, и человек 3 м 43 с видел «Защищено».
        final r = refusalFromResponse(
          statusCode: 403,
          body: 'Subscription inactive or expired',
        );

        expect(r, isNotNull);
        expect(r!.fromOperator, isTrue);
      },
    );

    test('свежий отказ переживает вопрос, оставшийся без ответа', () async {
      // Гонка с устройства: тап через полсекунды после отключения, сеть Android
      // ещё переключается, бюджет истекает раньше ответа. Без памяти об отказе
      // туннель поднимался на кэше и звался защитой.
      var answer = blocked;
      final guard = guardOf((_) async => answer);

      expect((await guard.checkBeforeConnect()).blocked, isTrue);
      answer = AccessVerdict.unknown;

      expect((await guard.checkBeforeConnect()).blocked, isTrue);
    });

    test('память об отказе не переживает свой срок', () async {
      // Оплативший тариф не должен упираться во вчерашний отказ.
      var answer = AccessVerdict.refused(
        const AccessState(mayConnect: false, kind: AccessKind.dailyQuota),
        statusCode: 403,
        at: DateTime.now().subtract(kRefusalMemory * 2),
      );
      final guard = guardOf((_) async => answer);

      expect((await guard.checkBeforeConnect()).blocked, isTrue);
      answer = AccessVerdict.unknown;

      expect(
        (await guard.checkBeforeConnect()).answer,
        AccessAnswer.unknown,
      );
    });

    test('перед подключением возвращает вердикт и запоминает его', () async {
      final guard = guardOf((_) async => blocked);

      final v = await guard.checkBeforeConnect();

      expect(v.blocked, isTrue);
      expect(guard.state.refusal!.kind, AccessKind.dailyQuota);
      guard.dispose();
    });

    test('новая попытка не тащит на экран прошлый отказ', () async {
      var answer = blocked;
      final guard = guardOf((_) async => answer);
      await guard.checkBeforeConnect();
      expect(guard.state.blocked, isTrue);

      answer = AccessVerdict.allowed();
      await guard.checkBeforeConnect();

      expect(guard.state.blocked, isFalse);
      guard.dispose();
    });

    test('на живом туннеле отказ доезжает без участия пользователя', () async {
      final guard = guardOf((_) async => blocked);

      guard.onStage(VpnStage.connected);
      await Future<void>.delayed(const Duration(milliseconds: 40));

      expect(guard.state.blocked, isTrue);
      guard.dispose();
    });

    test('простой на слабой сети щит не гасит', () async {
      // Человек подключился и убрал телефон в карман: ноль байт и молчащий
      // опрос. Это нормальная жизнь исправного туннеля, а не поломка.
      var asked = 0;
      final guard = guardOf((_) async {
        asked++;
        return AccessVerdict.unknown;
      });
      // Известно, что доступ есть.
      await guard.checkBeforeConnect();

      guard.onStage(VpnStage.connected);
      await Future<void>.delayed(const Duration(milliseconds: 40));

      expect(asked, greaterThan(1), reason: 'сторож обязан продолжать спрашивать');
      expect(guard.state.blocked, isFalse);
      guard.dispose();
    });

    test('обрыв туннеля будит сторожа спать', () async {
      var asked = 0;
      final guard = guardOf((_) async {
        asked++;
        return AccessVerdict.allowed();
      });

      guard.onStage(VpnStage.connected);
      await Future<void>.delayed(const Duration(milliseconds: 30));
      final duringSession = asked;
      guard.onStage(VpnStage.disconnected);
      await Future<void>.delayed(const Duration(milliseconds: 30));

      expect(asked, duringSession);
      guard.dispose();
    });

    test('отказ переживает обрыв: объяснение не исчезает вместе с попыткой', () async {
      final guard = guardOf((_) async => blocked);
      await guard.checkBeforeConnect();

      guard.onStage(VpnStage.disconnected);

      expect(guard.state.blocked, isTrue);
      guard.dispose();
    });
  });

  group('проводка провайдеров', () {
    // Отдельно от разбора: собранные по одному куски ничего не стоят, если
    // сторож в реальном контейнере спрашивает не ту подписку или его ответ не
    // доезжает до экранов.
    test('импортированная подписка спрашивается по своей ссылке, и отказ '
        'доезжает до общего провайдера доступа', () async {
      var asked = '';
      final container = ProviderContainer(
        overrides: [
          connectionProfilesStoreProvider.overrideWithValue(
            _Store(<ConnectionProfile>[_rawProfile], _rawProfile.id),
          ),
          subscriptionBodyFetchProvider.overrideWithValue((url) async {
            asked = url;
            throw SubscriptionFetchException(
              'ответ сервера 403',
              statusCode: 403,
              body: 'Subscription inactive or expired',
              headers: _dailyQuotaHeaders(),
            );
          }),
        ],
      );
      addTearDown(container.dispose);
      // Профили читаются асинхронно; до этого активного профиля ещё нет.
      await _profilesLoaded(container);

      final verdict = await container
          .read(accessGuardProvider.notifier)
          .checkBeforeConnect();

      expect(asked, 'https://app.exarobot.top/sub/feb7e480');
      expect(verdict.blocked, isTrue);
      expect(verdict.refusal!.kind, AccessKind.dailyQuota);
      // Тот же ответ обязан видеть весь остальной экранный слой — иначе список
      // серверов и карточка объяснения разошлись бы с дайлом.
      expect(
        container.read(subscriptionAccessProvider)?.kind,
        AccessKind.dailyQuota,
      );
    });

    test('вставленный текстом конфиг спрашивать негде — и это не отказ', () async {
      final pasted = _rawProfile.copyWith(source: '');
      final container = ProviderContainer(
        overrides: [
          connectionProfilesStoreProvider.overrideWithValue(
            _Store(<ConnectionProfile>[pasted], pasted.id),
          ),
          subscriptionBodyFetchProvider.overrideWithValue(
            (url) async => fail('ходить некуда: источника у профиля нет'),
          ),
        ],
      );
      addTearDown(container.dispose);
      await _profilesLoaded(container);

      final verdict = await container
          .read(accessGuardProvider.notifier)
          .checkBeforeConnect();

      expect(verdict.answer, AccessAnswer.unknown);
      expect(verdict.blocked, isFalse);
      expect(container.read(subscriptionAccessProvider), isNull);
    });
  });
}

/// Профили читаются из стора асинхронно; до этого активного профиля нет.
Future<void> _profilesLoaded(ProviderContainer container) async {
  for (var i = 0; i < 20; i++) {
    if (!container.read(connectionProfilesProvider).loading) return;
    await Future<void>.delayed(Duration.zero);
  }
}

const _rawProfile = ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Моя подписка',
  source: 'https://app.exarobot.top/sub/feb7e480',
  rawConfig: 'proxies: []',
  format: 'clash',
);

class _Store implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles;
  String? activeId;

  _Store(this.profiles, this.activeId);

  @override
  Future<List<ConnectionProfile>> readProfiles() async => profiles;

  @override
  Future<String?> readActiveId() async => activeId;

  @override
  Future<void> writeProfiles(List<ConnectionProfile> next) async =>
      profiles = next;

  @override
  Future<void> writeActiveId(String? id) async => activeId = id;

  @override
  Future<void> clear() async {
    profiles = const [];
    activeId = null;
  }
}

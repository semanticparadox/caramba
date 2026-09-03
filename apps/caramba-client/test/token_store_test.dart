// Хранилище токенов ключёвано по pid (02-SPEC.md 1.2).
//
// До этого рана TokenStore держал три фиксированных глобальных ключа с одним
// значением каждый, поэтому энроллмент второго оператора затирал сессию
// первого: продукт с несколькими операторами был сломан на слое хранения.
// Здесь проверяется, что корзины двух тенантов не пересекаются, что выход
// одного не выкидывает другого, и что миграция 06-MIGRATION.md 7.1 переносит
// единственную старую сессию, никого не потеряв.

import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/auth_tokens.dart';
import 'package:caramba_client/data/token_store.dart';

const String pidA = '226e8a20f699b964';
const String pidB = '8ac31d5540b2e70f';

AuthTokens tokensFor(String tag, int userId) => AuthTokens(
  accessToken: 'access-$tag',
  refreshToken: 'refresh-$tag',
  tokenType: 'Bearer',
  expiresIn: 900,
  userId: userId,
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late Map<String, String> backing;

  setUp(() {
    backing = <String, String>{};
    FlutterSecureStorage.setMockInitialValues(backing);
  });

  test('два оператора не затирают друг друга', () async {
    final a = TokenStore.forPid(pidA);
    final b = TokenStore.forPid(pidB);

    await a.save(tokensFor('a', 1));
    await b.save(tokensFor('b', 2));

    expect(await a.readAccess(), 'access-a');
    expect(await a.readRefresh(), 'refresh-a');
    expect(await a.readUserId(), 1);
    expect(await b.readAccess(), 'access-b');
    expect(await b.readRefresh(), 'refresh-b');
    expect(await b.readUserId(), 2);
  });

  test('имена ключей несут pid и не пересекаются', () async {
    await TokenStore.forPid(pidA).save(tokensFor('a', 1));
    await TokenStore.forPid(pidB).save(tokensFor('b', 2));

    expect(backing['caramba.$pidA.access_token'], 'access-a');
    expect(backing['caramba.$pidB.access_token'], 'access-b');
    // Глобальных имён при pid-корзине не появляется.
    expect(backing.containsKey('caramba.access_token'), isFalse);
    expect(backing.length, 6);
  });

  test('pid нормализуется, мусорный pid отвергается', () async {
    expect(TokenStore.forPid(pidA.toUpperCase()).pid, pidA);
    expect(TokenStore().pid, TokenStore.legacyPid);
    // Пространство ключей общее: точка внутри «pid» адресовала бы чужую
    // корзину, поэтому значение обязано быть настоящим pid.
    expect(() => TokenStore.forPid('caramba.access'), throwsArgumentError);
    expect(() => TokenStore.forPid('zz6e8a20f699b964'), throwsArgumentError);
    expect(() => TokenStore.forPid('226e8a20'), throwsArgumentError);
  });

  test('выход одного оператора не выкидывает другого', () async {
    final a = TokenStore.forPid(pidA);
    final b = TokenStore.forPid(pidB);
    await a.save(tokensFor('a', 1));
    await b.save(tokensFor('b', 2));

    await a.clear();

    expect(await a.hasSession(), isFalse);
    expect(await b.hasSession(), isTrue);
    expect(await b.readRefresh(), 'refresh-b');
  });

  group('миграция единственной сессии, 06-MIGRATION.md 7.1', () {
    setUp(() {
      backing
        ..['caramba.access_token'] = 'legacy-access'
        ..['caramba.refresh_token'] = 'legacy-refresh'
        ..['caramba.user_id'] = '42';
    });

    test(
      'старая сессия без метки переезжает к единственному владельцу',
      () async {
        final a = TokenStore.forPid(pidA);
        expect(await a.hasSession(), isFalse);

        expect(
          await a.adoptLegacySession(ownerId: 'profile-a', soleOwner: true),
          isTrue,
        );

        expect(await a.readAccess(), 'legacy-access');
        expect(await a.readRefresh(), 'legacy-refresh');
        expect(await a.readUserId(), 42);
        // Старые имена после переезда исчезают: второй дом у сессии не нужен.
        expect(backing.containsKey('caramba.refresh_token'), isFalse);
        expect(backing.containsKey('caramba.access_token'), isFalse);
        expect(backing.containsKey('caramba.user_id'), isFalse);
      },
    );

    test(
      'без метки и без единственного владельца перенос НЕ происходит',
      () async {
        // Двух операторов в установке достаточно, чтобы владелец блоба был
        // неизвестен. Отдать сессию первому спросившему значит подписать все его
        // запросы чужим bearer'ом.
        final a = TokenStore.forPid(pidA);
        expect(await a.adoptLegacySession(ownerId: 'profile-a'), isFalse);
        expect(await a.hasSession(), isFalse);
        expect(backing['caramba.refresh_token'], 'legacy-refresh');
      },
    );

    test('метка владельца отдаёт сессию только ему', () async {
      backing['caramba.session_owner'] = 'profile-b';

      final a = TokenStore.forPid(pidA);
      expect(
        await a.adoptLegacySession(ownerId: 'profile-a', soleOwner: true),
        isFalse,
      );
      expect(await a.hasSession(), isFalse);

      final b = TokenStore.forPid(pidB);
      expect(await b.adoptLegacySession(ownerId: 'profile-b'), isTrue);
      expect(await b.readRefresh(), 'legacy-refresh');
      // Метка уезжает вместе с блобом, а не остаётся указывать в пустоту.
      expect(backing.containsKey('caramba.session_owner'), isFalse);
    });

    test('перенос идемпотентен и не трогает чужую корзину', () async {
      final a = TokenStore.forPid(pidA);
      expect(
        await a.adoptLegacySession(ownerId: 'profile-a', soleOwner: true),
        isTrue,
      );
      expect(
        await a.adoptLegacySession(ownerId: 'profile-a', soleOwner: true),
        isFalse,
      );

      final b = TokenStore.forPid(pidB);
      expect(
        await b.adoptLegacySession(ownerId: 'profile-b', soleOwner: true),
        isFalse,
      );
      expect(await b.hasSession(), isFalse);
      expect(await a.readRefresh(), 'legacy-refresh');
    });

    test('своя сессия тенанта не затирается переносом', () async {
      final a = TokenStore.forPid(pidA);
      await a.save(tokensFor('a', 1));

      expect(
        await a.adoptLegacySession(ownerId: 'profile-a', soleOwner: true),
        isFalse,
      );

      expect(await a.readRefresh(), 'refresh-a');
      // Чужой блоб остаётся на месте, а не пропадает молча.
      expect(backing['caramba.refresh_token'], 'legacy-refresh');
    });

    test('legacy-корзина читает старые ключи и никого не теряет', () async {
      final legacy = TokenStore();

      expect(await legacy.hasSession(), isTrue);
      expect(await legacy.readAccess(), 'legacy-access');
      expect(await legacy.readUserId(), 42);
      // На самой legacy-корзине переносить некуда.
      expect(
        await legacy.adoptLegacySession(ownerId: 'profile-a', soleOwner: true),
        isFalse,
      );
    });

    test('save в legacy-корзину пишет метку владельца', () async {
      final legacy = TokenStore();
      await legacy.save(tokensFor('a', 7), ownerId: 'profile-a');
      expect(backing['caramba.session_owner'], 'profile-a');

      // И эта метка решает, кому блоб достанется.
      final b = TokenStore.forPid(pidB);
      expect(
        await b.adoptLegacySession(ownerId: 'profile-b', soleOwner: true),
        isFalse,
      );
      final a = TokenStore.forPid(pidA);
      expect(await a.adoptLegacySession(ownerId: 'profile-a'), isTrue);
      expect(await a.readRefresh(), 'refresh-a');
    });
  });
}

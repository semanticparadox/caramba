import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'package:caramba_client/data/models/auth_tokens.dart';

/// Хранилище JWT-пары в платформенном secure storage (Keychain/Keystore/
/// libsecret/DPAPI), КЛЮЧЁВАННОЕ ПО `pid`. Единственный источник истины по
/// токенам — всё чтение/запись токенов идёт через него.
///
/// 02-SPEC.md 1.2: «Каждое хранилище состояния профиля ОБЯЗАНО ключеваться по
/// `pid`». Изоляция тенантов в CSM/1 это свойство ДОКУМЕНТОВ и ХРАНИЛИЩА, а не
/// сессии. До этого класс держал три фиксированных глобальных ключа с одним
/// значением каждый, поэтому энроллмент второго оператора затирал сессию
/// первого: продукт с несколькими операторами был сломан на слое хранения.
///
/// Раскладка ключей:
///
///   * тенант с известным pid  -> `caramba.<pid>.access_token` и т.д.;
///   * профиль без pid (legacy `rawSub`, 06-MIGRATION.md 7.1) -> старые
///     глобальные ключи `caramba.access_token` и т.д.
///
/// Вторая ветка это НЕ дыра, а именованная legacy-корзина миграции: у профиля
/// типа `rawSub` тенанта нет, а уже вошедший пользователь не должен потерять
/// сессию от одного лишь обновления приложения. Как только pid профиля
/// известен, [adoptLegacySession] переносит эту единственную сессию в корзину
/// pid, и глобальные ключи исчезают.
///
/// `pid` это 16 шестнадцатеричных символов (`03-WIRE.md` 4). Значение
/// валидируется: пространство ключей общее, и произвольная строка с точкой
/// внутри позволила бы одному профилю адресовать корзину другого.
class TokenStore {
  /// Корзина профиля без тенанта (legacy `rawSub`, 06-MIGRATION.md 7.1).
  static const String legacyPid = '';

  /// Префикс пространства ключей. Общий для всех корзин.
  static const String _prefix = 'caramba';

  static const String _fieldAccess = 'access_token';
  static const String _fieldRefresh = 'refresh_token';
  static const String _fieldUserId = 'user_id';

  /// Владелец legacy-корзины: идентификатор профиля, который её записал.
  ///
  /// Метка появилась потому, что перенос без владельца это раздача чужой
  /// сессии. Legacy-корзина одна на установку, а профилей может быть
  /// несколько, и первый попавшийся `pid`, спросивший перенос, забирал access и
  /// refresh оператора A в корзину оператора B: с этого момента запросы B шли
  /// с bearer'ом A.
  static const String _fieldOwner = 'session_owner';

  /// Тенант, чью сессию держит этот экземпляр. Пустая строка — legacy-корзина.
  final String pid;

  final FlutterSecureStorage _storage;

  TokenStore({FlutterSecureStorage? storage, String pid = legacyPid})
    : pid = _normalizePid(pid),
      _storage =
          storage ??
          const FlutterSecureStorage(
            aOptions: AndroidOptions(encryptedSharedPreferences: true),
            iOptions: IOSOptions(
              accessibility: KeychainAccessibility.first_unlock,
            ),
          );

  /// Хранилище сессии конкретного тенанта.
  TokenStore.forPid(String pid, {FlutterSecureStorage? storage})
    : this(pid: pid, storage: storage);

  /// Приводит pid к канону и отвергает всё, что не 16 hex-символов.
  ///
  /// Пустая строка допустима и означает legacy-корзину. Любое другое значение
  /// обязано быть настоящим pid: иначе имя ключа перестаёт быть однозначным.
  static String _normalizePid(String raw) {
    final v = raw.trim().toLowerCase();
    if (v.isEmpty) return legacyPid;
    if (v.length != 16 || !_isHex(v)) {
      throw ArgumentError.value(raw, 'pid', 'pid обязан быть 16 hex-символами');
    }
    return v;
  }

  static bool _isHex(String v) {
    for (var i = 0; i < v.length; i++) {
      final c = v.codeUnitAt(i);
      final isDigit = c >= 0x30 && c <= 0x39;
      final isLower = c >= 0x61 && c <= 0x66;
      if (!isDigit && !isLower) return false;
    }
    return true;
  }

  /// Полное имя ключа поля в корзине этого pid.
  String keyFor(String field) =>
      pid.isEmpty ? '$_prefix.$field' : '$_prefix.$pid.$field';

  String get _kAccess => keyFor(_fieldAccess);

  String get _kRefresh => keyFor(_fieldRefresh);

  String get _kUserId => keyFor(_fieldUserId);

  String get _kOwner => keyFor(_fieldOwner);

  /// Сохраняет сессию.
  ///
  /// [ownerId] это идентификатор профиля, которому сессия принадлежит. В
  /// legacy-корзине он записывается рядом с токенами и позже решает, кому
  /// [adoptLegacySession] имеет право её отдать. В корзине с известным `pid`
  /// владелец и так однозначен, и метка не нужна.
  Future<void> save(AuthTokens tokens, {String? ownerId}) async {
    await _storage.write(key: _kAccess, value: tokens.accessToken);
    await _storage.write(key: _kRefresh, value: tokens.refreshToken);
    await _storage.write(key: _kUserId, value: tokens.userId.toString());
    if (pid.isEmpty && ownerId != null && ownerId.isNotEmpty) {
      await _storage.write(key: _kOwner, value: ownerId);
    }
  }

  Future<String?> readAccess() => _storage.read(key: _kAccess);

  Future<String?> readRefresh() => _storage.read(key: _kRefresh);

  Future<int?> readUserId() async {
    final v = await _storage.read(key: _kUserId);
    return v == null ? null : int.tryParse(v);
  }

  /// Есть ли вообще сохранённая сессия (refresh-токен).
  Future<bool> hasSession() async => (await readRefresh())?.isNotEmpty ?? false;

  /// Переносит единственную старую (глобальную) сессию в корзину этого pid.
  ///
  /// 06-MIGRATION.md 7.1: существующий единственный блоб переезжает в pid того
  /// профиля, КОТОРОМУ ОН ПРИНАДЛЕЖИТ, а блоб без владеющего pid остаётся в
  /// legacy-корзине `rawSub`. Слово "принадлежит" здесь и есть всё правило, и
  /// оно проверяется, а не подразумевается:
  ///
  ///   * на legacy-корзине это no-op (переносить некуда и незачем);
  ///   * если в корзине pid уже есть сессия, старые ключи не трогаются: чужая
  ///     запись не имеет права затереть сессию тенанта;
  ///   * если у блоба есть метка владельца и она не [ownerId], перенос НЕ
  ///     происходит: это сессия другого профиля, и отдать её сюда значит
  ///     подписать все запросы этого тенанта чужим bearer'ом;
  ///   * если метки нет (блоб записан версией до неё), перенос происходит
  ///     ТОЛЬКО когда вызывающий может утверждать [soleOwner], то есть в
  ///     установке ровно один профиль, способный владеть панельной сессией.
  ///     С двумя профилями владелец неизвестен, и правильный ответ это отказ,
  ///     а не догадка;
  ///   * копирование идёт ДО удаления, поэтому обрыв посередине оставляет
  ///     сессию читаемой хотя бы по одному из имён.
  ///
  /// Идемпотентен. Возвращает `true`, если перенос действительно случился.
  Future<bool> adoptLegacySession({
    required String ownerId,
    bool soleOwner = false,
  }) async {
    if (pid.isEmpty || ownerId.isEmpty) return false;
    if (await hasSession()) return false;

    final legacyRefresh = await _storage.read(key: '$_prefix.$_fieldRefresh');
    if (legacyRefresh == null || legacyRefresh.isEmpty) return false;

    final marker = await _storage.read(key: '$_prefix.$_fieldOwner');
    if (marker != null && marker.isNotEmpty) {
      if (marker != ownerId) return false;
    } else if (!soleOwner) {
      return false;
    }

    final legacyAccess = await _storage.read(key: '$_prefix.$_fieldAccess');
    final legacyUserId = await _storage.read(key: '$_prefix.$_fieldUserId');

    if (legacyAccess != null) {
      await _storage.write(key: _kAccess, value: legacyAccess);
    }
    await _storage.write(key: _kRefresh, value: legacyRefresh);
    if (legacyUserId != null) {
      await _storage.write(key: _kUserId, value: legacyUserId);
    }

    await _storage.delete(key: '$_prefix.$_fieldAccess');
    await _storage.delete(key: '$_prefix.$_fieldRefresh');
    await _storage.delete(key: '$_prefix.$_fieldUserId');
    await _storage.delete(key: '$_prefix.$_fieldOwner');
    return true;
  }

  /// Чистит ТОЛЬКО свою корзину. Выход одного оператора не выкидывает других.
  Future<void> clear() async {
    await _storage.delete(key: _kAccess);
    await _storage.delete(key: _kRefresh);
    await _storage.delete(key: _kUserId);
    await _storage.delete(key: _kOwner);
  }
}

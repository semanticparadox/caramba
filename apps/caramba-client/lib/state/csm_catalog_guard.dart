/// Сужение защиты, которое приходит В КАТАЛОГЕ, а не в настройке.
///
/// Нормативно: 02-SPEC.md 7.7 (закрытый список сужений), 7.7.1 (почему смена
/// ресурса это сужение), INV-22 (карточка «Оставить или Вернуть» безусловна),
/// 04-THREAT-MODEL.md 7.3 шаг 5 и таблица `rs`, `geo`, `ro[].rs`.
///
/// Каталог называет каждый rule-set и geo-файл путём и sha256, и INV-12 не даёт
/// применить байты, чей хеш не сошёлся. Это связывает КАЖДОГО ХОСТА на пути
/// выборки. Это не связывает того, кто ПОДПИСАЛ: путь и хеш выбрал он сам.
/// Враждебный или скомпрометированный оператор публикует rule-set, который
/// уводит названный набор доменов в DIRECT, хеш сходится, INV-12 доволен, и
/// трафик уходит с устройства в открытом виде, пока туннель показывает
/// «подключено». В формате нет ничего, что это ограничивало бы, поэтому
/// ограничивает клиент, и единственное его средство это карточка.
///
/// Три строки списка 7.7 приходят сюда:
///   1. набор провайдеров правил изменился: запись `rs` или `geo` добавлена,
///      удалена или у неё поменялось `n`;
///   2. у записи с прежним `n` поменялся хеш `h`;
///   3. у записи маршрута поменялся список `rs`.
///
/// Карточка поднимается БЕЗУСЛОВНО и независимо от происхождения, называет
/// имена провайдеров и НЕ рендерит ни одной строки оператора (INV-10).
library;

import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/prefs_store.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/vpn/vpn_service.dart';

/// Префикс ключа хранилища. Карточка обязана пережить перезапуск: карточка,
/// которую закрыл перезапуск, отвечена молчанием, а это ровно то, что
/// 02-SPEC.md 7.7 запрещает.
///
/// Ключ ключуется по профилю (02-SPEC.md 1.2). Один глобальный ключ означал бы,
/// что проверенный набор оператора A решает, поднимать ли карточку на первый
/// каталог оператора B, и что ответ пользователя одному оператору применяется
/// к другому.
const String kCsmCatalogGuardKey = 'caramba.csm_catalog_guard';

/// Ключ хранилища стража для профиля [profileId]. Пустой или `null` профиль
/// оставляет старое глобальное имя: так лежат установки, заведённые до
/// разделения по профилям, и терять их ответ незачем.
String csmCatalogGuardKeyFor(String? profileId) =>
    (profileId == null || profileId.isEmpty)
    ? kCsmCatalogGuardKey
    : '$kCsmCatalogGuardKey.$profileId';

/// Сколько карточек может висеть одновременно. Четвёртая схлопывается в самую
/// старую, и ни одна не выбрасывается (02-SPEC.md 7.7).
const int kCsmCatalogCardLimit = 3;

/// Одна запись `rs` или `geo` доверенного каталога.
class CsmResourceRef {
  const CsmResourceRef({
    required this.kind,
    required this.name,
    required this.hash,
  });

  /// `rs` либо `geo`.
  final String kind;
  final String name;

  /// Подписанный sha256 в hex.
  final String hash;

  /// Короткий вид хеша для экрана. Полный хеш это 64 символа, и строка из 64
  /// символов на карточке не читается человеком, ради которого она поднята.
  String get shortHash => hash.length <= 12 ? hash : hash.substring(0, 12);

  Map<String, Object?> toJson() => <String, Object?>{
    'kind': kind,
    'name': name,
    'hash': hash,
  };

  static CsmResourceRef? fromJson(Object? raw) {
    if (raw is! Map) {
      return null;
    }
    final name = raw['name'];
    if (name is! String || name.isEmpty) {
      return null;
    }
    return CsmResourceRef(
      kind: raw['kind'] is String ? raw['kind'] as String : 'rs',
      name: name,
      hash: raw['hash'] is String ? raw['hash'] as String : '',
    );
  }

  @override
  bool operator ==(Object other) =>
      other is CsmResourceRef &&
      other.kind == kind &&
      other.name == name &&
      other.hash == hash;

  @override
  int get hashCode => Object.hash(kind, name, hash);
}

/// Отпечаток каталога в той его части, которая относится к 7.7.1: набор
/// ресурсов и списки правил маршрутов.
class CsmCatalogFingerprint {
  const CsmCatalogFingerprint({
    this.resources = const <CsmResourceRef>[],
    this.routes = const <String, List<String>>{},
  });

  static const CsmCatalogFingerprint empty = CsmCatalogFingerprint();

  final List<CsmResourceRef> resources;

  /// `ro[].id` -> `ro[].rs`.
  final Map<String, List<String>> routes;

  bool get isEmpty => resources.isEmpty && routes.isEmpty;

  Map<String, CsmResourceRef> get byName => <String, CsmResourceRef>{
    for (final r in resources) r.name: r,
  };

  Map<String, Object?> toJson() => <String, Object?>{
    'resources': resources.map((r) => r.toJson()).toList(growable: false),
    'routes': routes,
  };

  static CsmCatalogFingerprint fromJson(Object? raw) {
    if (raw is! Map) {
      return empty;
    }
    final rawResources = raw['resources'];
    final resources = rawResources is List
        ? rawResources
              .map(CsmResourceRef.fromJson)
              .whereType<CsmResourceRef>()
              .toList(growable: false)
        : const <CsmResourceRef>[];
    final rawRoutes = raw['routes'];
    final routes = <String, List<String>>{};
    if (rawRoutes is Map) {
      for (final e in rawRoutes.entries) {
        final id = '${e.key}';
        final v = e.value;
        routes[id] = v is List
            ? v.whereType<String>().toList(growable: false)
            : const <String>[];
      }
    }
    return CsmCatalogFingerprint(resources: resources, routes: routes);
  }

  /// Собирает отпечаток из снимка ядра (`CsmStateJSON`).
  static CsmCatalogFingerprint fromCoreSnapshot(Map<Object?, Object?> snap) {
    final rawResources = snap['resources'];
    final resources = <CsmResourceRef>[];
    if (rawResources is List) {
      for (final e in rawResources) {
        final ref = CsmResourceRef.fromJson(e);
        if (ref != null) {
          resources.add(ref);
        }
      }
    }
    final routes = <String, List<String>>{};
    final rawRoutes = snap['routes'];
    if (rawRoutes is List) {
      for (final e in rawRoutes) {
        if (e is! Map) {
          continue;
        }
        final id = e['id'];
        if (id is! String || id.isEmpty) {
          continue;
        }
        final rs = e['rs'];
        routes[id] = rs is List
            ? rs.whereType<String>().toList(growable: false)
            : const <String>[];
      }
    }
    return CsmCatalogFingerprint(resources: resources, routes: routes);
  }

  bool sameAs(CsmCatalogFingerprint other) {
    if (resources.length != other.resources.length ||
        routes.length != other.routes.length) {
      return false;
    }
    final mine = byName;
    final theirs = other.byName;
    if (mine.length != theirs.length) {
      return false;
    }
    for (final e in mine.entries) {
      if (theirs[e.key] != e.value) {
        return false;
      }
    }
    for (final e in routes.entries) {
      final other0 = other.routes[e.key];
      if (other0 == null || other0.join(',') != e.value.join(',')) {
        return false;
      }
    }
    return true;
  }
}

/// Что именно поменялось. Словарь закрыт: три строки 02-SPEC.md 7.7.1, из
/// которых первая распадается на добавление, удаление и переименование.
enum CsmCatalogChangeKind {
  resourceAdded,
  resourceRemoved,

  /// Прежний хеш появился под другим именем. Формально это удаление плюс
  /// добавление; названо отдельно, потому что пользователю честнее сказать
  /// «переименован», чем показать два несвязанных события.
  resourceRenamed,

  /// Имя прежнее, хеш другой. Ровно та строка, ради которой написан 7.7.1:
  /// байты за прежним именем теперь другие, и что в них, клиент не знает.
  resourceHashChanged,

  /// У маршрута поменялся список правил.
  routeRulesChanged,
}

/// Одна строка карточки.
class CsmCatalogChangeRow {
  const CsmCatalogChangeRow({
    required this.kind,
    required this.name,
    this.previous = '',
    this.proposed = '',
  });

  final CsmCatalogChangeKind kind;

  /// Имя провайдера правил либо идентификатор маршрута. Это ЗНАЧЕНИЕ ИЗ
  /// ПОДПИСАННОГО КАТАЛОГА и рендерится инертным текстом; описания оператора
  /// на этой поверхности нет и быть не может (INV-10).
  final String name;

  final String previous;
  final String proposed;

  Map<String, Object?> toJson() => <String, Object?>{
    'kind': kind.name,
    'name': name,
    'previous': previous,
    'proposed': proposed,
  };

  static CsmCatalogChangeRow? fromJson(Object? raw) {
    if (raw is! Map) {
      return null;
    }
    final name = raw['name'];
    if (name is! String || name.isEmpty) {
      return null;
    }
    CsmCatalogChangeKind? kind;
    for (final k in CsmCatalogChangeKind.values) {
      if (k.name == raw['kind']) {
        kind = k;
      }
    }
    if (kind == null) {
      return null;
    }
    return CsmCatalogChangeRow(
      kind: kind,
      name: name,
      previous: raw['previous'] is String ? raw['previous'] as String : '',
      proposed: raw['proposed'] is String ? raw['proposed'] as String : '',
    );
  }
}

/// Карточка «Оставить или Вернуть» про каталог.
class CsmCatalogChange {
  const CsmCatalogChange({
    required this.id,
    required this.raisedMs,
    required this.rows,
    required this.proposed,
  });

  final String id;
  final int raisedMs;
  final List<CsmCatalogChangeRow> rows;

  /// Набор, который предлагает оператор. НЕ применён, пока пользователь не
  /// ответил.
  final CsmCatalogFingerprint proposed;

  Map<String, Object?> toJson() => <String, Object?>{
    'id': id,
    'raised_ms': raisedMs,
    'rows': rows.map((r) => r.toJson()).toList(growable: false),
    'proposed': proposed.toJson(),
  };

  static CsmCatalogChange? fromJson(Object? raw) {
    if (raw is! Map) {
      return null;
    }
    final id = raw['id'];
    if (id is! String || id.isEmpty) {
      return null;
    }
    final rawRows = raw['rows'];
    final rows = rawRows is List
        ? rawRows
              .map(CsmCatalogChangeRow.fromJson)
              .whereType<CsmCatalogChangeRow>()
              .toList(growable: false)
        : const <CsmCatalogChangeRow>[];
    if (rows.isEmpty) {
      return null;
    }
    return CsmCatalogChange(
      id: id,
      raisedMs: (raw['raised_ms'] as num?)?.toInt() ?? 0,
      rows: rows,
      proposed: CsmCatalogFingerprint.fromJson(raw['proposed']),
    );
  }
}

/// Состояние стража: набор, на котором клиент стоит, и висящие карточки.
class CsmCatalogGuardState {
  const CsmCatalogGuardState({
    this.verified,
    this.pending = const <CsmCatalogChange>[],
  });

  static const CsmCatalogGuardState empty = CsmCatalogGuardState();

  /// Ранее проверенный набор ресурсов, тот, на котором клиент остаётся, пока
  /// пользователь не ответил. `null` означает «каталога ещё не видели».
  final CsmCatalogFingerprint? verified;

  final List<CsmCatalogChange> pending;

  Map<String, Object?> toJson() => <String, Object?>{
    if (verified != null) 'verified': verified!.toJson(),
    'pending': pending.map((c) => c.toJson()).toList(growable: false),
  };

  static CsmCatalogGuardState fromJson(Object? raw) {
    if (raw is! Map) {
      return empty;
    }
    final rawPending = raw['pending'];
    return CsmCatalogGuardState(
      verified: raw['verified'] == null
          ? null
          : CsmCatalogFingerprint.fromJson(raw['verified']),
      pending: rawPending is List
          ? rawPending
                .map(CsmCatalogChange.fromJson)
                .whereType<CsmCatalogChange>()
                .toList(growable: false)
          : const <CsmCatalogChange>[],
    );
  }
}

/// Сравнивает два набора и возвращает строки карточки.
///
/// Пустой результат означает «ничего из закрытого списка 7.7.1 не изменилось».
List<CsmCatalogChangeRow> csmCatalogDiff(
  CsmCatalogFingerprint previous,
  CsmCatalogFingerprint next,
) {
  final out = <CsmCatalogChangeRow>[];
  final before = previous.byName;
  final after = next.byName;

  // Переименование: прежний хеш встретился под другим именем. Считается ДО
  // добавлений и удалений, чтобы одно событие не рассыпалось на два.
  final beforeByHash = <String, CsmResourceRef>{
    for (final r in previous.resources)
      if (r.hash.isNotEmpty) r.hash: r,
  };
  final renamedFrom = <String>{};
  final renamedTo = <String>{};
  for (final r in next.resources) {
    if (after.containsKey(r.name) && before.containsKey(r.name)) {
      continue;
    }
    final old = beforeByHash[r.hash];
    if (old != null && old.name != r.name && !after.containsKey(old.name)) {
      renamedFrom.add(old.name);
      renamedTo.add(r.name);
      out.add(
        CsmCatalogChangeRow(
          kind: CsmCatalogChangeKind.resourceRenamed,
          name: r.name,
          previous: old.name,
          proposed: r.name,
        ),
      );
    }
  }

  for (final r in next.resources) {
    if (renamedTo.contains(r.name)) {
      continue;
    }
    final old = before[r.name];
    if (old == null) {
      out.add(
        CsmCatalogChangeRow(
          kind: CsmCatalogChangeKind.resourceAdded,
          name: r.name,
          proposed: r.shortHash,
        ),
      );
    } else if (old.hash != r.hash) {
      out.add(
        CsmCatalogChangeRow(
          kind: CsmCatalogChangeKind.resourceHashChanged,
          name: r.name,
          previous: old.shortHash,
          proposed: r.shortHash,
        ),
      );
    }
  }
  for (final r in previous.resources) {
    if (after.containsKey(r.name) || renamedFrom.contains(r.name)) {
      continue;
    }
    out.add(
      CsmCatalogChangeRow(
        kind: CsmCatalogChangeKind.resourceRemoved,
        name: r.name,
        previous: r.shortHash,
      ),
    );
  }

  // Третья строка: список правил маршрута. Маршрут, которого раньше не было,
  // здесь не считается: его правила это не смена набора, а новый маршрут, и
  // выбирает его пользователь.
  for (final e in next.routes.entries) {
    final old = previous.routes[e.key];
    if (old == null) {
      continue;
    }
    if (old.join(',') != e.value.join(',')) {
      out.add(
        CsmCatalogChangeRow(
          kind: CsmCatalogChangeKind.routeRulesChanged,
          name: e.key,
          previous: old.join(', '),
          proposed: e.value.join(', '),
        ),
      );
    }
  }
  return List<CsmCatalogChangeRow>.unmodifiable(out);
}

/// Страж каталога: держит проверенный набор, сравнивает с приходящим и
/// поднимает карточку.
class CsmCatalogGuard extends StateNotifier<CsmCatalogGuardState> {
  CsmCatalogGuard(this._prefs) : super(CsmCatalogGuardState.empty);

  final PrefsStore? _prefs;

  /// Профиль, чей набор сейчас в руках. `null` до первой привязки.
  String? _profileId;

  /// Профиль, к которому привязан страж.
  String? get profileId => _profileId;

  /// Поднимает сохранённое состояние. Зовётся из [appBootProvider].
  void hydrate(CsmCatalogGuardState next) => state = next;

  /// Переключает стража на профиль [profileId].
  ///
  /// Проверенный набор и висящие карточки хранятся НА ПРОФИЛЬ: набор оператора
  /// A не имеет права решать, поднимать ли карточку на первый каталог
  /// оператора B, а ответ, данный одному оператору, не имеет права применяться
  /// к другому. Состояние берётся из корзины нового профиля, а не обнуляется:
  /// карточка, на которую пользователь ещё не ответил, обязана пережить и
  /// переключение профиля, и перезапуск.
  void bindProfile(String? profileId) {
    if (_profileId == profileId) {
      return;
    }
    _profileId = profileId;
    final raw = _prefs?.readJson(csmCatalogGuardKeyFor(profileId));
    if (raw == null || raw.isEmpty) {
      state = CsmCatalogGuardState.empty;
      return;
    }
    try {
      state = CsmCatalogGuardState.fromJson(raw);
    } catch (_) {
      // Сломанный снимок это отсутствие снимка, а не отказ.
      state = CsmCatalogGuardState.empty;
    }
  }

  /// Принимает набор, который принёс проверенный каталог.
  ///
  /// Первый в жизни профиля набор принимается МОЛЧА: сузить относительно
  /// ничего нельзя, и карточка здесь спрашивала бы про изменение, которого не
  /// было. Дальше любое расхождение поднимает карточку, безусловно и
  /// независимо от происхождения.
  bool observe(CsmCatalogFingerprint next, {required int nowMs}) {
    if (next.isEmpty) {
      return false;
    }
    final verified = state.verified;
    if (verified == null) {
      _commit(CsmCatalogGuardState(verified: next, pending: state.pending));
      return false;
    }
    if (verified.sameAs(next)) {
      return false;
    }
    // Тот же самый набор уже вынесен на карточку: второй такой же вопрос это
    // не второе изменение.
    for (final card in state.pending) {
      if (card.proposed.sameAs(next)) {
        return false;
      }
    }
    final rows = csmCatalogDiff(verified, next);
    if (rows.isEmpty) {
      return false;
    }
    final card = CsmCatalogChange(
      id: 'cat_$nowMs',
      raisedMs: nowMs,
      rows: rows,
      proposed: next,
    );
    _commit(
      CsmCatalogGuardState(
        verified: verified,
        pending: _appendCapped(state.pending, card),
      ),
    );
    return true;
  }

  /// «Оставить моё»: клиент остаётся на ранее проверенном наборе ресурсов.
  void keep(String cardId) {
    if (!_has(cardId)) {
      return;
    }
    _commit(
      CsmCatalogGuardState(
        verified: state.verified,
        pending: _without(state.pending, cardId),
      ),
    );
  }

  /// «Принять новое»: предложенный набор становится проверенным.
  void accept(String cardId) {
    CsmCatalogChange? card;
    for (final c in state.pending) {
      if (c.id == cardId) {
        card = c;
      }
    }
    if (card == null) {
      return;
    }
    _commit(
      CsmCatalogGuardState(
        verified: card.proposed,
        pending: _without(state.pending, cardId),
      ),
    );
  }

  /// Смена профиля: набор и карточки хранятся на профиль.
  void clear() => _commit(CsmCatalogGuardState.empty);

  bool _has(String id) => state.pending.any((c) => c.id == id);

  void _commit(CsmCatalogGuardState next) {
    state = next;
    _prefs?.writeJson(csmCatalogGuardKeyFor(_profileId), next.toJson());
  }

  /// Четвёртая карточка схлопывается в самую старую: карточки не выбрасываются
  /// (02-SPEC.md 7.7).
  static List<CsmCatalogChange> _appendCapped(
    List<CsmCatalogChange> cards,
    CsmCatalogChange card,
  ) {
    if (cards.length < kCsmCatalogCardLimit) {
      return List<CsmCatalogChange>.unmodifiable(<CsmCatalogChange>[
        ...cards,
        card,
      ]);
    }
    final oldest = cards.first;
    final merged = CsmCatalogChange(
      id: oldest.id,
      raisedMs: oldest.raisedMs,
      rows: List<CsmCatalogChangeRow>.unmodifiable(<CsmCatalogChangeRow>[
        ...oldest.rows,
        ...card.rows,
      ]),
      // Ответ на схлопнутую карточку отвечает про САМЫЙ СВЕЖИЙ набор: именно
      // его предлагает оператор сейчас.
      proposed: card.proposed,
    );
    return List<CsmCatalogChange>.unmodifiable(<CsmCatalogChange>[
      merged,
      ...cards.skip(1),
    ]);
  }

  static List<CsmCatalogChange> _without(
    List<CsmCatalogChange> cards,
    String id,
  ) => List<CsmCatalogChange>.unmodifiable(cards.where((c) => c.id != id));
}

final csmCatalogGuardProvider =
    StateNotifierProvider<CsmCatalogGuard, CsmCatalogGuardState>(
      (ref) => CsmCatalogGuard(ref.watch(prefsStoreProvider)),
    );

/// Висящие карточки каталога. Пусто, когда их нет.
final csmCatalogChangesProvider = Provider<List<CsmCatalogChange>>(
  (ref) => ref.watch(csmCatalogGuardProvider).pending,
);

/// Передаёт ответ пользователя на карточку каталога СНАЧАЛА ядру, и только
/// потом снимает карточку.
///
/// Порядок здесь и есть смысл функции. Ресурсы грузит ядро, и до этого вызова
/// оно удерживает прежний набор; ответ, оставшийся в слое Dart, не откатывал бы
/// ничего, а карточка при этом обещает пользователю, что прежний набор
/// действует, пока он не ответит (04-THREAT-MODEL.md 7.3 шаг 5).
///
/// Отказ ядра НЕ снимает карточку: пользователь, увидевший, что вопрос исчез,
/// считает, что ответил, а ответ никуда не дошёл. Возвращает `true`, когда
/// ответ принят.
Future<bool> csmAnswerCatalogCard({
  required VpnConnection connection,
  required CsmCatalogGuard guard,
  required String cardId,
  required bool accept,
}) async {
  try {
    await connection.csmAnswerCatalogChange(accept: accept);
  } on Object {
    return false;
  }
  if (accept) {
    guard.accept(cardId);
  } else {
    guard.keep(cardId);
  }
  return true;
}

/// Читает снимок ядра и отдаёт стражу набор ресурсов проверенного каталога.
///
/// Возвращает `true`, когда карточка поднялась. Ошибка чтения не является
/// изменением: мост, которого на этой платформе нет, не должен выглядеть как
/// оператор, убравший все правила.
Future<bool> csmPumpCatalogGuard({
  required VpnConnection connection,
  required CsmCatalogGuard guard,
  int? nowMs,
}) async {
  final String raw;
  try {
    raw = await connection.csmState();
  } on Object {
    return false;
  }
  if (raw.trim().isEmpty) {
    return false;
  }
  final Object? decoded;
  try {
    decoded = jsonDecode(raw);
  } on FormatException {
    return false;
  }
  if (decoded is! Map<Object?, Object?> || decoded['error'] is String) {
    return false;
  }
  final fingerprint = CsmCatalogFingerprint.fromCoreSnapshot(decoded);
  return guard.observe(
    fingerprint,
    nowMs: nowMs ?? DateTime.now().millisecondsSinceEpoch,
  );
}

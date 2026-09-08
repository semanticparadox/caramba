/// Словарь настроек CSM/1 как типизированная модель.
///
/// Нормативно: 02-SPEC.md 7 целиком. Здесь живут закрытый реестр настроек
/// (7.2), полные словари значений (7.3), трёхсторонняя семантика патча (7.5),
/// происхождение и старшинство (7.6), карточка «Оставить или Вернуть» (7.7) и
/// правило «оператор не поддерживает настройку» (7.9).
///
/// Что здесь НЕ живёт: разбор кадра (это packages/caramba_vpn/lib/src/csm) и
/// применение политики к ядру (это вызывающая сторона, собирающая CorePolicy).
library;

import 'package:caramba_vpn/csm.dart'
    show CborArray, CborBool, CborMap, CborText, CborUint, CborValue;

/// Происхождение значения одного ключа, `src` из 03-WIRE.md 5.
enum CsmProvenance {
  user(1),
  operator(2),
  byDefault(3);

  const CsmProvenance(this.wire);

  final int wire;

  static CsmProvenance? fromWire(int? raw) {
    for (final p in CsmProvenance.values) {
      if (p.wire == raw) {
        return p;
      }
    }
    return null;
  }
}

/// Область распространения настройки (02-SPEC.md 7.2).
enum CsmSettingScope {
  /// Значение расходится по всем устройствам пользователя.
  subscription,

  /// Значение локально для устройства: списки приложений и сетевые стеки
  /// платформозависимы.
  device,
}

/// Когда изменение оператора поднимает карточку «Оставить или Вернуть».
enum CsmCardPolicy {
  /// Никогда: контрол не рендерится в v1.
  never,

  /// Только если пользователь ставил этот ключ явно на этом устройстве.
  onlyIfUserSet,

  /// Всегда, безусловно, независимо от происхождения: сужение защиты.
  always,
}

/// Тип значения на проводе.
enum CsmSettingType { text, uint, boolean, textList }

/// Ключ настройки. Реестр закрытый: настройки, которой здесь нет, не
/// существует ни в одну сторону (02-SPEC.md 7.2).
enum CsmSettingKey {
  protocol(
    1,
    CsmSettingType.text,
    CsmSettingScope.subscription,
    operatorMayWrite: true,
    card: CsmCardPolicy.onlyIfUserSet,
  ),
  preset(
    2,
    CsmSettingType.text,
    CsmSettingScope.subscription,
    operatorMayWrite: true,
    card: CsmCardPolicy.onlyIfUserSet,
  ),
  relay(
    3,
    CsmSettingType.text,
    CsmSettingScope.subscription,
    operatorMayWrite: true,
    card: CsmCardPolicy.onlyIfUserSet,
  ),
  stack(
    4,
    CsmSettingType.text,
    CsmSettingScope.device,
    operatorMayWrite: false,
    card: CsmCardPolicy.never,
  ),
  mtu(
    5,
    CsmSettingType.uint,
    CsmSettingScope.device,
    operatorMayWrite: true,
    card: CsmCardPolicy.onlyIfUserSet,
  ),
  ipv6(
    6,
    CsmSettingType.boolean,
    CsmSettingScope.device,
    operatorMayWrite: false,
    card: CsmCardPolicy.never,
  ),
  fakeIp(
    7,
    CsmSettingType.boolean,
    CsmSettingScope.device,
    operatorMayWrite: false,
    card: CsmCardPolicy.never,
  ),
  killSwitch(
    8,
    CsmSettingType.boolean,
    CsmSettingScope.device,
    operatorMayWrite: true,
    card: CsmCardPolicy.always,
  ),
  dnsNameservers(
    9,
    CsmSettingType.textList,
    CsmSettingScope.device,
    operatorMayWrite: true,
    card: CsmCardPolicy.always,
  ),
  dnsFallback(
    10,
    CsmSettingType.textList,
    CsmSettingScope.device,
    operatorMayWrite: true,
    card: CsmCardPolicy.always,
  ),
  splitMode(
    11,
    CsmSettingType.text,
    CsmSettingScope.device,
    operatorMayWrite: true,
    card: CsmCardPolicy.always,
  );

  const CsmSettingKey(
    this.wire,
    this.type,
    this.scope, {
    required this.operatorMayWrite,
    required this.card,
  });

  /// Ключ в карте `pol` директивы и в теле запроса `want`.
  final int wire;
  final CsmSettingType type;
  final CsmSettingScope scope;

  /// `false` означает, что панель ОБЯЗАНА отвергнуть ключ в записи оператора, а
  /// клиент, получивший его с `src = operator`, ОБЯЗАН проигнорировать значение
  /// и оставить своё, записав событие в диагностику (02-SPEC.md 7.2).
  final bool operatorMayWrite;
  final CsmCardPolicy card;

  static CsmSettingKey? fromWire(int raw) {
    for (final k in CsmSettingKey.values) {
      if (k.wire == raw) {
        return k;
      }
    }
    return null;
  }
}

/// Значение одной настройки. Тип фиксирован ключом.
sealed class CsmSettingValue {
  const CsmSettingValue();

  /// Сравнение по значению: карточка поднимается на РАЗЛИЧИИ, а не на записи.
  bool sameAs(CsmSettingValue other);

  Object? toJson();

  static CsmSettingValue? fromJson(CsmSettingType type, Object? raw) {
    switch (type) {
      case CsmSettingType.text:
        return raw is String ? CsmText(raw) : null;
      case CsmSettingType.uint:
        return raw is num && raw >= 0 ? CsmUint(raw.toInt()) : null;
      case CsmSettingType.boolean:
        return raw is bool ? CsmBoolean(raw) : null;
      case CsmSettingType.textList:
        if (raw is! List) {
          return null;
        }
        final items = raw.whereType<String>().toList(growable: false);
        if (items.length != raw.length) {
          return null;
        }
        return CsmTextList(items);
    }
  }
}

class CsmText extends CsmSettingValue {
  const CsmText(this.value);

  final String value;

  @override
  bool sameAs(CsmSettingValue other) =>
      other is CsmText && other.value == value;

  @override
  Object? toJson() => value;

  @override
  String toString() => value;
}

class CsmUint extends CsmSettingValue {
  const CsmUint(this.value);

  final int value;

  @override
  bool sameAs(CsmSettingValue other) =>
      other is CsmUint && other.value == value;

  @override
  Object? toJson() => value;

  @override
  String toString() => '$value';
}

class CsmBoolean extends CsmSettingValue {
  const CsmBoolean(this.value);

  final bool value;

  @override
  bool sameAs(CsmSettingValue other) =>
      other is CsmBoolean && other.value == value;

  @override
  Object? toJson() => value;

  @override
  String toString() => value ? 'on' : 'off';
}

class CsmTextList extends CsmSettingValue {
  const CsmTextList(this.value);

  final List<String> value;

  @override
  bool sameAs(CsmSettingValue other) {
    if (other is! CsmTextList || other.value.length != value.length) {
      return false;
    }
    for (var i = 0; i < value.length; i++) {
      if (other.value[i] != value[i]) {
        return false;
      }
    }
    return true;
  }

  @override
  Object? toJson() => value;

  @override
  String toString() => value.join(', ');
}

// ---------------------------------------------------------------- словари

/// `pol[1]`, набор строк `CorePolicy.protocol`. Источник истины
/// libs/caramba-core/profile/profile.go:566-573. Комментарий в core_policy.dart
/// пропускает `VLESS` и копировать его нельзя (02-SPEC.md 7.3).
const Set<String> kCsmProtocolVocabulary = <String>{
  'auto',
  'AmneziaWG',
  'VLESS-Reality',
  'VLESS',
  'Hysteria2',
  'TUIC',
  'Shadowsocks',
};

/// `pol[2]`, девять идентификаторов пресетов ядра плюс пустая строка.
/// UI-идентификатор `full` на проводе не появляется никогда.
const Set<String> kCsmPresetVocabulary = <String>{
  '',
  'ru-smart',
  'ru-full',
  'telegram-only',
  'ir-smart',
  'by-smart',
  'cn-smart',
  'streaming',
  'adblock',
  'global',
};

/// `pol[4]`, канонические стеки ядра.
const Set<String> kCsmStackVocabulary = <String>{'gvisor', 'system', 'mixed'};

/// `pol[11]`, режимы раздельного туннелирования.
const Set<String> kCsmSplitModeVocabulary = <String>{'off', 'bypass', 'allow'};

/// Сентинел «без релея» у `pol[3]` и `sel.rcc`. `NO` для этого не годится: это
/// Норвегия (02-SPEC.md Correction 4).
const String kCsmNoRelay = '--';

/// Сентинел сброса к значению оператора в запросе `want` (02-SPEC.md 7.5).
/// Текстовая строка для ЛЮБОГО ключа, какого бы типа он ни был.
const String kCsmDefaultSentinel = 'default';

/// Максимум одновременно висящих карточек (02-SPEC.md 7.7).
const int kCsmMaxOutstandingCards = 3;

/// Глубина очереди записей (02-SPEC.md 7.8).
const int kCsmWriteQueueDepth = 32;

/// `pol[3]`: три состояния, не два. Пустая строка это «не выбрано, оператор
/// решает», а НЕ «выключено» (02-SPEC.md 7.3, Correction 15).
bool csmIsRelayValue(String v) {
  if (v.isEmpty || v == kCsmNoRelay) {
    return true;
  }
  if (v.length != 2) {
    return false;
  }
  final a = v.codeUnitAt(0);
  final b = v.codeUnitAt(1);
  return a >= 0x41 && a <= 0x5a && b >= 0x41 && b <= 0x5a;
}

/// Резолвер DNS обязан быть DoH (`https`) или DoT (`tls`). `http://` и голый
/// IP отвергаются: INV-8 распространяется на каждую выборку клиента, а DNS это
/// выборка. Клиент здесь и есть точка контроля, ядро её не делает
/// (02-SPEC.md 7.3).
bool csmIsResolverUrl(String v) {
  if (v.isEmpty || v.length > 128) {
    return false;
  }
  final lower = v.toLowerCase();
  if (!lower.startsWith('https://') && !lower.startsWith('tls://')) {
    return false;
  }
  final uri = Uri.tryParse(v);
  if (uri == null || uri.host.isEmpty) {
    return false;
  }
  return true;
}

/// Проверка значения против закрытого словаря его ключа.
///
/// Значение вне словаря в подписанном `pol` НЕ отвергает директиву: клиент
/// игнорирует ровно этот ключ, оставляет своё значение и записывает событие
/// (02-SPEC.md 7.3 и 7.9 последняя строка, INV-11).
bool csmValueInVocabulary(CsmSettingKey key, CsmSettingValue value) {
  switch (key) {
    case CsmSettingKey.protocol:
      return value is CsmText && kCsmProtocolVocabulary.contains(value.value);
    case CsmSettingKey.preset:
      return value is CsmText && kCsmPresetVocabulary.contains(value.value);
    case CsmSettingKey.relay:
      return value is CsmText && csmIsRelayValue(value.value);
    case CsmSettingKey.stack:
      return value is CsmText && kCsmStackVocabulary.contains(value.value);
    case CsmSettingKey.mtu:
      // 0 означает «по умолчанию ядра», иначе 576..9000 (policy_json.go:118).
      return value is CsmUint &&
          (value.value == 0 || (value.value >= 576 && value.value <= 9000));
    case CsmSettingKey.ipv6:
    case CsmSettingKey.fakeIp:
    case CsmSettingKey.killSwitch:
      return value is CsmBoolean;
    case CsmSettingKey.dnsNameservers:
    case CsmSettingKey.dnsFallback:
      return value is CsmTextList &&
          value.value.length <= 8 &&
          value.value.every(csmIsResolverUrl);
    case CsmSettingKey.splitMode:
      return value is CsmText && kCsmSplitModeVocabulary.contains(value.value);
  }
}

// ---------------------------------------------------------------- состояние

/// Одна запись состояния настроек: значение, его происхождение и отметка о
/// том, ставил ли пользователь этот ключ ЯВНО на этом устройстве.
///
/// Отметка `userSet` это собственная запись клиента, а не `src`: `src`
/// говорит, кто выиграл старшинство на панели, отметка говорит, трогал ли
/// пользователь ключ здесь (02-SPEC.md 7.6).
class CsmSettingEntry {
  const CsmSettingEntry({
    required this.value,
    required this.src,
    this.userSet = false,
  });

  final CsmSettingValue value;
  final CsmProvenance src;
  final bool userSet;

  CsmSettingEntry copyWith({
    CsmSettingValue? value,
    CsmProvenance? src,
    bool? userSet,
  }) => CsmSettingEntry(
    value: value ?? this.value,
    src: src ?? this.src,
    userSet: userSet ?? this.userSet,
  );

  Map<String, Object?> toJson() => <String, Object?>{
    'v': value.toJson(),
    'src': src.wire,
    'user_set': userSet,
  };

  static CsmSettingEntry? fromJson(CsmSettingKey key, Object? raw) {
    if (raw is! Map) {
      return null;
    }
    final value = CsmSettingValue.fromJson(key.type, raw['v']);
    if (value == null) {
      return null;
    }
    // Значение вне словаря не восстанавливается: INV-11 запрещает хранить то,
    // что нельзя проверить против закрытого словаря.
    if (!csmValueInVocabulary(key, value)) {
      return null;
    }
    return CsmSettingEntry(
      value: value,
      src:
          CsmProvenance.fromWire((raw['src'] as num?)?.toInt()) ??
          CsmProvenance.byDefault,
      userSet: raw['user_set'] == true,
    );
  }
}

/// Одно изменение, требующее ответа пользователя: карточка «Оставить или
/// Вернуть» (02-SPEC.md 7.7, INV-22).
enum CsmCardTrigger {
  /// Оператор поменял значение, которое пользователь ставил явно.
  operatorOverwroteUserSet,

  /// Сужение защиты, безусловно и независимо от происхождения.
  narrowing,
}

/// Один затронутый ключ внутри карточки. Карточка обязана назвать настройку,
/// старое значение, новое значение и происхождение.
class CsmCardItem {
  const CsmCardItem({
    required this.key,
    required this.current,
    required this.proposed,
    required this.src,
    required this.trigger,
  });

  final CsmSettingKey key;

  /// Значение, которое клиент удерживает и продолжает применять.
  final CsmSettingValue current;

  /// Значение, которое прислал оператор и которое НЕ применено.
  final CsmSettingValue proposed;
  final CsmProvenance src;
  final CsmCardTrigger trigger;

  Map<String, Object?> toJson() => <String, Object?>{
    'key': key.wire,
    'current': current.toJson(),
    'proposed': proposed.toJson(),
    'src': src.wire,
    'trigger': trigger.name,
  };

  static CsmCardItem? fromJson(Object? raw) {
    if (raw is! Map) {
      return null;
    }
    final key = CsmSettingKey.fromWire((raw['key'] as num?)?.toInt() ?? -1);
    if (key == null) {
      return null;
    }
    final current = CsmSettingValue.fromJson(key.type, raw['current']);
    final proposed = CsmSettingValue.fromJson(key.type, raw['proposed']);
    if (current == null || proposed == null) {
      return null;
    }
    return CsmCardItem(
      key: key,
      current: current,
      proposed: proposed,
      src:
          CsmProvenance.fromWire((raw['src'] as num?)?.toInt()) ??
          CsmProvenance.operator,
      trigger: raw['trigger'] == CsmCardTrigger.narrowing.name
          ? CsmCardTrigger.narrowing
          : CsmCardTrigger.operatorOverwroteUserSet,
    );
  }
}

/// Карточка. Живёт, пока пользователь не ответит: не истекает по таймеру, не
/// закрывается навигацией, не отвечается молчанием.
class CsmPendingChange {
  const CsmPendingChange({
    required this.id,
    required this.raisedMs,
    required this.items,
  });

  final String id;
  final int raisedMs;

  /// Больше одного элемента бывает у самой старой карточки, в которую
  /// схлопнулась четвёртая: карточки не выбрасываются (02-SPEC.md 7.7).
  final List<CsmCardItem> items;

  bool get isMultiKey => items.length > 1;

  Set<CsmSettingKey> get keys => items.map((e) => e.key).toSet();

  Map<String, Object?> toJson() => <String, Object?>{
    'id': id,
    'raised_ms': raisedMs,
    'items': items.map((e) => e.toJson()).toList(growable: false),
  };

  static CsmPendingChange? fromJson(Object? raw) {
    if (raw is! Map) {
      return null;
    }
    final rawItems = raw['items'];
    if (rawItems is! List) {
      return null;
    }
    final items = rawItems
        .map(CsmCardItem.fromJson)
        .whereType<CsmCardItem>()
        .toList(growable: false);
    if (items.isEmpty) {
      return null;
    }
    final id = raw['id'];
    if (id is! String || id.isEmpty) {
      return null;
    }
    return CsmPendingChange(
      id: id,
      raisedMs: (raw['raised_ms'] as num?)?.toInt() ?? 0,
      items: items,
    );
  }
}

/// Почему один ключ директивы не был применён. Всё это попадает на экран
/// диагностики и НЕ является отказом директивы.
enum CsmIgnoredReason {
  /// Значение прошло по типу и длине, но лежит вне словаря этой сборки.
  /// Рендерится как `app_version_unsupported` (02-SPEC.md 7.9).
  outsideVocabulary,

  /// Ключ, который оператору писать нельзя, пришёл с `src = operator`.
  operatorMayNotWrite,

  /// Ключ вне закрытого реестра настроек.
  unknownKey,

  /// Форма записи не та: не пара `[value, src]`, не тот тип значения.
  malformed,
}

/// Один проигнорированный ключ.
class CsmIgnoredSetting {
  const CsmIgnoredSetting({
    required this.wireKey,
    required this.reason,
    this.key,
    this.detail = '',
  });

  final int wireKey;
  final CsmSettingKey? key;
  final CsmIgnoredReason reason;
  final String detail;

  @override
  String toString() =>
      'pol[$wireKey] ${reason.name}'
      '${detail.isEmpty ? '' : ': $detail'}';
}

/// Что произошло при слиянии подписанного `pol` в локальное состояние.
class CsmSettingsMergeResult {
  const CsmSettingsMergeResult({
    required this.settings,
    required this.cards,
    required this.applied,
    required this.ignored,
  });

  /// Новое состояние настроек. Ключи, удержанные карточкой, здесь остались
  /// со СТАРЫМ значением: клиент не применяет то, о чём спрашивает.
  final CsmSettings settings;

  /// Карточки после слияния, включая ранее висевшие.
  final List<CsmPendingChange> cards;

  /// Ключи, чьё значение действительно изменилось и применено.
  final Set<CsmSettingKey> applied;

  final List<CsmIgnoredSetting> ignored;
}

/// Локальное состояние настроек профиля с происхождением по каждому ключу.
class CsmSettings {
  const CsmSettings({this.entries = const <CsmSettingKey, CsmSettingEntry>{}});

  static const CsmSettings empty = CsmSettings();

  final Map<CsmSettingKey, CsmSettingEntry> entries;

  CsmSettingEntry? operator [](CsmSettingKey key) => entries[key];

  CsmSettingValue? valueOf(CsmSettingKey key) => entries[key]?.value;

  bool isUserSet(CsmSettingKey key) => entries[key]?.userSet ?? false;

  /// Пользователь поставил значение сам: ключ помечается `userSet`, а
  /// происхождение становится `user`.
  CsmSettings setByUser(CsmSettingKey key, CsmSettingValue value) {
    if (!csmValueInVocabulary(key, value)) {
      return this;
    }
    return CsmSettings(
      entries: <CsmSettingKey, CsmSettingEntry>{
        ...entries,
        key: CsmSettingEntry(
          value: value,
          src: CsmProvenance.user,
          userSet: true,
        ),
      },
    );
  }

  /// Значение по умолчанию сборки, без отметки пользователя.
  CsmSettings setDefault(CsmSettingKey key, CsmSettingValue value) {
    if (!csmValueInVocabulary(key, value)) {
      return this;
    }
    return CsmSettings(
      entries: <CsmSettingKey, CsmSettingEntry>{
        ...entries,
        key: CsmSettingEntry(
          value: value,
          src: CsmProvenance.byDefault,
          userSet: entries[key]?.userSet ?? false,
        ),
      },
    );
  }

  Map<String, Object?> toJson() => <String, Object?>{
    for (final e in entries.entries) '${e.key.wire}': e.value.toJson(),
  };

  static CsmSettings fromJson(Object? raw) {
    if (raw is! Map) {
      return CsmSettings.empty;
    }
    final out = <CsmSettingKey, CsmSettingEntry>{};
    raw.forEach((k, v) {
      final wire = int.tryParse(k.toString());
      if (wire == null) {
        return;
      }
      final key = CsmSettingKey.fromWire(wire);
      if (key == null) {
        return;
      }
      final entry = CsmSettingEntry.fromJson(key, v);
      if (entry != null) {
        out[key] = entry;
      }
    });
    return CsmSettings(entries: out);
  }
}

// ------------------------------------------------------- сужение защиты

/// Сужает ли переход [from] -> [to] защиту пользователя. Список закрытый
/// (02-SPEC.md 7.7); две его строки приходят в каталоге и живут не здесь.
bool csmIsNarrowing(
  CsmSettingKey key,
  CsmSettingValue? from,
  CsmSettingValue to,
) {
  switch (key) {
    case CsmSettingKey.killSwitch:
      // true -> false и только так.
      return from is CsmBoolean && from.value && to is CsmBoolean && !to.value;
    case CsmSettingKey.splitMode:
      // bypass или allow -> off.
      if (from is! CsmText || to is! CsmText) {
        return false;
      }
      return (from.value == 'bypass' || from.value == 'allow') &&
          to.value == 'off';
    case CsmSettingKey.dnsNameservers:
    case CsmSettingKey.dnsFallback:
      // Любое изменение, в любую сторону: перенаправленный резолвер это
      // перенаправленный резолвер.
      return from == null || !from.sameAs(to);
    case CsmSettingKey.protocol:
    case CsmSettingKey.preset:
    case CsmSettingKey.relay:
    case CsmSettingKey.stack:
    case CsmSettingKey.mtu:
    case CsmSettingKey.ipv6:
    case CsmSettingKey.fakeIp:
      return false;
  }
}

// ------------------------------------------------------------ слияние pol

/// Сливает подписанный `pol` директивы в локальное состояние.
///
/// Клиент НЕ передаёт `pol` в `SetPolicyJSON`: он сливает его сюда, применяет
/// правила происхождения и карточек, и только потом пересобирает целиком свою
/// `CorePolicy` из результата (02-SPEC.md 7.11).
///
/// [pol] это карта ключ -> `[value, src]` ровно в том виде, в каком она лежит в
/// проверенном кадре. Разбор кадра уже гарантировал форму пары и словарь `src`;
/// здесь проверяются словари ЗНАЧЕНИЙ, которые намеренно нефатальны.
CsmSettingsMergeResult csmMergePolicy({
  required CsmSettings current,
  required Map<int, CsmPolicyEntry> pol,
  List<CsmPendingChange> cards = const <CsmPendingChange>[],
  required int nowMs,
  String Function()? idGenerator,
}) {
  final entries = Map<CsmSettingKey, CsmSettingEntry>.from(current.entries);
  final ignored = <CsmIgnoredSetting>[];
  final applied = <CsmSettingKey>{};
  var outCards = List<CsmPendingChange>.from(cards);
  var seq = 0;
  String nextId() {
    if (idGenerator != null) {
      return idGenerator();
    }
    seq++;
    return 'card_${nowMs}_$seq';
  }

  // Ключи, по которым уже висит неотвеченная карточка: их значение остаётся
  // локальным, и вторая карточка по тому же ключу не заводится.
  final held = <CsmSettingKey>{for (final c in outCards) ...c.keys};

  final sortedKeys = pol.keys.toList()..sort();
  for (final wire in sortedKeys) {
    final incoming = pol[wire]!;
    final key = CsmSettingKey.fromWire(wire);
    if (key == null) {
      ignored.add(
        CsmIgnoredSetting(wireKey: wire, reason: CsmIgnoredReason.unknownKey),
      );
      continue;
    }
    final value = incoming.value;
    if (value == null) {
      ignored.add(
        CsmIgnoredSetting(
          wireKey: wire,
          key: key,
          reason: CsmIgnoredReason.malformed,
          detail: 'value type does not match the key',
        ),
      );
      continue;
    }
    if (!csmValueInVocabulary(key, value)) {
      // 02-SPEC.md 7.9 последняя строка: игнорируем ровно этот ключ, держим
      // своё значение, ничего не сохраняем, директиву НЕ отвергаем.
      ignored.add(
        CsmIgnoredSetting(
          wireKey: wire,
          key: key,
          reason: CsmIgnoredReason.outsideVocabulary,
          detail: value.toString(),
        ),
      );
      continue;
    }
    if (!key.operatorMayWrite && incoming.src == CsmProvenance.operator) {
      ignored.add(
        CsmIgnoredSetting(
          wireKey: wire,
          key: key,
          reason: CsmIgnoredReason.operatorMayNotWrite,
        ),
      );
      continue;
    }
    if (held.contains(key)) {
      // Пока карточка не отвечена, эффективное значение это локальное
      // (02-SPEC.md 7.6). Второй карточки по этому ключу не заводим.
      continue;
    }

    final existing = entries[key];
    final currentValue = existing?.value;
    if (currentValue != null && currentValue.sameAs(value)) {
      // Значение то же: происхождение обновляем, карточку не поднимаем.
      entries[key] = existing!.copyWith(src: incoming.src);
      continue;
    }

    final narrowing = csmIsNarrowing(key, currentValue, value);
    final overwritesUserSet =
        incoming.src == CsmProvenance.operator && (existing?.userSet ?? false);
    final cardRequired = switch (key.card) {
      // Сужение поднимает карточку безусловно, независимо от происхождения и
      // от того, трогал ли пользователь ключ вообще.
      CsmCardPolicy.always => narrowing || overwritesUserSet,
      CsmCardPolicy.onlyIfUserSet => overwritesUserSet,
      // Контрол не рендерится в v1, спрашивать не о чем.
      CsmCardPolicy.never => false,
    };

    if (cardRequired && currentValue != null) {
      outCards = _raiseCard(
        outCards,
        CsmCardItem(
          key: key,
          current: currentValue,
          proposed: value,
          src: incoming.src,
          trigger: narrowing
              ? CsmCardTrigger.narrowing
              : CsmCardTrigger.operatorOverwroteUserSet,
        ),
        nowMs,
        nextId,
      );
      held.add(key);
      continue;
    }

    entries[key] = CsmSettingEntry(
      value: value,
      src: incoming.src,
      // Отметка пользователя переживает запись оператора: она про то, трогал
      // ли ключ пользователь, а не про то, кто выиграл старшинство.
      userSet: existing?.userSet ?? false,
    );
    applied.add(key);
  }

  return CsmSettingsMergeResult(
    settings: CsmSettings(entries: entries),
    cards: outCards,
    applied: applied,
    ignored: ignored,
  );
}

/// Поднимает карточку. Больше [kCsmMaxOutstandingCards] висеть не может:
/// четвёртая схлопывается в САМУЮ СТАРУЮ, которая становится многоключевой.
/// Карточки не выбрасываются никогда.
List<CsmPendingChange> _raiseCard(
  List<CsmPendingChange> cards,
  CsmCardItem item,
  int nowMs,
  String Function() nextId,
) {
  if (cards.length < kCsmMaxOutstandingCards) {
    return <CsmPendingChange>[
      ...cards,
      CsmPendingChange(
        id: nextId(),
        raisedMs: nowMs,
        items: <CsmCardItem>[item],
      ),
    ];
  }
  var oldestIndex = 0;
  for (var i = 1; i < cards.length; i++) {
    if (cards[i].raisedMs < cards[oldestIndex].raisedMs) {
      oldestIndex = i;
    }
  }
  final oldest = cards[oldestIndex];
  final merged = CsmPendingChange(
    id: oldest.id,
    raisedMs: oldest.raisedMs,
    items: <CsmCardItem>[...oldest.items, item],
  );
  final out = List<CsmPendingChange>.from(cards);
  out[oldestIndex] = merged;
  return out;
}

/// Пара `[value, src]` из карты `pol`, уже приведённая к типу ключа.
/// `value == null` означает, что тип значения не совпал с типом ключа.
class CsmPolicyEntry {
  const CsmPolicyEntry(this.value, this.src);

  final CsmSettingValue? value;
  final CsmProvenance src;
}

/// Читает карту `pol` из проверенного кадра директивы.
///
/// Разбор кадра (packages/caramba_vpn/lib/src/csm/documents.dart) уже проверил
/// форму пары и словарь `src`; здесь идёт только приведение к типу ключа.
/// Ключи выше критического диапазона пропускаются: правило расширения
/// 03-WIRE.md 3.3.
Map<int, CsmPolicyEntry> csmReadPolicyMap(CborValue? raw) {
  if (raw is! CborMap) {
    return const <int, CsmPolicyEntry>{};
  }
  final out = <int, CsmPolicyEntry>{};
  raw.entries.forEach((wire, v) {
    if (v is! CborArray || v.items.length != 2) {
      return;
    }
    final src = v.items[1];
    if (src is! CborUint) {
      return;
    }
    final provenance = CsmProvenance.fromWire(src.value);
    if (provenance == null) {
      return;
    }
    final key = CsmSettingKey.fromWire(wire);
    if (key == null) {
      out[wire] = CsmPolicyEntry(null, provenance);
      return;
    }
    out[wire] = CsmPolicyEntry(_coerce(key.type, v.items[0]), provenance);
  });
  return out;
}

CsmSettingValue? _coerce(CsmSettingType type, CborValue v) {
  switch (type) {
    case CsmSettingType.text:
      return v is CborText ? CsmText(v.value) : null;
    case CsmSettingType.uint:
      return v is CborUint ? CsmUint(v.value) : null;
    case CsmSettingType.boolean:
      return v is CborBool ? CsmBoolean(v.value) : null;
    case CsmSettingType.textList:
      if (v is! CborArray) {
        return null;
      }
      final items = <String>[];
      for (final x in v.items) {
        if (x is! CborText) {
          return null;
        }
        items.add(x.value);
      }
      return CsmTextList(items);
  }
}

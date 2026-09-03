/// Состояние CSM/1 на профиле подключения.
///
/// Нормативно: 02-SPEC.md 1.2 (профиль как единица состояния), 2.1 (автомат
/// профиля), 5.1 и 5.4 (отметки максимума версий, временной пол), 8.1 и 8.3
/// (ступени лестницы, порядок и включённость), 8.8.3 (где живёт каждое
/// значение и что будет, если его откатить).
///
/// Всё, что здесь сериализуется, лежит в secure storage на профиле. Старый
/// JSON без этих полей грузится как «CSM не заведён», ровно так же, как это
/// делает миграция поля `format` в ConnectionProfile.
library;

import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/csm_write.dart';

/// Как был установлен закреплённый корневой ключ. INV-18 требует показать это
/// пользователю на экране личности оператора.
enum CsmPinOrigin {
  /// Пин продиктован вне полосы: QR, бумага, голосом. Единственная форма,
  /// которая переживает захват канала оператора.
  outOfBand('out_of_band'),

  /// Пин пришёл в приложение по ссылке энроллмента с того же origin, что и
  /// документы. Слабее, и экран обязан это говорить.
  inApp('in_app');

  const CsmPinOrigin(this.wire);

  final String wire;

  static CsmPinOrigin fromWire(String? raw) =>
      raw == CsmPinOrigin.outOfBand.wire
      ? CsmPinOrigin.outOfBand
      : CsmPinOrigin.inApp;
}

/// Состояние профиля из автомата 02-SPEC.md 2.1.
enum CsmProfileStage {
  unenrolled('unenrolled'),
  pinning('pinning'),
  anchored('anchored'),
  enrolled('enrolled'),
  trusted('trusted'),
  trustedStale('trusted_stale'),
  grace('grace'),
  graceExhausted('grace_exhausted'),

  /// Терминальное и недиссмиссабельное. Достижимо ровно двумя путями:
  /// несовпадение корневого пина и отзыв ВСЕХ ключей roles[1].
  compromised('compromised');

  const CsmProfileStage(this.wire);

  final String wire;

  static CsmProfileStage fromWire(String? raw) {
    for (final s in CsmProfileStage.values) {
      if (s.wire == raw) {
        return s;
      }
    }
    return CsmProfileStage.unenrolled;
  }

  /// Единственное состояние, в котором клиент отказывается поднимать туннель
  /// на данных оператора (02-SPEC.md 2.1 правило 4 и 5).
  bool get refusesToConnect =>
      this == CsmProfileStage.graceExhausted ||
      this == CsmProfileStage.compromised;

  /// Профиль прошёл `anchored` и обратной дороги в непроверяемый режим нет
  /// (INV-13).
  bool get isPinned =>
      this != CsmProfileStage.unenrolled && this != CsmProfileStage.pinning;
}

/// Ступень транспортной лестницы, 02-SPEC.md 8.1.
enum CsmRung {
  cached(0, 'cached signed documents'),
  direct(1, 'direct HTTPS to the enrolled origin'),
  mirrors(2, 'signed mirrors'),
  doh(3, 'DoH-resolved address with explicit SNI'),
  tunnel(4, "through the app's own tunnel"),
  userProxy(5, 'user-entered SOCKS5 or HTTP proxy'),
  outOfBand(6, 'out of band');

  const CsmRung(this.id, this.label);

  final int id;
  final String label;

  /// R0 и R6 отключить нельзя никогда: чтение проверенного документа с диска и
  /// человек, несущий байты, это не политика (02-SPEC.md 8.3).
  bool get isMandatory => this == CsmRung.cached || this == CsmRung.outOfBand;

  static CsmRung? fromId(int id) {
    for (final r in CsmRung.values) {
      if (r.id == id) {
        return r;
      }
    }
    return null;
  }
}

/// Порядок по умолчанию, когда каталог не несёт `lad` (02-SPEC.md 14).
const List<int> kCsmDefaultLadderOrder = <int>[0, 1, 2, 3, 4, 5, 6];

/// Включённые по умолчанию: R4 нужен туннель, R5 нужен ввод пользователя.
const List<int> kCsmDefaultLadderEnabled = <int>[0, 1, 2, 3, 6];

/// Аппаратный уровень хранения ключа устройства (02-SPEC.md 9.4). Показывается
/// пользователю и сообщается оператору.
enum CsmHardwareTier {
  secureEnclave(1),
  teeOrStrongbox(2),
  software(3);

  const CsmHardwareTier(this.wire);

  final int wire;

  static CsmHardwareTier fromWire(int? raw) {
    for (final t in CsmHardwareTier.values) {
      if (t.wire == raw) {
        return t;
      }
    }
    return CsmHardwareTier.software;
  }
}

/// Закреплённый корень арендатора. Пока профиль не покинул `pinning`, это
/// единственное, что можно поменять; после этого пин неизменен на всю жизнь
/// профиля, и смена пина есть удаление профиля (02-SPEC.md 2.1 правило 1).
class CsmPin {
  const CsmPin({
    required this.pid,
    required this.linkPin,
    required this.origin,
    required this.establishedMs,
  });

  /// `sha256(root_pk)[0..8]` шестнадцатеричной строкой, 16 символов.
  final String pid;

  /// `base32_crockford(sha256(root_pk)[0..12])`, 20 символов.
  final String linkPin;

  final CsmPinOrigin origin;
  final int establishedMs;

  /// Отпечаток корневого ключа группами по четыре, как его показывает экран
  /// личности оператора (INV-18).
  String get fingerprint {
    final sb = StringBuffer();
    for (var i = 0; i < linkPin.length; i += 4) {
      if (i > 0) {
        sb.write('-');
      }
      sb.write(
        linkPin.substring(i, i + 4 > linkPin.length ? linkPin.length : i + 4),
      );
    }
    return sb.toString();
  }

  Map<String, Object?> toJson() => <String, Object?>{
    'pid': pid,
    'link_pin': linkPin,
    'origin': origin.wire,
    'established_ms': establishedMs,
  };

  static CsmPin? fromJson(Object? raw) {
    if (raw is! Map) {
      return null;
    }
    final pid = raw['pid'];
    final linkPin = raw['link_pin'];
    if (pid is! String ||
        pid.isEmpty ||
        linkPin is! String ||
        linkPin.isEmpty) {
      return null;
    }
    return CsmPin(
      pid: pid,
      linkPin: linkPin,
      origin: CsmPinOrigin.fromWire(raw['origin'] as String?),
      establishedMs: (raw['established_ms'] as num?)?.toInt() ?? 0,
    );
  }
}

/// Запись истории пина. INV-18 требует показать, менялся ли пин когда-нибудь;
/// в CSM/1 законный способ его сменить один, удалить профиль и завести заново,
/// поэтому непустая история это то, что пользователь обязан увидеть.
class CsmPinHistoryEntry {
  const CsmPinHistoryEntry({
    required this.pin,
    required this.retiredMs,
    this.note = '',
  });

  final CsmPin pin;
  final int retiredMs;
  final String note;

  Map<String, Object?> toJson() => <String, Object?>{
    'pin': pin.toJson(),
    'retired_ms': retiredMs,
    'note': note,
  };

  static CsmPinHistoryEntry? fromJson(Object? raw) {
    if (raw is! Map) {
      return null;
    }
    final pin = CsmPin.fromJson(raw['pin']);
    if (pin == null) {
      return null;
    }
    return CsmPinHistoryEntry(
      pin: pin,
      retiredMs: (raw['retired_ms'] as num?)?.toInt() ?? 0,
      note: (raw['note'] as String?) ?? '',
    );
  }
}

/// Проверенный документ, который клиент держит и на котором работает.
/// Рендерится в хроме проверки (INV-19): версия, выпущен, истекает, отпечаток
/// подписавшего, результат проверки.
class CsmDocumentRecord {
  const CsmDocumentRecord({
    required this.docType,
    required this.version,
    required this.issuedSec,
    required this.expiresSec,
    required this.signerFingerprints,
    required this.verifiedAtMs,
    this.scope = '',
    this.frameDigest = '',
    this.verdict = 'ok',
    this.viaRung,
  });

  /// Тип документа: 0x01 ключевой, 0x02 каталог, 0x03 директива, 0x08 резерв.
  final int docType;
  final int version;
  final int issuedSec;
  final int expiresSec;

  /// Усечённые идентификаторы ключей, чьи подписи сошлись, hex.
  final List<String> signerFingerprints;

  final int verifiedAtMs;

  /// Область отметки максимума версий: локатор для 0x03 и 0x08, cat_id для
  /// 0x02 и 0x04, пусто для 0x01 и 0x05 (02-SPEC.md 5.1).
  final String scope;

  /// sha256 кадра, hex. Для каталога это `chash`, который называет директива.
  final String frameDigest;

  /// `ok` или код отказа из реестра 03-WIRE.md 6.6.
  final String verdict;

  /// Ступень, принёсшая документ. `null` для документа, поднятого с диска.
  final int? viaRung;

  bool get isVerified => verdict == 'ok';

  /// Просрочен ли документ по своим собственным часам. Просрочка НЕ отключает
  /// туннель и НЕ чистит кэш: она означает отказ принимать новые инструкции и
  /// новый статус (INV-16).
  bool isExpiredAt(int nowSec) => nowSec > expiresSec + 300;

  Map<String, Object?> toJson() => <String, Object?>{
    'doc_type': docType,
    'ver': version,
    'iat': issuedSec,
    'exp': expiresSec,
    'signers': signerFingerprints,
    'verified_ms': verifiedAtMs,
    'scope': scope,
    'digest': frameDigest,
    'verdict': verdict,
    'via_rung': viaRung,
  };

  static CsmDocumentRecord? fromJson(Object? raw) {
    if (raw is! Map) {
      return null;
    }
    final docType = (raw['doc_type'] as num?)?.toInt();
    if (docType == null) {
      return null;
    }
    final signers = raw['signers'];
    return CsmDocumentRecord(
      docType: docType,
      version: (raw['ver'] as num?)?.toInt() ?? 0,
      issuedSec: (raw['iat'] as num?)?.toInt() ?? 0,
      expiresSec: (raw['exp'] as num?)?.toInt() ?? 0,
      signerFingerprints: signers is List
          ? signers.whereType<String>().toList(growable: false)
          : const <String>[],
      verifiedAtMs: (raw['verified_ms'] as num?)?.toInt() ?? 0,
      scope: (raw['scope'] as String?) ?? '',
      frameDigest: (raw['digest'] as String?) ?? '',
      verdict: (raw['verdict'] as String?) ?? 'ok',
      viaRung: (raw['via_rung'] as num?)?.toInt(),
    );
  }
}

/// Пользовательские настройки лестницы. Тронув их, пользователь побеждает
/// подписанные умолчания навсегда: более поздний каталог НЕ вправе молча
/// вернуть умолчание оператора поверх выбора пользователя (02-SPEC.md 8.3).
class CsmLadderPrefs {
  const CsmLadderPrefs({
    this.order = kCsmDefaultLadderOrder,
    this.enabled = kCsmDefaultLadderEnabled,
    this.userTouched = false,
  });

  static const CsmLadderPrefs defaults = CsmLadderPrefs();

  final List<int> order;
  final List<int> enabled;
  final bool userTouched;

  /// Действующий порядок. R0 первый всегда, что бы ни говорил `lad.ord`:
  /// прочитать проверенный документ с диска до открытия сокета это не выбор
  /// политики.
  List<CsmRung> get effectiveOrder {
    final out = <CsmRung>[CsmRung.cached];
    for (final id in order) {
      final r = CsmRung.fromId(id);
      if (r != null && r != CsmRung.cached && !out.contains(r)) {
        out.add(r);
      }
    }
    return out;
  }

  /// Включена ли ступень. R0 и R6 включены всегда.
  bool isEnabled(CsmRung rung) => rung.isMandatory || enabled.contains(rung.id);

  Map<String, Object?> toJson() => <String, Object?>{
    'order': order,
    'enabled': enabled,
    'user_touched': userTouched,
  };

  static CsmLadderPrefs fromJson(Object? raw) {
    if (raw is! Map) {
      return CsmLadderPrefs.defaults;
    }
    List<int> ints(Object? v, List<int> fallback) {
      if (v is! List) {
        return fallback;
      }
      final out = <int>[];
      for (final x in v) {
        if (x is num) {
          final id = x.toInt();
          if (id >= 0 && id <= 6 && !out.contains(id)) {
            out.add(id);
          }
        }
      }
      return out.isEmpty ? fallback : out;
    }

    final enabled = ints(raw['enabled'], kCsmDefaultLadderEnabled);
    return CsmLadderPrefs(
      order: ints(raw['order'], kCsmDefaultLadderOrder),
      // R0 и R6 обязательны: каталог, который их опускает, отвергается, и
      // сохранённая запись без них тоже чинится, а не принимается.
      enabled: <int>{...enabled, 0, 6}.toList(growable: false)..sort(),
      userTouched: raw['user_touched'] == true,
    );
  }
}

/// Состояние CSM/1 одного профиля. Отсутствует целиком на профиле, который
/// никогда не закреплял корневой ключ: `rawSub` из legacy-импорта таким и
/// остаётся, повышать его молча нельзя (02-SPEC.md 9.8).
/// Факты о связанном каталоге, которые нужны слою отрисовки.
///
/// Каталог целиком здесь не хранится намеренно: нужны ровно два ответа,
/// которые виджет иначе дать не может.
///
/// 1. Стоит ли за битом возможности непустой массив. 02-SPEC.md 6.2: бит,
///    поднятый в директиве, но не имеющий массива за собой в связанном
///    каталоге, ОБЯЗАН считаться нулём. Иначе экран ступеней предлагает R2 при
///    пустом `mir`, а бит 6 снова открывает выборку ресурсов, которую INV-12
///    ограничивает.
/// 2. Какие идентификаторы оператор вообще предлагает. 02-SPEC.md 7.2:
///    маршрут, чей `id` клиент не реализует, обязан отрисоваться видимым и
///    выключенным с причиной `app_version_unsupported`, а не пропасть, и
///    ограничение оператора обязано доезжать до пикера.
class CsmCatalogContent {
  const CsmCatalogContent({
    this.hasExits = false,
    this.hasMirrors = false,
    this.hasDoh = false,
    this.hasResources = false,
    this.offeredRoutes = const <String>[],
    this.offeredExits = const <String>[],
    this.offeredRelays = const <String>[],
    this.known = false,
  });

  /// Каталога ещё нет. Все факты ложны, и это правильная сторона отказа:
  /// возможность, за которой нет данных, не выдаётся.
  static const CsmCatalogContent unknown = CsmCatalogContent();

  final bool hasExits;
  final bool hasMirrors;
  final bool hasDoh;
  final bool hasResources;

  /// `ro[].id`, `ex[].id` и `re[].id` связанного каталога.
  final List<String> offeredRoutes;
  final List<String> offeredExits;
  final List<String> offeredRelays;

  /// Каталог уже проверялся. Пока ложь, пикеры не сужаются: сузить их по
  /// незнанию значит спрятать то, что оператор предлагает.
  final bool known;

  /// Ответ на вопрос [CsmCapabilitySet.withContentPresence].
  bool backs(CsmCapability c) => switch (c) {
    CsmCapability.perNodeMaterial => hasExits,
    CsmCapability.mirrorPool => hasMirrors,
    CsmCapability.dohEndpoints => hasDoh,
    CsmCapability.resourceHashes => hasResources,
    _ => true,
  };

  Map<String, dynamic> toJson() => <String, dynamic>{
    'known': known,
    'ex': hasExits,
    'mir': hasMirrors,
    'doh': hasDoh,
    'rs': hasResources,
    'ro_ids': offeredRoutes,
    'ex_ids': offeredExits,
    're_ids': offeredRelays,
  };

  static CsmCatalogContent fromJson(Object? raw) {
    if (raw is! Map) {
      return unknown;
    }
    List<String> ids(Object? v) => v is List
        ? v.whereType<String>().toList(growable: false)
        : const <String>[];
    return CsmCatalogContent(
      known: raw['known'] == true,
      hasExits: raw['ex'] == true,
      hasMirrors: raw['mir'] == true,
      hasDoh: raw['doh'] == true,
      hasResources: raw['rs'] == true,
      offeredRoutes: ids(raw['ro_ids']),
      offeredExits: ids(raw['ex_ids']),
      offeredRelays: ids(raw['re_ids']),
    );
  }
}

/// Авторитетный выбор из поля `sel` доверенной директивы.
class CsmSelectionState {
  const CsmSelectionState({this.exit, this.relay, this.relayCountry});

  /// `sel.exit`, идентификатор узла выхода.
  final String? exit;

  /// `sel.relay`, идентификатор узла реле. Это ИДЕНТИФИКАТОР УЗЛА, а не код
  /// страны: `pol[3]` и `sel.rcc` это страна, `sel.relay` это узел, и имена
  /// приглашают их спутать (02-SPEC.md 7.4).
  final String? relay;

  /// `sel.rcc`, код страны реле или сентинел `--`.
  final String? relayCountry;

  Map<String, dynamic> toJson() => <String, dynamic>{
    if (exit != null) 'exit': exit,
    if (relay != null) 'relay': relay,
    if (relayCountry != null) 'rcc': relayCountry,
  };

  static CsmSelectionState fromJson(Object? raw) {
    if (raw is! Map) {
      return const CsmSelectionState();
    }
    String? str(Object? v) => v is String && v.isNotEmpty ? v : null;
    return CsmSelectionState(
      exit: str(raw['exit']),
      relay: str(raw['relay']),
      relayCountry: str(raw['rcc']),
    );
  }
}

class CsmProfileState {
  const CsmProfileState({
    required this.pin,
    this.stage = CsmProfileStage.pinning,
    this.pinHistory = const <CsmPinHistoryEntry>[],
    this.highWaterMarks = const <String, int>{},
    this.timeFloorSec = 0,
    this.catalogCapabilities,
    this.directiveCapabilities,
    this.keyDocument,
    this.catalog,
    this.directive,
    this.locator,
    this.deviceThumbprint,
    this.hardwareTier = CsmHardwareTier.software,
    this.agreementKeyGeneration = 0,
    this.enrolledAtMs = 0,
    this.revoked = false,
    this.offlineGraceSec = 604800,
    this.settings = CsmSettings.empty,
    this.pendingChanges = const <CsmPendingChange>[],
    this.writeQueue = CsmWriteQueue.empty,
    this.ladder = CsmLadderPrefs.defaults,
    this.operatorName,
    this.missingCapability = false,
    this.fleetRootAnchored = true,
    this.catalogContent = CsmCatalogContent.unknown,
    this.storeInconsistent = false,
    this.selection = const CsmSelectionState(),
  });

  /// Закреплённый корень. Профиль без него не является профилем CSM.
  final CsmPin pin;

  final CsmProfileStage stage;
  final List<CsmPinHistoryEntry> pinHistory;

  /// Отметки максимума версий по `(doc_type, scope)`. Ключ строкой
  /// `doc_type|scope`: ровно одна карта на профиль, потому что два хранилища
  /// это дыра для отката, а не защита в глубину (02-SPEC.md 5.1).
  final Map<String, int> highWaterMarks;

  /// Временной пол: наибольший `iat` среди успешно проверенных документов.
  /// Монотонен, НИКОГДА не уменьшается, и НЕ берётся из заголовка `Date`
  /// (02-SPEC.md 5.4, Correction 1).
  final int timeFloorSec;

  /// `cap` последнего проверенного каталога.
  final CsmCapabilitySet? catalogCapabilities;

  /// `cap` последней проверенной директивы. Она и есть возможность оператора,
  /// пока не просрочена (02-SPEC.md 6.5).
  final CsmCapabilitySet? directiveCapabilities;

  final CsmDocumentRecord? keyDocument;
  final CsmDocumentRecord? catalog;
  final CsmDocumentRecord? directive;

  /// Текущий локатор, 24 символа. Директива вправе прислать новый, и тогда
  /// клиент ОБЯЗАН сохранить его и использовать в следующем запросе.
  final String? locator;

  /// `dtp`, hex, 32 символа.
  final String? deviceThumbprint;

  final CsmHardwareTier hardwareTier;
  final int agreementKeyGeneration;
  final int enrolledAtMs;

  /// Липкий флаг `st = 5`. Единственный статус, переживающий кэш: клиент,
  /// потерявший его, переподключается на отозванной подписке (02-SPEC.md
  /// 4.6.1, 8.8.3).
  final bool revoked;

  /// `exph`, окно офлайн-милости в секундах, уже зажатое до `EXPH_FLOOR`.
  final int offlineGraceSec;

  final CsmSettings settings;
  final List<CsmPendingChange> pendingChanges;

  /// Очередь подписанных записей настроек. Переживает перезапуск: изменение
  /// принимается локально сразу и уходит по любой доступной ступени потом, и
  /// ни одно изменение настроек никогда не блокируется на сети (7.8).
  final CsmWriteQueue writeQueue;

  final CsmLadderPrefs ladder;

  /// Инертное отображаемое имя оператора из бутстрап-блоба. Никогда не ключ,
  /// никогда не эхо (INV-11).
  final String? operatorName;

  /// Профиль получил документ без `cap`. На профиле, уже закрепившем корневой
  /// ключ, это жёсткая, недиссмиссабельная ошибка, а НЕ откат к «считаем, что
  /// можно всё» (INV-13, 02-SPEC.md 6.4).
  final bool missingCapability;

  /// Доверенный ключевой документ несёт запись `tiers` для тира действующей
  /// директивы. Ложь рендерится как `fleet not root-anchored`: список серверов
  /// оператора не покрыт его офлайновым ключом, значит компрометация онлайнового
  /// ключа здесь не будет поймана (02-SPEC.md 4.3, 8.8.2).
  ///
  /// Значение выставляет шаг проверки V14b, а не этот файл: вывести его из
  /// того, что здесь лежит, нельзя, и притворяться, что можно, хуже, чем
  /// хранить один бит.
  final bool fleetRootAnchored;

  /// Факты о СВЯЗАННОМ каталоге: за какими битами стоит непустой массив и
  /// какие идентификаторы оператор вообще предлагает.
  ///
  /// Здесь лежат факты, а не сам каталог: слой, который его держит, это слой
  /// проверки, а слой, который рисует, это виджеты, и без этих полей второй
  /// физически не может ответить на вопрос "есть ли за этим битом данные"
  /// (02-SPEC.md 6.2) и "какие пресеты предлагает оператор" (02-SPEC.md 7.2).
  final CsmCatalogContent catalogContent;

  /// Авторитетный выбор `sel` доверенной директивы. Нужен двум предикатам
  /// 02-SPEC.md 7.4, которые проверяются ПОСЛЕ проверки каталога, а не на
  /// разборе, потому что на разборе связанного каталога может ещё не быть.
  final CsmSelectionState selection;

  /// Запись CSM на диске прочиталась испорченной.
  ///
  /// Это НЕ то же самое, что её отсутствие. Обнулившееся хранилище неотличимо
  /// от отката (02-SPEC.md 8.8.3), поэтому испорченная запись оставляет липкое
  /// правило включённым, не обнуляет отметки и не обнуляет временной пол, а
  /// профиль считается закреплённым и непроверяемым до перепроверки из сети.
  final bool storeInconsistent;

  /// Действующий набор возможностей оператора с карве-аутом по содержимому.
  ///
  /// 02-SPEC.md 6.2: бит из [kCsmContentBackedCapabilities], поднятый в
  /// директиве, но не имеющий за собой массива в связанном каталоге, ОБЯЗАН
  /// считаться нулём. Это утверждение о байтах, которые клиент держит, а не
  /// переопределение политики, и выдать возможность оно не может.
  CsmCapabilitySet effectiveOperatorCapabilities(int nowSec) =>
      resolvedOperatorCapabilities(
        nowSec,
      ).withContentPresence(catalogContent.backs);

  /// Возможность оператора после разрешения разногласия каталога и директивы.
  ///
  /// Побеждает `cap` самой свежей ПРОВЕРЕННОЙ И НЕПРОСРОЧЕННОЙ директивы;
  /// каталог используется, только пока такой директивы нет (02-SPEC.md 6.5).
  CsmCapabilitySet resolvedOperatorCapabilities(int nowSec) {
    final d = directive;
    if (d != null && d.isVerified && !d.isExpiredAt(nowSec)) {
      final cap = directiveCapabilities;
      if (cap != null) {
        return cap;
      }
    }
    return catalogCapabilities ??
        directiveCapabilities ??
        CsmCapabilitySet.none;
  }

  /// Расходятся ли `cap` каталога и директивы. Записывается в хром проверки,
  /// чтобы оператор видел, что устройство живёт на старом каталоге.
  bool get capabilitiesDisagree {
    final c = catalogCapabilities;
    final d = directiveCapabilities;
    return c != null && d != null && c != d;
  }

  /// Возраст конфигурации в секундах: сколько прошло с проверки самой свежей
  /// директивы. INV-21 требует показывать это всегда, когда клиент работает на
  /// кэше.
  int? configurationAgeSec(int nowMs) {
    final at = directive?.verifiedAtMs ?? catalog?.verifiedAtMs;
    if (at == null || at == 0) {
      return null;
    }
    final age = (nowMs - at) ~/ 1000;
    return age < 0 ? 0 : age;
  }

  CsmProfileState copyWith({
    CsmPin? pin,
    CsmProfileStage? stage,
    List<CsmPinHistoryEntry>? pinHistory,
    Map<String, int>? highWaterMarks,
    int? timeFloorSec,
    CsmCapabilitySet? catalogCapabilities,
    CsmCapabilitySet? directiveCapabilities,
    CsmDocumentRecord? keyDocument,
    CsmDocumentRecord? catalog,
    CsmDocumentRecord? directive,
    String? locator,
    String? deviceThumbprint,
    CsmHardwareTier? hardwareTier,
    int? agreementKeyGeneration,
    int? enrolledAtMs,
    bool? revoked,
    int? offlineGraceSec,
    CsmSettings? settings,
    List<CsmPendingChange>? pendingChanges,
    CsmWriteQueue? writeQueue,
    CsmLadderPrefs? ladder,
    String? operatorName,
    bool? missingCapability,
    bool? fleetRootAnchored,
    CsmCatalogContent? catalogContent,
    bool? storeInconsistent,
    CsmSelectionState? selection,
  }) => CsmProfileState(
    pin: pin ?? this.pin,
    stage: stage ?? this.stage,
    pinHistory: pinHistory ?? this.pinHistory,
    highWaterMarks: highWaterMarks ?? this.highWaterMarks,
    timeFloorSec: timeFloorSec ?? this.timeFloorSec,
    catalogCapabilities: catalogCapabilities ?? this.catalogCapabilities,
    directiveCapabilities: directiveCapabilities ?? this.directiveCapabilities,
    keyDocument: keyDocument ?? this.keyDocument,
    catalog: catalog ?? this.catalog,
    directive: directive ?? this.directive,
    locator: locator ?? this.locator,
    deviceThumbprint: deviceThumbprint ?? this.deviceThumbprint,
    hardwareTier: hardwareTier ?? this.hardwareTier,
    agreementKeyGeneration:
        agreementKeyGeneration ?? this.agreementKeyGeneration,
    enrolledAtMs: enrolledAtMs ?? this.enrolledAtMs,
    revoked: revoked ?? this.revoked,
    offlineGraceSec: offlineGraceSec ?? this.offlineGraceSec,
    settings: settings ?? this.settings,
    pendingChanges: pendingChanges ?? this.pendingChanges,
    writeQueue: writeQueue ?? this.writeQueue,
    ladder: ladder ?? this.ladder,
    operatorName: operatorName ?? this.operatorName,
    missingCapability: missingCapability ?? this.missingCapability,
    fleetRootAnchored: fleetRootAnchored ?? this.fleetRootAnchored,
    catalogContent: catalogContent ?? this.catalogContent,
    storeInconsistent: storeInconsistent ?? this.storeInconsistent,
    selection: selection ?? this.selection,
  );

  Map<String, Object?> toJson() => <String, Object?>{
    'pin': pin.toJson(),
    'stage': stage.wire,
    'pin_history': pinHistory.map((e) => e.toJson()).toList(growable: false),
    'hwm': highWaterMarks,
    'time_floor': timeFloorSec,
    'cat_cap': catalogCapabilities?.raw,
    'dir_cap': directiveCapabilities?.raw,
    'key_document': keyDocument?.toJson(),
    'catalog': catalog?.toJson(),
    'directive': directive?.toJson(),
    'locator': locator,
    'dtp': deviceThumbprint,
    'hw_tier': hardwareTier.wire,
    'agree_gen': agreementKeyGeneration,
    'enrolled_ms': enrolledAtMs,
    'revoked': revoked,
    'exph': offlineGraceSec,
    'settings': settings.toJson(),
    'cards': pendingChanges.map((e) => e.toJson()).toList(growable: false),
    'queue': writeQueue.toJson(),
    'ladder': ladder.toJson(),
    'operator_name': operatorName,
    'missing_cap': missingCapability,
    'fleet_anchored': fleetRootAnchored,
    'catalog_content': catalogContent.toJson(),
    'sel': selection.toJson(),
    if (storeInconsistent) 'store_inconsistent': true,
  };

  /// Читает состояние CSM с профиля. `null` означает «профиль не закреплял
  /// корневой ключ», что для записи, сделанной до CSM, и есть верный ответ.
  static CsmProfileState? fromJson(Object? raw) {
    // Ключа нет вовсе: это запись, сделанная до CSM, и "корень не закреплён"
    // здесь верный ответ.
    if (raw == null) {
      return null;
    }
    // Ключ есть, но прочитать его не удалось. Отдать здесь null значило бы
    // сказать "профиль никогда не закреплял корневой ключ", а это ровно та
    // дыра отката, которую 02-SPEC.md 8.8.3 запрещает: обнулившееся хранилище
    // неотличимо от отката, поэтому испорченная запись обязана оставить липкое
    // правило включённым, а не снять его.
    if (raw is! Map) {
      return _inconsistent(null);
    }
    final pin = CsmPin.fromJson(raw['pin']);
    if (pin == null) {
      return _inconsistent(null);
    }
    final hwm = <String, int>{};
    final rawHwm = raw['hwm'];
    var damaged = false;
    if (rawHwm is Map) {
      rawHwm.forEach((k, v) {
        if (v is num) {
          hwm[k.toString()] = v.toInt();
        } else {
          damaged = true;
        }
      });
    } else if (rawHwm != null) {
      damaged = true;
    }
    // Пол, стадия и липкий флаг отзыва читаются строго: значение не того типа
    // это повреждение, а не умолчание. Обнулить пол или снять st = 5 по
    // мусорному значению значит потерять ровно то, что переживает кэш.
    final rawFloor = raw['time_floor'];
    if (rawFloor != null && rawFloor is! num) {
      damaged = true;
    }
    final rawStage = raw['stage'];
    if (rawStage != null && rawStage is! String) {
      damaged = true;
    }
    final rawRevoked = raw['revoked'];
    if (rawRevoked != null && rawRevoked is! bool) {
      damaged = true;
    }
    if (damaged) {
      return _inconsistent(pin);
    }
    final rawHistory = raw['pin_history'];
    final rawCards = raw['cards'];
    return CsmProfileState(
      pin: pin,
      stage: CsmProfileStage.fromWire(raw['stage'] as String?),
      pinHistory: rawHistory is List
          ? rawHistory
                .map(CsmPinHistoryEntry.fromJson)
                .whereType<CsmPinHistoryEntry>()
                .toList(growable: false)
          : const <CsmPinHistoryEntry>[],
      highWaterMarks: hwm,
      timeFloorSec: (raw['time_floor'] as num?)?.toInt() ?? 0,
      catalogCapabilities: _cap(raw['cat_cap']),
      directiveCapabilities: _cap(raw['dir_cap']),
      keyDocument: CsmDocumentRecord.fromJson(raw['key_document']),
      catalog: CsmDocumentRecord.fromJson(raw['catalog']),
      directive: CsmDocumentRecord.fromJson(raw['directive']),
      locator: _nonEmpty(raw['locator']),
      deviceThumbprint: _nonEmpty(raw['dtp']),
      hardwareTier: CsmHardwareTier.fromWire((raw['hw_tier'] as num?)?.toInt()),
      agreementKeyGeneration: (raw['agree_gen'] as num?)?.toInt() ?? 0,
      enrolledAtMs: (raw['enrolled_ms'] as num?)?.toInt() ?? 0,
      revoked: raw['revoked'] == true,
      offlineGraceSec: (raw['exph'] as num?)?.toInt() ?? 604800,
      settings: CsmSettings.fromJson(raw['settings']),
      pendingChanges: rawCards is List
          ? rawCards
                .map(CsmPendingChange.fromJson)
                .whereType<CsmPendingChange>()
                .toList(growable: false)
          : const <CsmPendingChange>[],
      writeQueue: CsmWriteQueue.fromJson(raw['queue']),
      ladder: CsmLadderPrefs.fromJson(raw['ladder']),
      operatorName: _nonEmpty(raw['operator_name']),
      missingCapability: raw['missing_cap'] == true,
      // Отсутствие ключа читается как «якорь есть»: запись, сделанная до этого
      // поля, не должна показывать пользователю тревогу, которой не было.
      fleetRootAnchored: raw['fleet_anchored'] != false,
      catalogContent: CsmCatalogContent.fromJson(raw['catalog_content']),
      selection: CsmSelectionState.fromJson(raw['sel']),
      storeInconsistent: raw['store_inconsistent'] == true,
    );
  }

  /// Профиль, чья запись CSM прочиталась испорченной: закреплённый и
  /// непроверяемый.
  ///
  /// Стадия `anchored`, а не `pinning`, потому что липкое правило INV-13
  /// смотрит именно на `isPinned`: прочитать испорченную запись как
  /// незакреплённую значит снять правило одним испорченным байтом на диске.
  /// Ни отметки, ни временной пол при этом не выставляются в ноль как
  /// значение; [missingCapability] делает ошибку недиссмиссабельной, а
  /// [storeInconsistent] называет интерфейсу и диагностике причину.
  static CsmProfileState _inconsistent(CsmPin? pin) => CsmProfileState(
    pin:
        pin ??
        const CsmPin(
          pid: '',
          linkPin: '',
          origin: CsmPinOrigin.inApp,
          establishedMs: 0,
        ),
    stage: CsmProfileStage.anchored,
    storeInconsistent: true,
    missingCapability: true,
  );

  static CsmCapabilitySet? _cap(Object? v) =>
      v is num ? CsmCapabilitySet(v.toInt()) : null;

  static String? _nonEmpty(Object? v) => v is String && v.isNotEmpty ? v : null;
}

/// Ключ отметки максимума версий. Область: локатор для 0x03 и 0x08, `cat_id`
/// для 0x02 и 0x04, пусто для 0x01 и 0x05 (02-SPEC.md 5.1).
String csmHighWaterKey(int docType, String scope) => '$docType|$scope';

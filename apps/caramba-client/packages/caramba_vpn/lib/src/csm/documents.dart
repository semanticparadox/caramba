import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/codec/cbor.dart';
import 'package:caramba_vpn/src/csm/crypto/ed25519.dart';
import 'package:caramba_vpn/src/csm/crypto/sha2.dart';
import 'package:caramba_vpn/src/csm/errors.dart';
import 'package:caramba_vpn/src/csm/fields.dart';
import 'package:caramba_vpn/src/csm/frame.dart';
import 'package:caramba_vpn/src/csm/ids.dart';

// Типизированные модели документов и шаги P10, P11, P12 из 03-WIRE.md 6.1.
// Таблицы полей это 03-WIRE.md 8, перечисления это раздел 5.

/// Максимальное время жизни документа по типу, 03-WIRE.md 8.0. Используется
/// шагом V11 в исправленной форме: iat + LIFETIME_MAX + 300 >= time_floor.
const Map<int, int> csmLifetimeMax = <int, int>{
  CsmDocType.keyDocument: 604800,
  CsmDocType.catalog: 2592000,
  CsmDocType.directive: 3600,
  CsmDocType.catalogChunk: 2592000,
  CsmDocType.bootstrapBlob: 2592000,
  CsmDocType.sealedDirective: 3600,
  CsmDocType.reservePool: 604800,
};

/// Размер среза каталога в одном чанке, 03-WIRE.md 11.3.
const int csmChunkPayloadMax = 2816;

const Set<int> _stValues = <int>{1, 2, 3, 4, 5, 6, 7, 8};
const Set<int> _prValues = <int>{1, 2, 3, 4, 5, 6, 7, 8};
const Set<int> _nwValues = <int>{1, 2, 3, 4, 5, 6};
const Set<int> _seValues = <int>{0, 1, 2};
const Set<int> _fpValues = <int>{1, 2, 3, 4, 5, 6, 7, 8, 9, 10};
const Set<int> _alpValues = <int>{1, 2, 3};
const Set<int> _cgValues = <int>{1, 2, 3};
const Set<int> _ssmValues = <int>{1, 2, 3, 4, 5, 6};
const Set<int> _rungValues = <int>{0, 1, 2, 3, 4, 5, 6};
const Set<int> _srcValues = <int>{1, 2, 3};
const Set<int> _uiKindValues = <int>{1, 2, 3, 4, 5};

/// Общий конверт, 03-WIRE.md 8.0.
abstract class CsmDocument {
  CsmDocument.withEnvelope({
    required this.docType,
    required this.payload,
    required this.v,
    required this.pid,
    required this.ver,
    required this.iat,
    required this.exp,
  });

  final int docType;

  /// Сырая карта нагрузки. Хранится, чтобы ни один путь кода не был вынужден
  /// пересобирать документ ради доступа к полю.
  final CborMap payload;

  final int v;
  final Uint8List pid;
  final int ver;
  final int iat;
  final int exp;

  /// Поле дополнения (ключ 9). Содержимое игнорируется, но ненулевой байт это
  /// отказ: иначе дополнение становится скрытым каналом.
  Uint8List? get padding {
    final p = payload[9];
    return p is CborBytes ? p.value : null;
  }
}

class CsmEnvelope {
  const CsmEnvelope(this.v, this.pid, this.ver, this.iat, this.exp);
  final int v;
  final Uint8List pid;
  final int ver;
  final int iat;
  final int exp;
}

/// Шаг P10: ключи конверта 1..5 присутствуют, правильно типизированы, v == 1.
CsmEnvelope _envelope(CborMap m) {
  for (var k = 1; k <= 5; k++) {
    if (!m.has(k)) {
      envelopeFail('envelope key $k is absent');
    }
  }
  final v = m[1];
  if (v is! CborUint || v.value != 1) {
    envelopeFail('v is absent, not an unsigned integer, or not 1');
  }
  final pid = m[2];
  if (pid is! CborBytes || pid.value.length != 8) {
    envelopeFail('pid is not a byte string of exactly 8 bytes');
  }
  final ver = m[3];
  if (ver is! CborUint || ver.value >= 4294967296) {
    envelopeFail('ver is not an unsigned integer below 2^32');
  }
  final iat = m[4];
  if (iat is! CborUint) {
    envelopeFail('iat is not an unsigned integer');
  }
  final exp = m[5];
  if (exp is! CborUint) {
    envelopeFail('exp is not an unsigned integer');
  }
  return CsmEnvelope(v.value, pid.value, ver.value, iat.value, exp.value);
}

/// Шаг P11 в части, общей для всех типов: зарезервированные критические ключи
/// 6..8 запрещены в v1, ключ 9 это дополнение и оно обязано быть нулевым.
void _commonFields(CborMap m) {
  for (final reserved in <int>[6, 7, 8]) {
    if (m.has(reserved)) {
      fieldFail('reserved critical key $reserved must not appear in v1');
    }
  }
  final pd = m[9];
  if (pd != null) {
    final bytes = asBytes(pd, 'pd', max: csmMaxBstrBytes);
    for (final b in bytes) {
      if (b != 0) {
        fieldFail('pd carries a non-zero byte');
      }
    }
  }
}

// ---------------------------------------------------------------- 0x01

class CsmKeyEntry {
  const CsmKeyEntry(this.kid, this.alg, this.pk);
  final Uint8List kid;
  final int alg;
  final Uint8List pk;
}

class CsmRoleEntry {
  const CsmRoleEntry(this.ks, this.thr);

  /// Разрешённый набор ключей роли, идентификаторы по 12 байт.
  final List<Uint8List> ks;
  final int thr;
}

class CsmDeprecation {
  const CsmDeprecation(this.surface, this.sunset);
  final String surface;
  final int sunset;
}

class CsmKeyDocument extends CsmDocument {
  CsmKeyDocument({
    required CborMap map,
    required CsmEnvelope envelope,
    required this.keys,
    required this.roles,
    required this.revokedKids,
    required this.revokedNodes,
    required this.tiers,
    required this.deprecations,
    required this.ttlk,
  }) : super.withEnvelope(
         docType: CsmDocType.keyDocument,
         payload: map,
         v: envelope.v,
         pid: envelope.pid,
         ver: envelope.ver,
         iat: envelope.iat,
         exp: envelope.exp,
       );

  final List<CsmKeyEntry> keys;

  /// Роли: 1 root, 2 online, 3 timestamp (зарезервирована, в v1 не бывает).
  final Map<int, CsmRoleEntry> roles;
  final List<Uint8List> revokedKids;
  final List<String> revokedNodes;
  final Map<int, Uint8List> tiers;
  final List<CsmDeprecation> deprecations;
  final int? ttlk;

  CsmKeyEntry? keyById(List<int> kid) {
    for (final k in keys) {
      if (csmBytesEqual(k.kid, kid)) {
        return k;
      }
    }
    return null;
  }

  bool isRevoked(List<int> kid) {
    for (final r in revokedKids) {
      if (csmBytesEqual(r, kid)) {
        return true;
      }
    }
    return false;
  }
}

const Set<int> _keyDocKeys = <int>{1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 15, 16};

CsmKeyDocument _parseKeyDocument(CborMap m, CsmEnvelope env) {
  requireKnownCriticalKeys(m, _keyDocKeys, 'key document');

  final keyItems = asArray(require(m, 10, 'keys'), 'keys', min: 1, max: 16);
  final keys = <CsmKeyEntry>[];
  for (final item in keyItems) {
    final e = asMap(item, 'key entry');
    requireKnownCriticalKeys(e, const <int>{1, 2, 3}, 'key entry');
    final kid = asBytes(require(e, 1, 'kid'), 'kid', exact: 12);
    final alg = asUint(require(e, 2, 'alg'), 'alg');
    requireEnum(alg, const <int>{1}, 'alg');
    final pk = asBytes(require(e, 3, 'pk'), 'pk', exact: 32);
    // Шаг P12. Единственное место, где ключевой материал входит в доверенный
    // набор, поэтому здесь и только здесь проверяются все три пункта 2.1 и
    // пересчитывается kid.
    final ingest = ed25519AcceptPublicKey(pk);
    if (!ingest.isOk) {
      csmFail(
        CsmErrorCode.parseField,
        'P12',
        'pk fails 03-WIRE.md 2.1 clause ${ingest.clause}: ${ingest.reason}',
      );
    }
    final expected = sha256(pk).sublist(0, 12);
    if (!csmBytesEqual(expected, kid)) {
      csmFail(
        CsmErrorCode.parseField,
        'P12',
        'kid does not equal sha256(pk)[0..12]',
      );
    }
    keys.add(CsmKeyEntry(kid, alg, pk));
  }

  final rolesMap = asMap(
    require(m, 11, 'roles'),
    'roles',
    minPairs: 1,
    maxPairs: 3,
  );
  final roles = <int, CsmRoleEntry>{};
  for (final entry in rolesMap.entries.entries) {
    final role = entry.key;
    requireEnum(role, const <int>{1, 2, 3}, 'role');
    if (role == 3) {
      fieldFail('role 3 (timestamp) is reserved and must not appear in v1');
    }
    final rv = asMap(entry.value, 'role entry');
    requireKnownCriticalKeys(rv, const <int>{1, 2}, 'role entry');
    final ksItems = asArray(require(rv, 1, 'ks'), 'ks', min: 1, max: 16);
    final ks = <Uint8List>[];
    for (final k in ksItems) {
      ks.add(asBytes(k, 'ks entry', exact: 12));
    }
    final thr = asUint(require(rv, 2, 'thr'), 'thr', min: 1, max: 16);
    if (thr > ks.length) {
      fieldFail('thr $thr exceeds the ${ks.length}-key set of role $role');
    }
    roles[role] = CsmRoleEntry(ks, thr);
  }

  // Роль живёт только в roles, у записей keys нет поля роли. Каждый kid из
  // каждого ks обязан быть в keys, и каждая запись keys обязана быть названа
  // хотя бы одной ролью: ключ без роли это ключ, который никакой путь кода не
  // имеет права вернуть.
  for (final role in roles.entries) {
    for (final kid in role.value.ks) {
      var found = false;
      for (final k in keys) {
        if (csmBytesEqual(k.kid, kid)) {
          found = true;
          break;
        }
      }
      if (!found) {
        fieldFail('roles[${role.key}].ks names a kid that is not in keys');
      }
    }
  }
  for (final k in keys) {
    var referenced = false;
    for (final role in roles.values) {
      for (final kid in role.ks) {
        if (csmBytesEqual(kid, k.kid)) {
          referenced = true;
          break;
        }
      }
    }
    if (!referenced) {
      fieldFail('keys carries an entry named by no role');
    }
  }

  final revokedKids = <Uint8List>[];
  final revokedNodes = <String>[];
  final rev = m[12];
  if (rev != null) {
    final rm = asMap(rev, 'rev');
    requireKnownCriticalKeys(rm, const <int>{1, 2}, 'rev');
    final kidsField = rm[1];
    if (kidsField != null) {
      for (final k in asArray(kidsField, 'rev.kids', max: 64)) {
        revokedKids.add(asBytes(k, 'rev.kids entry', exact: 12));
      }
    }
    final nodesField = rm[2];
    if (nodesField != null) {
      for (final n in asArray(nodesField, 'rev.nodes', max: 256)) {
        final id = asText(n, 'rev.nodes entry', min: 1, max: 24);
        if (!matchesCharset(id, isNodeIdChar)) {
          fieldFail('rev.nodes entry is outside the node id charset');
        }
        revokedNodes.add(id);
      }
    }
  }

  final tiers = <int, Uint8List>{};
  final tiersField = m[13];
  if (tiersField != null) {
    final tm = asMap(tiersField, 'tiers', maxPairs: 16);
    for (final e in tm.entries.entries) {
      // Идентификатор тарифа здесь это КЛЮЧ карты CBOR, поэтому он подчиняется
      // правилу 3.3: ключ 0 и ключи от 1024 декодер отвергает раньше. Диапазон
      // 1..1023 назван в 03-WIRE.md 8.1 явно, чтобы панель не могла подписать
      // документ, который ни один соответствующий верификатор не прочитает.
      if (e.key < csmTierMin || e.key > csmTierMax) {
        fieldFail(
          'tiers carries a tier id outside $csmTierMin..$csmTierMax',
        );
      }
      tiers[e.key] = asBytes(e.value, 'tiers entry', exact: 32);
    }
  }

  if (m.has(14)) {
    fieldFail('key 14 is reserved in the critical range');
  }

  final deprecations = <CsmDeprecation>[];
  final depField = m[15];
  if (depField != null) {
    for (final d in asArray(depField, 'dep', max: 16)) {
      final dm = asMap(d, 'dep entry');
      requireKnownCriticalKeys(dm, const <int>{1, 2}, 'dep entry');
      deprecations.add(
        CsmDeprecation(
          asText(require(dm, 1, 'dep.s'), 'dep.s', max: 48),
          asUint(require(dm, 2, 'dep.sun'), 'dep.sun'),
        ),
      );
    }
  }

  final ttlkField = m[16];
  final ttlk = ttlkField == null
      ? null
      : asUint(ttlkField, 'ttlk', min: 300, max: 86400);

  return CsmKeyDocument(
    map: m,
    envelope: env,
    keys: keys,
    roles: roles,
    revokedKids: revokedKids,
    revokedNodes: revokedNodes,
    tiers: tiers,
    deprecations: deprecations,
    ttlk: ttlk,
  );
}

// ---------------------------------------------------------------- 0x02

class CsmNode {
  const CsmNode(this.id, this.pn, this.cc, this.host, this.port, this.protocol);
  final String id;
  final String pn;
  final String cc;
  final String host;
  final int port;
  final int protocol;
}

class CsmMirror {
  const CsmMirror(this.host, this.sni, this.asn, this.cc);
  final String host;
  final String sni;
  final int asn;
  final String cc;
}

class CsmCatalog extends CsmDocument {
  CsmCatalog({
    required CborMap map,
    required CsmEnvelope envelope,
    required this.tier,
    required this.exits,
    required this.relays,
    required this.mirrors,
    required this.capabilities,
    required this.ttl,
    required this.jitter,
    required this.respMax,
    required this.connBytes,
    required this.connPackets,
  }) : super.withEnvelope(
         docType: CsmDocType.catalog,
         payload: map,
         v: envelope.v,
         pid: envelope.pid,
         ver: envelope.ver,
         iat: envelope.iat,
         exp: envelope.exp,
       );

  final int tier;
  final List<CsmNode> exits;
  final List<CsmNode> relays;
  final List<CsmMirror> mirrors;
  final Uint8List capabilities;
  final int ttl;
  final int jitter;
  final int respMax;
  final int connBytes;
  final int connPackets;

  /// Бит набора возможностей оператора, 03-WIRE.md 5.1.
  bool capability(int bit) {
    final value =
        (capabilities[0] << 24) |
        (capabilities[1] << 16) |
        (capabilities[2] << 8) |
        capabilities[3];
    return (value & (1 << bit)) != 0;
  }
}

const Set<int> _catalogKeys = <int>{
  1, 2, 3, 4, 5, 9, //
  10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
};

const Set<int> _nodeKeys = <int>{
  1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, //
  13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
};

CsmNode _parseNode(CborValue value, String where) {
  final n = asMap(value, where);
  requireKnownCriticalKeys(n, _nodeKeys, where);

  final id = asText(require(n, 1, '$where.id'), '$where.id', min: 1, max: 24);
  if (!matchesCharset(id, isNodeIdChar)) {
    fieldFail('$where.id is outside the charset [0-9A-Za-z_-]');
  }
  final pn = asText(require(n, 2, '$where.pn'), '$where.pn', max: 64);
  final cc = asText(require(n, 3, '$where.cc'), '$where.cc', exact: 2);
  if (!isUpperAlpha2(cc)) {
    fieldFail('$where.cc is not an uppercase alpha-2 code');
  }
  final host = asText(require(n, 4, '$where.h'), '$where.h', max: 64);
  if (!isValidHostname(host) && !isIpLiteral(host)) {
    fieldFail('$where.h is neither a valid hostname nor an IP literal');
  }
  final port = asUint(
    require(n, 5, '$where.p'),
    '$where.p',
    min: 1,
    max: 65535,
  );
  final pr = asUint(require(n, 6, '$where.pr'), '$where.pr');
  requireEnum(pr, _prValues, '$where.pr');
  final nw = asUint(require(n, 7, '$where.nw'), '$where.nw');
  requireEnum(nw, _nwValues, '$where.nw');
  final se = asUint(require(n, 8, '$where.se'), '$where.se');
  requireEnum(se, _seValues, '$where.se');

  final sni = n[9];
  if (sni != null) {
    final s = asText(sni, '$where.sni', max: 64);
    if (!isValidHostname(s)) {
      fieldFail('$where.sni is not a valid hostname');
    }
  }
  if (n[10] != null) {
    asBytes(n[10]!, '$where.pbk', exact: 32);
  }
  if (n[11] != null) {
    final sid = asText(n[11]!, '$where.sid', max: 16);
    if (!matchesCharset(sid, isHexChar)) {
      fieldFail('$where.sid is not hex');
    }
  }
  if (n[12] != null) {
    requireEnum(asUint(n[12]!, '$where.fp'), _fpValues, '$where.fp');
  }
  if (n[13] != null) {
    final fl = asUint(n[13]!, '$where.fl');
    // Значение 0 означает "отсутствует", и рендерер обязан опустить ключ
    // целиком, а не выдать пустую строку. Поэтому наличие ключа со значением 0
    // это и есть ошибка.
    requireEnum(fl, const <int>{1}, '$where.fl');
  }
  if (n[14] != null) {
    asText(n[14]!, '$where.pt', max: 96);
  }
  if (n[15] != null) {
    final hst = asText(n[15]!, '$where.hst', max: 64);
    if (!isValidHostname(hst)) {
      fieldFail('$where.hst is not a valid hostname');
    }
  }
  if (n[16] != null) {
    for (final a in asArray(n[16]!, '$where.alp', max: 3)) {
      requireEnum(
        asUint(a, '$where.alp entry'),
        _alpValues,
        '$where.alp entry',
      );
    }
  }
  if (n[17] != null) {
    asText(n[17]!, '$where.hop', max: 32);
  }
  if (n[18] != null) {
    asText(n[18]!, '$where.obf', max: 32);
  }
  if (n[19] != null) {
    requireEnum(asUint(n[19]!, '$where.cg'), _cgValues, '$where.cg');
  }
  if (n[20] != null) {
    asBool(n[20]!, '$where.zr');
  }
  if (n[21] != null) {
    asBool(n[21]!, '$where.ins');
  }
  if (n[22] != null) {
    final rl = asText(n[22]!, '$where.rl', max: 24);
    if (!matchesCharset(rl, isNodeIdChar)) {
      fieldFail('$where.rl is outside the node id charset');
    }
  }
  if (n[23] != null) {
    requireEnum(asUint(n[23]!, '$where.ssm'), _ssmValues, '$where.ssm');
  }
  if (n[24] != null) {
    asUint(n[24]!, '$where.mtu', min: 576, max: 1500);
  }

  return CsmNode(id, pn, cc, host, port, pr);
}

CsmMirror _parseMirror(CborValue value, String where) {
  final m = asMap(value, where);
  requireKnownCriticalKeys(m, const <int>{1, 2, 3, 4, 5, 6, 7}, where);
  final host = asText(require(m, 1, '$where.h'), '$where.h', max: 64);
  if (!isValidHostname(host)) {
    fieldFail('$where.h is not a valid hostname');
  }
  final sni = asText(require(m, 2, '$where.sni'), '$where.sni', max: 64);
  if (!isValidHostname(sni)) {
    fieldFail('$where.sni is not a valid hostname');
  }
  for (final p in asArray(
    require(m, 3, '$where.pin'),
    '$where.pin',
    min: 1,
    max: 4,
  )) {
    asBytes(p, '$where.pin entry', exact: 32);
  }
  final asn = asUint(
    require(m, 4, '$where.asn'),
    '$where.asn',
    max: 4294967295,
  );
  final cc = asText(require(m, 5, '$where.cc'), '$where.cc', exact: 2);
  if (!isUpperAlpha2(cc)) {
    fieldFail('$where.cc is not an uppercase alpha-2 code');
  }
  if (m[6] != null) {
    asUint(m[6]!, '$where.w', min: 1, max: 100);
  }
  if (m[7] != null) {
    for (final ip in asArray(m[7]!, '$where.ip', max: 4)) {
      final text = asText(ip, '$where.ip entry', max: 64);
      if (!isIpLiteral(text)) {
        fieldFail('$where.ip entry is not an IP literal');
      }
    }
  }
  return CsmMirror(host, sni, asn, cc);
}

void _parseDoh(CborValue value, String where) {
  final d = asMap(value, where);
  requireKnownCriticalKeys(d, const <int>{1, 2, 3, 4}, where);
  final host = asText(require(d, 1, '$where.h'), '$where.h', max: 64);
  if (!isValidHostname(host)) {
    fieldFail('$where.h is not a valid hostname');
  }
  final path = asText(require(d, 2, '$where.p'), '$where.p', max: 128);
  if (!isValidPath(path)) {
    fieldFail('$where.p is not a valid path-only field');
  }
  for (final ip in asArray(
    require(d, 3, '$where.ip'),
    '$where.ip',
    min: 1,
    max: 4,
  )) {
    final text = asText(ip, '$where.ip entry', max: 64);
    if (!isIpLiteral(text)) {
      fieldFail('$where.ip entry is not an IP literal');
    }
  }
  for (final p in asArray(
    require(d, 4, '$where.pin'),
    '$where.pin',
    min: 1,
    max: 4,
  )) {
    asBytes(p, '$where.pin entry', exact: 32);
  }
}

void _parseResource(CborValue value, String where) {
  final r = asMap(value, where);
  requireKnownCriticalKeys(r, const <int>{1, 2, 3, 4}, where);
  asText(require(r, 1, '$where.n'), '$where.n', max: 48);
  final url = asText(require(r, 2, '$where.u'), '$where.u', max: 128);
  if (!isValidPath(url)) {
    fieldFail('$where.u is not a valid path-only field');
  }
  asBytes(require(r, 3, '$where.h'), '$where.h', exact: 32);
  if (r[4] != null) {
    asUint(r[4]!, '$where.iv', min: 3600, max: 604800);
  }
}

CsmCatalog _parseCatalog(CborMap m, CsmEnvelope env) {
  requireKnownCriticalKeys(m, _catalogKeys, 'catalog');

  final tier = asUint(
    require(m, 10, 'tier'),
    'tier',
    min: csmTierMin,
    max: csmTierMax,
  );
  final exits = <CsmNode>[];
  for (final e in asArray(require(m, 11, 'ex'), 'ex', min: 1, max: 512)) {
    exits.add(_parseNode(e, 'ex entry'));
  }
  final relays = <CsmNode>[];
  if (m[12] != null) {
    for (final e in asArray(m[12]!, 're', max: 64)) {
      relays.add(_parseNode(e, 're entry'));
    }
  }
  if (m[13] != null) {
    for (final r in asArray(m[13]!, 'ro', max: 32)) {
      final ro = asMap(r, 'ro entry');
      requireKnownCriticalKeys(ro, const <int>{1, 2, 3}, 'ro entry');
      final id = asText(require(ro, 1, 'ro.id'), 'ro.id', min: 1, max: 32);
      if (!matchesCharset(id, isRouteIdChar)) {
        fieldFail('ro.id is outside the charset [a-z0-9-]');
      }
      asText(require(ro, 2, 'ro.nm'), 'ro.nm', max: 40);
      for (final n in asArray(require(ro, 3, 'ro.rs'), 'ro.rs', max: 32)) {
        asText(n, 'ro.rs entry', max: 48);
      }
    }
  }
  final capabilities = asBytes(require(m, 14, 'cap'), 'cap', exact: 4);
  final mirrors = <CsmMirror>[];
  if (m[15] != null) {
    for (final x in asArray(m[15]!, 'mir', max: 32)) {
      mirrors.add(_parseMirror(x, 'mir entry'));
    }
  }
  if (m[16] != null) {
    for (final x in asArray(m[16]!, 'doh', max: 8)) {
      _parseDoh(x, 'doh entry');
    }
  }
  if (m[17] != null) {
    for (final x in asArray(m[17]!, 'rs', max: 32)) {
      _parseResource(x, 'rs entry');
    }
  }
  if (m[18] != null) {
    for (final x in asArray(m[18]!, 'geo', max: 8)) {
      _parseResource(x, 'geo entry');
    }
  }
  final ttl = asUint(require(m, 19, 'ttl'), 'ttl', min: 300, max: 86400);
  final jitter = asUint(require(m, 20, 'jit'), 'jit', max: 50);

  final thr = asMap(require(m, 21, 'thr'), 'thr');
  requireKnownCriticalKeys(thr, const <int>{1, 2, 3}, 'thr');
  final connBytes = asUint(require(thr, 1, 'thr.conn_bytes'), 'thr.conn_bytes');
  final connPackets = asUint(
    require(thr, 2, 'thr.conn_packets'),
    'thr.conn_packets',
  );
  final respMax = asUint(require(thr, 3, 'thr.resp_max'), 'thr.resp_max');

  final pb = asArray(require(m, 22, 'pb'), 'pb', min: 2, max: 2);
  final pbLow = asUint(pb[0], 'pb[0]', max: 15);
  final pbHigh = asUint(pb[1], 'pb[1]', max: 15);
  if (pbLow > pbHigh) {
    fieldFail('pb[0] $pbLow exceeds pb[1] $pbHigh');
  }

  if (m[23] != null) {
    final lad = asMap(m[23]!, 'lad');
    requireKnownCriticalKeys(lad, const <int>{1, 2}, 'lad');
    final ord = <int>[];
    for (final r in asArray(
      require(lad, 1, 'lad.ord'),
      'lad.ord',
      min: 1,
      max: 7,
    )) {
      final rung = asUint(r, 'lad.ord entry');
      requireEnum(rung, _rungValues, 'lad.ord entry');
      if (ord.contains(rung)) {
        fieldFail('lad.ord repeats rung $rung');
      }
      ord.add(rung);
    }
    final en = <int>[];
    for (final r in asArray(require(lad, 2, 'lad.en'), 'lad.en', max: 7)) {
      final rung = asUint(r, 'lad.en entry');
      if (!ord.contains(rung)) {
        fieldFail('lad.en names rung $rung which is not in lad.ord');
      }
      en.add(rung);
    }
    // R0 и R6 отключить нельзя никогда.
    if (!en.contains(0) || !en.contains(6)) {
      fieldFail('lad.en must contain rung 0 and rung 6');
    }
  }

  if (m[24] != null) {
    for (final p in asArray(m[24]!, 'pin', max: 32)) {
      final pe = asMap(p, 'pin entry');
      requireKnownCriticalKeys(pe, const <int>{1, 2}, 'pin entry');
      final host = asText(require(pe, 1, 'pin.h'), 'pin.h', max: 64);
      if (!isValidHostname(host)) {
        fieldFail('pin.h is not a valid hostname');
      }
      for (final s in asArray(
        require(pe, 2, 'pin.spki'),
        'pin.spki',
        min: 1,
        max: 4,
      )) {
        asBytes(s, 'pin.spki entry', exact: 32);
      }
    }
  }

  final hpk = m[25];
  final hpkv = m[26];
  if (hpk != null) {
    asBytes(hpk, 'hpk', exact: 65);
    if (hpkv == null) {
      fieldFail('hpkv must be present exactly when hpk is present');
    }
    asUint(hpkv, 'hpkv', max: 65535);
  } else if (hpkv != null) {
    fieldFail('hpkv is present without hpk');
  }

  return CsmCatalog(
    map: m,
    envelope: env,
    tier: tier,
    exits: exits,
    relays: relays,
    mirrors: mirrors,
    capabilities: capabilities,
    ttl: ttl,
    jitter: jitter,
    respMax: respMax,
    connBytes: connBytes,
    connPackets: connPackets,
  );
}

// ---------------------------------------------------------------- 0x03

class CsmSelection {
  const CsmSelection({
    this.exit,
    this.relay,
    this.preset,
    this.variant,
    this.proto,
    this.relayCountry,
    this.nodeId,
  });

  final String? exit;
  final String? relay;
  final String? preset;
  final int? variant;
  final int? proto;

  /// Разрешённая страна релея: код alpha-2 или литерал `--`, означающий, что
  /// оператор разрешил "без релея". `NO` для этого не годится, это Норвегия.
  final String? relayCountry;
  final int? nodeId;
}

class CsmDirective extends CsmDocument {
  CsmDirective({
    required CborMap map,
    required CsmEnvelope envelope,
    required this.nonce,
    required this.dtp,
    required this.status,
    required this.reasonCode,
    required this.catalogHash,
    required this.chunkCount,
    required this.tier,
    required this.capabilities,
    required this.selection,
    required this.announce,
    required this.support,
    required this.ttl,
    required this.offlineGrace,
    required this.locator,
  }) : super.withEnvelope(
         docType: CsmDocType.directive,
         payload: map,
         v: envelope.v,
         pid: envelope.pid,
         ver: envelope.ver,
         iat: envelope.iat,
         exp: envelope.exp,
       );

  final Uint8List nonce;
  final Uint8List dtp;
  final int status;
  final int reasonCode;
  final Uint8List catalogHash;
  final int chunkCount;
  final int tier;
  final Uint8List capabilities;
  final CsmSelection? selection;
  final String? announce;
  final String? support;
  final int ttl;
  final int? offlineGrace;
  final String locator;
}

const Set<int> _directiveKeys = <int>{
  1, 2, 3, 4, 5, 9, //
  10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
};

const Map<int, String> _policyKinds = <int, String>{
  1: 'text',
  2: 'text',
  3: 'text',
  4: 'text',
  5: 'uint',
  6: 'bool',
  7: 'bool',
  8: 'bool',
  9: 'textArray',
  10: 'textArray',
  11: 'text',
};

/// Отображение 02-SPEC.md 7.4 из строки протокола pol[1] в перечисление pr
/// раздела 5. Хранится ДАННЫМИ, чтобы не разъезжаться с таблицей, и
/// авторитетно ровно в одну сторону: VLESS и VLESS-Reality оба дают 1, поэтому
/// восстанавливать pol[1] из sel.proto нельзя, и ни один путь кода этого не
/// делает.
const Map<String, int> csmProtoWire = <String, int>{
  'auto': 0,
  'VLESS-Reality': 1,
  'VLESS': 1,
  'Hysteria2': 4,
  'TUIC': 5,
  'Shadowsocks': 6,
  'AmneziaWG': 8,
};

void _checkSelPolAgreement(CsmSelection sel, Map<int, String> polText) {
  // 1. sel.preset и pol[2] присутствуют оба и различаются.
  final polPreset = polText[2];
  if (sel.preset != null && polPreset != null && sel.preset != polPreset) {
    fieldFail('sel.preset "${sel.preset}" disagrees with pol[2] "$polPreset"');
  }

  // 2. sel.proto не равен PROTO_WIRE[pol[1]]. Незнакомая строка протокола
  // отображения не имеет, и предикат по ней не решается.
  final polProto = polText[1];
  if (sel.proto != null && polProto != null) {
    final want = csmProtoWire[polProto];
    if (want != null && want != sel.proto) {
      fieldFail('sel.proto ${sel.proto} is not PROTO_WIRE["$polProto"] = $want');
    }
  }

  // 3. sel.rcc против pol[3]: код страны требует себя же в верхнем регистре,
  // "--" требует "--", пустое значение разрешает любое законное.
  final polRcc = polText[3];
  if (sel.relayCountry != null && polRcc != null && polRcc.isNotEmpty) {
    final want = polRcc == '--' ? '--' : polRcc.toUpperCase();
    if (sel.relayCountry != want) {
      fieldFail(
        'sel.rcc "${sel.relayCountry}" disagrees with pol[3] "$polRcc"',
      );
    }
  }
}

CsmDirective _parseDirective(CborMap m, CsmEnvelope env) {
  requireKnownCriticalKeys(m, _directiveKeys, 'directive');

  final nonce = asBytes(require(m, 10, 'nonce'), 'nonce', exact: 16);
  final dtp = asBytes(require(m, 11, 'dtp'), 'dtp', exact: 16);
  final st = asUint(require(m, 12, 'st'), 'st');
  requireEnum(st, _stValues, 'st');
  // rc это машинный код и намеренно открытое пространство расширения:
  // нераспознанный rc отображается общим текстом своего st и не является
  // отказом разбора.
  final rc = m[13] == null ? 0 : asUint(m[13]!, 'rc');
  final cat = asBytes(require(m, 14, 'cat'), 'cat', exact: 32);
  final cn = asUint(require(m, 15, 'cn'), 'cn', min: 1, max: 64);
  final tier = asUint(
    require(m, 16, 'tier'),
    'tier',
    min: csmTierMin,
    max: csmTierMax,
  );
  final cap = asBytes(require(m, 17, 'cap'), 'cap', exact: 4);

  CsmSelection? selection;
  if (m[18] != null) {
    final sel = asMap(m[18]!, 'sel');
    requireKnownCriticalKeys(sel, const <int>{1, 2, 3, 4, 5, 6, 7}, 'sel');
    String? rcc;
    if (sel[6] != null) {
      rcc = asText(sel[6]!, 'sel.rcc', exact: 2);
      if (rcc != '--' && !isUpperAlpha2(rcc)) {
        fieldFail('sel.rcc is neither an uppercase alpha-2 code nor --');
      }
    }
    selection = CsmSelection(
      exit: sel[1] == null ? null : asText(sel[1]!, 'sel.exit', max: 24),
      relay: sel[2] == null ? null : asText(sel[2]!, 'sel.relay', max: 24),
      preset: sel[3] == null ? null : asText(sel[3]!, 'sel.preset', max: 32),
      variant: sel[4] == null ? null : asUint(sel[4]!, 'sel.variant', max: 255),
      proto: sel[5] == null ? null : asUint(sel[5]!, 'sel.proto', max: 8),
      relayCountry: rcc,
      nodeId: sel[7] == null ? null : asUint(sel[7]!, 'sel.nid'),
    );
  }

  final polText = <int, String>{};
  if (m[19] != null) {
    final pol = asMap(m[19]!, 'pol');
    for (final e in pol.entries.entries) {
      if (e.key > csmCriticalKeyMax) {
        continue;
      }
      final kind = _policyKinds[e.key];
      if (kind == null) {
        fieldFail('pol carries unknown setting key ${e.key}');
      }
      final pair = asArray(e.value, 'pol[${e.key}]', min: 2, max: 2);
      switch (kind) {
        case 'text':
          polText[e.key] = asText(pair[0], 'pol[${e.key}] value', max: 256);
        case 'uint':
          asUint(pair[0], 'pol[${e.key}] value');
        case 'bool':
          asBool(pair[0], 'pol[${e.key}] value');
        case 'textArray':
          for (final x in asArray(pair[0], 'pol[${e.key}] value', max: 32)) {
            asText(x, 'pol[${e.key}] entry', max: 256);
          }
      }
      requireEnum(
        asUint(pair[1], 'pol[${e.key}] src'),
        _srcValues,
        'pol[${e.key}] src',
      );
    }
  }

  // 02-SPEC.md 7.4: три самодостаточных предиката согласия sel и pol. Каждый
  // решается по пришедшим байтам целиком, поэтому проверяется на разборе и
  // даёт E_PARSE_FIELD. Два предиката, зависящих от каталога, здесь НЕ
  // проверяются: на разборе связанного каталога может ещё не быть.
  if (selection != null) {
    _checkSelPolAgreement(selection, polText);
  }

  final ann = m[20] == null ? null : asText(m[20]!, 'ann', max: 80);
  final sup = m[21] == null ? null : asText(m[21]!, 'sup', max: 80);
  if (m[22] != null) {
    for (final h in asArray(m[22]!, 'ui', max: 4)) {
      final hm = asMap(h, 'ui entry');
      requireKnownCriticalKeys(hm, const <int>{1, 2}, 'ui entry');
      requireEnum(
        asUint(require(hm, 1, 'ui.k'), 'ui.k'),
        _uiKindValues,
        'ui.k',
      );
      asText(require(hm, 2, 'ui.t'), 'ui.t', max: 80);
    }
  }
  final ttl = asUint(require(m, 23, 'ttl'), 'ttl', min: 300, max: 86400);
  final exph = m[24] == null ? null : asUint(m[24]!, 'exph', max: 2592000);
  final loc = asText(require(m, 25, 'loc'), 'loc', exact: 24);
  // loc это base32 Crockford над 120 битами (03-WIRE.md раздел 4). Он уходит в
  // путь URL и в заголовок, поэтому набор символов проверяется здесь, на
  // границе разбора, а не там, где строку уже склеили.
  if (!csmIsCrockford(loc)) {
    fieldFail('loc is not 24 base32 Crockford characters');
  }
  if (m[26] != null) {
    final traf = asMap(m[26]!, 'traf');
    requireKnownCriticalKeys(traf, const <int>{1, 2, 3, 4}, 'traf');
    for (var k = 1; k <= 4; k++) {
      if (traf[k] != null) {
        asUint(traf[k]!, 'traf[$k]');
      }
    }
  }

  return CsmDirective(
    map: m,
    envelope: env,
    nonce: nonce,
    dtp: dtp,
    status: st,
    reasonCode: rc,
    catalogHash: cat,
    chunkCount: cn,
    tier: tier,
    capabilities: cap,
    selection: selection,
    announce: ann,
    support: sup,
    ttl: ttl,
    offlineGrace: exph,
    locator: loc,
  );
}

// ---------------------------------------------------------------- 0x04

class CsmCatalogChunk extends CsmDocument {
  CsmCatalogChunk({
    required CborMap map,
    required CsmEnvelope envelope,
    required this.cid,
    required this.index,
    required this.count,
    required this.totalLength,
    required this.slice,
  }) : super.withEnvelope(
         docType: CsmDocType.catalogChunk,
         payload: map,
         v: envelope.v,
         pid: envelope.pid,
         ver: envelope.ver,
         iat: envelope.iat,
         exp: envelope.exp,
       );

  /// chash[0..10] переносимого каталога, те же байты, что и cat_id до base32.
  final Uint8List cid;
  final int index;
  final int count;
  final int totalLength;
  final Uint8List slice;
}

const Set<int> _chunkKeys = <int>{1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14};

CsmCatalogChunk _parseChunk(CborMap m, CsmEnvelope env) {
  requireKnownCriticalKeys(m, _chunkKeys, 'catalog chunk');
  final cid = asBytes(require(m, 10, 'cid'), 'cid', exact: 10);
  final i = asUint(require(m, 11, 'i'), 'i', max: 63);
  final n = asUint(require(m, 12, 'n'), 'n', min: 1, max: 64);
  final tl = asUint(require(m, 13, 'tl'), 'tl', min: 1, max: csmPayloadLenMax);
  final d = asBytes(require(m, 14, 'd'), 'd', min: 1, max: csmChunkPayloadMax);
  if (i >= n) {
    fieldFail('chunk index $i is not below the chunk count $n');
  }
  final expectedCount = (tl + csmChunkPayloadMax - 1) ~/ csmChunkPayloadMax;
  if (expectedCount != n) {
    fieldFail('chunk count $n does not match ceil(tl / $csmChunkPayloadMax)');
  }
  final low = i * csmChunkPayloadMax;
  final expectedLength = (low + csmChunkPayloadMax <= tl)
      ? csmChunkPayloadMax
      : tl - low;
  if (d.length != expectedLength) {
    fieldFail(
      'chunk $i of $n carries ${d.length} bytes where $expectedLength is '
      'required; accepting it would let a mirror choose the reassembly offsets',
    );
  }
  return CsmCatalogChunk(
    map: m,
    envelope: env,
    cid: cid,
    index: i,
    count: n,
    totalLength: tl,
    slice: d,
  );
}

// ---------------------------------------------------------------- 0x05

class CsmBootstrapBlob extends CsmDocument {
  CsmBootstrapBlob({
    required CborMap map,
    required CsmEnvelope envelope,
    required this.origin,
    required this.code,
    required this.rootKey,
    required this.mirrors,
    required this.operatorName,
  }) : super.withEnvelope(
         docType: CsmDocType.bootstrapBlob,
         payload: map,
         v: envelope.v,
         pid: envelope.pid,
         ver: envelope.ver,
         iat: envelope.iat,
         exp: envelope.exp,
       );

  final String origin;
  final String code;
  final Uint8List rootKey;
  final List<CsmMirror> mirrors;
  final String? operatorName;
}

const Set<int> _blobKeys = <int>{1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14, 15};

CsmBootstrapBlob _parseBlob(CborMap m, CsmEnvelope env) {
  requireKnownCriticalKeys(m, _blobKeys, 'bootstrap blob');
  final org = asText(require(m, 10, 'org'), 'org', max: 96);
  if (!isValidOrigin(org)) {
    fieldFail('org is not an https origin without a path');
  }
  final code = asText(require(m, 11, 'code'), 'code', min: 1, max: 32);
  final rk = asBytes(require(m, 12, 'rk'), 'rk', exact: 32);
  final ingest = ed25519AcceptPublicKey(rk);
  if (!ingest.isOk) {
    fieldFail('rk fails 03-WIRE.md 2.1 clause ${ingest.clause}');
  }
  final mirrors = <CsmMirror>[];
  for (final x in asArray(require(m, 13, 'mir'), 'mir', min: 1, max: 32)) {
    mirrors.add(_parseMirror(x, 'mir entry'));
  }
  for (final x in asArray(require(m, 14, 'doh'), 'doh', min: 1, max: 8)) {
    _parseDoh(x, 'doh entry');
  }
  final nm = m[15] == null ? null : asText(m[15]!, 'nm', max: 40);
  return CsmBootstrapBlob(
    map: m,
    envelope: env,
    origin: org,
    code: code,
    rootKey: rk,
    mirrors: mirrors,
    operatorName: nm,
  );
}

// ---------------------------------------------------------------- 0x06

class CsmSealedDirective extends CsmDocument {
  CsmSealedDirective({
    required CborMap map,
    required CsmEnvelope envelope,
    required this.dtp,
    required this.kem,
    required this.kdf,
    required this.aead,
    required this.enc,
    required this.ciphertext,
    required this.recipientKeyGeneration,
  }) : super.withEnvelope(
         docType: CsmDocType.sealedDirective,
         payload: map,
         v: envelope.v,
         pid: envelope.pid,
         ver: envelope.ver,
         iat: envelope.iat,
         exp: envelope.exp,
       );

  final Uint8List dtp;
  final int kem;
  final int kdf;
  final int aead;
  final Uint8List enc;
  final Uint8List ciphertext;
  final int recipientKeyGeneration;
}

const Set<int> _sealedKeys = <int>{
  1,
  2,
  3,
  4,
  5,
  9,
  10,
  11,
  12,
  13,
  14,
  15,
  16,
};

CsmSealedDirective _parseSealed(CborMap m, CsmEnvelope env) {
  requireKnownCriticalKeys(m, _sealedKeys, 'sealed directive');
  final dtp = asBytes(require(m, 10, 'dtp'), 'dtp', exact: 16);
  // Значения набора здесь только читаются: несовпадение это E_SEAL_SUITE на
  // шаге 4 распечатывания, а не отказ разбора поля.
  final kem = asUint(require(m, 11, 'kem'), 'kem', max: 65535);
  final kdf = asUint(require(m, 12, 'kdf'), 'kdf', max: 65535);
  final aead = asUint(require(m, 13, 'aead'), 'aead', max: 65535);
  final enc = asBytes(require(m, 14, 'enc'), 'enc', exact: 65);
  final ct = asBytes(require(m, 15, 'ct'), 'ct', min: 242, max: 3072);
  final rkv = asUint(require(m, 16, 'rkv'), 'rkv', max: 65535);
  return CsmSealedDirective(
    map: m,
    envelope: env,
    dtp: dtp,
    kem: kem,
    kdf: kdf,
    aead: aead,
    enc: enc,
    ciphertext: ct,
    recipientKeyGeneration: rkv,
  );
}

// ---------------------------------------------------------------- 0x08

class CsmReservePool extends CsmDocument {
  CsmReservePool({
    required CborMap map,
    required CsmEnvelope envelope,
    required this.mirrors,
    required this.cohort,
  }) : super.withEnvelope(
         docType: CsmDocType.reservePool,
         payload: map,
         v: envelope.v,
         pid: envelope.pid,
         ver: envelope.ver,
         iat: envelope.iat,
         exp: envelope.exp,
       );

  final List<CsmMirror> mirrors;
  final int? cohort;
}

const Set<int> _reserveKeys = <int>{1, 2, 3, 4, 5, 9, 10, 11, 12};

CsmReservePool _parseReserve(CborMap m, CsmEnvelope env) {
  requireKnownCriticalKeys(m, _reserveKeys, 'reserve pool');
  final mirrors = <CsmMirror>[];
  for (final x in asArray(require(m, 10, 'mir'), 'mir', min: 1, max: 32)) {
    mirrors.add(_parseMirror(x, 'mir entry'));
  }
  if (m[11] != null) {
    for (final x in asArray(m[11]!, 'doh', max: 8)) {
      _parseDoh(x, 'doh entry');
    }
  }
  final coh = m[12] == null ? null : asUint(m[12]!, 'coh', max: 65535);
  return CsmReservePool(map: m, envelope: env, mirrors: mirrors, cohort: coh);
}

// ----------------------------------------------------------------

/// Шаги P9..P12: строгий разбор CBOR, конверт, поля типа документа и, для
/// ключевого документа, приём ключевого материала.
CsmDocument csmParseDocument(int docType, Uint8List payload) {
  final map = csmDecodePayload(payload);
  final env = _envelope(map);
  _commonFields(map);
  switch (docType) {
    case CsmDocType.keyDocument:
      return _parseKeyDocument(map, env);
    case CsmDocType.catalog:
      return _parseCatalog(map, env);
    case CsmDocType.directive:
      return _parseDirective(map, env);
    case CsmDocType.catalogChunk:
      return _parseChunk(map, env);
    case CsmDocType.bootstrapBlob:
      return _parseBlob(map, env);
    case CsmDocType.sealedDirective:
      return _parseSealed(map, env);
    case CsmDocType.reservePool:
      return _parseReserve(map, env);
    default:
      csmFail(CsmErrorCode.parseDocType, 'P3', 'unhandled doc_type $docType');
  }
}

/// Разобранный кадр вместе с его типизированным документом.
class CsmParsed {
  const CsmParsed(this.frame, this.document);
  final CsmFrame frame;
  final CsmDocument document;
}

/// Полный разбор, шаги P1..P12. Ошибка это отказ разбора, а не заявление о
/// подмене: подавляющее большинство причин это портал захвата, зеркало,
/// отдающее страницу ошибки, или обрезанный ответ.
CsmParsed csmParse(Uint8List bytes) {
  final frame = CsmFrame.parse(bytes);
  final doc = csmParseDocument(frame.docType, frame.payload);
  return CsmParsed(frame, doc);
}

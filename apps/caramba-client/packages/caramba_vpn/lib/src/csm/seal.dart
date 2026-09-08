import 'dart:convert';
import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/crypto/chacha20poly1305.dart';
import 'package:caramba_vpn/src/csm/crypto/hkdf.dart';
import 'package:caramba_vpn/src/csm/crypto/p256.dart';

// HPKE в объёме одного набора CSM/1, RFC 9180 Appendix A.5:
//   mode_base 0x00, DHKEM(P-256, HKDF-SHA256) 0x0010,
//   HKDF-SHA256 0x0001, ChaCha20Poly1305 0x0003.
// 03-WIRE.md 9.1. Клиент только распечатывает, поэтому здесь есть Open и нет
// Seal.

const int csmHpkeModeBase = 0x00;
const int csmHpkeKem = 0x0010;
const int csmHpkeKdf = 0x0001;
const int csmHpkeAead = 0x0003;

const String csmSealInfo = 'CSM1-seal-v1';

Uint8List _i2osp2(int v) =>
    Uint8List.fromList(<int>[(v >> 8) & 0xff, v & 0xff]);

Uint8List _kemSuiteId() =>
    Uint8List.fromList(<int>[...utf8.encode('KEM'), ..._i2osp2(csmHpkeKem)]);

Uint8List _hpkeSuiteId() => Uint8List.fromList(<int>[
  ...utf8.encode('HPKE'),
  ..._i2osp2(csmHpkeKem),
  ..._i2osp2(csmHpkeKdf),
  ..._i2osp2(csmHpkeAead),
]);

Uint8List _labeledExtract(
  List<int> suiteId,
  List<int>? salt,
  String label,
  List<int> ikm,
) => hkdfExtract(salt, <int>[
  ...utf8.encode('HPKE-v1'),
  ...suiteId,
  ...utf8.encode(label),
  ...ikm,
]);

Uint8List _labeledExpand(
  List<int> suiteId,
  List<int> prk,
  String label,
  List<int> info,
  int length,
) {
  final labeledInfo = <int>[
    ..._i2osp2(length),
    ...utf8.encode('HPKE-v1'),
    ...suiteId,
    ...utf8.encode(label),
    ...info,
  ];
  return hkdfExpand(prk, labeledInfo, length);
}

/// Контекст HPKE после key schedule. Экспортируется, потому что тестовый
/// вектор RFC 9180 A.5.1 проверяется поле за полем.
class CsmHpkeContext {
  const CsmHpkeContext({
    required this.keyScheduleContext,
    required this.secret,
    required this.key,
    required this.baseNonce,
    required this.exporterSecret,
  });

  final Uint8List keyScheduleContext;
  final Uint8List secret;
  final Uint8List key;
  final Uint8List baseNonce;
  final Uint8List exporterSecret;
}

/// Key schedule базового режима, RFC 9180 section 5.1.
CsmHpkeContext csmHpkeKeySchedule(List<int> sharedSecret, List<int> info) {
  final sid = _hpkeSuiteId();
  final pskIdHash = _labeledExtract(sid, null, 'psk_id_hash', const <int>[]);
  final infoHash = _labeledExtract(sid, null, 'info_hash', info);
  final ksContext = Uint8List.fromList(<int>[
    csmHpkeModeBase,
    ...pskIdHash,
    ...infoHash,
  ]);
  final secret = _labeledExtract(sid, sharedSecret, 'secret', const <int>[]);
  return CsmHpkeContext(
    keyScheduleContext: ksContext,
    secret: secret,
    key: _labeledExpand(sid, secret, 'key', ksContext, 32),
    baseNonce: _labeledExpand(sid, secret, 'base_nonce', ksContext, 12),
    exporterSecret: _labeledExpand(sid, secret, 'exp', ksContext, 32),
  );
}

/// DHKEM(P-256, HKDF-SHA256) Decap. Возвращает null, если `enc` не является
/// несжатой точкой на кривой или если ECDH выродился.
Uint8List? csmDhkemDecap(List<int> recipientScalar, List<int> enc) {
  final point = p256DecodeUncompressed(enc);
  if (point == null) {
    return null;
  }
  final dh = p256Ecdh(recipientScalar, point);
  if (dh == null) {
    return null;
  }
  final pkR = p256PublicKey(recipientScalar);
  if (pkR == null) {
    return null;
  }
  final kemContext = <int>[...enc, ...pkR.uncompressed];
  final eaePrk = _labeledExtract(_kemSuiteId(), null, 'eae_prk', dh);
  return _labeledExpand(_kemSuiteId(), eaePrk, 'shared_secret', kemContext, 32);
}

/// HPKE Open на нулевом порядковом номере: одно запечатывание на одну
/// директиву, потока здесь нет. Возвращает null на любой неудаче.
Uint8List? csmHpkeOpen({
  required List<int> recipientScalar,
  required List<int> enc,
  required List<int> info,
  required List<int> aad,
  required List<int> ciphertext,
}) {
  final shared = csmDhkemDecap(recipientScalar, enc);
  if (shared == null) {
    return null;
  }
  final ctx = csmHpkeKeySchedule(shared, info);
  return chacha20Poly1305Open(ctx.key, ctx.baseNonce, aad, ciphertext);
}

/// aad = "CSM1" || 0x06 || pid(8) || dtp(16) || u32be(ver), 33 байта.
/// Получатель ОБЯЗАН пересчитать её из полей внешней нагрузки и не имеет права
/// принимать aad с провода.
Uint8List csmSealAad(List<int> pid, List<int> dtp, int ver) =>
    Uint8List.fromList(<int>[
      ...utf8.encode('CSM1'),
      0x06,
      ...pid,
      ...dtp,
      (ver >> 24) & 0xff,
      (ver >> 16) & 0xff,
      (ver >> 8) & 0xff,
      ver & 0xff,
    ]);

import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/crypto/sha2.dart';

// HMAC-SHA256 и HKDF-SHA256, RFC 2104 и RFC 5869. Нужны для вывода loc
// (03-WIRE.md 4) и для схемы ключей HPKE (03-WIRE.md 9, RFC 9180).

const int _blockSize = 64;

Uint8List hmacSha256(List<int> key, List<int> message) {
  var k = Uint8List.fromList(key);
  if (k.length > _blockSize) {
    k = sha256(k);
  }
  final padded = Uint8List(_blockSize);
  padded.setRange(0, k.length, k);

  final inner = Uint8List(_blockSize + message.length);
  final outer = Uint8List(_blockSize + 32);
  for (var i = 0; i < _blockSize; i++) {
    inner[i] = padded[i] ^ 0x36;
    outer[i] = padded[i] ^ 0x5c;
  }
  inner.setRange(_blockSize, inner.length, message);
  outer.setRange(_blockSize, outer.length, sha256(inner));
  return sha256(outer);
}

/// HKDF-Extract, RFC 5869 section 2.2. Пустая соль это 32 нулевых байта.
Uint8List hkdfExtract(List<int>? salt, List<int> ikm) =>
    hmacSha256(salt ?? Uint8List(32), ikm);

/// HKDF-Expand, RFC 5869 section 2.3.
Uint8List hkdfExpand(List<int> prk, List<int> info, int length) {
  if (length > 255 * 32) {
    throw ArgumentError('hkdfExpand: length above 255 * HashLen');
  }
  final out = Uint8List(length);
  var t = <int>[];
  var done = 0;
  var counter = 1;
  while (done < length) {
    t = hmacSha256(prk, <int>[...t, ...info, counter]);
    final take = (length - done) < t.length ? (length - done) : t.length;
    out.setRange(done, done + take, t);
    done += take;
    counter++;
  }
  return out;
}

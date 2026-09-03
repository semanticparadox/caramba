import 'dart:typed_data';

// NIST P-256 (secp256r1) на BigInt: ровно столько, сколько нужно для
// DHKEM(P-256, HKDF-SHA256) из 03-WIRE.md 9.1. Умножение точки на скаляр,
// разбор несжатой кодировки и проверка принадлежности кривой. Ключ устройства
// именно P-256, потому что он обязан жить в Secure Enclave или StrongBox, а
// X25519 там не бывает.

final BigInt _p = BigInt.parse(
  'ffffffff00000001000000000000000000000000ffffffffffffffffffffffff',
  radix: 16,
);
final BigInt _a = _p - BigInt.from(3);
final BigInt _b = BigInt.parse(
  '5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b',
  radix: 16,
);
final BigInt _gx = BigInt.parse(
  '6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296',
  radix: 16,
);
final BigInt _gy = BigInt.parse(
  '4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5',
  radix: 16,
);
final BigInt _n = BigInt.parse(
  'ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551',
  radix: 16,
);

BigInt _mod(BigInt x) {
  final r = x % _p;
  return r.isNegative ? r + _p : r;
}

/// Точка в якобиевых координатах. Бесконечность это z == 0.
class _Jac {
  const _Jac(this.x, this.y, this.z);

  final BigInt x;
  final BigInt y;
  final BigInt z;

  bool get isInfinity => z == BigInt.zero;
}

_Jac _double(_Jac q) {
  if (q.isInfinity || q.y == BigInt.zero) {
    return _Jac(BigInt.one, BigInt.one, BigInt.zero);
  }
  final yy = _mod(q.y * q.y);
  final s = _mod(BigInt.from(4) * q.x * yy);
  final zz = _mod(q.z * q.z);
  final m = _mod(BigInt.from(3) * q.x * q.x + _a * zz * zz);
  final x3 = _mod(m * m - BigInt.two * s);
  final y3 = _mod(m * (s - x3) - BigInt.from(8) * yy * yy);
  final z3 = _mod(BigInt.two * q.y * q.z);
  return _Jac(x3, y3, z3);
}

_Jac _add(_Jac p1, _Jac p2) {
  if (p1.isInfinity) {
    return p2;
  }
  if (p2.isInfinity) {
    return p1;
  }
  final z1z1 = _mod(p1.z * p1.z);
  final z2z2 = _mod(p2.z * p2.z);
  final u1 = _mod(p1.x * z2z2);
  final u2 = _mod(p2.x * z1z1);
  final s1 = _mod(p1.y * z2z2 * p2.z);
  final s2 = _mod(p2.y * z1z1 * p1.z);
  if (u1 == u2) {
    if (s1 != s2) {
      return _Jac(BigInt.one, BigInt.one, BigInt.zero);
    }
    return _double(p1);
  }
  final h = _mod(u2 - u1);
  final r = _mod(s2 - s1);
  final hh = _mod(h * h);
  final hhh = _mod(hh * h);
  final v = _mod(u1 * hh);
  final x3 = _mod(r * r - hhh - BigInt.two * v);
  final y3 = _mod(r * (v - x3) - s1 * hhh);
  final z3 = _mod(p1.z * p2.z * h);
  return _Jac(x3, y3, z3);
}

_Jac _mul(_Jac q, BigInt k) {
  var acc = _Jac(BigInt.one, BigInt.one, BigInt.zero);
  for (var i = k.bitLength - 1; i >= 0; i--) {
    acc = _double(acc);
    if ((k >> i).isOdd) {
      acc = _add(acc, q);
    }
  }
  return acc;
}

List<BigInt>? _affine(_Jac q) {
  if (q.isInfinity) {
    return null;
  }
  final zi = q.z.modInverse(_p);
  final zi2 = _mod(zi * zi);
  return <BigInt>[_mod(q.x * zi2), _mod(q.y * zi2 * zi)];
}

Uint8List _be32(BigInt v) {
  final out = Uint8List(32);
  var x = v;
  for (var i = 31; i >= 0; i--) {
    out[i] = (x & BigInt.from(0xff)).toInt();
    x = x >> 8;
  }
  return out;
}

BigInt _beToBig(List<int> b) {
  var v = BigInt.zero;
  for (final x in b) {
    v = (v << 8) | BigInt.from(x);
  }
  return v;
}

/// Открытая точка P-256, разобранная из 65-байтовой несжатой кодировки.
class P256Point {
  const P256Point(this.x, this.y);

  final BigInt x;
  final BigInt y;

  Uint8List get uncompressed =>
      Uint8List.fromList(<int>[0x04, ..._be32(x), ..._be32(y)]);
}

/// Разбор `0x04 || X || Y`. Сжатые и гибридные кодировки отвергаются: 9.1
/// требует именно несжатую форму, а точка обязана лежать на кривой.
P256Point? p256DecodeUncompressed(List<int> encoded) {
  if (encoded.length != 65 || encoded[0] != 0x04) {
    return null;
  }
  final x = _beToBig(encoded.sublist(1, 33));
  final y = _beToBig(encoded.sublist(33, 65));
  if (x >= _p || y >= _p) {
    return null;
  }
  final lhs = _mod(y * y);
  final rhs = _mod(x * x * x + _a * x + _b);
  if (lhs != rhs) {
    return null;
  }
  if (x == BigInt.zero && y == BigInt.zero) {
    return null;
  }
  return P256Point(x, y);
}

/// Публичный ключ для 32-байтового скаляра. Возвращает null для скаляра вне
/// диапазона [1, n-1].
P256Point? p256PublicKey(List<int> scalar) {
  if (scalar.length != 32) {
    return null;
  }
  final k = _beToBig(scalar);
  if (k == BigInt.zero || k >= _n) {
    return null;
  }
  final aff = _affine(_mul(_Jac(_gx, _gy, BigInt.one), k));
  if (aff == null) {
    return null;
  }
  return P256Point(aff[0], aff[1]);
}

/// ECDH: 32 байта X-координаты общей точки, как требует RFC 9180 для
/// DHKEM(P-256). Возвращает null, если результат это бесконечность.
Uint8List? p256Ecdh(List<int> scalar, P256Point peer) {
  final k = _beToBig(scalar);
  if (k == BigInt.zero || k >= _n) {
    return null;
  }
  final aff = _affine(_mul(_Jac(peer.x, peer.y, BigInt.one), k));
  if (aff == null) {
    return null;
  }
  return _be32(aff[0]);
}

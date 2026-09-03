import 'dart:typed_data';

// SHA-256 и SHA-512 на чистом Dart. Зависимостей нет намеренно: строгий
// профиль 03-WIRE.md 2 требует проверок, которых готовые пакеты не делают, а
// хеши всё равно нужны для kid, pid, chash, HMAC и HKDF.
//
// Таблицы констант не переписаны руками, а вычислены из тех же простых чисел,
// из которых их выводит FIPS 180-4: K256 это дробная часть квадратного корня
// первых 64 простых, K512 это дробная часть кубического корня первых 80.
// Ошибка переписывания восьмидесяти 64-битных констант вероятнее ошибки в
// восьми строках целочисленного корня, и обе ловятся тестом на известные
// digest'ы sha256("abc") и sha512("abc").

List<int> _firstPrimes(int count) {
  final out = <int>[];
  var n = 2;
  while (out.length < count) {
    var isPrime = true;
    for (var d = 2; d * d <= n; d++) {
      if (n % d == 0) {
        isPrime = false;
        break;
      }
    }
    if (isPrime) {
      out.add(n);
    }
    n++;
  }
  return out;
}

/// Целая часть корня степени [k] из [n], метод Ньютона на BigInt.
BigInt _iroot(BigInt n, int k) {
  if (n < BigInt.two) {
    return n;
  }
  final bk = BigInt.from(k);
  final bk1 = BigInt.from(k - 1);
  var x = BigInt.one << ((n.bitLength + k - 1) ~/ k);
  while (true) {
    final y = (bk1 * x + n ~/ x.pow(k - 1)) ~/ bk;
    if (y >= x) {
      return x;
    }
    x = y;
  }
}

final BigInt _mask32 = (BigInt.one << 32) - BigInt.one;
final BigInt _mask64 = (BigInt.one << 64) - BigInt.one;

Uint32List _fracRoot32(int count, int shift, int degree) {
  final primes = _firstPrimes(count);
  final out = Uint32List(count);
  for (var i = 0; i < count; i++) {
    final scaled = BigInt.from(primes[i]) << shift;
    out[i] = (_iroot(scaled, degree) & _mask32).toInt();
  }
  return out;
}

List<int> _fracRoot64(int count, int shift, int degree) {
  final primes = _firstPrimes(count);
  final out = List<int>.filled(count, 0);
  for (var i = 0; i < count; i++) {
    final scaled = BigInt.from(primes[i]) << shift;
    out[i] = (_iroot(scaled, degree) & _mask64).toSigned(64).toInt();
  }
  return out;
}

final Uint32List _k256 = _fracRoot32(64, 96, 3);
final Uint32List _h256 = _fracRoot32(8, 64, 2);
final List<int> _k512 = _fracRoot64(80, 192, 3);
final List<int> _h512 = _fracRoot64(8, 128, 2);

int _rotr32(int x, int n) => ((x >>> n) | (x << (32 - n))) & 0xffffffff;

/// SHA-256 по FIPS 180-4.
Uint8List sha256(List<int> data) {
  final h = Uint32List.fromList(_h256);
  final bitLen = data.length * 8;
  final padded = <int>[...data, 0x80];
  while (padded.length % 64 != 56) {
    padded.add(0);
  }
  for (var i = 7; i >= 0; i--) {
    padded.add((bitLen >>> (i * 8)) & 0xff);
  }

  final w = Uint32List(64);
  for (var off = 0; off < padded.length; off += 64) {
    for (var i = 0; i < 16; i++) {
      final j = off + i * 4;
      w[i] = (padded[j] << 24) |
          (padded[j + 1] << 16) |
          (padded[j + 2] << 8) |
          padded[j + 3];
    }
    for (var i = 16; i < 64; i++) {
      final s0 = _rotr32(w[i - 15], 7) ^ _rotr32(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      final s1 = _rotr32(w[i - 2], 17) ^ _rotr32(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) & 0xffffffff;
    }
    var a = h[0];
    var b = h[1];
    var c = h[2];
    var d = h[3];
    var e = h[4];
    var f = h[5];
    var g = h[6];
    var hh = h[7];
    for (var i = 0; i < 64; i++) {
      final s1 = _rotr32(e, 6) ^ _rotr32(e, 11) ^ _rotr32(e, 25);
      final ch = (e & f) ^ ((~e & 0xffffffff) & g);
      final t1 = (hh + s1 + ch + _k256[i] + w[i]) & 0xffffffff;
      final s0 = _rotr32(a, 2) ^ _rotr32(a, 13) ^ _rotr32(a, 22);
      final maj = (a & b) ^ (a & c) ^ (b & c);
      final t2 = (s0 + maj) & 0xffffffff;
      hh = g;
      g = f;
      f = e;
      e = (d + t1) & 0xffffffff;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) & 0xffffffff;
    }
    h[0] = (h[0] + a) & 0xffffffff;
    h[1] = (h[1] + b) & 0xffffffff;
    h[2] = (h[2] + c) & 0xffffffff;
    h[3] = (h[3] + d) & 0xffffffff;
    h[4] = (h[4] + e) & 0xffffffff;
    h[5] = (h[5] + f) & 0xffffffff;
    h[6] = (h[6] + g) & 0xffffffff;
    h[7] = (h[7] + hh) & 0xffffffff;
  }

  final out = Uint8List(32);
  for (var i = 0; i < 8; i++) {
    out[i * 4] = (h[i] >>> 24) & 0xff;
    out[i * 4 + 1] = (h[i] >>> 16) & 0xff;
    out[i * 4 + 2] = (h[i] >>> 8) & 0xff;
    out[i * 4 + 3] = h[i] & 0xff;
  }
  return out;
}

int _rotr64(int x, int n) => (x >>> n) | (x << (64 - n));

/// SHA-512 по FIPS 180-4. Работает на 64-битном int Dart VM.
Uint8List sha512(List<int> data) {
  final h = List<int>.from(_h512);
  final bitLen = data.length * 8;
  final padded = <int>[...data, 0x80];
  while (padded.length % 128 != 112) {
    padded.add(0);
  }
  for (var i = 0; i < 8; i++) {
    padded.add(0);
  }
  for (var i = 7; i >= 0; i--) {
    padded.add((bitLen >>> (i * 8)) & 0xff);
  }

  final w = List<int>.filled(80, 0);
  for (var off = 0; off < padded.length; off += 128) {
    for (var i = 0; i < 16; i++) {
      var v = 0;
      for (var b = 0; b < 8; b++) {
        v = (v << 8) | padded[off + i * 8 + b];
      }
      w[i] = v;
    }
    for (var i = 16; i < 80; i++) {
      final s0 = _rotr64(w[i - 15], 1) ^ _rotr64(w[i - 15], 8) ^ (w[i - 15] >>> 7);
      final s1 = _rotr64(w[i - 2], 19) ^ _rotr64(w[i - 2], 61) ^ (w[i - 2] >>> 6);
      w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    var a = h[0];
    var b = h[1];
    var c = h[2];
    var d = h[3];
    var e = h[4];
    var f = h[5];
    var g = h[6];
    var hh = h[7];
    for (var i = 0; i < 80; i++) {
      final s1 = _rotr64(e, 14) ^ _rotr64(e, 18) ^ _rotr64(e, 41);
      final ch = (e & f) ^ (~e & g);
      final t1 = hh + s1 + ch + _k512[i] + w[i];
      final s0 = _rotr64(a, 28) ^ _rotr64(a, 34) ^ _rotr64(a, 39);
      final maj = (a & b) ^ (a & c) ^ (b & c);
      final t2 = s0 + maj;
      hh = g;
      g = f;
      f = e;
      e = d + t1;
      d = c;
      c = b;
      b = a;
      a = t1 + t2;
    }
    h[0] += a;
    h[1] += b;
    h[2] += c;
    h[3] += d;
    h[4] += e;
    h[5] += f;
    h[6] += g;
    h[7] += hh;
  }

  final out = Uint8List(64);
  for (var i = 0; i < 8; i++) {
    for (var b = 0; b < 8; b++) {
      out[i * 8 + b] = (h[i] >>> (56 - b * 8)) & 0xff;
    }
  }
  return out;
}

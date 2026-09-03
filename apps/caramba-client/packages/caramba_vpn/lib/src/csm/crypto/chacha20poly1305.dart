import 'dart:typed_data';

// ChaCha20-Poly1305 AEAD, RFC 8439. Нужен как AEAD набора HPKE 0x0003
// (03-WIRE.md 9.1). Реализация только на распечатывание: клиент никогда не
// запечатывает 0x06, он его только открывает.

Uint32List _chachaBlock(Uint8List key, Uint8List nonce, int counter) {
  final state = Uint32List(16);
  state[0] = 0x61707865;
  state[1] = 0x3320646e;
  state[2] = 0x79622d32;
  state[3] = 0x6b206574;
  for (var i = 0; i < 8; i++) {
    state[4 + i] = key[i * 4] |
        (key[i * 4 + 1] << 8) |
        (key[i * 4 + 2] << 16) |
        (key[i * 4 + 3] << 24);
  }
  state[12] = counter;
  for (var i = 0; i < 3; i++) {
    state[13 + i] = nonce[i * 4] |
        (nonce[i * 4 + 1] << 8) |
        (nonce[i * 4 + 2] << 16) |
        (nonce[i * 4 + 3] << 24);
  }

  final w = Uint32List.fromList(state);
  int rotl(int v, int n) => ((v << n) | (v >>> (32 - n))) & 0xffffffff;
  void quarter(int a, int b, int c, int d) {
    w[a] = (w[a] + w[b]) & 0xffffffff;
    w[d] = rotl(w[d] ^ w[a], 16);
    w[c] = (w[c] + w[d]) & 0xffffffff;
    w[b] = rotl(w[b] ^ w[c], 12);
    w[a] = (w[a] + w[b]) & 0xffffffff;
    w[d] = rotl(w[d] ^ w[a], 8);
    w[c] = (w[c] + w[d]) & 0xffffffff;
    w[b] = rotl(w[b] ^ w[c], 7);
  }

  for (var i = 0; i < 10; i++) {
    quarter(0, 4, 8, 12);
    quarter(1, 5, 9, 13);
    quarter(2, 6, 10, 14);
    quarter(3, 7, 11, 15);
    quarter(0, 5, 10, 15);
    quarter(1, 6, 11, 12);
    quarter(2, 7, 8, 13);
    quarter(3, 4, 9, 14);
  }
  for (var i = 0; i < 16; i++) {
    w[i] = (w[i] + state[i]) & 0xffffffff;
  }
  return w;
}

Uint8List _chacha20(Uint8List key, Uint8List nonce, int counter, List<int> input) {
  final out = Uint8List(input.length);
  var block = Uint32List(0);
  for (var i = 0; i < input.length; i++) {
    if (i % 64 == 0) {
      block = _chachaBlock(key, nonce, counter + i ~/ 64);
    }
    final word = block[(i % 64) ~/ 4];
    final byte = (word >>> ((i % 4) * 8)) & 0xff;
    out[i] = input[i] ^ byte;
  }
  return out;
}

BigInt _leToBig(List<int> b) {
  var v = BigInt.zero;
  for (var i = b.length - 1; i >= 0; i--) {
    v = (v << 8) | BigInt.from(b[i]);
  }
  return v;
}

final BigInt _poly1305P = (BigInt.one << 130) - BigInt.from(5);

Uint8List _poly1305(Uint8List key, List<int> message) {
  final rBytes = Uint8List.fromList(key.sublist(0, 16));
  rBytes[3] &= 15;
  rBytes[7] &= 15;
  rBytes[11] &= 15;
  rBytes[15] &= 15;
  rBytes[4] &= 252;
  rBytes[8] &= 252;
  rBytes[12] &= 252;
  final r = _leToBig(rBytes);
  final s = _leToBig(key.sublist(16, 32));

  var acc = BigInt.zero;
  for (var i = 0; i < message.length; i += 16) {
    final end = (i + 16 < message.length) ? i + 16 : message.length;
    final chunk = <int>[...message.sublist(i, end), 1];
    acc = (acc + _leToBig(chunk)) * r % _poly1305P;
  }
  acc = (acc + s) & ((BigInt.one << 128) - BigInt.one);

  final out = Uint8List(16);
  var v = acc;
  for (var i = 0; i < 16; i++) {
    out[i] = (v & BigInt.from(0xff)).toInt();
    v = v >> 8;
  }
  return out;
}

List<int> _pad16(List<int> x) {
  final rem = x.length % 16;
  return rem == 0 ? x : <int>[...x, ...List<int>.filled(16 - rem, 0)];
}

Uint8List _le64(int v) {
  final out = Uint8List(8);
  for (var i = 0; i < 8; i++) {
    out[i] = (v >>> (i * 8)) & 0xff;
  }
  return out;
}

/// Распечатывание AEAD. Возвращает null, если тег Poly1305 не сошёлся; это
/// E_SEAL_OPEN и событие безопасности.
Uint8List? chacha20Poly1305Open(
  List<int> key,
  List<int> nonce,
  List<int> aad,
  List<int> ciphertextWithTag,
) {
  if (key.length != 32 || nonce.length != 12 || ciphertextWithTag.length < 16) {
    return null;
  }
  final k = Uint8List.fromList(key);
  final n = Uint8List.fromList(nonce);
  final ct = ciphertextWithTag.sublist(0, ciphertextWithTag.length - 16);
  final tag = ciphertextWithTag.sublist(ciphertextWithTag.length - 16);

  final polyKey = _chacha20(k, n, 0, Uint8List(32));
  final mac = _poly1305(polyKey, <int>[
    ..._pad16(aad),
    ..._pad16(ct),
    ..._le64(aad.length),
    ..._le64(ct.length),
  ]);

  var diff = 0;
  for (var i = 0; i < 16; i++) {
    diff |= mac[i] ^ tag[i];
  }
  if (diff != 0) {
    return null;
  }
  return _chacha20(k, n, 1, ct);
}

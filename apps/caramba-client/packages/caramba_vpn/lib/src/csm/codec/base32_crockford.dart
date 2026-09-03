import 'dart:typed_data';

// base32 Crockford, 03-WIRE.md 4.1. Алфавит выбран так, чтобы строку можно
// было продиктовать по телефону и чтобы каждый её символ попадал в
// алфавитно-цифровой режим QR.

const String csmCrockfordAlphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

/// Кодирование: поток бит, старший бит первым, по 5 бит на символ, хвост
/// добивается нулевыми битами. Символов дополнения не бывает никогда.
String base32CrockfordEncode(List<int> data) {
  final sb = StringBuffer();
  var buffer = 0;
  var bits = 0;
  for (final byte in data) {
    buffer = (buffer << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      sb.write(csmCrockfordAlphabet[(buffer >> (bits - 5)) & 0x1f]);
      bits -= 5;
      buffer &= (1 << bits) - 1;
    }
  }
  if (bits > 0) {
    sb.write(csmCrockfordAlphabet[(buffer << (5 - bits)) & 0x1f]);
  }
  return sb.toString();
}

int? _decodeChar(int code) {
  if (code >= 0x30 && code <= 0x39) {
    return code - 0x30;
  }
  var c = code;
  if (c >= 0x61 && c <= 0x7a) {
    c -= 32;
  }
  switch (c) {
    case 0x49: // I
    case 0x4c: // L
      return 1;
    case 0x4f: // O
      return 0;
    case 0x55: // U исключён по построению
      return null;
  }
  final idx = csmCrockfordAlphabet.indexOf(String.fromCharCode(c));
  return idx < 0 ? null : idx;
}

/// Декодирование. Возвращает null на любом символе вне алфавита и на ненулевых
/// хвостовых битах дополнения, чтобы у каждой строки байт была ровно одна
/// принимаемая запись. Дефисы игнорируются в любой позиции.
Uint8List? base32CrockfordDecode(String text) {
  final out = <int>[];
  var buffer = 0;
  var bits = 0;
  for (final rune in text.codeUnits) {
    if (rune == 0x2d) {
      continue;
    }
    final v = _decodeChar(rune);
    if (v == null) {
      return null;
    }
    buffer = (buffer << 5) | v;
    bits += 5;
    if (bits >= 8) {
      out.add((buffer >> (bits - 8)) & 0xff);
      bits -= 8;
      buffer &= (1 << bits) - 1;
    }
  }
  if (bits > 0 && (buffer & ((1 << bits) - 1)) != 0) {
    return null;
  }
  return Uint8List.fromList(out);
}

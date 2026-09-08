import 'dart:convert';
import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/errors.dart';

// Строгий профиль разбора CBOR, 03-WIRE.md 3. Каноничность здесь это
// локальный предикат над входящими байтами, а не перекодирование и сравнение.
// Декодер физически не умеет принять ничего, что профиль запрещает: нет
// неопределённых длин, нет тегов, нет чисел с плавающей точкой, нет
// отрицательных целых, нет null, нет строковых ключей карты.
//
// Ограничения проверяются по мере чтения заголовка, ДО выделения памяти,
// чтобы враждебный документ не мог заставить нас выделить буфер по его слову.

const int csmMaxDepth = 6;
const int csmMaxMapPairs = 64;
const int csmMaxArrayItems = 512;
const int csmMaxTstrBytes = 256;
const int csmMaxBstrBytes = 3072;
const int csmMaxUint = 9007199254740991; // 2^53 - 1

const int csmCriticalKeyMax = 63;
const int csmNonCriticalKeyMax = 1023;

/// Разобранное значение CBOR. Замкнутый набор: профиль допускает ровно эти
/// пять форм и никаких других.
sealed class CborValue {
  const CborValue();
}

class CborUint extends CborValue {
  const CborUint(this.value);
  final int value;
}

class CborBool extends CborValue {
  const CborBool(this.value);
  final bool value;
}

class CborBytes extends CborValue {
  const CborBytes(this.value);
  final Uint8List value;
}

class CborText extends CborValue {
  const CborText(this.value);
  final String value;
}

class CborArray extends CborValue {
  const CborArray(this.items);
  final List<CborValue> items;
}

class CborMap extends CborValue {
  const CborMap(this.entries);

  /// Пары в том порядке, в каком они пришли; порядок строго возрастающий по
  /// правилу C10, поэтому итерация ключей и есть отсортированный обход.
  final Map<int, CborValue> entries;

  CborValue? operator [](int key) => entries[key];
  bool has(int key) => entries.containsKey(key);
}

class _Reader {
  _Reader(this.bytes);

  final Uint8List bytes;
  int offset = 0;

  int get remaining => bytes.length - offset;

  Never fail(String detail) => csmFail(CsmErrorCode.parseCbor, 'P9', detail);

  int byte() {
    if (remaining < 1) {
      fail('payload ended inside a head');
    }
    return bytes[offset++];
  }

  /// Читает аргумент заголовка в кратчайшей форме (правило C4) и возвращает
  /// его значение. Неопределённая длина и значения 28..30 отвергаются здесь.
  int argument(int ai) {
    if (ai < 24) {
      return ai;
    }
    switch (ai) {
      case 24:
        final v = byte();
        if (v < 24) {
          fail('non-minimal one-byte argument for value $v');
        }
        return v;
      case 25:
        if (remaining < 2) {
          fail('payload ended inside a two-byte argument');
        }
        final v = (bytes[offset] << 8) | bytes[offset + 1];
        offset += 2;
        if (v < 256) {
          fail('non-minimal two-byte argument for value $v');
        }
        return v;
      case 26:
        if (remaining < 4) {
          fail('payload ended inside a four-byte argument');
        }
        final v = (bytes[offset] << 24) |
            (bytes[offset + 1] << 16) |
            (bytes[offset + 2] << 8) |
            bytes[offset + 3];
        offset += 4;
        if (v < 65536) {
          fail('non-minimal four-byte argument for value $v');
        }
        return v;
      case 27:
        if (remaining < 8) {
          fail('payload ended inside an eight-byte argument');
        }
        final hi = (bytes[offset] << 24) |
            (bytes[offset + 1] << 16) |
            (bytes[offset + 2] << 8) |
            bytes[offset + 3];
        final lo = (bytes[offset + 4] << 24) |
            (bytes[offset + 5] << 16) |
            (bytes[offset + 6] << 8) |
            bytes[offset + 7];
        offset += 8;
        if (hi == 0) {
          fail('non-minimal eight-byte argument');
        }
        // MAX_UINT это 2^53 - 1 именно для того, чтобы декодер на Dart или на
        // JavaScript никогда не терял точность молча.
        if (hi > 0x1fffff) {
          fail('unsigned integer above MAX_UINT');
        }
        return hi * 4294967296 + lo;
      case 31:
        fail('indefinite length is forbidden by rule C3');
      default:
        fail('reserved additional information $ai');
    }
  }

  Uint8List take(int n) {
    if (remaining < n) {
      fail('payload ended inside a string of $n bytes');
    }
    final out = Uint8List.sublistView(bytes, offset, offset + n);
    offset += n;
    return Uint8List.fromList(out);
  }
}

CborValue _value(_Reader r, int depth) {
  final ib = r.byte();
  final major = ib >> 5;
  final ai = ib & 0x1f;

  switch (major) {
    case 0:
      final v = r.argument(ai);
      if (v > csmMaxUint) {
        r.fail('unsigned integer above MAX_UINT');
      }
      return CborUint(v);
    case 1:
      r.fail('negative integers are forbidden by rule C8');
    case 2:
      final n = r.argument(ai);
      if (n > csmMaxBstrBytes) {
        r.fail('byte string of $n bytes exceeds MAX_BSTR_BYTES');
      }
      return CborBytes(r.take(n));
    case 3:
      final n = r.argument(ai);
      if (n > csmMaxTstrBytes) {
        r.fail('text string of $n bytes exceeds MAX_TSTR_BYTES');
      }
      final raw = r.take(n);
      try {
        return CborText(const Utf8Decoder().convert(raw));
      } on FormatException {
        r.fail('text string is not well-formed UTF-8');
      }
    case 4:
      if (depth > csmMaxDepth) {
        r.fail('nesting depth exceeds MAX_DEPTH');
      }
      final n = r.argument(ai);
      if (n > csmMaxArrayItems) {
        r.fail('array of $n items exceeds MAX_ARRAY_ITEMS');
      }
      final items = <CborValue>[];
      for (var i = 0; i < n; i++) {
        items.add(_value(r, depth + 1));
      }
      return CborArray(items);
    case 5:
      if (depth > csmMaxDepth) {
        r.fail('nesting depth exceeds MAX_DEPTH');
      }
      final n = r.argument(ai);
      if (n > csmMaxMapPairs) {
        r.fail('map of $n pairs exceeds MAX_MAP_PAIRS');
      }
      final entries = <int, CborValue>{};
      var previous = -1;
      for (var i = 0; i < n; i++) {
        final kb = r.byte();
        if ((kb >> 5) != 0) {
          r.fail('map key is not an unsigned integer, rule C9');
        }
        final key = r.argument(kb & 0x1f);
        // Правило C10 поглощает обнаружение дубликата в проверку порядка: одно
        // сравнение на ключ, и его нельзя забыть отдельно.
        if (key <= previous) {
          r.fail('map keys are not strictly ascending, rule C10');
        }
        previous = key;
        if (key == 0) {
          r.fail('map key 0 is rejected, 03-WIRE.md 3.3');
        }
        if (key > csmNonCriticalKeyMax) {
          r.fail('map key $key is at or above 1024, 03-WIRE.md 3.3');
        }
        entries[key] = _value(r, depth + 1);
      }
      return CborMap(entries);
    case 6:
      r.fail('tags are forbidden by rule C5');
    default:
      switch (ib) {
        case 0xf4:
          return const CborBool(false);
        case 0xf5:
          return const CborBool(true);
        case 0xf9:
        case 0xfa:
        case 0xfb:
          r.fail('floats are forbidden by rule C6');
        default:
          r.fail('simple value ${ib & 0x1f} is forbidden by rule C7');
      }
  }
}

/// Разбирает полезную нагрузку кадра как ровно один элемент CBOR верхнего
/// уровня, который обязан быть картой, и обязан израсходовать ровно
/// `payload_len` байт (правила C1 и C2).
CborMap csmDecodePayload(Uint8List payload) {
  final r = _Reader(payload);
  final v = _value(r, 1);
  if (r.remaining != 0) {
    csmFail(
      CsmErrorCode.parseCbor,
      'P9',
      'payload carries ${r.remaining} trailing bytes, rule C2',
    );
  }
  if (v is! CborMap) {
    csmFail(
      CsmErrorCode.parseCbor,
      'P9',
      'top-level item is not a map, rule C1',
    );
  }
  return v;
}

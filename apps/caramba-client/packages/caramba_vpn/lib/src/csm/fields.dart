import 'dart:convert';
import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/codec/cbor.dart';
import 'package:caramba_vpn/src/csm/errors.dart';

// Помощники шага P11: типы, границы и замкнутые словари полей из 03-WIRE.md 8,
// плюс ограничения на имена хостов и пути из 03-WIRE.md 14.
//
// Всё, что здесь падает, падает с E_PARSE_FIELD: это отказ разбора, решаемый
// целиком по входящим байтам и по типу документа, без ключевого материала.

Never fieldFail(String detail) =>
    csmFail(CsmErrorCode.parseField, 'P11', detail);

Never envelopeFail(String detail) =>
    csmFail(CsmErrorCode.parseEnvelope, 'P10', detail);

/// Диапазон идентификатора тарифа, 03-WIRE.md 8.1.
///
/// Верхняя граница 1023, а не 65535, потому что идентификатор тарифа служит
/// КЛЮЧОМ карты CBOR в поле tiers, а правило 3.3 отвергает ключ 0 и любой ключ
/// от 1024. Пока в таблице стояло < 2^16, соответствующая панель могла подписать
/// ключевой документ, который ни один соответствующий верификатор не
/// декодирует. Потолок когорты равен 16, так что 1023 недостижимо на практике.
const int csmTierMin = 1;
const int csmTierMax = 1023;

int textBytes(String s) => utf8.encode(s).length;

CborValue require(CborMap m, int key, String name) {
  final v = m[key];
  if (v == null) {
    fieldFail('mandatory field $name (key $key) is absent');
  }
  return v;
}

int asUint(CborValue v, String name, {int? min, int? max}) {
  if (v is! CborUint) {
    fieldFail('$name is not an unsigned integer');
  }
  if (min != null && v.value < min) {
    fieldFail('$name is ${v.value}, below the minimum $min');
  }
  if (max != null && v.value > max) {
    fieldFail('$name is ${v.value}, above the maximum $max');
  }
  return v.value;
}

bool asBool(CborValue v, String name) {
  if (v is! CborBool) {
    fieldFail('$name is not a boolean');
  }
  return v.value;
}

Uint8List asBytes(CborValue v, String name, {int? exact, int? min, int? max}) {
  if (v is! CborBytes) {
    fieldFail('$name is not a byte string');
  }
  final n = v.value.length;
  if (exact != null && n != exact) {
    fieldFail('$name is $n bytes, must be exactly $exact');
  }
  if (min != null && n < min) {
    fieldFail('$name is $n bytes, below the minimum $min');
  }
  if (max != null && n > max) {
    fieldFail('$name is $n bytes, above the cap $max');
  }
  return v.value;
}

String asText(CborValue v, String name, {int? exact, int? min, int? max}) {
  if (v is! CborText) {
    fieldFail('$name is not a text string');
  }
  final n = textBytes(v.value);
  if (exact != null && n != exact) {
    fieldFail('$name is $n bytes, must be exactly $exact');
  }
  if (min != null && n < min) {
    fieldFail('$name is $n bytes, below the minimum $min');
  }
  if (max != null && n > max) {
    fieldFail('$name is $n bytes, above the cap $max');
  }
  return v.value;
}

List<CborValue> asArray(CborValue v, String name, {int? min, int? max}) {
  if (v is! CborArray) {
    fieldFail('$name is not an array');
  }
  final n = v.items.length;
  if (min != null && n < min) {
    fieldFail('$name has $n items, below the minimum $min');
  }
  if (max != null && n > max) {
    fieldFail('$name has $n items, above the cap $max');
  }
  return v.items;
}

CborMap asMap(CborValue v, String name, {int? minPairs, int? maxPairs}) {
  if (v is! CborMap) {
    fieldFail('$name is not a map');
  }
  final n = v.entries.length;
  if (minPairs != null && n < minPairs) {
    fieldFail('$name has $n pairs, below the minimum $minPairs');
  }
  if (maxPairs != null && n > maxPairs) {
    fieldFail('$name has $n pairs, above the cap $maxPairs');
  }
  return v;
}

/// Замкнутый словарь: значение вне перечисления это отказ разбора, если поле
/// лежит в критическом диапазоне (03-WIRE.md 5).
void requireEnum(int value, Set<int> allowed, String name) {
  if (!allowed.contains(value)) {
    fieldFail('$name carries $value, outside its closed vocabulary');
  }
}

/// Проверяет, что все критические ключи карты известны для этого типа
/// документа, а некритические (64..1023) молча игнорируются. Это и есть
/// механизм расширения 03-WIRE.md 3.3: критическое поле проваливается
/// закрыто, старый клиент отказывается, а не игнорирует.
void requireKnownCriticalKeys(CborMap m, Set<int> known, String where) {
  for (final key in m.entries.keys) {
    if (key > csmCriticalKeyMax) {
      continue;
    }
    if (!known.contains(key)) {
      fieldFail('$where carries unknown critical key $key');
    }
  }
}

bool _isLowerHostChar(int c) =>
    (c >= 0x61 && c <= 0x7a) || (c >= 0x30 && c <= 0x39) || c == 0x2d;

/// Имя хоста по 03-WIRE.md 14.1. Заглавные буквы отвергаются, а не
/// приводятся к нижнему регистру: две записи одного хоста дали бы два chash
/// для одного каталога.
bool isValidHostname(String h) {
  if (h.isEmpty || h.length > 64) {
    return false;
  }
  for (final c in h.codeUnits) {
    if (c > 0x7f) {
      return false;
    }
  }
  if (h.endsWith('.')) {
    return false;
  }
  for (final label in h.split('.')) {
    if (label.isEmpty || label.length > 63) {
      return false;
    }
    if (label.startsWith('-') || label.endsWith('-')) {
      return false;
    }
    for (final c in label.codeUnits) {
      if (!_isLowerHostChar(c)) {
        return false;
      }
    }
  }
  return true;
}

bool isIpv4Literal(String s) {
  final parts = s.split('.');
  if (parts.length != 4) {
    return false;
  }
  for (final p in parts) {
    if (p.isEmpty || p.length > 3) {
      return false;
    }
    for (final c in p.codeUnits) {
      if (c < 0x30 || c > 0x39) {
        return false;
      }
    }
    if (int.parse(p) > 255) {
      return false;
    }
  }
  return true;
}

bool isIpv6Literal(String s) {
  if (!s.contains(':') || s.length > 45) {
    return false;
  }
  for (final c in s.codeUnits) {
    final ok = (c >= 0x30 && c <= 0x39) ||
        (c >= 0x61 && c <= 0x66) ||
        c == 0x3a ||
        c == 0x2e;
    if (!ok) {
      return false;
    }
  }
  return true;
}

bool isIpLiteral(String s) => isIpv4Literal(s) || isIpv6Literal(s);

const String _pathExtra = "-._~!\$&'()*+,;=/:@?%";

/// Поле-путь по 03-WIRE.md 14.2. Подписанный документ может назвать путь, но
/// он никогда не может назвать хост, которого нет в пуле.
bool isValidPath(String p) {
  if (p.isEmpty || p.length > 128) {
    return false;
  }
  for (final c in p.codeUnits) {
    if (c <= 0x20 || c > 0x7e) {
      return false;
    }
  }
  if (p[0] != '/') {
    return false;
  }
  if (p.length > 1 && p[1] == '/') {
    return false;
  }
  if (p.contains('://') || p.contains(r'\')) {
    return false;
  }
  final lower = p.toLowerCase();
  if (lower.contains('%2f')) {
    return false;
  }
  for (final segment in p.split('/')) {
    if (segment == '..') {
      return false;
    }
  }
  for (var i = 0; i < p.length; i++) {
    final ch = p[i];
    final code = p.codeUnitAt(i);
    final alnum = (code >= 0x41 && code <= 0x5a) ||
        (code >= 0x61 && code <= 0x7a) ||
        (code >= 0x30 && code <= 0x39);
    if (!alnum && !_pathExtra.contains(ch)) {
      return false;
    }
    if (ch == '%') {
      if (i + 2 >= p.length ||
          !_isHexDigit(p.codeUnitAt(i + 1)) ||
          !_isHexDigit(p.codeUnitAt(i + 2))) {
        return false;
      }
    }
  }
  return true;
}

bool _isHexDigit(int c) =>
    (c >= 0x30 && c <= 0x39) ||
    (c >= 0x41 && c <= 0x46) ||
    (c >= 0x61 && c <= 0x66);

/// Origin `https://host[:port]` без пути, запроса и фрагмента, 03-WIRE.md 14.3.
bool isValidOrigin(String origin) {
  const prefix = 'https://';
  if (!origin.startsWith(prefix) || origin.length > 96) {
    return false;
  }
  final rest = origin.substring(prefix.length);
  if (rest.contains('/') || rest.contains('?') || rest.contains('#')) {
    return false;
  }
  final colon = rest.lastIndexOf(':');
  if (colon < 0) {
    return isValidHostname(rest);
  }
  final host = rest.substring(0, colon);
  final port = rest.substring(colon + 1);
  if (port.isEmpty || port.length > 5) {
    return false;
  }
  for (final c in port.codeUnits) {
    if (c < 0x30 || c > 0x39) {
      return false;
    }
  }
  final value = int.parse(port);
  return value >= 1 && value <= 65535 && isValidHostname(host);
}

bool isUpperAlpha2(String s) {
  if (s.length != 2) {
    return false;
  }
  for (final c in s.codeUnits) {
    if (c < 0x41 || c > 0x5a) {
      return false;
    }
  }
  return true;
}

bool matchesCharset(String s, bool Function(int) predicate) {
  for (final c in s.codeUnits) {
    if (!predicate(c)) {
      return false;
    }
  }
  return true;
}

bool isNodeIdChar(int c) =>
    (c >= 0x30 && c <= 0x39) ||
    (c >= 0x41 && c <= 0x5a) ||
    (c >= 0x61 && c <= 0x7a) ||
    c == 0x5f ||
    c == 0x2d;

bool isRouteIdChar(int c) =>
    (c >= 0x30 && c <= 0x39) || (c >= 0x61 && c <= 0x7a) || c == 0x2d;

bool isHexChar(int c) => _isHexDigit(c);

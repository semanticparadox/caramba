import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/crypto/sha2.dart';

// Строгий профиль Ed25519 из 03-WIRE.md 2. Это требование соответствия, а не
// совет, поэтому реализация написана здесь целиком поверх BigInt, а не поверх
// готового пакета: ни один доступный Dart-пакет не делает ни проверку
// каноничности S, ни отбраковку публичного ключа малого порядка, ни
// безкофакторное уравнение проверки, а корпус 05-TEST-VECTORS ловит все три.
// Скорость здесь не важна, важна побайтовая согласованность с Rust и Go.

final BigInt _p = (BigInt.one << 255) - BigInt.from(19);
final BigInt _l = (BigInt.one << 252) +
    BigInt.parse('27742317777372353535851937790883648493');
final BigInt _d = (_p - BigInt.from(121665)) *
        BigInt.from(121666).modInverse(_p) %
    _p;
final BigInt _sqrtM1 = BigInt.two.modPow((_p - BigInt.one) >> 2, _p);
final BigInt _pMinus5Div8 = (_p - BigInt.from(5)) >> 3;

BigInt _mod(BigInt x) {
  final r = x % _p;
  return r.isNegative ? r + _p : r;
}

/// Точка Edwards25519 в расширенных координатах (X : Y : Z : T), T = XY/Z.
class EdPoint {
  const EdPoint(this.x, this.y, this.z, this.t);

  final BigInt x;
  final BigInt y;
  final BigInt z;
  final BigInt t;

  static final EdPoint identity =
      EdPoint(BigInt.zero, BigInt.one, BigInt.one, BigInt.zero);

  bool get isIdentity => _mod(x) == BigInt.zero && _mod(y - z) == BigInt.zero;

  /// Унифицированная формула сложения для a = -1, она же годится для удвоения.
  EdPoint operator +(EdPoint o) {
    final a = _mod((y - x) * (o.y - o.x));
    final b = _mod((y + x) * (o.y + o.x));
    final c = _mod(t * BigInt.two * _d * o.t);
    final dd = _mod(z * BigInt.two * o.z);
    final e = b - a;
    final f = dd - c;
    final g = dd + c;
    final h = b + a;
    return EdPoint(_mod(e * f), _mod(g * h), _mod(f * g), _mod(e * h));
  }

  EdPoint get negated => EdPoint(_mod(-x), y, z, _mod(-t));

  EdPoint doubled() => this + this;

  bool equalsPoint(EdPoint o) =>
      _mod(x * o.z - o.x * z) == BigInt.zero &&
      _mod(y * o.z - o.y * z) == BigInt.zero;

  Uint8List encode() {
    final zi = z.modInverse(_p);
    final ax = _mod(x * zi);
    final ay = _mod(y * zi);
    final out = Uint8List(32);
    var v = ay;
    for (var i = 0; i < 32; i++) {
      out[i] = (v & BigInt.from(0xff)).toInt();
      v = v >> 8;
    }
    if (ax.isOdd) {
      out[31] |= 0x80;
    }
    return out;
  }
}

BigInt _leToBig(List<int> b) {
  var v = BigInt.zero;
  for (var i = b.length - 1; i >= 0; i--) {
    v = (v << 8) | BigInt.from(b[i]);
  }
  return v;
}

/// Результат разбора кодировки точки. [clause] называет пункт 03-WIRE.md 2.1,
/// который отклонил вход, и нужен только для диагностики.
class EdDecodeResult {
  const EdDecodeResult.ok(this.point)
      : clause = 0,
        reason = '';
  const EdDecodeResult.fail(this.clause, this.reason) : point = null;

  final EdPoint? point;
  final int clause;
  final String reason;

  bool get isOk => point != null;
}

/// Разбор 32-байтовой кодировки точки: пункты 1 и 2 профиля 03-WIRE.md 2.1.
/// Пункт 3 (малый порядок) здесь НЕ применяется, он относится только к приёму
/// публичного ключа; для R из подписи 2.2 пункт 2 разрешает малый порядок.
EdDecodeResult ed25519Decode(List<int> encoded) {
  if (encoded.length != 32) {
    return const EdDecodeResult.fail(1, 'encoding is not 32 bytes');
  }
  final raw = Uint8List.fromList(encoded);
  final sign = (raw[31] >> 7) & 1;
  raw[31] &= 0x7f;
  final y = _leToBig(raw);
  // Пункт 1: y строго меньше p, иначе кодировка неканонична.
  if (y >= _p) {
    return const EdDecodeResult.fail(1, 'y >= p, non-canonical encoding');
  }
  // Пункт 2: точка должна лежать на кривой.
  final y2 = _mod(y * y);
  final u = _mod(y2 - BigInt.one);
  final v = _mod(_d * y2 + BigInt.one);
  if (v == BigInt.zero) {
    return const EdDecodeResult.fail(2, 'denominator is zero');
  }
  final v3 = _mod(v * v % _p * v);
  final v7 = _mod(v3 * v3 % _p * v);
  var x = _mod(u * v3 % _p * (_mod(u * v7)).modPow(_pMinus5Div8, _p));
  final check = _mod(v * x % _p * x);
  if (check != _mod(u)) {
    if (check == _mod(-u)) {
      x = _mod(x * _sqrtM1);
    } else {
      return const EdDecodeResult.fail(2, 'point is not on the curve');
    }
  }
  if (x == BigInt.zero && sign == 1) {
    return const EdDecodeResult.fail(2, 'x is zero with the sign bit set');
  }
  if ((x.isOdd ? 1 : 0) != sign) {
    x = _p - x;
  }
  return EdDecodeResult.ok(EdPoint(x, y, BigInt.one, _mod(x * y)));
}

/// Приём публичного ключа: все три пункта 03-WIRE.md 2.1. Выполняется и на
/// шаге P12 (каждый pk внутри ключевого документа), и на шаге V6 (публичный
/// ключ каждого слота подписи), и повторяется на каждом применении, а не
/// кешируется битом.
EdDecodeResult ed25519AcceptPublicKey(List<int> pk) {
  final decoded = ed25519Decode(pk);
  if (!decoded.isOk) {
    return decoded;
  }
  // Пункт 3: [8]A не должно быть нейтральным элементом. Именно три удвоения и
  // проверка на единицу, а не список запрещённых кодировок: предикат точен, а
  // список переписывают с ошибками.
  final a = decoded.point!;
  final a8 = a.doubled().doubled().doubled();
  if (a8.isIdentity) {
    return const EdDecodeResult.fail(3, '[8]A is the identity, small order');
  }
  return decoded;
}

EdPoint _basePoint() {
  final enc = Uint8List(32);
  for (var i = 0; i < 32; i++) {
    enc[i] = 0x66;
  }
  enc[0] = 0x58;
  return ed25519Decode(enc).point!;
}

final EdPoint _b = _basePoint();

List<EdPoint> _windowTable(EdPoint p) {
  final table = <EdPoint>[EdPoint.identity, p];
  for (var i = 2; i < 16; i++) {
    table.add(table[i - 1] + p);
  }
  return table;
}

EdPoint _scalarMul(EdPoint p, BigInt k) {
  if (k == BigInt.zero) {
    return EdPoint.identity;
  }
  final table = _windowTable(p);
  var acc = EdPoint.identity;
  final bits = k.bitLength;
  final start = ((bits + 3) ~/ 4) * 4 - 4;
  for (var shift = start; shift >= 0; shift -= 4) {
    acc = acc.doubled().doubled().doubled().doubled();
    final nibble = ((k >> shift) & BigInt.from(0xf)).toInt();
    if (nibble != 0) {
      acc = acc + table[nibble];
    }
  }
  return acc;
}

/// Проверка подписи по строгому профилю 03-WIRE.md 2.2 над байтами [message]
/// как они пришли. Никакой перекодировки сообщения перед проверкой нет и быть
/// не может: подписан прообраз кадра ровно в том виде, в каком он получен.
bool ed25519VerifyStrict(List<int> publicKey, List<int> message, List<int> signature) {
  if (signature.length != 64) {
    return false;
  }
  // Пункт 1: S каноничен, S < L. Не приведение по модулю, а отказ: приводящий
  // проверяющий превращает одну подпись в несколько, а каталог адресуется по
  // sha256 всего кадра.
  final s = _leToBig(signature.sublist(32, 64));
  if (s >= _l) {
    return false;
  }
  // Публичный ключ проходит все три пункта 2.1.
  final a = ed25519AcceptPublicKey(publicKey);
  if (!a.isOk) {
    return false;
  }
  // Пункт 2: R декодируется по пунктам 1 и 2 профиля приёма ключа.
  final rBytes = signature.sublist(0, 32);
  final r = ed25519Decode(rBytes);
  if (!r.isOk) {
    return false;
  }
  final digest = sha512(<int>[...rBytes, ...publicKey, ...message]);
  final k = _leToBig(digest) % _l;
  // Пункт 3: безкофакторное уравнение [S]B == R + [k]A. Кофакторный вариант
  // принимает подписи, которые Rust и Go отвергнут, и это расхождение между
  // тем, что показывает интерфейс, и тем, куда звонит туннель.
  final lhs = _scalarMul(_b, s);
  final rhs = r.point! + _scalarMul(a.point!, k);
  return lhs.equalsPoint(rhs);
}

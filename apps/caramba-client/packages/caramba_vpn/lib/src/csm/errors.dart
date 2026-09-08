// Реестр кодов отказа CSM/1. 03-WIRE.md 6.6.
//
// Разбор и проверка подписи это два разных исхода с разными кодами: разбор
// решается целиком по входящим байтам, без ключевого материала и без
// сохранённого состояния, проверка требует доверенного ключевого документа,
// закреплённого pid, отметки hwm, часов или ожидаемого nonce.
//
// Все три реализации (Rust, Go, Dart) обязаны возвращать один и тот же код на
// одном и том же фикстуре: согласие в том, что документ отвергнут, но по
// разным причинам, это и есть способ спрятать настоящее расхождение.

/// Код отказа из реестра 03-WIRE.md 6.6.
enum CsmErrorCode {
  parseShort('E_PARSE_SHORT'),
  parseMagic('E_PARSE_MAGIC'),
  parseDocType('E_PARSE_DOCTYPE'),
  parseLen('E_PARSE_LEN'),
  parseNsigs('E_PARSE_NSIGS'),
  parseFraming('E_PARSE_FRAMING'),
  parseSlotOrder('E_PARSE_SLOTORDER'),
  parseCbor('E_PARSE_CBOR'),
  parseEnvelope('E_PARSE_ENVELOPE'),
  parseField('E_PARSE_FIELD'),
  verifyRole('E_VERIFY_ROLE'),
  verifyNoAnchor('E_VERIFY_NOANCHOR'),
  verifyUnauthorized('E_VERIFY_UNAUTHORIZED'),
  verifyRevoked('E_VERIFY_REVOKED'),
  verifySig('E_VERIFY_SIG'),
  verifyThreshold('E_VERIFY_THRESHOLD'),
  verifyPid('E_VERIFY_PID'),
  verifyVersion('E_VERIFY_VERSION'),
  verifyRotation('E_VERIFY_ROTATION'),
  verifyIat('E_VERIFY_IAT'),
  verifyExpired('E_VERIFY_EXPIRED'),
  verifyNonce('E_VERIFY_NONCE'),
  verifyDevice('E_VERIFY_DEVICE'),
  verifyCatHash('E_VERIFY_CATHASH'),
  sealRecipient('E_SEAL_RECIPIENT'),
  sealSuite('E_SEAL_SUITE'),
  sealOpen('E_SEAL_OPEN');

  const CsmErrorCode(this.wire);

  /// Идентификатор кода в том виде, в каком он записан в реестре и в корпусе.
  final String wire;

  /// Отказ разбора: байты не являются кадром CSM/1. Правильная реакция это
  /// выбросить их и считать, что ступень транспорта не вернула ничего.
  bool get isParse => wire.startsWith('E_PARSE_');

  /// Отказ проверки: событие безопасности, его нельзя проглатывать молча.
  bool get isVerify => wire.startsWith('E_VERIFY_');

  /// Отказ распечатывания конверта HPKE (03-WIRE.md 9).
  bool get isSeal => wire.startsWith('E_SEAL_');

  static CsmErrorCode? fromWire(String wire) {
    for (final c in CsmErrorCode.values) {
      if (c.wire == wire) {
        return c;
      }
    }
    return null;
  }
}

/// Отказ CSM/1. Несёт код реестра, шаг (P1..P12, V1..V14b, seal step N) и
/// человекочитаемую подробность. Шаг диагностический, нормативен только код.
class CsmError implements Exception {
  const CsmError(this.code, this.step, this.detail);

  final CsmErrorCode code;
  final String step;
  final String detail;

  @override
  String toString() => '${code.wire} at $step: $detail';
}

/// Бросает отказ разбора.
Never csmFail(CsmErrorCode code, String step, String detail) =>
    throw CsmError(code, step, detail);

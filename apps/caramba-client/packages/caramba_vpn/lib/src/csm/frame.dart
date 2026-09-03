import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/errors.dart';

// Кадр CSM/1, 03-WIRE.md 1. Плоская строка байт без вложенности и без внешнего
// префикса длины.
//
//   0       4               magic = "CSM1"
//   4       1               doc_type
//   5       2               payload_len, big-endian, 1..49152
//   7       payload_len     payload
//   7+L     1               nsigs, 1..4
//   8+L     76 * nsigs      slots { keyid_trunc(12) || sig(64) }

const List<int> csmMagic = <int>[0x43, 0x53, 0x4d, 0x31];
const int csmPayloadLenMax = 49152;
const int csmNsigsMax = 4;
const int csmSlotBytes = 76;
const int csmKeyIdBytes = 12;
const int csmSignatureBytes = 64;

/// Типы документов, 03-WIRE.md 1.2. Всё, чего здесь нет, это отказ разбора на
/// шаге P3: проверяющий не может определить, какая роль должна была подписать
/// такой документ, а угадывать это ровно та подмена, от которой существует
/// разделитель домена.
class CsmDocType {
  static const int keyDocument = 0x01;
  static const int catalog = 0x02;
  static const int directive = 0x03;
  static const int catalogChunk = 0x04;
  static const int bootstrapBlob = 0x05;
  static const int sealedDirective = 0x06;
  static const int reservePool = 0x08;

  static const Set<int> defined = <int>{
    keyDocument,
    catalog,
    directive,
    catalogChunk,
    bootstrapBlob,
    sealedDirective,
    reservePool,
  };

  static String name(int docType) {
    switch (docType) {
      case keyDocument:
        return 'k1';
      case catalog:
        return 'c1';
      case directive:
        return 'm1';
      case catalogChunk:
        return 'c1c';
      case bootstrapBlob:
        return 'b1';
      case sealedDirective:
        return 'm1s';
      case reservePool:
        return 'r1';
      default:
        return 'unknown';
    }
  }
}

/// Слот подписи: 12 байт подсказки для поиска ключа и 64 байта подписи.
/// keyid_trunc это подсказка, а не авторизация; авторизация приходит из
/// таблицы ролей 03-WIRE.md 7.1.
class CsmSignatureSlot {
  const CsmSignatureSlot(this.keyIdTrunc, this.signature);

  final Uint8List keyIdTrunc;
  final Uint8List signature;
}

/// Разобранный кадр. Хранит принятые байты и вид на прообраз подписи; ни один
/// путь кода не пересобирает полезную нагрузку перед проверкой.
class CsmFrame {
  const CsmFrame({
    required this.bytes,
    required this.docType,
    required this.payloadLen,
    required this.payload,
    required this.preImage,
    required this.slots,
  });

  /// Полные байты кадра, как они пришли.
  final Uint8List bytes;
  final int docType;
  final int payloadLen;

  /// Полезная нагрузка, вид на [bytes], не копия.
  final Uint8List payload;

  /// Подписанная строка байт: первые 7 + payload_len байт кадра. nsigs и слоты
  /// в неё не входят, и это безопасно только благодаря правилу точной длины.
  final Uint8List preImage;

  final List<CsmSignatureSlot> slots;

  int get totalLen => bytes.length;

  /// Разбор кадра, шаги P1..P8 из 03-WIRE.md 6.1.
  static CsmFrame parse(Uint8List input) {
    // P1
    if (input.length < 8) {
      csmFail(CsmErrorCode.parseShort, 'P1', 'total_len ${input.length} < 8');
    }
    // P2
    for (var i = 0; i < 4; i++) {
      if (input[i] != csmMagic[i]) {
        csmFail(CsmErrorCode.parseMagic, 'P2', 'magic is not CSM1');
      }
    }
    // P3
    final docType = input[4];
    if (!CsmDocType.defined.contains(docType)) {
      csmFail(
        CsmErrorCode.parseDocType,
        'P3',
        'doc_type 0x${docType.toRadixString(16).padLeft(2, '0')} is undefined, '
            'reserved or private',
      );
    }
    // P4
    final payloadLen = (input[5] << 8) | input[6];
    if (payloadLen < 1 || payloadLen > csmPayloadLenMax) {
      csmFail(
        CsmErrorCode.parseLen,
        'P4',
        'payload_len $payloadLen outside 1..$csmPayloadLenMax',
      );
    }
    // P5
    if (input.length < 8 + payloadLen) {
      csmFail(
        CsmErrorCode.parseShort,
        'P5',
        'total_len ${input.length} < 8 + payload_len ${8 + payloadLen}',
      );
    }
    // P6
    final nsigs = input[7 + payloadLen];
    if (nsigs < 1 || nsigs > csmNsigsMax) {
      csmFail(
        CsmErrorCode.parseNsigs,
        'P6',
        'nsigs $nsigs outside 1..$csmNsigsMax',
      );
    }
    // P7, правило точной длины. Хвостовые байты это отказ, а не игнорируемый
    // суффикс, и раздутый nsigs ловится именно здесь, до всякой работы с
    // подписями.
    final expected = 7 + payloadLen + 1 + csmSlotBytes * nsigs;
    if (input.length != expected) {
      csmFail(
        CsmErrorCode.parseFraming,
        'P7',
        'total_len ${input.length} != 7 + $payloadLen + 1 + '
            '$csmSlotBytes * $nsigs = $expected',
      );
    }
    // P8
    final slots = <CsmSignatureSlot>[];
    var slotOffset = 8 + payloadLen;
    Uint8List? previous;
    for (var i = 0; i < nsigs; i++) {
      final kid = Uint8List.fromList(
        Uint8List.sublistView(input, slotOffset, slotOffset + csmKeyIdBytes),
      );
      final sig = Uint8List.fromList(
        Uint8List.sublistView(
          input,
          slotOffset + csmKeyIdBytes,
          slotOffset + csmSlotBytes,
        ),
      );
      if (previous != null && csmCompareBytes(previous, kid) >= 0) {
        csmFail(
          CsmErrorCode.parseSlotOrder,
          'P8',
          'signature slots are not in strictly ascending keyid_trunc order',
        );
      }
      previous = kid;
      slots.add(CsmSignatureSlot(kid, sig));
      slotOffset += csmSlotBytes;
    }

    return CsmFrame(
      bytes: input,
      docType: docType,
      payloadLen: payloadLen,
      payload: Uint8List.sublistView(input, 7, 7 + payloadLen),
      preImage: Uint8List.sublistView(input, 0, 7 + payloadLen),
      slots: slots,
    );
  }
}

/// Сравнение байтовых строк как беззнаковых, лексикографически.
int csmCompareBytes(List<int> a, List<int> b) {
  final n = a.length < b.length ? a.length : b.length;
  for (var i = 0; i < n; i++) {
    if (a[i] != b[i]) {
      return a[i] < b[i] ? -1 : 1;
    }
  }
  return a.length.compareTo(b.length);
}

/// Побайтовое равенство без ранних выходов по содержимому.
bool csmBytesEqual(List<int> a, List<int> b) {
  if (a.length != b.length) {
    return false;
  }
  var diff = 0;
  for (var i = 0; i < a.length; i++) {
    diff |= a[i] ^ b[i];
  }
  return diff == 0;
}

/// Разбивает поток кадров на отдельные кадры, используя payload_len и nsigs
/// каждого. 03-WIRE.md 10.1: оставшиеся байты и обрезанный последний кадр это
/// одинаково E_PARSE_FRAMING.
List<Uint8List> csmSplitFrames(Uint8List stream) {
  final out = <Uint8List>[];
  var offset = 0;
  while (offset < stream.length) {
    if (stream.length - offset < 8) {
      csmFail(
        CsmErrorCode.parseFraming,
        'P7',
        'frame stream truncated at offset $offset',
      );
    }
    final payloadLen = (stream[offset + 5] << 8) | stream[offset + 6];
    if (offset + 7 + payloadLen + 1 > stream.length) {
      csmFail(
        CsmErrorCode.parseFraming,
        'P7',
        'frame stream truncated inside the payload at offset $offset',
      );
    }
    final nsigs = stream[offset + 7 + payloadLen];
    final total = 7 + payloadLen + 1 + csmSlotBytes * nsigs;
    if (offset + total > stream.length) {
      csmFail(
        CsmErrorCode.parseFraming,
        'P7',
        'frame stream truncated inside the signature slots at offset $offset',
      );
    }
    out.add(
      Uint8List.fromList(Uint8List.sublistView(stream, offset, offset + total)),
    );
    offset += total;
  }
  if (out.isEmpty) {
    csmFail(CsmErrorCode.parseFraming, 'P7', 'frame stream is empty');
  }
  return out;
}

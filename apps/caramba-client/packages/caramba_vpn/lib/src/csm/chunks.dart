import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/crypto/sha2.dart';
import 'package:caramba_vpn/src/csm/documents.dart';
import 'package:caramba_vpn/src/csm/errors.dart';
import 'package:caramba_vpn/src/csm/frame.dart';

// Сборка каталога из чанков, 03-WIRE.md 8.4.
//
// Чанк несёт срез полного КАДРА каталога, а не его нагрузки. Каждый чанк
// подписан отдельно, поэтому подменённый чанк ловится до сборки, а собранные
// байты это полный кадр 0x02, который затем проверяется целиком заново. Две
// независимые проверки, один путь кода.
//
// Каждый каталог чанкуется, включая каталог из одного чанка. Ветки по размеру
// не существует, и это стоит того конверта, который она экономит.

class CsmChunkAssembler {
  CsmChunkAssembler(this.cid, this.count, this.totalLength, {this.directiveCn});

  /// Создаёт сборщик по первому увиденному чанку.
  ///
  /// [directiveCn] это cn ДОВЕРЕННОЙ директивы. 03-WIRE.md 8.4 требует
  /// n == directive.cn отдельным правилом: без него расхождение всплывает
  /// косвенно, отказом по индексу, то есть с верным исходом и неверной
  /// причиной.
  factory CsmChunkAssembler.fromFirst(CsmCatalogChunk chunk, {int? directiveCn}) {
    if (directiveCn != null && chunk.count != directiveCn) {
      csmFail(
        CsmErrorCode.parseField,
        'P11',
        'chunk says n ${chunk.count}, the trusted directive said cn '
            '$directiveCn',
      );
    }
    return CsmChunkAssembler(
      chunk.cid,
      chunk.count,
      chunk.totalLength,
      directiveCn: directiveCn,
    );
  }

  final Uint8List cid;
  final int count;
  final int totalLength;

  /// cn доверенной директивы, когда он известен.
  final int? directiveCn;

  final Map<int, Uint8List> _slices = <int, Uint8List>{};

  bool get isComplete => _slices.length == count;

  Set<int> get missing {
    final out = <int>{};
    for (var i = 0; i < count; i++) {
      if (!_slices.containsKey(i)) {
        out.add(i);
      }
    }
    return out;
  }

  /// Все n чанков обязаны нести одинаковые cid, n и tl. Расхождение это
  /// E_PARSE_FIELD.
  void add(CsmCatalogChunk chunk) {
    if (!csmBytesEqual(chunk.cid, cid) ||
        chunk.count != count ||
        chunk.totalLength != totalLength) {
      csmFail(
        CsmErrorCode.parseField,
        'P11',
        'chunk disagrees with the set on cid, n or tl',
      );
    }
    final existing = _slices[chunk.index];
    if (existing != null && !csmBytesEqual(existing, chunk.slice)) {
      csmFail(
        CsmErrorCode.parseField,
        'P11',
        'chunk ${chunk.index} arrived twice with different data',
      );
    }
    _slices[chunk.index] = chunk.slice;
  }

  /// Сборка. Проверяет sha256(собранное)[0..10] == cid и длину, после чего
  /// вызывающий обязан проверить результат как кадр 0x02 с шага P1.
  Uint8List assemble() {
    if (!isComplete) {
      csmFail(
        CsmErrorCode.parseField,
        'P11',
        'catalog is incomplete, missing chunks $missing',
      );
    }
    final out = Uint8List(totalLength);
    var offset = 0;
    for (var i = 0; i < count; i++) {
      final slice = _slices[i]!;
      out.setRange(offset, offset + slice.length, slice);
      offset += slice.length;
    }
    if (offset != totalLength) {
      csmFail(
        CsmErrorCode.parseField,
        'P11',
        'reassembled length $offset does not equal tl $totalLength',
      );
    }
    final digest = sha256(out);
    if (!csmBytesEqual(digest.sublist(0, 10), cid)) {
      csmFail(
        CsmErrorCode.parseField,
        'P11',
        'sha256(reassembled)[0..10] does not equal cid',
      );
    }
    return out;
  }
}

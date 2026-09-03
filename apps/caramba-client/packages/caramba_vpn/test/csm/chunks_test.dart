import 'dart:io';
import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/csm.dart';
import 'package:flutter_test/flutter_test.dart';

// Сборка каталога из чанков и адресация по содержимому. Корпус проверяет
// каждый чанк по отдельности; здесь проверяется то, что происходит ПОСЛЕ:
// собранные байты это полный кадр 0x02, который проверяется целиком заново.

const String _corpusRelative = 'docs/protocol/05-TEST-VECTORS';

Directory _findCorpus() {
  var dir = Directory.current.absolute;
  for (var i = 0; i < 8; i++) {
    final candidate = Directory('${dir.path}/$_corpusRelative');
    if (candidate.existsSync()) {
      return candidate;
    }
    final parent = dir.parent;
    if (parent.path == dir.path) {
      break;
    }
    dir = parent;
  }
  throw StateError('CSM/1 corpus not found above ${Directory.current.path}');
}

void main() {
  final corpus = _findCorpus();
  Uint8List fixture(String relative) =>
      File('${corpus.path}/$relative').readAsBytesSync();

  CsmCatalogChunk chunkOf(String relative) =>
      csmParse(fixture(relative)).document as CsmCatalogChunk;

  test('a one-chunk catalog reassembles to the published minimum catalog', () {
    final chunk = chunkOf('bin/positive/c1c_min_0.bin');
    final assembler = CsmChunkAssembler.fromFirst(chunk)..add(chunk);
    expect(assembler.isComplete, isTrue);

    final reassembled = assembler.assemble();
    expect(reassembled, fixture('bin/positive/c1_min.bin'));
    expect(csmCatalogId(sha256(reassembled)), 'XDE36CGS838HG4W4');

    // Собранные байты проверяются заново как кадр 0x02, с шага P1.
    final parsed = csmParse(reassembled);
    expect(parsed.frame.docType, CsmDocType.catalog);
    expect((parsed.document as CsmCatalog).exits.length, 1);
  });

  test('the two chunks of the typical catalog reassemble in any order', () {
    final zero = chunkOf('bin/positive/c1c_typ_0.bin');
    final one = chunkOf('bin/positive/c1c_typ_1.bin');
    expect(zero.count, 2);
    expect(zero.slice.length, csmChunkPayloadMax);

    final assembler = CsmChunkAssembler.fromFirst(one)
      ..add(one)
      ..add(zero);
    expect(assembler.missing, isEmpty);
    final reassembled = assembler.assemble();
    expect(reassembled, fixture('bin/positive/c1_typical.bin'));
    expect(sha256(reassembled).sublist(0, 10), zero.cid);
  });

  test('an incomplete set names the missing ordinals rather than joining', () {
    final zero = chunkOf('bin/positive/c1c_typ_0.bin');
    final assembler = CsmChunkAssembler.fromFirst(zero)..add(zero);
    expect(assembler.isComplete, isFalse);
    expect(assembler.missing, <int>{1});
    expect(
      () => assembler.assemble(),
      throwsA(
        isA<CsmError>().having(
          (e) => e.code,
          'code',
          CsmErrorCode.parseField,
        ),
      ),
    );
  });

  test('a chunk from another catalog is refused by the set', () {
    final typ = chunkOf('bin/positive/c1c_typ_0.bin');
    final min = chunkOf('bin/positive/c1c_min_0.bin');
    final assembler = CsmChunkAssembler.fromFirst(typ)..add(typ);
    expect(
      () => assembler.add(min),
      throwsA(
        isA<CsmError>().having(
          (e) => e.code,
          'code',
          CsmErrorCode.parseField,
        ),
      ),
    );
  });

  test('the maximum catalog reassembles from all eighteen chunks', () {
    final chunks = <CsmCatalogChunk>[
      for (var i = 0; i < 18; i++) chunkOf('bin/positive/c1c_max_$i.bin'),
    ];
    final assembler = CsmChunkAssembler.fromFirst(chunks.first);
    for (final c in chunks) {
      assembler.add(c);
    }
    final reassembled = assembler.assemble();
    expect(reassembled, fixture('bin/positive/c1_max.bin'));
    final catalog = csmParse(reassembled).document as CsmCatalog;
    expect(catalog.exits.length, 448);
    expect(catalog.tier, 2);
  });
}

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/csm.dart';
import 'package:flutter_test/flutter_test.dart';

// Шлюз слияния. Корпус 05-TEST-VECTORS порождён независимой реализацией и
// читается отсюда с диска по относительному пути: один корпус обслуживает три
// реализации, и копировать фикстуры в пакет нельзя.
//
// Пропускать вектор, который ещё не реализован, запрещено: пропуск это то, как
// шлюз перестаёт быть шлюзом.

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
  throw StateError(
    'CSM/1 corpus not found: expected $_corpusRelative at or above '
    '${Directory.current.path}',
  );
}

Uint8List _hexBytes(String hex) {
  final out = Uint8List(hex.length ~/ 2);
  for (var i = 0; i < out.length; i++) {
    out[i] = int.parse(hex.substring(i * 2, i * 2 + 2), radix: 16);
  }
  return out;
}

/// Хранилище отметок корпуса. README 5: если вектор не сказал иного, каждая
/// фикстура проверяется в свежей области, поэтому отметка здесь зависит только
/// от типа документа.
class _CorpusStore implements CsmHighWaterStore {
  _CorpusStore(this.marks, this.frames);

  final Map<int, int> marks;
  final Map<int, Uint8List> frames;

  @override
  int mark(int docType, String scope) => marks[docType] ?? 0;

  @override
  Uint8List? storedFrame(int docType, String scope) => frames[docType];

  @override
  void advance(int docType, String scope, int version, Uint8List frame) {}
}

void main() {
  final corpus = _findCorpus();
  final vectors = jsonDecode(
    File('${corpus.path}/vectors.json').readAsStringSync(),
  ) as Map<String, dynamic>;

  Uint8List readFixture(String relative) =>
      File('${corpus.path}/$relative').readAsBytesSync();

  final contexts = vectors['contexts'] as Map<String, dynamic>;

  CsmTrustState buildState(String contextName, Map<String, dynamic>? override) {
    final ctx = contexts[contextName] as Map<String, dynamic>;

    CsmKeyDocument? anchor;
    final anchorFile = ctx['anchor'] as String?;
    if (anchorFile != null) {
      final parsed = csmParse(readFixture(anchorFile));
      anchor = parsed.document as CsmKeyDocument;
    }

    final marks = <int, int>{};
    (ctx['hwm'] as Map<String, dynamic>).forEach((k, v) {
      marks[int.parse(k)] = v as int;
    });
    final frames = <int, Uint8List>{};
    // cat и tier доверенной директивы приходят ПОЛЯМИ контекста: шаги V14a и
    // V14b читают их из ранее доверенного документа, и харнесс обязан
    // подставлять их так же явно, а не выковыривать из текста заметки.
    var boundCat = ctx['bound_cat'] as String?;
    var boundTier = ctx['bound_tier'] as int?;

    if (override != null) {
      final hwm = override['hwm'] as Map<String, dynamic>?;
      hwm?.forEach((k, v) {
        marks[int.parse(k)] = v as int;
      });
      final stored = override['stored_frame'] as String?;
      if (stored != null) {
        final bytes = readFixture(stored);
        final docType = bytes[4];
        frames[docType] = bytes;
      }
      boundCat = (override['bound_cat'] as String?) ?? boundCat;
      boundTier = (override['bound_tier'] as int?) ?? boundTier;
    }

    return CsmTrustState(
      pinnedPid: _hexBytes(ctx['pinned_pid'] as String),
      linkPin: ctx['link_pin'] as String?,
      trustedKeyDocument: anchor,
      store: _CorpusStore(marks, frames),
      now: ctx['now'] as int,
      timeFloor: ctx['time_floor'] as int,
      expectedNonce: _hexBytes(ctx['expected_nonce'] as String),
      deviceThumbprint: _hexBytes(ctx['device_dtp'] as String),
      boundCatalogHash: boundCat == null ? null : sha256(readFixture(boundCat)),
      boundTier: boundTier,
      agreementPrivateKeys: <int, Uint8List>{
        1: _hexBytes(ctx['device_agreement_sk'] as String),
      },
    );
  }

  group('CSM/1 frame vectors', () {
    final list = (vectors['vectors'] as List<dynamic>).cast<Map<String, dynamic>>();

    test('the corpus index is the expected size', () {
      expect(list.length, 143);
    });

    for (final vector in list) {
      final id = vector['id'] as String;
      test(id, () {
        final bytes = readFixture(vector['file'] as String);
        expect(bytes.length, vector['bytes'] as int, reason: '$id byte length');
        expect(
          csmHex(sha256(bytes)),
          vector['sha256'] as String,
          reason: '$id sha256',
        );

        final state = buildState(
          vector['context'] as String,
          vector['context_override'] as Map<String, dynamic>?,
        );
        final verifier = CsmVerifier(state);

        if (vector['verdict'] == 'accept') {
          try {
            verifier.verify(bytes);
          } on CsmError catch (e) {
            fail('$id expected accept, got ${e.code.wire} at ${e.step}: '
                '${e.detail}');
          }
        } else {
          CsmError? caught;
          try {
            verifier.verify(bytes);
          } on CsmError catch (e) {
            caught = e;
          }
          expect(
            caught,
            isNotNull,
            reason: '$id expected reject ${vector['code']}, got accept',
          );
          expect(
            caught!.code.wire,
            vector['code'] as String,
            reason: '$id expected at step ${vector['step']}, this '
                'implementation failed at ${caught.step}: ${caught.detail}',
          );
        }
      });
    }
  });

  group('Ed25519 public key ingest', () {
    final list = (vectors['ed25519_public_key_ingest'] as List<dynamic>)
        .cast<Map<String, dynamic>>();

    test('the section is the expected size', () => expect(list.length, 17));

    for (final entry in list) {
      test(entry['id'] as String, () {
        final pk = _hexBytes(entry['public_key'] as String);
        final result = ed25519AcceptPublicKey(pk);
        expect(
          result.isOk,
          entry['verdict'] == 'accept',
          reason: '${entry['id']}: ${entry['clause'] ?? 'accepted'}',
        );
      });
    }
  });

  group('Ed25519 signature', () {
    final list = (vectors['ed25519_signature'] as List<dynamic>)
        .cast<Map<String, dynamic>>();

    test('the section is the expected size', () => expect(list.length, 7));

    for (final entry in list) {
      test(entry['id'] as String, () {
        final ok = ed25519VerifyStrict(
          _hexBytes(entry['public_key'] as String),
          _hexBytes(entry['message_hex'] as String),
          _hexBytes(entry['signature'] as String),
        );
        expect(
          ok,
          entry['verdict'] == 'accept',
          reason: '${entry['id']}: ${entry['clause'] ?? 'accepted'}',
        );
      });
    }
  });

  group('armored form', () {
    final list =
        (vectors['armor'] as List<dynamic>).cast<Map<String, dynamic>>();

    test('the section is the expected size', () => expect(list.length, 11));

    for (final entry in list) {
      test(entry['id'] as String, () {
        final text = File('${corpus.path}/${entry['file']}').readAsStringSync();
        if (entry['verdict'] == 'accept') {
          final stream = csmArmorDecode(text.split('\n'));
          expect(stream.length, entry['stream_bytes'] as int);
          expect(csmBundleId(stream), entry['bid'] as String);
          final frames = csmSplitFrames(stream);
          expect(frames.length, entry['frames'] as int);
          // Каждый несомый кадр обязан разбираться как кадр CSM/1.
          for (final f in frames) {
            csmParse(f);
          }
        } else {
          CsmError? caught;
          try {
            final stream = csmArmorDecode(text.split('\n'));
            csmSplitFrames(stream);
          } on CsmError catch (e) {
            caught = e;
          }
          expect(caught, isNotNull, reason: '${entry['id']} expected reject');
          expect(caught!.code.wire, entry['code'] as String);
        }
      });
    }
  });

  group('derived identifiers', () {
    final keys = vectors['fixture_keys'] as Map<String, dynamic>;
    final rootPk = _hexBytes(keys['root_public'] as String);

    test('pid', () {
      expect(csmHex(csmPid(rootPk)), keys['pid'] as String);
    });

    test('kid_root', () {
      expect(csmHex(csmKeyId(rootPk)), keys['root_kid'] as String);
    });

    test('kid_online', () {
      expect(
        csmHex(csmKeyId(_hexBytes(keys['online_public'] as String))),
        keys['online_kid'] as String,
      );
    });

    test('link_pin', () {
      expect(csmLinkPin(rootPk), keys['link_pin'] as String);
    });

    test('loc', () {
      expect(
        csmLocator(
          _hexBytes(keys['loc_hmac_secret'] as String),
          keys['subscription_uuid_for_loc'] as String,
          1,
        ),
        keys['loc'] as String,
      );
    });

    test('dtp', () {
      expect(
        csmHex(csmDeviceThumbprint(utf8.encode('csm1-doc-example-device-spki'))),
        keys['dtp'] as String,
      );
    });

    test('crockford round trip', () {
      final input = Uint8List.fromList(List<int>.generate(16, (i) => i));
      const expected = '000G40R40M30E209185GR38E1W';
      expect(base32CrockfordEncode(input), expected);
      expect(base32CrockfordDecode(expected), input);
    });

    test('cat_id of the minimum catalog', () {
      final frame = readFixture('bin/positive/c1_min.bin');
      expect(csmCatalogId(sha256(frame)), 'XDE36CGS838HG4W4');
    });
  });

  group('HPKE', () {
    final hpke = vectors['hpke'] as Map<String, dynamic>;

    test('RFC 9180 A.5 key schedule re-derives', () {
      final v = hpke['rfc9180_key_schedule_vector'] as Map<String, dynamic>;
      final shared = csmDhkemDecap(
        _hexBytes(v['skRm'] as String),
        _hexBytes(v['enc'] as String),
      );
      expect(shared, isNotNull);
      expect(csmHex(shared!), v['shared_secret'] as String);

      final ctx = csmHpkeKeySchedule(shared, _hexBytes(v['info'] as String));
      expect(
        csmHex(ctx.keyScheduleContext),
        v['key_schedule_context'] as String,
      );
      expect(csmHex(ctx.secret), v['secret'] as String);
      expect(csmHex(ctx.key), v['key'] as String);
      expect(csmHex(ctx.baseNonce), v['base_nonce'] as String);
      expect(csmHex(ctx.exporterSecret), v['exporter_secret'] as String);
    });

    test('the fixture aad is recomputed, never taken from the wire', () {
      final keys = vectors['fixture_keys'] as Map<String, dynamic>;
      final aad = csmSealAad(
        _hexBytes(keys['pid'] as String),
        _hexBytes(keys['dtp'] as String),
        412,
      );
      expect(csmHex(aad), hpke['aad_fixture'] as String);
    });
  });

  group('published document checks', () {
    test('the five frame digests of 03-WIRE.md 15 reproduce', () {
      final checks = (vectors['published_digest_check'] as List<dynamic>)
          .cast<Map<String, dynamic>>();
      expect(checks.length, 5);
      for (final c in checks) {
        final bytes = readFixture(c['file'] as String);
        expect(bytes.length, c['bytes'] as int, reason: c['document'] as String);
        expect(
          csmHex(sha256(bytes)),
          c['expected_by_wire_15'] as String,
          reason: c['document'] as String,
        );
        expect(c['match'], isTrue, reason: c['document'] as String);
      }
    });

    test('the armored line of 03-WIRE.md 10.5 reproduces', () {
      final check = vectors['published_armor_check'] as Map<String, dynamic>;
      final frame = readFixture('bin/positive/b1_wire_8_5.bin');
      final lines = csmArmorEncode(frame);
      expect(lines.length, 1);
      expect(lines.first, check['expected'] as String);
      expect(check['match'], isTrue);
    });
  });

  group('primitive self-checks', () {
    test('SHA-2 reproduces the FIPS 180-4 examples', () {
      expect(
        csmHex(sha256(utf8.encode('abc'))),
        'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
      );
      expect(
        csmHex(sha256(<int>[])),
        'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
      );
      expect(
        csmHex(sha512(utf8.encode('abc'))),
        'ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a'
        '2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f',
      );
    });

    test('HMAC-SHA256 reproduces RFC 4231 test case 1', () {
      final key = Uint8List.fromList(List<int>.filled(20, 0x0b));
      expect(
        csmHex(hmacSha256(key, utf8.encode('Hi There'))),
        'b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7',
      );
    });
  });
}

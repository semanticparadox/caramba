import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/authorization.dart';
import 'package:caramba_vpn/src/csm/crypto/ed25519.dart';
import 'package:caramba_vpn/src/csm/crypto/sha2.dart';
import 'package:caramba_vpn/src/csm/documents.dart';
import 'package:caramba_vpn/src/csm/errors.dart';
import 'package:caramba_vpn/src/csm/frame.dart';
import 'package:caramba_vpn/src/csm/ids.dart';
import 'package:caramba_vpn/src/csm/seal.dart';

// Шаги проверки V1..V14b из 03-WIRE.md 6.2 и правило распечатывания 9.4.
//
// Граница разбора и проверки несущая. Отказ разбора решается целиком по байтам,
// отказ проверки требует доверенного ключевого документа, закреплённого pid,
// сохранённой отметки версий, часов или ожидаемого nonce. Всё, что вычислимо
// без секретов, происходит раньше, поэтому враждебный кадр не может завести
// проверяющий в ключевой материал до того, как он окажется корректным.

const int csmSkewSeconds = 300;

/// Состояние доверия профиля: всё, что проверка знает и чего нет в кадре.
class CsmTrustState {
  CsmTrustState({
    required this.pinnedPid,
    required this.store,
    required this.now,
    required this.timeFloor,
    this.clockTrusted = true,
    this.linkPin,
    this.trustedKeyDocument,
    this.expectedNonce,
    this.deviceThumbprint,
    this.boundCatalogHash,
    this.boundTier,
    this.ownLocator,
    this.cachedReplay = false,
    Map<int, Uint8List>? agreementPrivateKeys,
  }) : agreementPrivateKeys =
            agreementPrivateKeys ?? const <int, Uint8List>{};

  /// Закреплённая личность арендатора, sha256(root_pk)[0..8].
  final Uint8List pinnedPid;

  /// link_pin в текстовой форме base32 Crockford, 20 символов. Якорь первого
  /// доверия и единственный якорь для 0x05.
  final String? linkPin;

  /// Ранее доверенный ключевой документ. Просроченный ключевой документ
  /// остаётся действующим якорем авторизации: V12 относится к проверяемому
  /// документу, а не к якорю, читаемому на шаге V3.
  final CsmKeyDocument? trustedKeyDocument;

  final CsmHighWaterStore store;
  final int now;
  final int timeFloor;
  final bool clockTrusted;

  final Uint8List? expectedNonce;
  final Uint8List? deviceThumbprint;

  /// chash каталога, который назвала доверенная директива. Пока директивы нет,
  /// V14a нечего сравнивать и он не выполняется.
  final Uint8List? boundCatalogHash;

  /// tier, который назвала ДОВЕРЕННАЯ директива, для шага V14b. Обязателен
  /// всякий раз, когда каталог проверяется против якоря, публикующего tiers:
  /// подставить сюда tier проверяемого каталога нельзя, иначе строку таблицы
  /// выбирает тот, кого проверяют.
  final int? boundTier;

  /// Локатор ЭТОГО профиля. Область действия отметки версии для 0x03 и 0x08
  /// берётся из него, а не из документа под проверкой: иначе подписант выдумает
  /// свежую область с отметкой 0 и обойдёт V9. Пока локатора нет (первое
  /// доверие), берётся локатор документа.
  final String? ownLocator;

  /// Объявляет, что проверяется УЖЕ ПРИНЯТЫЙ кадр, перечитанный с диска. Снимает
  /// шаг V13 и только его, и действует ТОЛЬКО при побайтовом совпадении с
  /// сохранённым кадром: nonce привязан к конкретному запросу, повторить его на
  /// перезагрузке нечем, а подмена кадра тем же флагом невозможна.
  final bool cachedReplay;

  /// Приватные скаляры P-256 ключа соглашения устройства по поколению rkv.
  final Map<int, Uint8List> agreementPrivateKeys;

  Uint8List? get linkPinKid =>
      linkPin == null ? null : csmLinkPinToKeyId(linkPin!);
}

/// Результат успешной проверки.
class CsmVerified {
  const CsmVerified({
    required this.frame,
    required this.document,
    required this.role,
    required this.validSigners,
    this.inner,
  });

  final CsmFrame frame;
  final CsmDocument document;
  final int role;

  /// Идентификаторы ключей, чьи подписи проверились.
  final List<Uint8List> validSigners;

  /// Для 0x06: восстановленная и проверенная целиком внутренняя директива.
  final CsmVerified? inner;

  Uint8List get frameDigest => sha256(frame.bytes);
}

/// Проверяющий CSM/1.
class CsmVerifier {
  const CsmVerifier(this.state);

  final CsmTrustState state;

  /// Полный проход: P1..P12, затем V1..V14b, затем, для 0x06, шаги
  /// распечатывания 3..7.
  CsmVerified verify(Uint8List frameBytes) {
    final parsed = csmParse(frameBytes);
    return _verifyParsed(parsed, frameBytes);
  }

  CsmVerified _verifyParsed(CsmParsed parsed, Uint8List frameBytes) {
    final frame = parsed.frame;
    final doc = parsed.document;

    // V1: роль определяется по doc_type, а не по содержимому документа.
    final row = csmAuthorizationTable[frame.docType]!;
    final role = row.requiredRole;

    // V2: якорь.
    final trusted = state.trustedKeyDocument;
    final firstTrust = trusted == null;
    if (firstTrust) {
      final anchorable = frame.docType == CsmDocType.keyDocument ||
          frame.docType == CsmDocType.bootstrapBlob;
      if (!anchorable || state.linkPin == null) {
        csmFail(
          CsmErrorCode.verifyNoAnchor,
          'V2',
          'no trusted key document for the pinned pid, and '
              '${CsmDocType.name(frame.docType)} cannot be anchored by link_pin',
        );
      }
    }
    if (row.keySetSource == CsmKeySetSource.linkPin && state.linkPin == null) {
      csmFail(
        CsmErrorCode.verifyNoAnchor,
        'V2',
        'link_pin is required to anchor a bootstrap blob',
      );
    }

    // V3: набор ключей и порог читаются из РАНЕЕ доверенного документа.
    final authorized = <Uint8List>[];
    var threshold = 1;
    final candidates = <CsmKeyEntry>[];

    switch (row.keySetSource) {
      case CsmKeySetSource.linkPin:
        final kid = state.linkPinKid!;
        authorized.add(kid);
        threshold = 1;
        final blob = doc as CsmBootstrapBlob;
        candidates.add(
          CsmKeyEntry(sha256(blob.rootKey).sublist(0, 12), 1, blob.rootKey),
        );
        // rk обязан совпадать с продиктованным пином. Отказ жёсткий, пути
        // "всё равно продолжить" не существует ни на одном пути кода.
        if (!csmBytesEqual(sha256(blob.rootKey).sublist(0, 12), kid)) {
          csmFail(
            CsmErrorCode.verifyUnauthorized,
            'V4',
            'sha256(rk)[0..12] does not equal the dictated link_pin',
          );
        }
      case CsmKeySetSource.trustedDocument:
        final entry = trusted!.roles[role];
        if (entry == null) {
          csmFail(
            CsmErrorCode.verifyRole,
            'V3',
            'the trusted key document publishes no ${CsmRoles.name(role)} role',
          );
        }
        authorized.addAll(entry.ks);
        threshold = entry.thr;
        candidates.addAll(_keysOf(trusted.keys, entry.ks));
      case CsmKeySetSource.trustedDocumentAndSelf:
        final self = doc as CsmKeyDocument;
        final selfRole = self.roles[role];
        if (selfRole == null) {
          csmFail(
            CsmErrorCode.verifyRole,
            'V3',
            'the key document under verification publishes no root role',
          );
        }
        if (firstTrust) {
          // 7.2: первый принятый ключевой документ обязан нести ровно один
          // ключ под ролью root, чей sha256[0..12] совпадает с link_pin.
          final kid = state.linkPinKid!;
          if (selfRole.ks.length != 1 || !csmBytesEqual(selfRole.ks[0], kid)) {
            csmFail(
              CsmErrorCode.verifyUnauthorized,
              'V4',
              'first trust requires exactly one root key matching link_pin',
            );
          }
          authorized.add(kid);
          threshold = 1;
        } else {
          final entry = trusted.roles[role];
          if (entry == null) {
            csmFail(
              CsmErrorCode.verifyRole,
              'V3',
              'the trusted key document publishes no root role',
            );
          }
          authorized.addAll(entry.ks);
          threshold = entry.thr;
          candidates.addAll(_keysOf(trusted.keys, entry.ks));
        }
        // Ротация требует ОБА набора, поэтому ключи проверяемого документа
        // тоже входят в разрешённый набор шага V4.
        authorized.addAll(selfRole.ks);
        candidates.addAll(_keysOf(self.keys, selfRole.ks));
    }

    // V4: каждый слот обязан быть в разрешённом наборе. Неавторизованный слот
    // не пропускается, отвергается весь кадр: кадр с таким слотом это кадр,
    // который кто-то пытался отмыть.
    for (final slot in frame.slots) {
      var found = false;
      for (final kid in authorized) {
        if (csmBytesEqual(kid, slot.keyIdTrunc)) {
          found = true;
          break;
        }
      }
      if (!found) {
        csmFail(
          CsmErrorCode.verifyUnauthorized,
          'V4',
          'slot ${csmHex(slot.keyIdTrunc)} is not in the authorized key set '
              'for role ${CsmRoles.name(role)}',
        );
      }
    }

    // V5: отзыв читается из доверенного документа и действует на всё, что этот
    // ключ подписал, включая уже лежащее на диске.
    if (trusted != null) {
      for (final slot in frame.slots) {
        if (trusted.isRevoked(slot.keyIdTrunc)) {
          csmFail(
            CsmErrorCode.verifyRevoked,
            'V5',
            'slot ${csmHex(slot.keyIdTrunc)} appears in rev.kids',
          );
        }
      }
    }

    // V6: публичный ключ каждого слота проходит 2.1, каждая подпись проходит
    // 2.2 над прообразом как он получен.
    final validSigners = <Uint8List>[];
    for (final slot in frame.slots) {
      final key = _findKey(candidates, slot.keyIdTrunc);
      if (key == null) {
        csmFail(
          CsmErrorCode.verifySig,
          'V6',
          'no key material for slot ${csmHex(slot.keyIdTrunc)}',
        );
      }
      if (!ed25519VerifyStrict(key.pk, frame.preImage, slot.signature)) {
        csmFail(
          CsmErrorCode.verifySig,
          'V6',
          'signature of slot ${csmHex(slot.keyIdTrunc)} fails the strict '
              'Ed25519 profile',
        );
      }
      validSigners.add(slot.keyIdTrunc);
    }

    // V7: число различных проверившихся подписантов не ниже порога.
    if (validSigners.length < threshold) {
      csmFail(
        CsmErrorCode.verifyThreshold,
        'V7',
        '${validSigners.length} valid signers below the threshold $threshold',
      );
    }

    // V8
    if (!csmBytesEqual(doc.pid, state.pinnedPid)) {
      csmFail(
        CsmErrorCode.verifyPid,
        'V8',
        'payload pid ${csmHex(doc.pid)} is not the pinned pid',
      );
    }

    // V9: правило версий в области (pid, doc_type, scope).
    final scope = _scopeOf(frame, doc);
    final mark = state.store.mark(frame.docType, scope);
    if (doc.ver < mark) {
      csmFail(
        CsmErrorCode.verifyVersion,
        'V9',
        'ver ${doc.ver} is below the stored high-water mark $mark',
      );
    }
    if (doc.ver == mark) {
      final stored = state.store.storedFrame(frame.docType, scope);
      if (stored == null || !csmBytesEqual(stored, frameBytes)) {
        csmFail(
          CsmErrorCode.verifyVersion,
          'V9',
          'ver ${doc.ver} equals the high-water mark with different bytes',
        );
      }
    }

    // V10: правило ротации корня, только для 0x01 и только когда версия
    // РАСТЁТ. 03-WIRE.md 6.3 принимает ver == mark при побайтовом совпадении и
    // говорит прямо: это тот же документ, состояние не меняется. Ротации там не
    // происходит, а 7.3 описывает документ с ver = N+1, поэтому применять V10 к
    // перечитанному кадру значило бы отвергать ровно то, что V9 только что
    // принял. Порог при этом не пропадает: на ветке перечитывания он считается
    // здесь, потому что для 0x01 шаг V7 выше идёт по объединённому набору.
    if (frame.docType == CsmDocType.keyDocument && doc.ver > mark) {
      _rotation(frame, doc as CsmKeyDocument, trusted, mark, candidates);
    }

    // V11
    if (state.clockTrusted && doc.iat > state.now + csmSkewSeconds) {
      csmFail(
        CsmErrorCode.verifyIat,
        'V11',
        'iat ${doc.iat} is beyond now + $csmSkewSeconds',
      );
    }
    final lifetime = csmLifetimeMax[frame.docType]!;
    // Нижний порог 03-WIRE.md 6.4 и 02-SPEC.md 5.4 в буквальной нормативной
    // форме. Более ранняя редакция несла двойной срок жизни, чтобы развести
    // neg-verify-iat-below-floor и neg-verify-expired, которые при буквальной
    // форме оба отвергались на V11; вместо этого исправлена сама фикстура
    // neg-verify-expired, её iat поднят внутрь окна, где документ уже просрочен
    // относительно now, но ещё не был мёртв на момент floor. Коды разделяет
    // предикат спецификации, а не выдуманная константа.
    final floorAllowance = lifetime + csmSkewSeconds;
    if (doc.iat + floorAllowance < state.timeFloor) {
      csmFail(
        CsmErrorCode.verifyIat,
        'V11',
        'iat ${doc.iat} + $floorAllowance is below the time floor '
            '${state.timeFloor}; the document had already expired when the '
            'profile last heard from the panel',
      );
    }

    // V12. Просрочка означает отказ принимать НОВЫЕ указания и НОВЫЙ статус, и
    // ничего больше: она не рвёт туннель и не чистит кеш.
    if (state.clockTrusted && state.now > doc.exp + csmSkewSeconds) {
      csmFail(
        CsmErrorCode.verifyExpired,
        'V12',
        'now ${state.now} is beyond exp ${doc.exp} + $csmSkewSeconds',
      );
    }

    // V13. Шаг безусловен: у директивы нет ветки "nonce не выдавали". Снять
    // его может только явное объявление перечитывания уже принятого кадра, и
    // только при побайтовом совпадении с сохранённым.
    final storedForScope = state.store.storedFrame(frame.docType, scope);
    final cachedReplay = state.cachedReplay &&
        storedForScope != null &&
        csmBytesEqual(storedForScope, frameBytes);
    if (doc is CsmDirective && !cachedReplay) {
      final expected = state.expectedNonce;
      if (expected == null || !csmBytesEqual(doc.nonce, expected)) {
        csmFail(
          CsmErrorCode.verifyNonce,
          'V13',
          'the echoed nonce is not the one this device sent',
        );
      }
      final dtp = state.deviceThumbprint;
      if (dtp == null || !csmBytesEqual(doc.dtp, dtp)) {
        csmFail(
          CsmErrorCode.verifyDevice,
          'V13',
          'dtp names another device',
        );
      }
    }

    // V14a и V14b
    if (doc is CsmCatalog) {
      final digest = sha256(frame.bytes);
      final bound = state.boundCatalogHash;
      if (bound != null && !csmBytesEqual(digest, bound)) {
        csmFail(
          CsmErrorCode.verifyCatHash,
          'V14a',
          'sha256(frame) is not the cat the trusted directive named',
        );
      }
      // V14b работает только когда доверенный ключевой документ опубликовал
      // хеш для тарифа ДИРЕКТИВЫ. Прочитать тариф из проверяемого каталога
      // значило бы отдать выбор строки tiers тому, кого проверяют:
      // скомпрометированный онлайн-ключ подписал бы каталог с тарифом, для
      // которого корень ничего не публиковал, шаг тихо не выполнился бы, и
      // "V14b is what stops a compromised online key inventing a fleet"
      // перестало бы что-либо значить.
      final tiers = trusted?.tiers;
      if (tiers != null && tiers.isNotEmpty) {
        final bound = state.boundTier;
        if (bound == null) {
          csmFail(
            CsmErrorCode.verifyCatHash,
            'V14b',
            'the trusted key document publishes tiers but no directive tier '
                'was bound for this catalog',
          );
        }
        final tierHash = tiers[bound];
        if (tierHash != null && !csmBytesEqual(digest, tierHash)) {
          csmFail(
            CsmErrorCode.verifyCatHash,
            'V14b',
            'sha256(frame) is not the root-signed tiers[$bound] entry',
          );
        }
      }
    }

    CsmVerified? inner;
    if (doc is CsmSealedDirective) {
      inner = _openSeal(doc);
    }

    return CsmVerified(
      frame: frame,
      document: doc,
      role: role,
      validSigners: validSigners,
      inner: inner,
    );
  }

  /// Оставляет только тот ключевой материал, чей kid назван набором РОЛИ.
  ///
  /// Без этого список кандидатов набирался бы из keys документа целиком, и
  /// _findKey структурно возвращал бы ключ без его роли. Сегодня это не
  /// эксплуатируемо, потому что V4 отвергает слот вне authorized раньше, но
  /// правило "ни один путь кода не возвращает ключ без роли" должно держаться
  /// строением, а не порядком шагов.
  static List<CsmKeyEntry> _keysOf(
    List<CsmKeyEntry> keys,
    List<Uint8List> ks,
  ) {
    final out = <CsmKeyEntry>[];
    for (final k in keys) {
      for (final kid in ks) {
        if (csmBytesEqual(k.kid, kid)) {
          out.add(k);
          break;
        }
      }
    }
    return out;
  }

  CsmKeyEntry? _findKey(List<CsmKeyEntry> candidates, Uint8List kid) {
    for (final k in candidates) {
      if (csmBytesEqual(k.kid, kid)) {
        return k;
      }
    }
    return null;
  }

  String _scopeOf(CsmFrame frame, CsmDocument doc) {
    if (doc is CsmDirective) {
      // Локатор берётся из СОБСТВЕННОГО состояния профиля. Прочитать его из
      // кадра перед собой значит позволить подписанту выдумать свежую область
      // с отметкой 0 и шагнуть мимо V9; на первом доверии своего локатора ещё
      // нет, и только там берётся локатор документа.
      return csmScopeFor(
        frame.docType,
        locator: state.ownLocator ?? doc.locator,
      );
    }
    if (doc is CsmCatalog) {
      return csmScopeFor(
        frame.docType,
        catId: csmCatalogId(sha256(frame.bytes)),
      );
    }
    if (doc is CsmCatalogChunk) {
      return csmScopeFor(frame.docType, catId: csmCatalogIdFromCid(doc.cid));
    }
    return csmScopeFor(frame.docType);
  }

  void _rotation(
    CsmFrame frame,
    CsmKeyDocument doc,
    CsmKeyDocument? trusted,
    int mark,
    List<CsmKeyEntry> candidates,
  ) {
    // N это СОХРАНЁННАЯ отметка версии, а не ver доверенного документа в
    // памяти: 7.3 говорит про ver = N+1, где N это то, что персистентно и
    // монотонно (6.3), и подставлять сюда что-то другое значит менять
    // границу отката на значение, которого на диске нет.
    final n = mark;
    if (doc.ver != n + 1) {
      csmFail(
        CsmErrorCode.verifyRotation,
        'V10',
        'key document at ver ${doc.ver} against a trusted version $n; a client '
            'must refuse to skip a version',
      );
    }
    // Один и тот же прообраз проверяется дважды: против roles[1] текущего
    // доверенного документа и против roles[1] документа под проверкой. Оба
    // прохода обязаны пройти, иначе любой, кто может отдать байты, поставит
    // свой корень.
    final trustedRole = trusted?.roles[CsmRoles.root];
    final List<Uint8List> trustedKs;
    final int trustedThr;
    if (trustedRole != null) {
      trustedKs = trustedRole.ks;
      trustedThr = trustedRole.thr;
    } else {
      trustedKs = <Uint8List>[state.linkPinKid!];
      trustedThr = 1;
    }
    final selfRole = doc.roles[CsmRoles.root]!;

    if (_countValid(frame, candidates, trustedKs) < trustedThr) {
      csmFail(
        CsmErrorCode.verifyRotation,
        'V10',
        'the rotation does not satisfy the key set of the currently trusted '
            'key document',
      );
    }
    if (_countValid(frame, candidates, selfRole.ks) < selfRole.thr) {
      csmFail(
        CsmErrorCode.verifyRotation,
        'V10',
        'the rotation does not satisfy the key set of the document under '
            'verification',
      );
    }
  }

  int _countValid(
    CsmFrame frame,
    List<CsmKeyEntry> candidates,
    List<Uint8List> keySet,
  ) {
    var count = 0;
    for (final slot in frame.slots) {
      var inSet = false;
      for (final kid in keySet) {
        if (csmBytesEqual(kid, slot.keyIdTrunc)) {
          inSet = true;
          break;
        }
      }
      if (!inSet) {
        continue;
      }
      final key = _findKey(candidates, slot.keyIdTrunc);
      if (key == null) {
        continue;
      }
      if (ed25519VerifyStrict(key.pk, frame.preImage, slot.signature)) {
        count++;
      }
    }
    return count;
  }

  /// Шаги 3..7 из 03-WIRE.md 9.4.
  CsmVerified _openSeal(CsmSealedDirective sealed) {
    // Шаг 3. Не событие безопасности: так выглядит зеркало, отдавшее
    // закешированный ответ для другого устройства.
    final dtp = state.deviceThumbprint;
    if (dtp == null || !csmBytesEqual(sealed.dtp, dtp)) {
      csmFail(
        CsmErrorCode.sealRecipient,
        'seal step 3',
        'the sealed directive is addressed to another device thumbprint',
      );
    }
    // Шаг 4.
    if (sealed.kem != csmHpkeKem ||
        sealed.kdf != csmHpkeKdf ||
        sealed.aead != csmHpkeAead) {
      csmFail(
        CsmErrorCode.sealSuite,
        'seal step 4',
        'suite is (${sealed.kem}, ${sealed.kdf}, ${sealed.aead}), required is '
            '($csmHpkeKem, $csmHpkeKdf, $csmHpkeAead)',
      );
    }
    if (sealed.enc[0] != 0x04) {
      csmFail(
        CsmErrorCode.sealSuite,
        'seal step 4',
        'enc is not an uncompressed P-256 point',
      );
    }
    // Шаг 5.
    final scalar = state.agreementPrivateKeys[sealed.recipientKeyGeneration];
    if (scalar == null) {
      csmFail(
        CsmErrorCode.sealRecipient,
        'seal step 5',
        'this device holds no agreement key of generation '
            '${sealed.recipientKeyGeneration}',
      );
    }
    // Шаг 6. aad пересчитывается из полей внешней нагрузки, с провода она не
    // принимается никогда.
    final aad = csmSealAad(sealed.pid, sealed.dtp, sealed.ver);
    final plaintext = csmHpkeOpen(
      recipientScalar: scalar,
      enc: sealed.enc,
      info: const <int>[67, 83, 77, 49, 45, 115, 101, 97, 108, 45, 118, 49],
      aad: aad,
      ciphertext: sealed.ciphertext,
    );
    if (plaintext == null) {
      csmFail(
        CsmErrorCode.sealOpen,
        'seal step 6',
        'HPKE Open failed; the AEAD tag did not verify',
      );
    }
    if (plaintext.length < 5 ||
        !csmBytesEqual(plaintext.sublist(0, 4), csmMagic) ||
        plaintext[4] != CsmDocType.directive) {
      csmFail(
        CsmErrorCode.sealOpen,
        'seal step 6',
        'the recovered plaintext does not begin 43 53 4d 31 03',
      );
    }
    // Шаг 7. Внутренний кадр проверяется целиком, с P1 по V14b. Внешняя
    // проверка не сокращает ни одной внутренней.
    return _verifyParsed(csmParse(plaintext), plaintext);
  }
}

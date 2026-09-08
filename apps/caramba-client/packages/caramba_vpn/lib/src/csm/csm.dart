// CSM/1, Caramba Signed Manifest, версия 1. Проверяющий на Dart.
//
// Точка входа пакета: приложение кодирует против этого файла и не должно
// импортировать внутренние модули напрямую.
//
// Нормативные документы: apps/caramba-client/docs/protocol/03-WIRE.md (байты,
// профили разбора и подписи, реестр кодов), 02-SPEC.md (модель документов,
// автоматы состояний, свежесть), 04-THREAT-MODEL.md и 06-MIGRATION.md.
// Общий корпус apps/caramba-client/docs/protocol/05-TEST-VECTORS это шлюз
// слияния: тест packages/caramba_vpn/test/csm/corpus_test.dart читает его с
// диска по относительному пути и обязан сойтись с каждым ожидаемым вердиктом и
// кодом отказа.
//
// Третьих зависимостей нет ни одной. SHA-2, HMAC, HKDF, Ed25519, P-256,
// ChaCha20-Poly1305 и base32 Crockford написаны здесь, потому что строгий
// профиль 03-WIRE.md 2 требует проверок, которых доступные пакеты Dart не
// делают: каноничности S, отбраковки публичного ключа малого порядка и
// безкофакторного уравнения проверки.

export 'package:caramba_vpn/src/csm/armor.dart';
export 'package:caramba_vpn/src/csm/authorization.dart';
export 'package:caramba_vpn/src/csm/chunks.dart';
export 'package:caramba_vpn/src/csm/codec/base32_crockford.dart';
export 'package:caramba_vpn/src/csm/codec/cbor.dart'
    show
        CborArray,
        CborBool,
        CborBytes,
        CborMap,
        CborText,
        CborUint,
        CborValue,
        csmDecodePayload,
        csmMaxArrayItems,
        csmMaxBstrBytes,
        csmMaxDepth,
        csmMaxMapPairs,
        csmMaxTstrBytes,
        csmMaxUint;
export 'package:caramba_vpn/src/csm/crypto/ed25519.dart'
    show EdDecodeResult, ed25519AcceptPublicKey, ed25519VerifyStrict;
export 'package:caramba_vpn/src/csm/crypto/hkdf.dart'
    show hkdfExpand, hkdfExtract, hmacSha256;
export 'package:caramba_vpn/src/csm/crypto/sha2.dart' show sha256, sha512;
export 'package:caramba_vpn/src/csm/documents.dart';
export 'package:caramba_vpn/src/csm/errors.dart';
export 'package:caramba_vpn/src/csm/frame.dart';
export 'package:caramba_vpn/src/csm/ids.dart';
export 'package:caramba_vpn/src/csm/seal.dart';
export 'package:caramba_vpn/src/csm/verifier.dart';

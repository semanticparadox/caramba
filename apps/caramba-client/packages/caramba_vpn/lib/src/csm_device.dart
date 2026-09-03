/// Ключи устройства CSM/1 на стороне Dart (ABI v3, 02-SPEC.md 12.2).
///
/// Три символа, `CarambaDeviceKeygen`, `CarambaDeviceSign` и
/// `CarambaDeviceAgree`, существуют потому, что 02-SPEC.md 9.4 кладёт ключ
/// подписи устройства в Secure Enclave или StrongBox, а до обоих не
/// дотягивается ни Dart, ни Go: реализация на Go по построению кладёт ключ в
/// файл, то есть в программный уровень. Держатель ключа это платформенный
/// мост, ядро зовёт его через границу, а Dart видит один и тот же результат
/// на всех пяти мостах.
///
/// Ничего секретного через эту границу не проходит. Наружу выходят только
/// открытые ключи, отпечаток и честно названный уровень хранения.
library;

import 'dart:convert';
import 'dart:typed_data';

import 'package:caramba_vpn/src/core_models.dart' show decodeJsonMap;

/// Личность устройства: то, что регистрируется у оператора телом
/// 03-WIRE.md 13.8 и что клиент хранит между запусками.
class CsmDeviceKey {
  const CsmDeviceKey({
    required this.signingSpki,
    required this.agreementPublicKey,
    required this.deviceThumbprint,
    required this.hardwareTier,
    required this.agreementKeyGeneration,
  });

  /// Открытый ключ подписи как DER `SubjectPublicKeyInfo`. Уходит в поле
  /// `spki` обоих тел регистрации.
  final Uint8List signingSpki;

  /// Открытый ключ согласования, ровно 65 байт несжатой точки P-256.
  final Uint8List agreementPublicKey;

  /// `dtp = sha256(spki)[0..16]`, шестнадцатеричной строкой из 32 символов.
  final String deviceThumbprint;

  /// Уровень хранения как его назвало ядро: `1` Secure Enclave, `2` StrongBox
  /// или TEE, `3` программное хранилище.
  ///
  /// Значение НЕ улучшается по дороге. Сборка без аппаратного хранилища
  /// возвращает 3, и экран личности оператора показывает именно это: ложь
  /// здесь стоила бы дороже отсутствия аппаратуры.
  final int hardwareTier;

  /// `rkv`, поколение ключа согласования. Начинается с 1.
  final int agreementKeyGeneration;

  /// Разбирает ответ `CarambaDeviceKeygen`. Бросает [FormatException], когда
  /// ядро вернуло `{"error":...}` или форму, которой нельзя пользоваться:
  /// половина личности хуже её отсутствия.
  factory CsmDeviceKey.fromJson(String source) {
    final map = decodeJsonMap(source);
    final err = map['error'];
    if (err is String && err.isNotEmpty) {
      throw FormatException(err);
    }
    final spki = _b64(map['spki_b64']);
    final agree = _b64(map['agree_pub_b64']);
    final dtp = map['dtp_hex'];
    final tier = (map['tier'] as num?)?.toInt() ?? 0;
    if (spki.isEmpty) {
      throw const FormatException('ядро не вернуло SPKI ключа подписи');
    }
    if (agree.length != 65 || agree[0] != 0x04) {
      throw const FormatException(
        'ключ согласования обязан быть несжатой точкой P-256, 65 байт',
      );
    }
    if (dtp is! String || dtp.length != 32) {
      throw const FormatException('dtp обязан быть 16 байтами в hex');
    }
    if (tier < 1 || tier > 3) {
      throw FormatException('уровень хранения $tier вне 1..3');
    }
    return CsmDeviceKey(
      signingSpki: spki,
      agreementPublicKey: agree,
      deviceThumbprint: dtp,
      hardwareTier: tier,
      agreementKeyGeneration: (map['generation'] as num?)?.toInt() ?? 1,
    );
  }
}

/// Результат ECDH ключом согласования устройства.
class CsmAgreement {
  const CsmAgreement({required this.shared, required this.ownPublicKey});

  /// 32 байта общей координаты X.
  final Uint8List shared;

  /// 65 байт СОБСТВЕННОГО открытого ключа этого поколения. Он входит в
  /// `kem_context` DHKEM и известен только держателю ключа, поэтому приходит
  /// вместе с секретом, а не выводится вызывающим.
  final Uint8List ownPublicKey;

  factory CsmAgreement.fromJson(String source) {
    final map = decodeJsonMap(source);
    final err = map['error'];
    if (err is String && err.isNotEmpty) {
      throw FormatException(err);
    }
    final shared = _b64(map['shared_b64']);
    final own = _b64(map['own_pub_b64']);
    if (shared.length != 32 || own.length != 65) {
      throw const FormatException('согласование вернуло не 32 и 65 байт');
    }
    return CsmAgreement(shared: shared, ownPublicKey: own);
  }
}

/// Разбирает ответ `CarambaDeviceSign`: ровно 64 байта `r || s`.
///
/// Длина проверяется ЗДЕСЬ, а не только в ядре. И `SecKeyCreateSignature`, и
/// `Signature` на Android по умолчанию отдают ASN.1 DER, и подпись такой формы
/// панель отвергнет, а выглядеть это будет как подделка, а не как дефект
/// клиента (03-WIRE.md 13.6).
Uint8List csmDeviceSignatureFromJson(String source) {
  final map = decodeJsonMap(source);
  final err = map['error'];
  if (err is String && err.isNotEmpty) {
    throw FormatException(err);
  }
  final sig = _b64(map['sig_b64']);
  if (sig.length != 64) {
    throw FormatException(
      'подпись устройства ${sig.length} байт, требуется 64 (r || s, не DER)',
    );
  }
  return sig;
}

Uint8List _b64(Object? v) {
  if (v is! String || v.isEmpty) {
    return Uint8List(0);
  }
  try {
    return base64.decode(base64.normalize(v));
  } on FormatException {
    return Uint8List(0);
  }
}

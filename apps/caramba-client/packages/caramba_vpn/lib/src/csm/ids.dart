import 'dart:convert';
import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/codec/base32_crockford.dart';
import 'package:caramba_vpn/src/csm/crypto/hkdf.dart';
import 'package:caramba_vpn/src/csm/crypto/sha2.dart';

// Выводимые идентификаторы, 03-WIRE.md 4. Ни один из них не выдаётся
// последовательностью в базе: последовательность коррелирует арендаторов, а
// выведенный идентификатор проверяющий может пересчитать, имея только входы.

/// pid = sha256(root_ed25519_public_key)[0..8].
Uint8List csmPid(List<int> rootPublicKey) =>
    Uint8List.fromList(sha256(rootPublicKey).sublist(0, 8));

/// keyid_trunc = sha256(ed25519_public_key)[0..12].
Uint8List csmKeyId(List<int> publicKey) =>
    Uint8List.fromList(sha256(publicKey).sublist(0, 12));

/// link_pin = base32_crockford(sha256(root_public_key)[0..12]), 20 символов.
String csmLinkPin(List<int> rootPublicKey) =>
    base32CrockfordEncode(csmKeyId(rootPublicKey));

/// Обратное преобразование link_pin в 12 байт идентификатора ключа. Дефисы
/// игнорируются, регистр не важен.
Uint8List? csmLinkPinToKeyId(String linkPin) {
  final raw = base32CrockfordDecode(linkPin);
  if (raw == null || raw.length != 12) {
    return null;
  }
  return raw;
}

/// loc = base32_crockford(HMAC-SHA256(secret, "csm1-loc" || 0x00 ||
/// subscription_uuid || u32be(gen))[0..15]), 24 символа.
///
/// subscription_uuid это ASCII-текст UUID ровно так, как его хранит панель:
/// нижний регистр с дефисами, 36 байт, а не 16 сырых.
String csmLocator(List<int> secret, String subscriptionUuid, int generation) {
  final message = <int>[
    ...utf8.encode('csm1-loc'),
    0x00,
    ...utf8.encode(subscriptionUuid),
    (generation >> 24) & 0xff,
    (generation >> 16) & 0xff,
    (generation >> 8) & 0xff,
    generation & 0xff,
  ];
  return base32CrockfordEncode(hmacSha256(secret, message).sublist(0, 15));
}

/// dtp = sha256(device_signing_SPKI_DER)[0..16].
Uint8List csmDeviceThumbprint(List<int> spkiDer) =>
    Uint8List.fromList(sha256(spkiDer).sublist(0, 16));

/// chash = sha256(полный кадр каталога), подписи включительно.
Uint8List csmCatalogHash(List<int> catalogFrame) => sha256(catalogFrame);

/// cat_id = base32_crockford(chash[0..10]), 16 символов.
String csmCatalogId(List<int> chash) =>
    base32CrockfordEncode(chash.sublist(0, 10));

/// Тот же cat_id, посчитанный от поля cid чанка, которое и есть chash[0..10].
String csmCatalogIdFromCid(List<int> cid) => base32CrockfordEncode(cid);

/// bid = base32_crockford(sha256(поток кадров)[0..5]), 8 символов.
String csmBundleId(List<int> frameStream) =>
    base32CrockfordEncode(sha256(frameStream).sublist(0, 5));

/// Шестнадцатеричная запись, только для сообщений об ошибках и журналов.
String csmHex(List<int> bytes) {
  final sb = StringBuffer();
  for (final b in bytes) {
    sb.write(b.toRadixString(16).padLeft(2, '0'));
  }
  return sb.toString();
}

/// Проверяет, что строка это символы КАНОНИЧЕСКОГО алфавита base32 Crockford в
/// верхнем регистре, ровно [length] штук.
///
/// Псевдонимы I, L, O и строчные буквы, которые декодер принимает, здесь
/// отвергаются: у идентификатора, уходящего в путь URL и в заголовок, должно
/// быть ровно одно написание.
bool csmIsCrockford(String s, {int length = 24}) {
  if (s.length != length) {
    return false;
  }
  for (var i = 0; i < s.length; i++) {
    if (!csmCrockfordAlphabet.contains(s[i])) {
      return false;
    }
  }
  return true;
}

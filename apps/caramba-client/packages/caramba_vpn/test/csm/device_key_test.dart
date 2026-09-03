// Ключи устройства CSM/1 на стороне Dart (ABI v3, 02-SPEC.md 12.2).
//
// Проверяется граница, а не криптография: формы ответа ядра, отказ принимать
// половину личности и то, что уровень хранения не улучшается по дороге.

import 'dart:convert';
import 'dart:typed_data';

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter_test/flutter_test.dart';

String _b64(List<int> b) => base64.encode(b);

Uint8List _point() {
  // 0x04 || X || Y: содержимое здесь не проверяется, важна форма.
  final out = Uint8List(65);
  out[0] = 0x04;
  for (var i = 1; i < 65; i++) {
    out[i] = i;
  }
  return out;
}

void main() {
  group('CarambaDeviceKeygen', () {
    test('разбирает полную личность устройства', () {
      final key = CsmDeviceKey.fromJson(
        jsonEncode(<String, Object?>{
          'spki_b64': _b64(List<int>.filled(91, 7)),
          'agree_pub_b64': _b64(_point()),
          'dtp_hex': '4f0f22569564aab09a2d1a75c132d955',
          'tier': 2,
          'generation': 3,
        }),
      );
      expect(key.signingSpki.length, 91);
      expect(key.agreementPublicKey.length, 65);
      expect(key.deviceThumbprint, '4f0f22569564aab09a2d1a75c132d955');
      expect(key.hardwareTier, 2);
      expect(key.agreementKeyGeneration, 3);
    });

    test('уровень не улучшается по дороге: 3 остаётся 3', () {
      final key = CsmDeviceKey.fromJson(
        jsonEncode(<String, Object?>{
          'spki_b64': _b64(List<int>.filled(91, 7)),
          'agree_pub_b64': _b64(_point()),
          'dtp_hex': '4f0f22569564aab09a2d1a75c132d955',
          'tier': 3,
          'generation': 1,
        }),
      );
      expect(key.hardwareTier, 3);
    });

    test('половина личности отвергается, а не достраивается', () {
      // Нет SPKI.
      expect(
        () => CsmDeviceKey.fromJson(
          jsonEncode(<String, Object?>{
            'agree_pub_b64': _b64(_point()),
            'dtp_hex': '4f0f22569564aab09a2d1a75c132d955',
            'tier': 3,
          }),
        ),
        throwsFormatException,
      );
      // Ключ согласования сжатой формы.
      expect(
        () => CsmDeviceKey.fromJson(
          jsonEncode(<String, Object?>{
            'spki_b64': _b64(List<int>.filled(91, 7)),
            'agree_pub_b64': _b64(List<int>.filled(33, 2)),
            'dtp_hex': '4f0f22569564aab09a2d1a75c132d955',
            'tier': 3,
          }),
        ),
        throwsFormatException,
      );
      // dtp не 16 байт.
      expect(
        () => CsmDeviceKey.fromJson(
          jsonEncode(<String, Object?>{
            'spki_b64': _b64(List<int>.filled(91, 7)),
            'agree_pub_b64': _b64(_point()),
            'dtp_hex': 'abcd',
            'tier': 3,
          }),
        ),
        throwsFormatException,
      );
      // Уровень вне 1..3.
      expect(
        () => CsmDeviceKey.fromJson(
          jsonEncode(<String, Object?>{
            'spki_b64': _b64(List<int>.filled(91, 7)),
            'agree_pub_b64': _b64(_point()),
            'dtp_hex': '4f0f22569564aab09a2d1a75c132d955',
            'tier': 9,
          }),
        ),
        throwsFormatException,
      );
    });

    test('ошибка ядра поднимается, а не подменяется пустой личностью', () {
      expect(
        () => CsmDeviceKey.fromJson('{"error":"хранилище недоступно"}'),
        throwsFormatException,
      );
    });
  });

  group('CarambaDeviceSign', () {
    test('принимает ровно 64 байта r || s', () {
      final sig = csmDeviceSignatureFromJson(
        jsonEncode(<String, Object?>{'sig_b64': _b64(List<int>.filled(64, 1))}),
      );
      expect(sig.length, 64);
    });

    test('ASN.1 DER отвергается на границе', () {
      // Ровно то, что SecKeyCreateSignature и Signature на Android отдают по
      // умолчанию: последовательность DER, а не 64 байта r || s.
      final der = <int>[0x30, 0x44, ...List<int>.filled(68, 3)];
      expect(
        () => csmDeviceSignatureFromJson(
          jsonEncode(<String, Object?>{'sig_b64': _b64(der)}),
        ),
        throwsFormatException,
      );
    });
  });

  group('CarambaDeviceAgree', () {
    test('несёт собственный открытый ключ вместе с секретом', () {
      final a = CsmAgreement.fromJson(
        jsonEncode(<String, Object?>{
          'shared_b64': _b64(List<int>.filled(32, 5)),
          'own_pub_b64': _b64(_point()),
        }),
      );
      expect(a.shared.length, 32);
      expect(a.ownPublicKey.length, 65);
    });

    test('ответ без собственного ключа отвергается', () {
      expect(
        () => CsmAgreement.fromJson(
          jsonEncode(<String, Object?>{
            'shared_b64': _b64(List<int>.filled(32, 5)),
          }),
        ),
        throwsFormatException,
      );
    });
  });

  group('мок', () {
    test('называет свой уровень хранения своим именем', () async {
      final mock = MockVpnConnection<Object>();
      final key = await mock.deviceKeygen();
      expect(key.hardwareTier, 3);
      expect(key.agreementPublicKey.length, 65);
      expect(key.deviceThumbprint.length, 32);
      await mock.dispose();
    });

    test('согласование симметрично: мок с самим собой сходится', () async {
      final a = MockVpnConnection<Object>();
      final key = await a.deviceKeygen();
      final agreed = await a.deviceAgree(peerPublicKey: key.agreementPublicKey);
      expect(agreed.shared.length, 32);
      expect(agreed.ownPublicKey, key.agreementPublicKey);
      await a.dispose();
    });

    test(
      'запись настроек запоминает карту want и не выдумывает директиву',
      () async {
        final mock = MockVpnConnection<Object>();
        final out = await mock.csmRequestSettings(
          want: <int, Object?>{1: 'auto', 5: 1280, 8: true},
        );
        expect(mock.lastSettingsWrite, <int, Object?>{
          1: 'auto',
          5: 1280,
          8: true,
        });
        expect(out, '{}');
        await mock.dispose();
      },
    );
  });
}

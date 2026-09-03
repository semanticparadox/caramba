// INV-8 в точке ввода ссылки, и объяснимый отказ.
//
// 02-SPEC.md 8.10 называет обычный `http://` MUST stop для любой выборки
// манифеста, конфигурации, правил и geo. URL панели это уже проверял, а
// `ImportLink.fromUrl` принимал любой `http://`: ссылка подписки это ровно
// такая выборка конфигурации, и импорт по открытому каналу проходил молча.
// Второй половиной идёт причина отказа: голый `null` заставлял UI писать
// «проверьте ссылку», а после отказа по INV-8 адрес выглядит рабочим и человек
// вводит его снова.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/enrollment.dart';

void main() {
  group('ImportLink и INV-8', () {
    test('голый http отвергается', () {
      expect(ImportLink.fromUrl('http://sub.example/a'), isNull);
      expect(
        ImportLink.tryParse(
          'carambaconnect://import?url=${Uri.encodeComponent('http://sub.example/a')}',
        ),
        isNull,
      );
    });

    test('https проходит, .onion по http остаётся исключением', () {
      expect(
        ImportLink.fromUrl('https://sub.example/a')?.url,
        'https://sub.example/a',
      );
      expect(
        ImportLink.fromUrl('http://abcdefgh.onion/sub')?.url,
        'http://abcdefgh.onion/sub',
      );
      expect(
        ImportLink.fromUrl('http://ABCDEFGH.ONION/sub')?.url,
        'http://ABCDEFGH.ONION/sub',
      );
    });

    test('отказ по http объясняется, а не молчит', () {
      final r = ImportLink.parseUrl('http://sub.example/a');
      expect(r.isOk, isFalse);
      expect(r.refusal, LinkRefusal.insecureTransport);
      expect(r.message, contains('https'));
      expect(r.message, contains('.onion'));
      // Не-http схема это другой отказ: причины не сливаются в одну.
      expect(
        ImportLink.parseUrl('vless://x@h:443').refusal,
        LinkRefusal.malformedUrl,
      );
    });
  });

  group('EnrollLink и INV-8', () {
    test('http-панель отвергается с объяснением', () {
      final r = EnrollLink.parsePanelUrl('http://panel.example.net');
      expect(r.isOk, isFalse);
      expect(r.refusal, LinkRefusal.insecureTransport);
      expect(r.message, contains('https'));
      expect(EnrollLink.normalizePanelUrl('http://panel.example.net'), isNull);
    });

    test('https и .onion проходят', () {
      expect(
        EnrollLink.parsePanelUrl('https://panel.example.net/api/v2').value,
        'https://panel.example.net',
      );
      expect(
        EnrollLink.parsePanelUrl('http://abcdefgh.onion').value,
        'http://abcdefgh.onion',
      );
      // Ввод без схемы по-прежнему достраивается до https.
      expect(
        EnrollLink.parsePanelUrl('panel.example.net').value,
        'https://panel.example.net',
      );
    });

    test('причины отказа различимы', () {
      expect(
        EnrollLink.parse('https://panel.example.net').refusal,
        LinkRefusal.notOurLink,
      );
      expect(
        EnrollLink.parse(
          'carambaconnect://import?url=https://s.example',
        ).refusal,
        LinkRefusal.notOurLink,
      );
      expect(
        EnrollLink.parseParts(
          panelUrl: 'https://panel.example',
          code: '  ',
        ).refusal,
        LinkRefusal.emptyCode,
      );
      expect(
        EnrollLink.parseParts(panelUrl: '', code: 'ABC123').refusal,
        LinkRefusal.malformedUrl,
      );
      expect(
        EnrollLink.parse(
          'carambaconnect://enroll?panel=http://panel.example.net&code=ABC123',
        ).refusal,
        LinkRefusal.insecureTransport,
      );
    });

    test('успешный разбор доносит значение', () {
      final r = EnrollLink.parse(
        'carambaconnect://enroll?panel=https://panel.example.net&code=ABC123',
      );
      expect(r.isOk, isTrue);
      expect(r.message, isNull);
      expect(r.value!.panelUrl, 'https://panel.example.net');
      expect(r.value!.code, 'ABC123');
    });
  });
}

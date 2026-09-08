// Разбор deeplink'ов схемы carambaconnect.
//
// У схемы два действия, и они не должны пересекаться: enroll ведёт в панель по
// инвайт-коду, import — в generic-режим по ссылке подписки. Добавление второго
// не имеет права ослабить разбор первого, поэтому обратная совместимость
// EnrollLink.tryParse проверяется явно.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/enrollment.dart';

void main() {
  group('EnrollLink.tryParse (обратная совместимость)', () {
    test('разбирает ссылку с действием в хосте', () {
      final link = EnrollLink.tryParse(
        'carambaconnect://enroll?panel=https://panel.example&code=ABC123',
      );
      expect(link?.panelUrl, 'https://panel.example');
      expect(link?.code, 'ABC123');
    });

    test('разбирает ссылку с действием в первом сегменте пути', () {
      final link = EnrollLink.tryParse(
        'carambaconnect:///enroll?panel=panel.example&code=X1',
      );
      // Схема достраивается, URL сводится к origin.
      expect(link?.panelUrl, 'https://panel.example');
    });

    test('отвергает чужое действие, чужую схему и неполные параметры', () {
      expect(
        EnrollLink.tryParse('carambaconnect://import?url=https://s.example'),
        isNull,
      );
      expect(
        EnrollLink.tryParse('https://panel.example/enroll?code=X'),
        isNull,
      );
      expect(EnrollLink.tryParse('carambaconnect://enroll?code=X'), isNull);
      expect(
        EnrollLink.tryParse('carambaconnect://enroll?panel=https://p.example'),
        isNull,
      );
      // Не-http(s) панель отвергается: туда нельзя слать креды.
      expect(
        EnrollLink.tryParse(
          'carambaconnect://enroll?panel=ftp://p.example&code=X',
        ),
        isNull,
      );
    });
  });

  group('ImportLink.tryParse', () {
    test('разбирает ссылку подписки из query', () {
      final link = ImportLink.tryParse(
        'carambaconnect://import?url=${Uri.encodeComponent('https://sub.example/a?token=1')}',
      );
      expect(link?.url, 'https://sub.example/a?token=1');
    });

    test('принимает действие в первом сегменте пути', () {
      final link = ImportLink.tryParse(
        'carambaconnect:///import?url=https%3A%2F%2Fsub.example%2Fb',
      );
      expect(link?.url, 'https://sub.example/b');
    });

    test('отвергает enroll-ссылку, чужую схему и не-http url', () {
      expect(
        ImportLink.tryParse(
          'carambaconnect://enroll?panel=https://p.example&code=X',
        ),
        isNull,
      );
      expect(ImportLink.tryParse('https://sub.example/a'), isNull);
      expect(ImportLink.tryParse('carambaconnect://import'), isNull);
      expect(ImportLink.tryParse('carambaconnect://import?url='), isNull);
      // vless:// это конфиг, а не ссылка на подписку: его вставляют в поле.
      expect(
        ImportLink.tryParse(
          'carambaconnect://import?url=vless%3A%2F%2Fx%40h%3A443',
        ),
        isNull,
      );
    });

    test('fromUrl принимает голый http(s) адрес', () {
      expect(
        ImportLink.fromUrl(' https://sub.example/x ')?.url,
        'https://sub.example/x',
      );
      expect(ImportLink.fromUrl('sub.example/x'), isNull);
      expect(ImportLink.fromUrl(''), isNull);
    });
  });
}

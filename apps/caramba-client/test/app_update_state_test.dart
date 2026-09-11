// Обновления приложения: чистая логика без сети и платформы.
//
// Три вещи, которые нельзя проверять глазами: разбор версии из PackageInfo
// (buildNumber на macOS не всегда число), разбор ответа панели (старая панель
// отвечает не тем) и само решение «молчать / предложить / заблокировать».

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/state/app_update_state.dart';

AppVersionInfo _latest({
  int build = 110,
  int minBuild = 0,
  String notes = '',
}) => AppVersionInfo(
  platform: 'android',
  version: '1.0.0',
  build: build,
  minBuild: minBuild,
  notes: notes,
);

const _installed = InstalledVersion(version: '1.0.0', build: 109);

void main() {
  group('InstalledVersion.parse', () {
    test('обычный pubspec: версия и сборка', () {
      final v = InstalledVersion.parse(version: '1.0.0', buildNumber: '109');
      expect(v.version, '1.0.0');
      expect(v.build, 109);
      expect(v.isKnown, isTrue);
      expect(v.headerValue, '1.0.0+109');
      expect(v.label, '1.0.0 (109)');
    });

    test('macOS может отдать составной buildNumber: берём последнее число', () {
      final v = InstalledVersion.parse(
        version: '1.0.0',
        buildNumber: '1.0.0.110',
      );
      expect(v.build, 110);
    });

    test('мусор и пустота — «неизвестно», заголовок не отправляется', () {
      expect(
        InstalledVersion.parse(version: '', buildNumber: '109'),
        InstalledVersion.unknown,
      );
      expect(
        InstalledVersion.parse(version: '1.0.0', buildNumber: 'abc'),
        InstalledVersion.unknown,
      );
      expect(InstalledVersion.unknown.headerValue, '');
      expect(InstalledVersion.unknown.label, 'неизвестно');
    });
  });

  group('AppVersionInfo.fromJson', () {
    test('полный ответ панели разбирается целиком', () {
      final info = AppVersionInfo.fromJson(<String, dynamic>{
        'platform': 'windows',
        'version': '1.0.0',
        'build': 110,
        'download_url': 'https://app.example.com/downloads/x.exe',
        'size': 12345,
        'sha256': 'ABCD',
        'published_at': '2026-09-11T10:00:00Z',
        'min_build': 105,
        'notes': ' Трей ',
        'source': 'manifest',
      })!;
      expect(info.platform, 'windows');
      expect(info.build, 110);
      expect(info.downloadUrl, 'https://app.example.com/downloads/x.exe');
      expect(info.size, 12345);
      expect(info.sha256, 'abcd', reason: 'хэш нормализуется в нижний регистр');
      expect(info.publishedAt, isNotNull);
      expect(info.minBuild, 105);
      expect(info.notes, 'Трей');
      expect(info.label, '1.0.0 (110)');
    });

    test('пустой объект и ответ без сборки — не версия', () {
      expect(AppVersionInfo.fromJson(<String, dynamic>{}), isNull);
      expect(
        AppVersionInfo.fromJson(<String, dynamic>{'version': '1.0.0'}),
        isNull,
      );
      expect(
        AppVersionInfo.fromJson(<String, dynamic>{
          'version': '1.0.0',
          'build': '0',
        }),
        isNull,
      );
    });

    test('числа строками тоже принимаются, необязательные поля — null', () {
      final info = AppVersionInfo.fromJson(<String, dynamic>{
        'version': '1.0.1',
        'build': '111',
      })!;
      expect(info.build, 111);
      expect(info.downloadUrl, isNull);
      expect(info.size, isNull);
      expect(info.sha256, isNull);
      expect(info.publishedAt, isNull);
      expect(info.minBuild, 0);
      expect(info.notes, '');
    });
  });

  group('decideUpdate', () {
    test('без версии панели или без своей версии — молчим', () {
      expect(
        decideUpdate(installed: _installed, latest: null),
        UpdateVerdict.none,
      );
      expect(
        decideUpdate(installed: InstalledVersion.unknown, latest: _latest()),
        UpdateVerdict.none,
      );
    });

    test('сборка новее — предлагаем; та же или старее — молчим', () {
      expect(
        decideUpdate(installed: _installed, latest: _latest(build: 110)),
        UpdateVerdict.available,
      );
      expect(
        decideUpdate(installed: _installed, latest: _latest(build: 109)),
        UpdateVerdict.none,
      );
      expect(
        decideUpdate(installed: _installed, latest: _latest(build: 100)),
        UpdateVerdict.none,
      );
    });

    test('«Позже» прячет ровно эту сборку, следующая снова покажется', () {
      expect(
        decideUpdate(
          installed: _installed,
          latest: _latest(build: 110),
          dismissedBuild: 110,
        ),
        UpdateVerdict.none,
      );
      expect(
        decideUpdate(
          installed: _installed,
          latest: _latest(build: 111),
          dismissedBuild: 110,
        ),
        UpdateVerdict.available,
      );
    });

    test('ниже минимальной — блокируем, и «Позже» не действует', () {
      expect(
        decideUpdate(
          installed: _installed,
          latest: _latest(build: 110, minBuild: 110),
          dismissedBuild: 110,
        ),
        UpdateVerdict.required,
      );
      // Минимум равен установленной — не блокируем, только предлагаем.
      expect(
        decideUpdate(
          installed: _installed,
          latest: _latest(build: 110, minBuild: 109),
        ),
        UpdateVerdict.available,
      );
    });
  });

  group('AppUpdateState', () {
    test('hasNewer не зависит от «Позже», verdict зависит', () {
      final s = AppUpdateState(
        installed: _installed,
        latest: _latest(build: 110),
        dismissedBuild: 110,
      );
      expect(s.hasNewer, isTrue);
      expect(s.verdict, UpdateVerdict.none);
      expect(s.copyWith(dismissedBuild: 0).verdict, UpdateVerdict.available);
      expect(s.copyWith(clearLatest: true).hasNewer, isFalse);
    });
  });
}

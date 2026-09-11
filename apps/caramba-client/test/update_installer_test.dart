// Установка обновления по платформам: решение без сети, диска и процессов.
//
// Все побочные действия инъецируются; проверяется, ЧТО установщик сделал бы:
// на Windows скачал, сверил хэш и запустил; на остальных открыл ссылку;
// битую ссылку не открыл вовсе.

import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/features/updates/update_installer.dart';
import 'package:caramba_client/state/app_update_state.dart';

const _bytes = <int>[1, 2, 3, 4, 5];

AppVersionInfo _info({
  String? url =
      'https://app.example.com/downloads/Caramba-Connect-Setup-x64.exe',
  String? sha,
  int? size,
}) => AppVersionInfo(
  platform: 'windows',
  version: '1.0.0',
  build: 110,
  downloadUrl: url,
  sha256: sha,
  size: size,
);

class _Recorder {
  final List<Uri> opened = <Uri>[];
  final List<Uri> downloaded = <Uri>[];
  final List<String> written = <String>[];
  final List<String> started = <String>[];
  List<int> payload = _bytes;
  bool openResult = true;

  PlatformUpdateInstaller installer(TargetPlatform platform) =>
      PlatformUpdateInstaller(
        platform: platform,
        isWeb: false,
        open: (uri) async {
          opened.add(uri);
          return openResult;
        },
        download: (uri) async {
          downloaded.add(uri);
          return payload;
        },
        write: (path, bytes) async => written.add(path),
        start: (path) async => started.add(path),
        tempDir: () => Directory('/tmp/caramba-test'),
      );
}

void main() {
  group('ссылка на файл', () {
    test('только https и только известные схемы', () {
      expect(updateDownloadUri('https://app.example.com/x.apk'), isNotNull);
      expect(updateDownloadUri('http://app.example.com/x.apk'), isNull);
      expect(updateDownloadUri('javascript:alert(1)'), isNull);
      expect(updateDownloadUri(''), isNull);
      expect(updateDownloadUri(null), isNull);
    });

    test('имя временного файла берётся из ссылки, если оно похоже на .exe', () {
      expect(
        installerFileName(
          Uri.parse('https://x/downloads/Caramba-Connect-Setup-x64.exe'),
        ),
        'Caramba-Connect-Setup-x64.exe',
      );
      expect(
        installerFileName(Uri.parse('https://x/downloads/setup')),
        'Caramba-Connect-Setup.exe',
      );
      expect(
        installerFileName(Uri.parse('https://x/a%20b.exe')),
        'Caramba-Connect-Setup.exe',
        reason: 'пробел в имени — не наш файл',
      );
    });
  });

  group('Windows', () {
    test('скачивает, сверяет хэш и запускает установщик', () async {
      final r = _Recorder();
      final sha = sha256.convert(_bytes).toString();
      final msg = await r
          .installer(TargetPlatform.windows)
          .install(_info(sha: sha, size: _bytes.length));
      expect(r.downloaded, hasLength(1));
      expect(r.written.single, endsWith('Caramba-Connect-Setup-x64.exe'));
      expect(r.started.single, r.written.single);
      expect(r.opened, isEmpty, reason: 'браузер на Windows не открывается');
      expect(msg, contains('Установщик запущен'));
    });

    test('неверный хэш — файл не запускается', () async {
      final r = _Recorder();
      final msg = await r
          .installer(TargetPlatform.windows)
          .install(_info(sha: 'ff'.padLeft(64, '0')));
      expect(r.started, isEmpty);
      expect(r.written, isEmpty);
      expect(msg, contains('Контрольная сумма'));
    });

    test('неполная загрузка — файл не запускается', () async {
      final r = _Recorder();
      final msg = await r
          .installer(TargetPlatform.windows)
          .install(_info(size: 999));
      expect(r.started, isEmpty);
      expect(msg, contains('не полностью'));
    });

    test('без хэша в манифесте проверять нечего — запускаем', () async {
      final r = _Recorder();
      final msg = await r.installer(TargetPlatform.windows).install(_info());
      expect(r.started, hasLength(1));
      expect(msg, contains('Установщик запущен'));
    });
  });

  group('остальные платформы', () {
    test('Android, macOS и Linux открывают ссылку и объясняют шаг', () async {
      for (final (platform, word) in <(TargetPlatform, String)>[
        (TargetPlatform.android, 'уведомления'),
        (TargetPlatform.macOS, 'DMG'),
        (TargetPlatform.linux, 'install.sh'),
      ]) {
        final r = _Recorder();
        final msg = await r.installer(platform).install(_info());
        expect(r.opened, hasLength(1), reason: '$platform');
        expect(r.downloaded, isEmpty, reason: '$platform');
        expect(msg, contains(word), reason: '$platform');
      }
    });

    test('ссылка не открылась — честный отказ', () async {
      final r = _Recorder()..openResult = false;
      final msg = await r.installer(TargetPlatform.android).install(_info());
      expect(msg, contains('Не удалось открыть'));
    });

    test('без ссылки от панели — отправляем в бота', () async {
      final r = _Recorder();
      final msg = await r
          .installer(TargetPlatform.android)
          .install(_info(url: null));
      expect(r.opened, isEmpty);
      expect(msg, contains('/apk'));
    });
  });

  test('форматирование размера и даты', () {
    expect(formatUpdateSize(null), '');
    expect(formatUpdateSize(512), '1 КБ');
    expect(formatUpdateSize(5 * 1024 * 1024), '5.0 МБ');
    expect(formatUpdateDate(DateTime(2026, 9, 1)), '01.09.2026');
    expect(formatUpdateDate(null), '');
  });
}

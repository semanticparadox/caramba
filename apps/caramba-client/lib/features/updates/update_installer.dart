/// Установка найденного обновления способом платформы.
///
/// Единого пути нет и быть не может:
///   * Android — открываем `download_url` в браузере; систему ставит APK сама
///     (тот же ключ подписи, обновление поверх), магазина у приложения нет;
///   * Windows — скачиваем инсталлятор Inno Setup во временную папку и
///     запускаем его: он сам обновляет установку поверх (`CloseApplications=yes`
///     в .iss попросит закрыть приложение). Скачивать в браузер и просить
///     человека найти файл в «Загрузках» значило бы потерять половину людей на
///     полпути;
///   * macOS и Linux — открываем `download_url`: DMG и tar.gz без подписи
///     ставятся руками, автоматизировать это нечестно.
///
/// Скачанный файл проверяется по sha256 из манифеста, когда он есть: файл
/// едет через зеркало и прокси, и запускать без проверки то, что приехало,
/// нельзя.
library;

import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:dio/dio.dart';
import 'package:flutter/foundation.dart'
    show TargetPlatform, defaultTargetPlatform, kIsWeb;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import 'package:caramba_client/data/safe_url.dart';
import 'package:caramba_client/state/app_update_state.dart';

/// Ссылка на файл принимается только по https и только на известные схемы:
/// `download_url` приходит с панели, и открывать по нему `javascript:` или
/// схему стороннего приложения нельзя.
Uri? updateDownloadUri(String? raw) {
  if (raw == null || raw.trim().isEmpty) return null;
  final uri = csmSafeExternalUri(raw.trim());
  if (uri == null || uri.scheme != 'https') return null;
  return uri;
}

/// Имя временного файла для инсталлятора: последний сегмент ссылки, если он
/// похож на имя файла, иначе фиксированное.
String installerFileName(Uri uri) {
  final last = uri.pathSegments.isEmpty ? '' : uri.pathSegments.last;
  final ok = RegExp(r'^[A-Za-z0-9._-]+\.exe$').hasMatch(last);
  return ok ? last : 'Caramba-Connect-Setup.exe';
}

/// Проверка контрольной суммы: пустой ожидаемый хэш — проверять нечего.
bool sha256Matches(List<int> bytes, String? expected) {
  if (expected == null || expected.trim().isEmpty) return true;
  return sha256.convert(bytes).toString() == expected.trim().toLowerCase();
}

/// Установщик по платформам. Все побочные действия (сеть, диск, запуск
/// процесса, браузер) вынесены в инъекции, чтобы решение проверялось тестом.
class PlatformUpdateInstaller implements UpdateInstaller {
  final TargetPlatform? _platform;
  final bool _isWeb;
  final Future<bool> Function(Uri uri) _open;
  final Future<List<int>> Function(Uri uri) _download;
  final Future<void> Function(String path, List<int> bytes) _write;
  final Future<void> Function(String path) _start;
  final Directory Function() _tempDir;

  PlatformUpdateInstaller({
    TargetPlatform? platform,
    bool? isWeb,
    Future<bool> Function(Uri uri)? open,
    Future<List<int>> Function(Uri uri)? download,
    Future<void> Function(String path, List<int> bytes)? write,
    Future<void> Function(String path)? start,
    Directory Function()? tempDir,
  }) : _platform = platform,
       _isWeb = isWeb ?? kIsWeb,
       _open = open ?? _launch,
       _download = download ?? _fetch,
       _write = write ?? _writeFile,
       _start = start ?? _startDetached,
       _tempDir = tempDir ?? (() => Directory.systemTemp);

  TargetPlatform get platform => _platform ?? defaultTargetPlatform;

  @override
  Future<String> install(AppVersionInfo info) async {
    final uri = updateDownloadUri(info.downloadUrl);
    if (uri == null) {
      return 'Панель не дала ссылку на файл. Скачайте обновление в боте: '
          'команда /apk.';
    }
    if (!_isWeb && platform == TargetPlatform.windows) {
      return _installWindows(uri, info);
    }
    final ok = await _open(uri);
    if (!ok) return 'Не удалось открыть ссылку на скачивание.';
    return switch (platform) {
      TargetPlatform.android =>
        'Файл скачивается. Откройте его из уведомления и подтвердите '
            'установку: приложение обновится поверх текущего.',
      TargetPlatform.macOS =>
        'Образ DMG скачивается. Закройте приложение и перетащите новую '
            'версию в «Программы» поверх старой.',
      TargetPlatform.linux =>
        'Архив скачивается. Распакуйте его и запустите install.sh: '
            'установка обновится в /opt/caramba-connect.',
      _ => 'Ссылка на скачивание открыта.',
    };
  }

  Future<String> _installWindows(Uri uri, AppVersionInfo info) async {
    final List<int> bytes;
    try {
      bytes = await _download(uri);
    } catch (e) {
      return 'Не удалось скачать установщик: $e';
    }
    if (bytes.isEmpty) return 'Скачанный установщик пуст.';
    if (info.size != null && info.size! > 0 && bytes.length != info.size) {
      return 'Установщик скачался не полностью '
          '(${bytes.length} из ${info.size} байт). Попробуйте ещё раз.';
    }
    if (!sha256Matches(bytes, info.sha256)) {
      return 'Контрольная сумма установщика не совпала. Файл не запущен: '
          'попробуйте позже или скачайте его в боте командой /apk.';
    }
    final path =
        '${_tempDir().path}${Platform.pathSeparator}'
        '${installerFileName(uri)}';
    try {
      await _write(path, bytes);
      await _start(path);
    } catch (e) {
      return 'Не удалось запустить установщик: $e';
    }
    return 'Установщик запущен. Следуйте его шагам: он обновит приложение '
        'поверх текущего и попросит закрыть его.';
  }

  static Future<bool> _launch(Uri uri) =>
      launchUrl(uri, mode: LaunchMode.externalApplication);

  static Future<List<int>> _fetch(Uri uri) async {
    final res = await Dio(
      BaseOptions(
        connectTimeout: const Duration(seconds: 20),
        receiveTimeout: const Duration(minutes: 10),
        responseType: ResponseType.bytes,
      ),
    ).getUri<List<int>>(uri);
    final code = res.statusCode ?? 0;
    if (code < 200 || code >= 300) {
      throw HttpException('HTTP $code', uri: uri);
    }
    return res.data ?? const <int>[];
  }

  static Future<void> _writeFile(String path, List<int> bytes) =>
      File(path).writeAsBytes(bytes, flush: true);

  /// Отдельный процесс, не привязанный к нашему: установщик обязан пережить
  /// закрытие приложения, о котором сам же и попросит.
  static Future<void> _startDetached(String path) async {
    await Process.start(
      path,
      const <String>[],
      mode: ProcessStartMode.detached,
    );
  }
}

/// Установщик приложения. Тесты подменяют его целиком.
final updateInstallerProvider = Provider<UpdateInstaller>(
  (ref) => PlatformUpdateInstaller(),
);

/// Человеческий размер файла для экрана «Обновления».
String formatUpdateSize(int? bytes) {
  if (bytes == null || bytes <= 0) return '';
  const mb = 1024 * 1024;
  if (bytes >= mb) return '${(bytes / mb).toStringAsFixed(1)} МБ';
  return '${(bytes / 1024).round()} КБ';
}

/// Дата публикации для экрана «Обновления».
String formatUpdateDate(DateTime? at) {
  if (at == null) return '';
  String two(int v) => v.toString().padLeft(2, '0');
  return '${two(at.day)}.${two(at.month)}.${at.year}';
}

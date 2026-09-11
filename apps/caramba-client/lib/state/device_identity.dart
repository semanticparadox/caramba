/// Идентичность УСТРОЙСТВА (а не сессии и не тенанта).
///
/// ЗАЧЕМ ОТДЕЛЬНОЕ ПОНЯТИЕ. Панель до сих пор узнавала устройство по
/// отпечатку `sha256(subscription_id + User-Agent)`: он менялся при смене
/// тарифа (новая строка подписки) и при обновлении приложения (новый UA), из-за
/// чего лиза устройства «слетала», лимит устройств считался неверно, а из
/// списка в профиле пропадали строки, которые человек хотел отвязать. Стабильный
/// идентификатор, который приложение генерирует один раз и хранит само, чинит
/// это на корню: устройство остаётся тем же самым, что бы ни происходило с
/// подпиской и версией.
///
/// ЧТО ЭТО НЕ ЕСТЬ. Не идентификатор пользователя и не отпечаток железа: UUID
/// генерируется случайно при первом запуске и живёт в том же защищённом
/// хранилище, что и токены. Переустановка приложения даёт новое устройство, и
/// это правильно — привязка не должна переживать удаление приложения.
///
/// ГДЕ ИСПОЛЬЗУЕТСЯ. [ApiClient] добавляет [kDeviceIdHeader] и
/// [kDeviceNameHeader] к каждому авторизованному запросу к панели; панель по
/// первому заводит/находит лизу устройства, по второму берёт имя по умолчанию.
library;

import 'dart:async';
import 'dart:io' show Platform;
import 'dart:math';

import 'package:flutter/foundation.dart'
    show TargetPlatform, defaultTargetPlatform, kIsWeb;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'package:caramba_client/data/secure_storage_options.dart';

/// Заголовок со стабильным идентификатором устройства (`client_device_id`).
const String kDeviceIdHeader = 'X-Caramba-Device-Id';

/// Заголовок с именем устройства по умолчанию.
///
/// ТОЛЬКО ASCII (см. [asciiHeaderValue]). HTTP-заголовки едут latin-1, и
/// кириллическое имя в них либо ломает запрос, либо приезжает мусором. Имя,
/// которое человек задал руками, отправляется не заголовком, а телом
/// `PATCH /devices/{id}` — там UTF-8 законен; заголовок же нужен лишь для того,
/// чтобы новая лиза не называлась «Устройство».
const String kDeviceNameHeader = 'X-Caramba-Device-Name';

/// Заголовок с платформой устройства.
///
/// Панель кладёт его в колонку `platform` лизы и по ней подписывает строку в
/// кабинете. Выводить платформу из User-Agent она тоже умеет, но у нашего
/// приложения UA один на все пять платформ — без этого заголовка Windows и
/// Linux в списке неотличимы.
const String kDevicePlatformHeader = 'X-Caramba-Device-Platform';

/// Максимальная длина имени в заголовке. Длиннее не несёт смысла и только
/// раздувает каждый запрос.
const int kDeviceNameMaxLength = 64;

/// Что приложение знает о себе как об устройстве.
class DeviceIdentity {
  /// Стабильный UUID v4, сгенерированный при первом запуске.
  final String clientDeviceId;

  /// Имя для списка устройств: авто из платформы/хоста либо заданное человеком.
  final String displayName;

  /// Канон платформы: `android`, `ios`, `macos`, `windows`, `linux`, `web`.
  /// Пусто — платформа неизвестна (такого быть не должно, но парсер терпит).
  final String platform;

  const DeviceIdentity({
    required this.clientDeviceId,
    required this.displayName,
    required this.platform,
  });

  /// Идентичности ещё нет (хранилище недоступно): заголовки не отправляются,
  /// панель падает на прежний отпечаток по User-Agent.
  static const DeviceIdentity unknown = DeviceIdentity(
    clientDeviceId: '',
    displayName: '',
    platform: '',
  );

  bool get isKnown => clientDeviceId.isNotEmpty;

  /// Заголовки для запроса к панели. Пустая карта, если идентичности нет:
  /// отправлять пустой `X-Caramba-Device-Id` хуже, чем не отправлять ничего —
  /// панель завела бы лизу с пустым ключом на все устройства сразу.
  Map<String, String> get headers {
    if (!isKnown) return const <String, String>{};
    final name = asciiHeaderValue(displayName);
    return <String, String>{
      kDeviceIdHeader: clientDeviceId,
      if (name.isNotEmpty) kDeviceNameHeader: name,
      if (platform.isNotEmpty) kDevicePlatformHeader: platform,
    };
  }

  DeviceIdentity copyWith({String? displayName}) => DeviceIdentity(
    clientDeviceId: clientDeviceId,
    displayName: displayName ?? this.displayName,
    platform: platform,
  );
}

/// Приводит имя к значению, которое законно едет HTTP-заголовком.
///
/// Оставляем печатный ASCII, выкидываем управляющие символы (ими подделывают
/// заголовки) и всё за пределами latin-1; повторные пробелы схлопываем. Если
/// после чистки ничего не осталось (имя целиком кириллическое) — пусто, и
/// заголовок просто не отправляется: пусть панель поставит своё имя, а точное
/// придёт отдельным вызовом переименования.
String asciiHeaderValue(String raw) {
  final buf = StringBuffer();
  var lastWasSpace = false;
  for (final rune in raw.runes) {
    final isPrintable = rune >= 0x20 && rune <= 0x7e;
    if (!isPrintable) continue;
    final isSpace = rune == 0x20;
    if (isSpace && (buf.isEmpty || lastWasSpace)) continue;
    buf.writeCharCode(rune);
    lastWasSpace = isSpace;
    if (buf.length >= kDeviceNameMaxLength) break;
  }
  return buf.toString().trim();
}

/// Канон платформы для поля `platform` лизы устройства.
String devicePlatformOf({TargetPlatform? platform, bool isWeb = kIsWeb}) {
  if (isWeb) return 'web';
  switch (platform ?? defaultTargetPlatform) {
    case TargetPlatform.android:
      return 'android';
    case TargetPlatform.iOS:
      return 'ios';
    case TargetPlatform.macOS:
      return 'macos';
    case TargetPlatform.windows:
      return 'windows';
    case TargetPlatform.linux:
      return 'linux';
    case TargetPlatform.fuchsia:
      return '';
  }
}

/// Человеческое имя платформы для списка устройств.
String devicePlatformLabel(String platform) {
  switch (platform) {
    case 'android':
      return 'Android';
    case 'ios':
      return 'iPhone или iPad';
    case 'macos':
      return 'Mac';
    case 'windows':
      return 'Windows';
    case 'linux':
      return 'Linux';
    case 'web':
      return 'Браузер';
    default:
      return '';
  }
}

/// Имя устройства по умолчанию.
///
/// На десктопе берём имя хоста — человек узнаёт свой компьютер именно по нему.
/// На телефоне модели без нового плагина взять негде (и ради одной строки
/// заводить зависимость не стоит), поэтому имя собирается из платформы и
/// четырёх символов идентификатора: два Android-телефона одного аккаунта
/// обязаны отличаться в списке хоть чем-то.
String defaultDeviceName({
  required String platform,
  required String clientDeviceId,
  String? hostname,
}) {
  final host = _cleanHostname(hostname);
  if (host.isNotEmpty) return host;
  final label = devicePlatformLabel(platform);
  final base = label.isEmpty ? 'Устройство' : label;
  final suffix = clientDeviceId.replaceAll('-', '');
  if (suffix.length < 4) return base;
  return '$base (${suffix.substring(0, 4)})';
}

/// Имя хоста, пригодное для показа. Отсекаем служебные суффиксы и заглушки:
/// «localhost» на экране устройств не говорит ничего.
String _cleanHostname(String? raw) {
  var v = (raw ?? '').trim();
  if (v.isEmpty) return '';
  for (final suffix in const <String>['.local', '.lan', '.home']) {
    if (v.toLowerCase().endsWith(suffix)) {
      v = v.substring(0, v.length - suffix.length);
      break;
    }
  }
  v = v.trim();
  final lower = v.toLowerCase();
  if (lower == 'localhost' || lower == 'android' || lower == 'unknown') {
    return '';
  }
  if (v.length > kDeviceNameMaxLength) v = v.substring(0, kDeviceNameMaxLength);
  return v;
}

/// Генерирует UUID v4 строкой канонического вида.
String generateClientDeviceId({Random? random}) {
  final rnd = random ?? Random.secure();
  final bytes = List<int>.generate(16, (_) => rnd.nextInt(256));
  // Версия 4 и вариант RFC 4122 — чтобы значение было настоящим UUID, а не
  // просто 32 случайными знаками: панель складывает его в колонку uuid-вида.
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  String hex(int from, int to) => bytes
      .sublist(from, to)
      .map((b) => b.toRadixString(16).padLeft(2, '0'))
      .join();
  return '${hex(0, 4)}-${hex(4, 6)}-${hex(6, 8)}-${hex(8, 10)}-${hex(10, 16)}';
}

/// Хранилище идентичности устройства в платформенном secure storage.
///
/// НЕ КЛЮЧУЕТСЯ ПО `pid`, в отличие от [TokenStore]. Сессия принадлежит
/// оператору, а устройство — человеку: один и тот же телефон обязан остаться
/// одной и той же лизой при смене оператора, иначе лимит устройств снова
/// начнёт считать один телефон за несколько.
class DeviceIdentityStore {
  /// Ключ идентификатора. Пространство имён общее с токенами, поэтому имя
  /// несёт свой префикс `device.`.
  static const String idKey = 'caramba.device.client_id';

  /// Ключ имени устройства.
  static const String nameKey = 'caramba.device.name';

  /// Общий экземпляр: устройство одно на процесс, и читать его хранилище
  /// дважды незачем. [ApiClient] по умолчанию берёт именно его.
  static final DeviceIdentityStore instance = DeviceIdentityStore();

  final FlutterSecureStorage _storage;
  final String _platform;
  final String? _hostname;
  final Random? _random;

  /// Уже вычисленная идентичность. Держится в памяти, потому что заголовки
  /// нужны КАЖДОМУ запросу, а ходить в связку ключей на каждый запрос — это
  /// системный вызов на ровном месте.
  DeviceIdentity? _cached;

  /// Single-flight: параллельные первые запросы не должны сгенерировать два
  /// разных UUID и записать их друг поверх друга.
  Future<DeviceIdentity>? _loading;

  DeviceIdentityStore({
    FlutterSecureStorage? storage,
    String? platform,
    String? hostname,
    Random? random,
  }) : _storage = storage ?? createSecureStorage(),
       _platform = platform ?? devicePlatformOf(),
       _hostname = hostname,
       _random = random;

  /// Последняя известная идентичность без обращения к хранилищу.
  DeviceIdentity? get cached => _cached;

  /// Идентичность устройства: читает сохранённую либо создаёт и сохраняет.
  ///
  /// НИКОГДА НЕ БРОСАЕТ. Недоступная связка ключей (заблокированный экран,
  /// песочница без entitlement) не имеет права уронить запрос к панели: в этом
  /// случае возвращается идентичность, живущая только в памяти процесса, и
  /// панель просто заведёт новую лизу при следующем запуске.
  Future<DeviceIdentity> ensure() {
    final done = _cached;
    if (done != null) return Future<DeviceIdentity>.value(done);
    return _loading ??= _load().whenComplete(() => _loading = null);
  }

  Future<DeviceIdentity> _load() async {
    String? id;
    String? name;
    try {
      id = await _storage.read(key: idKey);
      name = await _storage.read(key: nameKey);
    } catch (_) {
      // Хранилище недоступно — идём дальше на памяти процесса.
    }
    final hasId = id != null && id.trim().isNotEmpty;
    final resolvedId = hasId
        ? id.trim()
        : generateClientDeviceId(random: _random);
    final hasName = name != null && name.trim().isNotEmpty;
    final resolvedName = hasName
        ? name.trim()
        : defaultDeviceName(
            platform: _platform,
            clientDeviceId: resolvedId,
            hostname: _hostname ?? _localHostname(),
          );
    if (!hasId || !hasName) {
      try {
        if (!hasId) await _storage.write(key: idKey, value: resolvedId);
        if (!hasName) await _storage.write(key: nameKey, value: resolvedName);
      } catch (_) {
        // Молча: идентичность уже собрана, следующий запуск попробует снова.
      }
    }
    final identity = DeviceIdentity(
      clientDeviceId: resolvedId,
      displayName: resolvedName,
      platform: _platform,
    );
    _cached = identity;
    return identity;
  }

  /// Локальное переименование устройства.
  ///
  /// Вызывается ПОСЛЕ успешного `PATCH /devices/{id}`: панель хранит своё имя,
  /// но заголовок следующего запроса обязан нести то же самое, иначе панель
  /// решит, что имя устройства изменилось обратно, и откатит переименование.
  /// Пустая строка сбрасывает имя на авто.
  Future<DeviceIdentity> rename(String name) async {
    final identity = await ensure();
    final trimmed = name.trim();
    final resolved = trimmed.isEmpty
        ? defaultDeviceName(
            platform: identity.platform,
            clientDeviceId: identity.clientDeviceId,
            hostname: _hostname ?? _localHostname(),
          )
        : (trimmed.length > kDeviceNameMaxLength
              ? trimmed.substring(0, kDeviceNameMaxLength)
              : trimmed);
    final next = identity.copyWith(displayName: resolved);
    _cached = next;
    try {
      await _storage.write(key: nameKey, value: resolved);
    } catch (_) {
      // Имя останется до перезапуска — это лучше, чем отказ переименования.
    }
    return next;
  }

  /// Имя хоста платформы. На web и там, где dart:io его не знает, — `null`.
  String? _localHostname() {
    if (kIsWeb) return null;
    try {
      return Platform.localHostname;
    } catch (_) {
      return null;
    }
  }
}

/// Хранилище идентичности для виджетов. Подменяется в тестах одной вставкой.
final deviceIdentityStoreProvider = Provider<DeviceIdentityStore>(
  (ref) => DeviceIdentityStore.instance,
);

/// Идентичность этого устройства для экранов: отмечает в списке строку «это
/// устройство» и даёт имя по умолчанию полю переименования.
final deviceIdentityProvider = FutureProvider<DeviceIdentity>(
  (ref) => ref.watch(deviceIdentityStoreProvider).ensure(),
);

/// Обновления приложения: своя версия, последняя версия у панели и решение,
/// что с этим делать.
///
/// ЗАЧЕМ. До этого приложение не знало даже собственной версии в рантайме, а
/// панель не знала, что вышла новая сборка. Владелец увидел следствие: люди
/// сидят на старой версии и не подозревают об этом. Теперь панель отдаёт
/// `GET /api/v2/app/version?platform=…` (манифест CI в downloads/), а это
/// состояние сравнивает номер сборки с установленной и решает: молчать,
/// предложить обновиться (баннер на «Подключении», экран «Обновления») или
/// не пускать дальше (`min_build` панели — экран «Нужно обновиться»).
///
/// Сравнивается НОМЕР СБОРКИ (`+N` из pubspec), а не строка версии: он
/// монотонный, целый и одинаков у всех платформ одного релиза. Строка версии
/// показывается человеку, но решений по ней нет.
///
/// Проверка идёт при старте (с небольшой задержкой, чтобы не толкаться с
/// восстановлением сессии) и раз в [kUpdateCheckInterval]; результат
/// ошибки не показывается на главном экране (панель могла быть просто старой
/// и не знать маршрута), только на экране «Обновления».
library;

import 'dart:async';

import 'package:flutter/widgets.dart'
    show WidgetsBinding, WidgetsFlutterBinding;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:package_info_plus/package_info_plus.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/features/updates/update_installer.dart';
import 'package:caramba_client/state/device_identity.dart';
import 'package:caramba_client/state/providers.dart';

/// Заголовок с версией установленного приложения (`1.0.0+110`). Панель пишет
/// его в лизу устройства: в кабинете и админке видно, кто на какой версии.
const String kAppVersionHeader = 'X-Caramba-App-Version';

/// Как часто перепроверять версию у панели.
const Duration kUpdateCheckInterval = Duration(hours: 6);

/// Задержка первой проверки после старта: сессия и профили в этот момент
/// ещё восстанавливаются, и лишний запрос в ту же секунду ничего не ускорит.
const Duration kUpdateFirstCheckDelay = Duration(seconds: 3);

/// Ключ prefs: сборка, которую человек отложил кнопкой «Позже».
const String kDismissedUpdateBuildKey = 'caramba.update.dismissed_build';

/// Версия установленного приложения.
class InstalledVersion {
  /// `X.Y.Z` из pubspec.
  final String version;

  /// `+N` из pubspec. Ноль — версия неизвестна (плагин недоступен, тест).
  final int build;

  const InstalledVersion({required this.version, required this.build});

  static const InstalledVersion unknown = InstalledVersion(
    version: '',
    build: 0,
  );

  bool get isKnown => build > 0;

  /// Для показа: `1.0.0 (110)`; при неизвестной версии — честное «неизвестно».
  String get label => isKnown ? '$version ($build)' : 'неизвестно';

  /// Значение заголовка [kAppVersionHeader]: `1.0.0+110`. Пусто — заголовок
  /// не отправляется.
  String get headerValue => isKnown ? '$version+$build' : '';

  /// Разбор `PackageInfo`: `buildNumber` на некоторых платформах несёт не
  /// число (macOS может отдать «1.0.0.110»), поэтому берём последнюю числовую
  /// группу, а мусор превращаем в «неизвестно», не в ноль-с-версией.
  static InstalledVersion parse({
    required String version,
    required String buildNumber,
  }) {
    final v = version.trim();
    final match = RegExp(r'(\d+)\s*$').firstMatch(buildNumber.trim());
    final build = match == null ? 0 : (int.tryParse(match.group(1)!) ?? 0);
    if (v.isEmpty || build <= 0) return InstalledVersion.unknown;
    return InstalledVersion(version: v, build: build);
  }

  static Future<InstalledVersion>? _loading;
  static InstalledVersion? _cached;

  /// Последняя прочитанная версия без обращения к платформе.
  static InstalledVersion? get cached => _cached;

  /// Подменяет версию (тесты и брендированные сборки).
  static void setForTesting(InstalledVersion? value) {
    _cached = value;
    _loading = null;
  }

  /// Версия для заголовка прямо сейчас, без ожидания платформы.
  ///
  /// Заголовок не имеет права задерживать запрос: канал `PackageInfo` в
  /// тестовом биндинге не отвечает вовсе, и `await` на нём подвешивал бы
  /// каждый вызов API. Поэтому запрос берёт то, что уже прочитано, а чтение
  /// запускается фоном: первые запросы после холодного старта могут уйти без
  /// версии, следующие её уже несут.
  static InstalledVersion current() {
    final done = _cached;
    if (done != null) return done;
    unawaited(ensure());
    return unknown;
  }

  /// Версия из платформы, один раз на процесс. НИКОГДА НЕ БРОСАЕТ: без
  /// плагина (тесты, web) возвращает [unknown], и заголовок просто не идёт.
  static Future<InstalledVersion> ensure() {
    final done = _cached;
    if (done != null) return Future<InstalledVersion>.value(done);
    return _loading ??= _load().whenComplete(() => _loading = null);
  }

  static Future<InstalledVersion> _load() async {
    try {
      final info = await PackageInfo.fromPlatform();
      final parsed = parse(
        version: info.version,
        buildNumber: info.buildNumber,
      );
      _cached = parsed;
      return parsed;
    } catch (_) {
      // Кэшировать «неизвестно» нельзя: плагин мог не успеть подняться, и
      // следующий вызов имеет право получить настоящую версию.
      return InstalledVersion.unknown;
    }
  }
}

/// Последняя версия клиента по ответу панели (`/api/v2/app/version`).
class AppVersionInfo {
  final String platform;
  final String version;
  final int build;
  final String? downloadUrl;
  final int? size;
  final String? sha256;
  final DateTime? publishedAt;

  /// Минимальная сборка панели: ниже неё приложение не пускают. Ноль — не
  /// требовать.
  final int minBuild;

  /// «Что нового» из настроек панели (может быть пустым).
  final String notes;

  const AppVersionInfo({
    required this.platform,
    required this.version,
    required this.build,
    this.downloadUrl,
    this.size,
    this.sha256,
    this.publishedAt,
    this.minBuild = 0,
    this.notes = '',
  });

  /// Ответ старой панели или пустой объект — не версия: `null`, чтобы
  /// вызывающий не сравнивал ничего с нулём.
  static AppVersionInfo? fromJson(Map<String, dynamic> json) {
    final build = _int(json['build']);
    final version = (json['version'] as String? ?? '').trim();
    if (build <= 0 || version.isEmpty) return null;
    final url = (json['download_url'] as String? ?? '').trim();
    final sha = (json['sha256'] as String? ?? '').trim();
    final publishedRaw = json['published_at'] as String?;
    return AppVersionInfo(
      platform: (json['platform'] as String? ?? '').trim(),
      version: version,
      build: build,
      downloadUrl: url.isEmpty ? null : url,
      size: json['size'] == null ? null : _int(json['size']),
      sha256: sha.isEmpty ? null : sha.toLowerCase(),
      publishedAt: publishedRaw == null
          ? null
          : DateTime.tryParse(publishedRaw)?.toLocal(),
      minBuild: _int(json['min_build']),
      notes: (json['notes'] as String? ?? '').trim(),
    );
  }

  static int _int(Object? v) {
    if (v is int) return v;
    if (v is num) return v.toInt();
    if (v is String) return int.tryParse(v.trim()) ?? 0;
    return 0;
  }

  /// Для показа: `1.0.0 (110)`.
  String get label => '$version ($build)';
}

/// Что делать с найденной версией.
enum UpdateVerdict {
  /// Обновлений нет, либо сравнивать не с чем.
  none,

  /// Есть версия новее: баннер и экран «Обновления».
  available,

  /// Установленная сборка ниже минимальной: экран «Нужно обновиться».
  required,
}

/// Чистое решение: что показывать при известных фактах.
///
/// Порядок — правила:
///   1. версии панели нет или своя версия неизвестна — молчим (сравнивать
///      неизвестное с чем угодно нельзя, а «обновитесь» без основания —
///      ложь);
///   2. своя сборка ниже минимальной — блокируем, и «Позже» тут не действует;
///   3. есть сборка новее и её не откладывали — предлагаем;
///   4. иначе молчим.
UpdateVerdict decideUpdate({
  required InstalledVersion installed,
  required AppVersionInfo? latest,
  int dismissedBuild = 0,
}) {
  if (latest == null || !installed.isKnown) return UpdateVerdict.none;
  if (latest.minBuild > 0 && installed.build < latest.minBuild) {
    return UpdateVerdict.required;
  }
  if (latest.build > installed.build && latest.build != dismissedBuild) {
    return UpdateVerdict.available;
  }
  return UpdateVerdict.none;
}

/// Снимок состояния обновлений для экранов.
class AppUpdateState {
  final InstalledVersion installed;
  final AppVersionInfo? latest;

  /// Сборка, отложенная кнопкой «Позже» (0 — ничего не откладывали).
  final int dismissedBuild;
  final bool checking;
  final DateTime? checkedAt;

  /// Текст последней ошибки проверки; на главном экране не показывается.
  final String? error;

  /// Идёт скачивание/запуск установщика (Windows) или открытие ссылки.
  final bool installing;

  /// Что сообщить после попытки установки (успех или причина отказа).
  final String? installMessage;

  const AppUpdateState({
    this.installed = InstalledVersion.unknown,
    this.latest,
    this.dismissedBuild = 0,
    this.checking = false,
    this.checkedAt,
    this.error,
    this.installing = false,
    this.installMessage,
  });

  UpdateVerdict get verdict => decideUpdate(
    installed: installed,
    latest: latest,
    dismissedBuild: dismissedBuild,
  );

  /// Версия новее установленной есть (даже если отложена «Позже»): экран
  /// «Обновления» показывает её всегда, откладывается только баннер.
  bool get hasNewer =>
      latest != null && installed.isKnown && latest!.build > installed.build;

  AppUpdateState copyWith({
    InstalledVersion? installed,
    AppVersionInfo? latest,
    bool clearLatest = false,
    int? dismissedBuild,
    bool? checking,
    DateTime? checkedAt,
    String? error,
    bool clearError = false,
    bool? installing,
    String? installMessage,
    bool clearInstallMessage = false,
  }) => AppUpdateState(
    installed: installed ?? this.installed,
    latest: clearLatest ? null : (latest ?? this.latest),
    dismissedBuild: dismissedBuild ?? this.dismissedBuild,
    checking: checking ?? this.checking,
    checkedAt: checkedAt ?? this.checkedAt,
    error: clearError ? null : (error ?? this.error),
    installing: installing ?? this.installing,
    installMessage: clearInstallMessage
        ? null
        : (installMessage ?? this.installMessage),
  );
}

/// Как приложение ставит найденное обновление. Реализация по платформам и
/// провайдер — в `features/updates/update_installer.dart`; тесты подменяют
/// провайдер, чтобы не открывать браузер и не запускать установщики.
abstract class UpdateInstaller {
  /// Возвращает текст для человека: что произошло. Бросать не должен.
  Future<String> install(AppVersionInfo info);
}

/// Периодичность фоновой перепроверки. `null` выключает таймеры (тесты).
final updateCheckIntervalProvider = Provider<Duration?>(
  (ref) => kUpdateCheckInterval,
);

/// Задержка первой проверки. `Duration.zero` — проверять сразу.
final updateFirstCheckDelayProvider = Provider<Duration>(
  (ref) => kUpdateFirstCheckDelay,
);

/// Платформа, за которую спрашиваем версию. Подменяется в тестах.
final updatePlatformProvider = Provider<String>((ref) => devicePlatformOf());

/// Таймеры фоновой проверки заводятся только в настоящем приложении.
///
/// Тестовый биндинг (`flutter_test`) — не [WidgetsFlutterBinding]; он
/// проверяет, что после теста не осталось живых таймеров, а контейнер
/// провайдеров в тестах закрывается не всегда и не вовремя. Таймер на шесть
/// часов, заведённый экраном настроек, ронял бы чужие тесты, которым до
/// обновлений нет дела. Проверку в тестах зовут явно.
bool get _timersAllowed => WidgetsBinding.instance is WidgetsFlutterBinding;

class AppUpdateNotifier extends Notifier<AppUpdateState> {
  Timer? _first;
  Timer? _periodic;
  bool _disposed = false;

  /// Single-flight: повторный вызов во время проверки ждёт её, а не
  /// запускает вторую и не возвращается раньше первой.
  Future<void>? _inFlight;

  @override
  AppUpdateState build() {
    // Смена панели (другой профиль) — другая последняя версия: состояние
    // перестраивается вместе с клиентом, старый ответ не переживает.
    ref.watch(apiClientProvider);
    final prefs = ref.watch(prefsStoreProvider);
    final dismissed =
        int.tryParse(prefs.readString(kDismissedUpdateBuildKey) ?? '') ?? 0;

    _disposed = false;
    ref.onDispose(() {
      _disposed = true;
      _first?.cancel();
      _periodic?.cancel();
    });

    final delay = ref.watch(updateFirstCheckDelayProvider);
    if (delay == Duration.zero) {
      unawaited(Future<void>.microtask(check));
    } else if (_timersAllowed) {
      _first = Timer(delay, () => unawaited(check()));
    }
    final interval = ref.watch(updateCheckIntervalProvider);
    if (interval != null && _timersAllowed) {
      _periodic = Timer.periodic(interval, (_) => unawaited(check()));
    }

    // Своя версия читается фоном и дописывается, когда платформа ответит:
    // ждать её здесь нельзя (см. [InstalledVersion.current]).
    final installed = InstalledVersion.cached;
    if (installed == null) {
      unawaited(
        InstalledVersion.ensure().then((v) {
          if (!_disposed && v.isKnown) state = state.copyWith(installed: v);
        }),
      );
    }

    return AppUpdateState(
      installed: installed ?? InstalledVersion.unknown,
      dismissedBuild: dismissed,
    );
  }

  /// Спросить панель о последней версии. Без панели (generic-режим) проверять
  /// не у кого — состояние остаётся «обновлений нет», без ошибки.
  Future<void> check() =>
      _inFlight ??= _check().whenComplete(() => _inFlight = null);

  Future<void> _check() async {
    final installed = InstalledVersion.current();
    state = state.copyWith(installed: installed, checking: true);
    final api = ref.read(apiClientProvider);
    if (!api.hasPanel) {
      state = state.copyWith(
        checking: false,
        clearLatest: true,
        clearError: true,
      );
      return;
    }
    try {
      final latest = await api.getAppVersion(ref.read(updatePlatformProvider));
      state = state.copyWith(
        checking: false,
        latest: latest,
        clearLatest: latest == null,
        checkedAt: DateTime.now(),
        clearError: true,
      );
    } catch (e) {
      state = state.copyWith(
        checking: false,
        checkedAt: DateTime.now(),
        error: e is ApiException ? e.message : 'Не удалось проверить: $e',
      );
    }
  }

  /// «Позже»: прячет баннер до следующей сборки. Обязательное обновление
  /// отложить нельзя — решение [decideUpdate] это правило и держит.
  Future<void> dismiss() async {
    final latest = state.latest;
    if (latest == null) return;
    state = state.copyWith(dismissedBuild: latest.build);
    await ref
        .read(prefsStoreProvider)
        .writeString(kDismissedUpdateBuildKey, '${latest.build}');
  }

  /// «Скачать»: ставит обновление способом платформы (см. [UpdateInstaller]).
  Future<void> install() async {
    final latest = state.latest;
    if (latest == null || state.installing) return;
    state = state.copyWith(installing: true, clearInstallMessage: true);
    final message = await ref.read(updateInstallerProvider).install(latest);
    state = state.copyWith(installing: false, installMessage: message);
  }
}

final appUpdateProvider = NotifierProvider<AppUpdateNotifier, AppUpdateState>(
  AppUpdateNotifier.new,
);

/// Нужно ли перекрыть приложение экраном «Нужно обновиться». Роутер слушает
/// именно это, а не всё состояние: перерисовывать навигацию на каждый тик
/// проверки незачем.
final updateRequiredProvider = Provider<bool>(
  (ref) => ref.watch(appUpdateProvider).verdict == UpdateVerdict.required,
);

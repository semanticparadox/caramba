/// Разбор отчёта ядра о ФАКТИЧЕСКИ применённом маршруте.
///
/// Владелец сформулировал претензию так: «настройки по типу блок рекламы или
/// стриминг непонятно работают или нет». До появления `Core.RouteReportJSON()`
/// ответить было нечем — приложение отдавало ядру идентификатор пресета и
/// больше про него ничего не слышало, поэтому единственно возможной формой
/// ответа была галочка «включено», которая означала лишь «мы попросили».
///
/// Здесь эта галочка разбирается на два разных утверждения:
///   * пресет ПРИМЕНЁН — ядро подняло туннель именно с ним;
///   * источники его правил РАЗРЕШИЛИСЬ (или не разрешились, и тогда названо,
///     какой именно список не доехал и почему).
///
/// Ни одно поле здесь не додумывается. `known: false` это «подъёма не было», а
/// не «всё хорошо»; `rules: null` это «состав правил выбрало ядро», а не ноль;
/// `geosite.state: unknown` это «проверить нечем», а не «работает». Текст
/// `detail` ядра написан по-английски для журнала и пользователю не
/// показывается — на экран идут только переводы машинных кодов.
library;

import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/vpn_state.dart';

/// Откуда ядро взяло состав правил.
enum AppliedRouteSource {
  /// Пресет из реестра ядра.
  preset,

  /// Правила пришли готовыми (`SetRouting`).
  custom,

  /// Ядро выбрало правила само: `GEOIP,private` + `MATCH`.
  coreDefault,

  /// Поля `source` в отчёте нет.
  unknown,
}

/// Судьба одного внешнего списка правил (`preset.sources[]`).
enum RuleSourceState {
  /// Список взят из проверенного локального файла.
  file,

  /// Список выпущен ссылкой на зеркало: доедет ли он, отсюда не видно.
  mirror,

  /// Список выброшен из сборки: его правил в туннеле нет.
  dropped,

  /// Незнакомое ядру состояние.
  unknown,
}

/// Состояние базы GEOSITE — той самой, без которой блок рекламы и стриминг
/// это пустые правила.
enum GeositeState {
  /// Пресету GEOSITE не нужен.
  notRequired,

  /// База проверена каталогом.
  verified,

  /// Файл на месте, но подписи под ним нет.
  present,

  /// Каталог в силе и `GeoSite.dat` в нём не назван: `geox-url` пуст, движок
  /// базу не скачает, и КАЖДОЕ правило GEOSITE заведомо мертво.
  refused,

  /// Проверить нечем.
  unknown,
}

/// Что стало со страной входа.
enum AppliedRelayState { notRequested, ignored, sent, unknown }

/// Один внешний список правил и его судьба.
class AppliedRuleSource {
  final String name;
  final RuleSourceState state;

  /// Машинный код ядра: `not_in_catalog`, `no_mirror`.
  final String? reason;

  /// Сколько правил давал бы этот список.
  final int rules;

  /// Сколько из них доехало до сборки.
  final int keptRules;

  const AppliedRuleSource({
    required this.name,
    required this.state,
    this.reason,
    this.rules = 0,
    this.keptRules = 0,
  });

  bool get isDropped => state == RuleSourceState.dropped;

  /// Русский текст судьбы списка. Английский `detail` ядра сюда не попадает
  /// намеренно: он для журнала.
  String get message => switch (state) {
    RuleSourceState.file => 'список на месте, правил: $keptRules',
    RuleSourceState.mirror =>
      'выпущен ссылкой на зеркало оператора; доехал ли он, ядро не видит',
    RuleSourceState.dropped => switch (reason) {
      'no_mirror' =>
        'выброшен: файла нет и адреса зеркала тоже, скачивать неоткуда '
            '(потеряно правил: $rules)',
      'not_in_catalog' =>
        'выброшен: каталог оператора этот список не подписал '
            '(потеряно правил: $rules)',
      _ => 'выброшен из сборки (потеряно правил: $rules)',
    },
    RuleSourceState.unknown => 'состояние списка ядру неизвестно',
  };

  static AppliedRuleSource? _parse(Object? raw) {
    if (raw is! Map) return null;
    final m = raw.cast<String, Object?>();
    final name = _str(m['name']);
    if (name.isEmpty) return null;
    return AppliedRuleSource(
      name: name,
      state: switch (_str(m['state'])) {
        'file' => RuleSourceState.file,
        'mirror' => RuleSourceState.mirror,
        'dropped' => RuleSourceState.dropped,
        _ => RuleSourceState.unknown,
      },
      reason: _strOrNull(m['reason']),
      rules: _int(m['rules']) ?? 0,
      keptRules: _int(m['kept_rules']) ?? 0,
    );
  }
}

/// Применённый пресет: то, что ядро действительно собрало.
class AppliedPreset {
  final String id;
  final String name;
  final String emoji;
  final String countryCode;

  /// Сколько правил вошло в сборку.
  final int rules;

  /// Сколько правил потеряно вместе с не доехавшими списками.
  final int droppedRules;

  /// Теги GEOSITE, на которых держится пресет.
  final List<String> geositeTags;

  final List<AppliedRuleSource> sources;

  const AppliedPreset({
    required this.id,
    required this.name,
    required this.emoji,
    required this.countryCode,
    required this.rules,
    required this.droppedRules,
    required this.geositeTags,
    required this.sources,
  });

  /// Списки, которых в сборке нет.
  List<AppliedRuleSource> get droppedSources =>
      sources.where((s) => s.isDropped).toList(growable: false);

  /// Режет ли этот пресет рекламу — по реестру ядра, а не по имени.
  /// `null` — пресета с таким id в зеркале реестра нет (ядро ушло вперёд).
  bool? get blocksAds => coreRoutePresetById(id)?.blocksAds;

  /// Уводит ли он стриминг в туннель.
  bool? get routesStreaming => coreRoutePresetById(id)?.routesStreaming;

  static AppliedPreset? _parse(Object? raw) {
    if (raw is! Map) return null;
    final m = raw.cast<String, Object?>();
    final id = _str(m['preset_id']);
    if (id.isEmpty) return null;
    return AppliedPreset(
      id: id,
      name: _str(m['preset_name']),
      emoji: _str(m['emoji']),
      countryCode: _str(m['country']),
      rules: _int(m['rules']) ?? 0,
      droppedRules: _int(m['dropped_rules']) ?? 0,
      geositeTags: _strList(m['geosite_tags']),
      sources: <AppliedRuleSource>[
        if (m['sources'] is List)
          for (final e in m['sources']! as List)
            if (AppliedRuleSource._parse(e) case final s?) s,
      ],
    );
  }
}

/// Состояние базы GEOSITE в применённой сборке.
class AppliedGeosite {
  /// Нужна ли база этому пресету вообще.
  final bool required;

  final GeositeState state;

  /// Машинный код: `geox_unmanaged`, `not_in_catalog`, `file_missing`,
  /// `not_raised`.
  final String? reason;

  final List<String> tags;

  const AppliedGeosite({
    required this.required,
    required this.state,
    this.reason,
    this.tags = const <String>[],
  });

  static const AppliedGeosite nothing = AppliedGeosite(
    required: false,
    state: GeositeState.unknown,
    reason: 'not_raised',
  );

  /// Работают ли правила GEOSITE. `true` — база проверена, `false` — она
  /// заведомо недоступна, `null` — проверить нечем.
  bool? get resolved => switch (state) {
    GeositeState.verified || GeositeState.present => true,
    GeositeState.refused => false,
    GeositeState.notRequired || GeositeState.unknown => null,
  };

  String get message => switch (state) {
    GeositeState.notRequired => 'Базе GEOSITE здесь нечего делать: правила '
        'пресета её не спрашивают.',
    GeositeState.verified => 'База GEOSITE проверена каталогом оператора.',
    GeositeState.present =>
      'Файл базы GEOSITE на месте, но подписи оператора под ним нет.',
    GeositeState.refused =>
      'Каталог оператора не назвал GeoSite.dat, адрес загрузки пуст — все '
          'правила GEOSITE в этой сборке мертвы.',
    GeositeState.unknown => switch (reason) {
      'geox_unmanaged' =>
        'Подтвердить базу GEOSITE нечем: доверенного каталога нет, и достанет '
            'ли движок свою встроенную базу — отсюда не видно.',
      'file_missing' => 'Локального файла базы GEOSITE нет.',
      'not_in_catalog' => 'Каталог оператора базу GEOSITE не называет.',
      'not_raised' => 'Туннель ещё не поднимали.',
      _ => 'Состояние базы GEOSITE ядру неизвестно.',
    },
  };

  static AppliedGeosite _parse(Object? raw) {
    if (raw is! Map) return nothing;
    final m = raw.cast<String, Object?>();
    return AppliedGeosite(
      required: m['required'] == true,
      state: switch (_str(m['state'])) {
        'not_required' => GeositeState.notRequired,
        'verified' => GeositeState.verified,
        'present' => GeositeState.present,
        'refused' => GeositeState.refused,
        _ => GeositeState.unknown,
      },
      reason: _strOrNull(m['reason']),
      tags: _strList(m['tags']),
    );
  }
}

/// Судьба страны входа в применённой сборке.
class AppliedRelay {
  /// Что просило приложение (ISO-2); пусто — не просило ничего.
  final String requested;

  final AppliedRelayState state;

  /// Нашёлся ли в применённом теле ключ `dialer-proxy` — единственный, которым
  /// mihomo строит цепочку. Это НАБЛЮДЕНИЕ над телом, а не обещание панели.
  final bool dialerProxySeen;

  /// Машинный код возможности (`raw_profile`, ...); `null` — ядро молчит.
  final String? capabilityReason;

  const AppliedRelay({
    required this.requested,
    required this.state,
    required this.dialerProxySeen,
    this.capabilityReason,
  });

  static const AppliedRelay nothing = AppliedRelay(
    requested: '',
    state: AppliedRelayState.notRequested,
    dialerProxySeen: false,
  );

  /// Цепочка подтверждена телом конфига.
  bool get chained => dialerProxySeen;

  String get message => switch (state) {
    AppliedRelayState.notRequested => 'Вход не запрашивался.',
    AppliedRelayState.ignored =>
      'Вход «$requested» этот путь выразить не может, и он отброшен.',
    AppliedRelayState.sent => dialerProxySeen
        ? 'Вход «$requested» запрошен, и цепочка в применённом конфиге есть.'
        : 'Вход «$requested» ушёл оператору, но цепочки в применённом конфиге '
              'нет: трафик идёт прямо на выход.',
    AppliedRelayState.unknown => 'Состояние входа ядру неизвестно.',
  };

  static AppliedRelay _parse(Object? raw) {
    if (raw is! Map) return nothing;
    final m = raw.cast<String, Object?>();
    final cap = m['capability'];
    return AppliedRelay(
      requested: _str(m['requested']),
      state: switch (_str(m['state'])) {
        'not_requested' => AppliedRelayState.notRequested,
        'ignored' => AppliedRelayState.ignored,
        'sent' => AppliedRelayState.sent,
        _ => AppliedRelayState.unknown,
      },
      // Молчание про ключ не значит, что он есть.
      dialerProxySeen: m['dialer_proxy_seen'] == true,
      capabilityReason: cap is Map
          ? _strOrNull(cap.cast<String, Object?>()['reason'])
          : null,
    );
  }
}

/// Полный отчёт ядра о применённом маршруте.
class AppliedRoute {
  /// Было ли вообще что применять. `false` — туннель этим экземпляром ядра ещё
  /// не поднимали, и это НЕ «всё хорошо».
  final bool known;

  /// Машинный код, почему неизвестно (`not_raised`).
  final String? reason;

  /// Держится ли туннель прямо сейчас.
  final bool tunnelUp;

  final AppliedRouteSource source;

  /// Применённый пресет; `null` — правила выбрало ядро или пришли готовыми.
  final AppliedPreset? preset;

  /// Сколько правил в сборке. `null` — состав выбрало ядро, и это ответ,
  /// отличный от нуля.
  final int? rules;

  final AppliedGeosite geosite;
  final AppliedRelay relay;

  /// Ядро этой сборки отчёт не отдаёт вовсе (пустая строка на проводе).
  final bool supported;

  const AppliedRoute({
    required this.known,
    required this.tunnelUp,
    required this.source,
    required this.geosite,
    required this.relay,
    this.reason,
    this.preset,
    this.rules,
    this.supported = true,
  });

  /// Ядро отчёта не знает: старая сборка или мост недоступен.
  static const AppliedRoute unsupported = AppliedRoute(
    known: false,
    tunnelUp: false,
    source: AppliedRouteSource.unknown,
    geosite: AppliedGeosite.nothing,
    relay: AppliedRelay.nothing,
    reason: 'no_bridge',
    supported: false,
  );

  /// Есть ли что показывать пользователю.
  bool get hasReport => supported && known;

  /// Режет ли применённая сборка рекламу. `null` — сборка про это молчит
  /// (пресета нет или он не из зеркала реестра).
  bool? get blocksAds => preset?.blocksAds;

  bool? get routesStreaming => preset?.routesStreaming;

  /// Правила, зависящие от GEOSITE, точно мертвы.
  bool get geositeDead => geosite.required && geosite.resolved == false;

  /// Списки, не доехавшие до сборки.
  List<AppliedRuleSource> get droppedSources =>
      preset?.droppedSources ?? const <AppliedRuleSource>[];

  /// Разбирает отчёт ядра. Пустая строка или мусор — [unsupported]: выдумывать
  /// здоровый отчёт нельзя, иначе экран покажет зелёную галочку там, где ядро
  /// не сказало ничего.
  factory AppliedRoute.parse(String raw) {
    if (raw.trim().isEmpty) return unsupported;
    final Object? decoded;
    try {
      decoded = jsonDecode(raw);
    } catch (_) {
      return unsupported;
    }
    if (decoded is! Map) return unsupported;
    final m = decoded.cast<String, Object?>();
    return AppliedRoute(
      known: m['known'] == true,
      reason: _strOrNull(m['reason']),
      tunnelUp: m['tunnel_up'] == true,
      source: switch (_str(m['source'])) {
        'preset' => AppliedRouteSource.preset,
        'custom' => AppliedRouteSource.custom,
        'core_default' => AppliedRouteSource.coreDefault,
        _ => AppliedRouteSource.unknown,
      },
      preset: AppliedPreset._parse(m['preset']),
      // Именно `_int`, а не `?? 0`: ноль правил и «состав выбрало ядро» —
      // разные ответы, и склеивать их значит терять единственный, ради
      // которого отчёт и появился.
      rules: _int(m['rules']),
      geosite: AppliedGeosite._parse(m['geosite']),
      relay: AppliedRelay._parse(m['relay']),
    );
  }
}

/// Отчёт ядра о применённом маршруте, перечитываемый на каждой смене стадии
/// туннеля.
///
/// Стадия здесь не ради значения, а ради зависимости: отчёт меняется ровно в
/// момент подъёма, и без подписки карточка показывала бы вечное «подъёма не
/// было» уже после подключения.
final appliedRouteProvider = FutureProvider<AppliedRoute>((ref) async {
  ref.watch(vpnProvider.select((s) => s.stage));
  try {
    return AppliedRoute.parse(await ref.watch(vpnConnectionProvider).routeReport());
  } catch (_) {
    // Мост может отсутствовать в этой сборке. Это «нечем проверить», а не
    // ошибка пользователя, и наверх идёт то же состояние, что и у пустой
    // строки.
    return AppliedRoute.unsupported;
  }
});

String _str(Object? v) => v is String ? v.trim() : '';

String? _strOrNull(Object? v) {
  final s = _str(v);
  return s.isEmpty ? null : s;
}

int? _int(Object? v) => v is num ? v.toInt() : null;

List<String> _strList(Object? v) => v is List
    ? <String>[
        for (final e in v)
          if (_str(e).isNotEmpty) _str(e),
      ]
    : const <String>[];

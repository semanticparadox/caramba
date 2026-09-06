import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/prefs_store.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/providers.dart';

/// Индекс глобального пресета `global` в [RoutingMode.defaults].
///
/// Продублирован числом намеренно: [kLegacyRouteIndexByCoreId] это карта, и
/// чтение из неё пришлось бы дополнять запасным значением — а единственное
/// «безопасное» запасное здесь это 0, то есть ровно тот российский пресет, от
/// которого мы уходим. Расхождение ловит тест, а не молчаливый `?? 0`.
const int kGlobalRouteIndex = 8;

/// Пресет маршрутизации по умолчанию для пользователя из страны [iso].
///
/// Правило: национальный пресет получает тот, кто в этой стране находится;
/// все остальные — глобальный `global` (весь трафик в туннель, напрямую только
/// локальная сеть).
///
/// Раньше умолчанием у ВСЕХ был индекс 0, то есть `ru-smart`. Для человека вне
/// России это не «настройка по вкусу», а выключенный VPN: у `ru-smart`
/// финальное действие DIRECT, и в туннель уходит только то, что заблокировано
/// в РФ. Владелец платил за туннель, туннель поднимался — и почти весь трафик
/// шёл мимо него.
///
/// [iso] == null или пусто означает «страна неизвестна». Такой пользователь
/// получает `global`, а не национальный режим какой-либо страны: угадать здесь
/// значит молча увести человека в чужой набор правил.
int defaultRouteIndexForCountry(String? iso) {
  final cc = (iso ?? '').trim().toUpperCase();
  if (cc.length != 2) return kGlobalRouteIndex;
  for (final p in kCoreRoutePresets) {
    if (p.countryCode == cc) {
      final index = kLegacyRouteIndexByCoreId[p.id];
      if (index != null) return index;
    }
  }
  return kGlobalRouteIndex;
}

/// Выбор пользователя по конфигурации ядра (caramba-core `Policy`). Один срез
/// состояния под Home config-rows и экран Настройки. Хранит индексы выбранных
/// опций в соответствующих списках; провайдеры списков ниже отдают сами опции.
///
/// Маппинг в caramba-core: protocol -> Policy.Protocol; route -> ApplyPreset;
/// relay -> SetRelay(relay_country); stack/dns/mtu/fakeip/ipv6 -> Policy.Tun/DNS;
/// split -> Policy.Split. Готовая политика собирается в
/// `core_policy_mapping.dart` и уходит ядру через `setPolicy` перед каждым
/// поднятием туннеля. Персист — [PrefsStore] (JSON под одним ключом).
class CoreConfig {
  final int protocol; // индекс в ProtocolOption.defaults (0 = Авто)
  final int route; // индекс в RoutingMode.defaults

  /// Пользователь выбирал маршрут САМ (открыл пикер и нажал), а не получил
  /// умолчание.
  ///
  /// Без этого флага «маршрут по умолчанию» и «маршрут, выбранный человеком»
  /// неразличимы: [toJson] пишет все поля сразу, поэтому у любого, кто хоть раз
  /// тронул любую настройку, в prefs лежит `route: 0` — и осознанный выбор
  /// `ru-smart`, и никогда не открывавшийся пикер выглядят одинаково.
  /// Флаг разделяет эти два случая, и только он даёт право подставить маршрут
  /// по стране: чужой явный выбор трогать нельзя.
  final bool routeChosen;
  final int relay; // индекс в Relay.defaults (0 = Выкл)
  final int stack; // индекс в CoreOption.stacks
  final int dns; // индекс в CoreOption.dns
  final int mtu; // индекс в CoreOption.mtu
  final bool fakeIp;
  final bool ipv6;
  final bool killSwitch;
  final bool autoConnect;

  // Раздельное туннелирование.
  final SplitMode splitMode;
  final Set<String> splitApps; // выбранные id приложений

  /// Домены мимо туннеля, как их ввёл пользователь (запятые/переводы строк).
  /// Сырой текст храним намеренно: он должен переживать редактирование,
  /// включая промежуточные состояния вроде висящей запятой.
  final String bypassDomains;

  /// Домены, которые в режиме «только выбранные» идут ЧЕРЕЗ туннель. Тот же
  /// сырой текст и по той же причине.
  final String allowDomains;

  /// Готовые наборы сайтов того же списка (теги GEOSITE из [kAllowSiteTags]).
  final Set<String> allowSites;

  /// Блок рекламы и трекеров поверх выбранного режима страны.
  ///
  /// Локальное поле устройства: ключа в словаре CSM у него нет, оператор его
  /// не задаёт и не видит. Уходит только в политику ядра.
  final bool blockAds;

  /// Умолчание [route] — [kGlobalRouteIndex], а НЕ 0.
  ///
  /// 0 это `ru-smart`, национальный режим одной конкретной страны. Ставить его
  /// всем — это и есть та самая ошибка: пока страна пользователя неизвестна,
  /// единственный честный ответ «вести весь трафик через туннель», а не
  /// «считать, что человек в России». Как только страна станет известна,
  /// [CoreConfigNotifier.adoptUserCountry] заменит это значение на подходящее
  /// — в том числе вернёт `ru-smart` россиянину.
  const CoreConfig({
    this.protocol = 0,
    this.route = kGlobalRouteIndex,
    this.routeChosen = false,
    this.relay = 0,
    this.stack = 0,
    this.dns = 0,
    this.mtu = 0,
    this.fakeIp = true,
    this.ipv6 = false,
    this.killSwitch = true,
    this.autoConnect = false,
    this.splitMode = SplitMode.off,
    this.splitApps = const {},
    this.bypassDomains = '',
    this.allowDomains = '',
    this.allowSites = const {},
    this.blockAds = false,
  });

  int get splitCount => splitMode == SplitMode.off ? 0 : splitApps.length;

  /// Сколько сайтов перечислено в активном списке. Ноль при выключенном
  /// режиме: список, который никуда не уходит, ничего не значит.
  int get siteRuleCount => switch (splitMode) {
    SplitMode.off => 0,
    SplitMode.onlySelected => allowDomainList.length + allowSites.length,
    SplitMode.bypassSelected => bypassDomainList.length,
  };

  /// Режим «через VPN только эти сайты» действительно включён — то есть в нём
  /// есть хотя бы одна цель. Пустой список в этом режиме увёл бы ВЕСЬ трафик
  /// мимо туннеля, поэтому он не считается включённым нигде: ни в подписи,
  /// ни в политике ядра.
  bool get allowSitesActive =>
      splitMode == SplitMode.onlySelected &&
      (allowDomainList.isNotEmpty || allowSites.isNotEmpty);

  /// Домены из [bypassDomains], разобранные по запятым/переводам строк.
  /// Пустые куски и пробелы отбрасываются.
  List<String> get bypassDomainList => _domains(bypassDomains);

  /// То же для [allowDomains].
  List<String> get allowDomainList => _domains(allowDomains);

  static List<String> _domains(String raw) => raw
      .split(RegExp(r'[,\n\r;]+'))
      .map((e) => e.trim())
      .where((e) => e.isNotEmpty)
      .toList(growable: false);

  CoreConfig copyWith({
    int? protocol,
    int? route,
    bool? routeChosen,
    int? relay,
    int? stack,
    int? dns,
    int? mtu,
    bool? fakeIp,
    bool? ipv6,
    bool? killSwitch,
    bool? autoConnect,
    SplitMode? splitMode,
    Set<String>? splitApps,
    String? bypassDomains,
    String? allowDomains,
    Set<String>? allowSites,
    bool? blockAds,
  }) => CoreConfig(
    protocol: protocol ?? this.protocol,
    route: route ?? this.route,
    routeChosen: routeChosen ?? this.routeChosen,
    relay: relay ?? this.relay,
    stack: stack ?? this.stack,
    dns: dns ?? this.dns,
    mtu: mtu ?? this.mtu,
    fakeIp: fakeIp ?? this.fakeIp,
    ipv6: ipv6 ?? this.ipv6,
    killSwitch: killSwitch ?? this.killSwitch,
    autoConnect: autoConnect ?? this.autoConnect,
    splitMode: splitMode ?? this.splitMode,
    splitApps: splitApps ?? this.splitApps,
    bypassDomains: bypassDomains ?? this.bypassDomains,
    allowDomains: allowDomains ?? this.allowDomains,
    allowSites: allowSites ?? this.allowSites,
    blockAds: blockAds ?? this.blockAds,
  );

  Map<String, dynamic> toJson() => {
    'protocol': protocol,
    'route': route,
    'route_chosen': routeChosen,
    'relay': relay,
    'stack': stack,
    'dns': dns,
    'mtu': mtu,
    'fake_ip': fakeIp,
    'ipv6': ipv6,
    'kill_switch': killSwitch,
    'auto_connect': autoConnect,
    'split_mode': splitMode.name,
    'split_apps': splitApps.toList(growable: false),
    'bypass_domains': bypassDomains,
    'allow_domains': allowDomains,
    'allow_sites': allowSites.toList(growable: false)..sort(),
    'block_ads': blockAds,
  };

  /// Читает сохранённый снимок. Каждое поле независимо падает на дефолт, так
  /// что запись, сделанная более старой версией, грузится без миграции.
  /// Индексы клампятся по длине соответствующих списков: набор опций мог
  /// сократиться между версиями, а выход за границы уронил бы экраны.
  factory CoreConfig.fromJson(Map<String, dynamic> json) {
    const d = CoreConfig();
    // Маршрут читается по СТАРОМУ умолчанию 0, а не по новому: у записи,
    // сделанной прежней версией, ключа `route_chosen` нет, и подставить туда
    // `global` значило бы молча сменить маршрут живому пользователю — в том
    // числе россиянину, у которого `ru-smart` работает правильно.
    final storedRoute = _idx(json['route'], RoutingMode.defaults.length, 0);
    return CoreConfig(
      protocol: _idx(
        json['protocol'],
        ProtocolOption.defaults.length,
        d.protocol,
      ),
      route: storedRoute,
      // Миграция записи без `route_chosen`.
      //
      // При старой схеме ненулевой индекс мог появиться ТОЛЬКО из пикера:
      // умолчанием был 0. Значит `route != 0` это доказанный выбор человека, и
      // он неприкосновенен. А `route == 0` неразличимо: это либо выбранный
      // `ru-smart`, либо никогда не тронутый пикер. Неоднозначность решается в
      // пользу страны — там, где она известна, россиянин остаётся на
      // `ru-smart`, а американец наконец уходит с него.
      //
      // До того как страна станет известна, значение остаётся прежним (0), то
      // есть апгрейд сам по себе не меняет поведение ни у кого.
      routeChosen: _bool(json['route_chosen'], storedRoute != 0),
      // Список relay приходит с панели и может быть любой длины: здесь только
      // отсекаем отрицательные значения, кламп по длине делает провайдер.
      relay: _idx(json['relay'], 1 << 20, d.relay),
      stack: _idx(json['stack'], CoreOption.stacks.length, d.stack),
      dns: _idx(json['dns'], CoreOption.dns.length, d.dns),
      mtu: _idx(json['mtu'], CoreOption.mtu.length, d.mtu),
      fakeIp: _bool(json['fake_ip'], d.fakeIp),
      ipv6: _bool(json['ipv6'], d.ipv6),
      killSwitch: _bool(json['kill_switch'], d.killSwitch),
      autoConnect: _bool(json['auto_connect'], d.autoConnect),
      splitMode: _splitMode(json['split_mode']),
      splitApps: <String>{
        ...?(json['split_apps'] as List?)?.whereType<String>(),
      },
      bypassDomains: json['bypass_domains'] is String
          ? json['bypass_domains'] as String
          : d.bypassDomains,
      allowDomains: json['allow_domains'] is String
          ? json['allow_domains'] as String
          : d.allowDomains,
      // Теги, которых эта сборка не знает, отбрасываются: ядро отвергло бы
      // патч целиком, и одна незнакомая строка из чужой версии выключила бы
      // весь список сайтов.
      allowSites: <String>{
        ...?(json['allow_sites'] as List?)?.whereType<String>().where(
          (t) => kAllowSiteTags.any((s) => s.tag == t),
        ),
      },
      blockAds: _bool(json['block_ads'], d.blockAds),
    );
  }

  /// Не-булево значение (запись чужой версии) читается как дефолт.
  static bool _bool(Object? v, bool fallback) => v is bool ? v : fallback;

  static int _idx(Object? v, int length, int fallback) {
    if (v is! num) return fallback;
    final i = v.toInt();
    if (i < 0 || i >= length) return fallback;
    return i;
  }

  static SplitMode _splitMode(Object? v) {
    for (final m in SplitMode.values) {
      if (m.name == v) return m;
    }
    return SplitMode.off;
  }
}

class CoreConfigNotifier extends StateNotifier<CoreConfig> {
  final PrefsStore? _prefs;

  CoreConfigNotifier([this._prefs]) : super(const CoreConfig());

  /// Ставит снимок, прочитанный из [PrefsStore] на старте. Не пишет обратно:
  /// это загрузка, а не пользовательская правка.
  void hydrate(CoreConfig config) => super.state = config;

  /// Любая пользовательская правка сразу уходит в prefs (write-through).
  @override
  set state(CoreConfig value) {
    super.state = value;
    unawaited(_prefs?.writeJson(PrefsStore.kCoreConfig, value.toJson()));
  }

  void setProtocol(int i) => state = state.copyWith(protocol: i);

  /// Выбор маршрута человеком. Помечает [CoreConfig.routeChosen], после чего
  /// [adoptUserCountry] к этому полю больше не прикасается — даже если
  /// выбранный режим «не подходит» стране, где человек находится. Он мог
  /// выбрать его специально.
  void setRoute(int i) => state = state.copyWith(route: i, routeChosen: true);

  /// Сообщает, в какой стране находится пользователь (ISO-2, как её увидела
  /// панель: поле `client_country` в `GET /api/v2/app/subscription` или
  /// заголовок `x-client-country` на теле подписки). `null` или пустая строка —
  /// «панель не определила», и тогда НИЧЕГО не меняется: неизвестность не повод
  /// перекладывать чужой маршрут.
  ///
  /// Меняет маршрут только пока [CoreConfig.routeChosen] == false, и не
  /// поднимает этот флаг: следующий, более точный ответ панели (человек
  /// переехал, GeoIP наконец ответил) должен уметь поправить умолчание ещё раз.
  /// Ручной выбор при этом всегда сильнее.
  void adoptUserCountry(String? iso) {
    final cc = (iso ?? '').trim();
    if (cc.length != 2) return;
    if (state.routeChosen) return;
    final next = defaultRouteIndexForCountry(cc);
    if (next == state.route) return;
    state = state.copyWith(route: next);
  }

  void setRelay(int i) => state = state.copyWith(relay: i);
  void setStack(int i) => state = state.copyWith(stack: i);
  void setDns(int i) => state = state.copyWith(dns: i);
  void setMtu(int i) => state = state.copyWith(mtu: i);
  void setFakeIp(bool v) => state = state.copyWith(fakeIp: v);
  void setIpv6(bool v) => state = state.copyWith(ipv6: v);
  void setKillSwitch(bool v) => state = state.copyWith(killSwitch: v);
  void setAutoConnect(bool v) => state = state.copyWith(autoConnect: v);

  void setSplitMode(SplitMode m) => state = state.copyWith(splitMode: m);

  void setBypassDomains(String v) => state = state.copyWith(bypassDomains: v);

  void setAllowDomains(String v) => state = state.copyWith(allowDomains: v);

  void setBlockAds(bool v) => state = state.copyWith(blockAds: v);

  /// Переключает готовый набор сайтов. Незнакомый ядру тег не принимается:
  /// ядро отвергает такой патч целиком, и молча положить его в состояние
  /// значило бы выключить весь список при следующем подъёме.
  void toggleAllowSite(String tag) {
    if (!kAllowSiteTags.any((s) => s.tag == tag)) return;
    final next = {...state.allowSites};
    if (!next.add(tag)) next.remove(tag);
    state = state.copyWith(allowSites: next);
  }

  void toggleSplitApp(String id) {
    final next = {...state.splitApps};
    if (!next.add(id)) next.remove(id);
    state = state.copyWith(splitApps: next);
  }

  /// Применяет результат автоподбора (autotune).
  void applyAutotune({int? protocol, int? stack}) {
    state = state.copyWith(protocol: protocol, stack: stack);
  }
}

final coreConfigProvider =
    StateNotifierProvider<CoreConfigNotifier, CoreConfig>(
      (ref) => CoreConfigNotifier(ref.watch(prefsStoreProvider)),
    );

/// Список протоколов (пока статичный набор caramba-core).
final protocolsProvider = Provider<List<ProtocolOption>>(
  (ref) => ProtocolOption.defaults,
);

/// Список пресетов маршрутизации.
final routingModesProvider = Provider<List<RoutingMode>>(
  (ref) => RoutingMode.defaults,
);

/// Список relay-входов для синхронного чтения (Home config-row). Берёт реальные
/// relay-страны из [apiRelaysProvider] когда они загружены, иначе
/// [Relay.defaults].
///
/// Откат на [Relay.defaults] больше не подставляет стран: там остались только
/// «Выкл» и «Авто», истинные при любом флоте. Пока панель молчит, пикер честно
/// пуст на страны — вместо трёх выдуманных, которые он показывал раньше. Сами
/// входы (узлами, а не странами) живут в слое предложения:
/// `domain/offering/offering_providers.dart`, `relayOffersProvider`.
/// Picker'ы, которым нужны loading/error, читают [apiRelaysProvider] напрямую.
final relaysProvider = Provider<List<Relay>>((ref) {
  return ref.watch(apiRelaysProvider).valueOrNull ?? Relay.defaults;
});


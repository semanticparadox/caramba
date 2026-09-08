/// Зеркало реестра пресетов маршрутизации ядра.
///
/// Оригинал — `presetList` в libs/caramba-core/routing/presets.go: девять
/// пресетов. Приложение перепечатывало пять из них своими словами, поэтому
/// «Иран», «Беларусь», «Китай» и глобальный `global` не существовали для
/// пользователя вовсе, а `ru-smart` показывался как «Россия» — без указания,
/// что это УМНЫЙ режим, а не полный обход.
///
/// Здесь ровно девять записей, `id`/`name`/`description` побайтово из реестра.
/// Плоского канала «отдай мне свои пресеты» у ядра нет: `routing.Presets()` не
/// вынесен в ABI. Поэтому это зеркало, и тест `route_presets_test.dart`
/// фиксирует состав — расхождение с ядром обязано ломать сборку, а не тихо
/// показывать несуществующий маршрут.
library;

import 'package:caramba_client/domain/offering/availability.dart';

/// Один пресет ядра. Только факты реестра, без UI-решений.
class CoreRoutePreset {
  /// `Preset.ID` — строка, которую ядро ждёт в `Policy.Preset`.
  final String id;

  /// `Preset.Name` из реестра.
  final String name;

  final String emoji;

  /// `Preset.Country`; пусто — глобальный пресет.
  final String countryCode;

  /// `Preset.Description` из реестра.
  final String description;

  /// Пресет режет рекламу: в его правилах есть `geosite:category-ads-all` с
  /// действием REJECT (`leadIn(true)`). Верно ровно для двух пресетов.
  final bool blocksAds;

  /// Пресет уводит стриминговые сервисы в туннель (`netflix`, `youtube`,
  /// `spotify`, `disney`, `openai`).
  final bool routesStreaming;

  /// Имена внешних списков, без которых пресет теряет часть правил:
  /// `Preset.Providers[].Name`. Пусто — пресету хватает встроенных geosite/geoip
  /// баз mihomo, и он работает без зеркала панели.
  final List<String> requiredRulesets;

  const CoreRoutePreset({
    required this.id,
    required this.name,
    required this.emoji,
    required this.countryCode,
    required this.description,
    this.blocksAds = false,
    this.routesStreaming = false,
    this.requiredRulesets = const <String>[],
  });

  /// Работает ли пресет целиком на встроенных базах ядра.
  bool get isSelfContained => requiredRulesets.isEmpty;

  static const Provenance origin = Provenance(
    OfferingSource.coreRegistry,
    'libs/caramba-core/routing/presets.go presetList',
  );
}

/// Все девять пресетов в порядке реестра ядра.
const List<CoreRoutePreset> kCoreRoutePresets = <CoreRoutePreset>[
  CoreRoutePreset(
    id: 'ru-smart',
    name: 'Россия (умный)',
    emoji: '🇷🇺',
    countryCode: 'RU',
    description:
        'По умолчанию напрямую. Через VPN — только заблокированные сервисы '
        '(Telegram, Instagram, X, YouTube, Discord, ChatGPT и список '
        'заблокированного в РФ). Российские сайты и банки — напрямую.',
    requiredRulesets: <String>['ru-blocked', 'ru-blocked-ip'],
  ),
  CoreRoutePreset(
    id: 'ru-full',
    name: 'Россия (полный обход)',
    emoji: '🇷🇺',
    countryCode: 'RU',
    description:
        'Весь трафик через VPN, напрямую — только российские сайты, российские '
        'IP и локальная сеть.',
  ),
  CoreRoutePreset(
    id: 'telegram-only',
    name: 'Только Telegram',
    emoji: '✈️',
    countryCode: '',
    description:
        'Через VPN идёт только Telegram (приложение + домены + IP-диапазоны). '
        'Всё остальное — напрямую.',
  ),
  CoreRoutePreset(
    id: 'ir-smart',
    name: 'Иран (умный)',
    emoji: '🇮🇷',
    countryCode: 'IR',
    description:
        'По умолчанию напрямую. Через VPN — заблокированные в Иране ресурсы. '
        'Иранские сайты и IP — напрямую.',
    requiredRulesets: <String>['ir-blocked'],
  ),
  CoreRoutePreset(
    id: 'by-smart',
    name: 'Беларусь (умный)',
    emoji: '🇧🇾',
    countryCode: 'BY',
    description:
        'По умолчанию напрямую. Через VPN — заблокированные сервисы. '
        'Белорусское и LAN — напрямую.',
    requiredRulesets: <String>['by-blocked'],
  ),
  CoreRoutePreset(
    id: 'cn-smart',
    name: 'Китай (умный)',
    emoji: '🇨🇳',
    countryCode: 'CN',
    description:
        'Весь зарубежный трафик через VPN, китайские сайты и IP — напрямую '
        '(классическая схема GFW).',
    blocksAds: true,
  ),
  CoreRoutePreset(
    id: 'streaming',
    name: 'Стриминг и AI',
    emoji: '🌍',
    countryCode: '',
    description:
        'По умолчанию напрямую. Через VPN — Netflix, YouTube, Spotify, '
        'Disney+, ChatGPT (обход гео-ограничений).',
    routesStreaming: true,
  ),
  CoreRoutePreset(
    id: 'adblock',
    name: 'Только блок рекламы',
    emoji: '🛡️',
    countryCode: '',
    description:
        'VPN не меняет маршрут трафика — только блокирует рекламу и трекеры на '
        'уровне DNS/правил.',
    blocksAds: true,
  ),
  CoreRoutePreset(
    id: 'global',
    name: 'Полный обход',
    emoji: '🌐',
    countryCode: '',
    description: 'Весь трафик через VPN, напрямую — только локальная сеть.',
  ),
];

/// Индекс пресета в устаревшем `RoutingMode.defaults`, которым до сих пор
/// адресуется сохранённое поле `CoreConfig.route`.
///
/// Список ядра и список UI совпадают по составу, но не по порядку, и порядок
/// UI трогать НЕЛЬЗЯ: `route` это сохранённый ИНДЕКС, и перестановка сдвинула бы
/// выбор живых пользователей на соседний маршрут. Поэтому четыре пресета,
/// которых в UI не было (`ir-smart`, `by-smart`, `cn-smart`, `global`),
/// дописаны в КОНЕЦ, а пять прежних остались на своих местах. `ru-full` в UI
/// исторически называется `full` — переименование живёт в `kRoutingPresetWire`.
///
/// Соответствие фиксируется тестом: разъехавшись, эта карта отправила бы
/// пользователя в другой маршрут молча.
const Map<String, int> kLegacyRouteIndexByCoreId = <String, int>{
  'ru-smart': 0,
  'telegram-only': 1,
  'ru-full': 2,
  'streaming': 3,
  'adblock': 4,
  'ir-smart': 5,
  'by-smart': 6,
  'cn-smart': 7,
  'global': 8,
};

/// Готовый набор доменов сервиса для списка «через VPN только эти сайты».
///
/// Это тег GEOSITE, а не домен: правило `GEOSITE,telegram,CARAMBA` покрывает
/// весь набор адресов сервиса, который сам по себе меняется чаще, чем выходят
/// сборки.
class SiteTag {
  /// Тег базы GeoSite.dat, он же значение на проводе (`split.allowSites`).
  final String tag;

  /// Как его называть человеку.
  final String name;

  const SiteTag(this.tag, this.name);
}

/// Закрытый словарь тегов, которые принимает ядро.
///
/// Зеркало `allowedSiteTags` из libs/caramba-core/api/policy_json.go, и
/// зеркало намеренное: ядро отвергает незнакомый тег целиком, потому что
/// mihomo на такой тег молча не сопоставит ни одного правила, и строка
/// выглядела бы включённой, ничего не включая. Порядок здесь — порядок показа,
/// а не порядок ядра; состав обязан совпадать, и это фиксирует тест.
///
/// Все теги взяты из встроенных пресетов ядра (`presets.go`), то есть уже
/// работают на живом флоте. Добавлять сюда тег «по названию сервиса» нельзя:
/// пока его нет в GeoSite.dat, это ровно та галочка, которая ничего не делает.
const List<SiteTag> kAllowSiteTags = <SiteTag>[
  SiteTag('telegram', 'Telegram'),
  SiteTag('youtube', 'YouTube'),
  SiteTag('instagram', 'Instagram'),
  SiteTag('twitter', 'X (Twitter)'),
  SiteTag('facebook', 'Facebook'),
  SiteTag('discord', 'Discord'),
  SiteTag('openai', 'ChatGPT и OpenAI'),
  SiteTag('netflix', 'Netflix'),
  SiteTag('spotify', 'Spotify'),
  SiteTag('disney', 'Disney+'),
];

/// Человеческое имя тега; сам тег — если он ядру знаком, а нам нет.
String siteTagName(String tag) {
  for (final t in kAllowSiteTags) {
    if (t.tag == tag) return t.name;
  }
  return tag;
}

/// Пресет по идентификатору ядра; `null` — такого в реестре нет.
CoreRoutePreset? coreRoutePresetById(String id) {
  for (final p in kCoreRoutePresets) {
    if (p.id == id) return p;
  }
  return null;
}

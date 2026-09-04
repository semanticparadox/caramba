/// Предложение: что этот пользователь может выбрать ПРЯМО СЕЙЧАС.
///
/// Один неизменяемый снимок на все режимы — панель и импортированная подписка
/// отвечают на одни и те же вопросы одним и тем же типом. До него каждый экран
/// сам ветвился по режиму и сам решал, что показать, когда источник промолчал;
/// расхождения между ветками и были тем, что пользователь видел как «протокол
/// это выбор восьми серверов, которых нет».
///
/// Правила слоя:
///  * ничего не выдумывать — каждое поле приходит с [Provenance];
///  * ничего не прятать — вариант, которого флот не даёт, остаётся строкой с
///    причиной;
///  * молчание источника это [OfferingStatus.unknown], а не разрешение.
library;

import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';

/// Тройка, которая и есть протокол в понимании оператора: `vless/tcp/reality` и
/// `vless/tcp/tls` — РАЗНЫЕ строки пикера. Склеивание их в слово «vless»
/// (именно так выглядит импортированное тело) предлагает выбор, который ничего
/// не выбирает.
class ProtocolKey {
  /// `vless`, `hysteria2`, `shadowsocks`, `naive`, ... в нижнем регистре.
  final String protocol;

  /// Транспорт: `tcp`, `ws`, `grpc`, `httpupgrade`, `udp`, `xhttp`.
  /// Пусто — источник транспорт не называет.
  final String transport;

  /// `reality`, `tls`, `none`. Пусто — источник TLS не называет.
  final String security;

  const ProtocolKey({
    required this.protocol,
    this.transport = '',
    this.security = '',
  });

  /// Знает ли источник форму целиком. У импортированного тела — нет.
  bool get isFullyQualified => transport.isNotEmpty && security.isNotEmpty;

  /// `vless · tcp · reality`; неизвестные части опускаются, а не выдумываются.
  String get label => <String>[
    protocol,
    transport,
    security,
  ].where((p) => p.isNotEmpty).join(' · ');

  @override
  bool operator ==(Object other) =>
      other is ProtocolKey &&
      other.protocol == protocol &&
      other.transport == transport &&
      other.security == security;

  @override
  int get hashCode => Object.hash(protocol, transport, security);

  @override
  String toString() => 'ProtocolKey($label)';
}

/// Один инбаунд конкретного узла — то, из чего состоит пикер протокола.
class InboundOffer {
  /// `inbounds.id` панели; `null` у легаси-прокси, который генератор
  /// синтезирует из колонок узла, и у всего, что пришло не из панели.
  final int? panelInboundId;

  /// Тег оператора — его собственная идентичность строки.
  final String tag;

  final ProtocolKey key;

  /// Порт; `null` — источник его не называет.
  final int? port;

  /// Короткая подпись генератора: `Stealth`, `Secure`, `Speed`, `Shadow`.
  final String label;

  /// ТОЧНОЕ имя прокси в теле конфига. Это единственный ключ, которым строка
  /// пикера связывается с выбором в селекторе ядра (`Server.ID` у Go-ядра).
  /// `null` — прокси в теле нет; тогда смотри [availability].
  final String? proxyName;

  final Availability availability;

  const InboundOffer({
    required this.tag,
    required this.key,
    required this.label,
    required this.availability,
    this.panelInboundId,
    this.port,
    this.proxyName,
  });

  bool get isAvailable => availability.isAvailable;

  Provenance get origin => availability.origin;

  @override
  bool operator ==(Object other) =>
      other is InboundOffer &&
      other.panelInboundId == panelInboundId &&
      other.tag == tag &&
      other.key == key &&
      other.port == port &&
      other.proxyName == proxyName &&
      other.availability == availability;

  @override
  int get hashCode =>
      Object.hash(panelInboundId, tag, key, port, proxyName, availability);

  @override
  String toString() =>
      'InboundOffer($tag, ${key.label}, ${availability.status.name})';
}

/// Узел выхода — «сервер» в терминах владельца. Восемь инбаундов одной машины
/// это ОДИН сервер, а не восемь.
class ExitOffer {
  /// Стабильный ключ в пределах источника: `nodes.id` строкой на панели, адрес
  /// машины в импортированном теле.
  final String key;

  /// `nodes.id`; `null` везде, кроме панели.
  final int? panelNodeId;

  /// ISO-2 в верхнем регистре; пусто — страна неизвестна.
  final String countryCode;

  /// Человекочитаемое имя страны (или сам код).
  final String countryName;

  /// Имя узла, как его назвал оператор.
  final String label;

  /// Пинг в мс: `null` — не мерили, отрицательное — таймаут.
  final int? pingMs;

  /// Загрузка 0..100; `null` — источник её не сообщает.
  final double? loadPct;

  /// Инбаунды ЭТОГО узла. Пусто вместе с [inboundsKnown] в состоянии
  /// [OfferingStatus.unknown] означает «неизвестно», а вместе с `available` —
  /// «у узла их действительно нет». Это разные вещи.
  final List<InboundOffer> inbounds;

  /// Знает ли источник инбаунды узла.
  final Availability inboundsKnown;

  /// Релэй, к которому узел привязан в панели; `null` — привязки нет.
  final RelayHopRef? viaRelay;

  final Availability availability;

  const ExitOffer({
    required this.key,
    required this.countryCode,
    required this.countryName,
    required this.label,
    required this.inbounds,
    required this.inboundsKnown,
    required this.availability,
    this.panelNodeId,
    this.pingMs,
    this.loadPct,
    this.viaRelay,
  });

  bool get isAvailable => availability.isAvailable;

  Provenance get origin => availability.origin;

  /// Инбаунды, которые действительно доедут до конфига.
  List<InboundOffer> get liveInbounds =>
      inbounds.where((i) => i.isAvailable).toList(growable: false);

  @override
  bool operator ==(Object other) =>
      other is ExitOffer && other.key == key && other.origin == origin;

  @override
  int get hashCode => Object.hash(key, origin);

  @override
  String toString() =>
      'ExitOffer($key, $countryCode, ${inbounds.length} inbounds)';
}

/// Ссылка на релэй со стороны узла выхода (`/app/servers[].via_relay`).
class RelayHopRef {
  final int nodeId;
  final String name;
  final String countryCode;

  /// Строит ли генератор, чей конфиг читает приложение, настоящую цепочку через
  /// этот релэй. На clash/mihomo — нет.
  final bool chainedInConfig;

  const RelayHopRef({
    required this.nodeId,
    required this.name,
    required this.countryCode,
    required this.chainedInConfig,
  });

  @override
  bool operator ==(Object other) =>
      other is RelayHopRef &&
      other.nodeId == nodeId &&
      other.chainedInConfig == chainedInConfig;

  @override
  int get hashCode => Object.hash(nodeId, chainedInConfig);
}

/// Вход (релэй) — как узел, а не как страна.
///
/// Владелец сформулировал это прямо: входов должно быть столько, сколько
/// релэй-нод настроено в панели. `GET /relays` агрегирует их по странам, и
/// приложение показывало одну «Россию» вместо машин. Узел появляется там, где
/// панель его называет — в `via_relay` выходов; страна без единой такой
/// привязки остаётся строкой с [OfferingReason.panelReportsRelaysByCountryOnly]
/// в [reachability], а не исчезает.
class RelayOffer {
  /// `nodes.id` релэя; `null` — панель назвала только страну.
  final int? panelNodeId;

  final String countryCode;
  final String countryName;

  /// Имя релэй-узла или имя страны, когда узел не назван.
  final String label;

  /// Сколько релэй-узлов панель насчитала в этой стране (`/relays.node_count`);
  /// `null` — счётчика нет.
  final int? nodeCountInCountry;

  /// Ключи выходов, из которых этот вход достижим. Пусто вместе с
  /// [reachability] `unknown` — «панель не говорит», а с `available` —
  /// «ни один выход через него не идёт».
  final List<String> reachableFromExitKeys;

  /// Знает ли источник, из каких выходов вход достижим.
  final Availability reachability;

  /// Можно ли им реально воспользоваться. На clash-теле — нет, и причина
  /// названа: цепочки в конфиге не появляется.
  final Availability availability;

  const RelayOffer({
    required this.countryCode,
    required this.countryName,
    required this.label,
    required this.reachableFromExitKeys,
    required this.reachability,
    required this.availability,
    this.panelNodeId,
    this.nodeCountInCountry,
  });

  bool get isAvailable => availability.isAvailable;

  Provenance get origin => availability.origin;

  @override
  bool operator ==(Object other) =>
      other is RelayOffer &&
      other.panelNodeId == panelNodeId &&
      other.countryCode == countryCode &&
      other.availability == availability;

  @override
  int get hashCode => Object.hash(panelNodeId, countryCode, availability);

  @override
  String toString() =>
      'RelayOffer(${panelNodeId ?? countryCode}, ${availability.status.name})';
}

/// Пресет маршрутизации как предложение: факт реестра ядра плюс вывод о том,
/// можно ли на него положиться в этом источнике.
class RoutePresetOffer {
  final CoreRoutePreset preset;

  /// Индекс в устаревшем `RoutingMode.defaults`, которым до сих пор адресуется
  /// `CoreConfig.route`. `null` — пресет в том списке не представлен.
  final int? legacyIndex;

  final Availability availability;

  const RoutePresetOffer({
    required this.preset,
    required this.availability,
    this.legacyIndex,
  });

  String get id => preset.id;
  String get name => preset.name;
  String get description => preset.description;
  bool get isAvailable => availability.isAvailable;
  Provenance get origin => availability.origin;

  @override
  bool operator ==(Object other) =>
      other is RoutePresetOffer &&
      other.preset.id == preset.id &&
      other.availability == availability;

  @override
  int get hashCode => Object.hash(preset.id, availability);
}

/// Возможность как таковая — и отдельно вопрос, можно ли убедиться, что она
/// сработала.
///
/// Разделение появилось из жалобы владельца «настройки по типу блок рекламы или
/// стриминг непонятно работают или нет». Ответ на неё честный только в двух
/// полях: настройка ДОСТУПНА (пресет с нужными правилами в реестре есть), а
/// ПРОВЕРИТЬ её работу нечем — ни панель, ни ядро о срабатывании правил не
/// отчитываются. Одно поле такое состояние выразить не может.
class CapabilityOffer {
  final Availability availability;

  /// Есть ли канал, по которому приложение узнаёт, что возможность работает.
  final Availability verification;

  const CapabilityOffer({
    required this.availability,
    required this.verification,
  });

  bool get isAvailable => availability.isAvailable;

  /// Приложение может подтвердить работу.
  bool get isVerifiable => verification.isAvailable;
}

/// Пять бит возможностей, которые экраны обязаны показывать, а не угадывать.
class Capabilities {
  /// Цепочка «вход → выход».
  final CapabilityOffer relayChaining;

  /// Блокировка рекламы.
  final CapabilityOffer adBlock;

  /// Разблокировка стриминга.
  final CapabilityOffer streaming;

  /// Закрепление протокола (конкретного инбаунда).
  final CapabilityOffer protocolPin;

  /// Закрепление узла выхода.
  final CapabilityOffer nodePin;

  const Capabilities({
    required this.relayChaining,
    required this.adBlock,
    required this.streaming,
    required this.protocolPin,
    required this.nodePin,
  });
}

/// Область, к которой относится список протоколов.
enum ProtocolScope {
  /// Инбаунды ОДНОГО выбранного узла — правило владельца в чистом виде.
  singleExit,

  /// Узел не закреплён: объединение по всем доступным узлам. Это ответ на
  /// вопрос «что вообще бывает», а не «что применится», и экран обязан назвать
  /// это отличие.
  wholeFleet,

  /// Перечислять нечего.
  none,
}

/// Одна строка пикера протокола.
class ProtocolRow {
  final ProtocolKey key;

  /// Подпись генератора (`Stealth`, `Speed`), когда источник её даёт.
  final String label;

  /// Узлы, на которых эта тройка встретилась.
  final List<String> exitKeys;

  /// Имена прокси в теле конфига для этих узлов — то, чем выбор закрепляется.
  final List<String> proxyNames;

  final Availability availability;

  const ProtocolRow({
    required this.key,
    required this.label,
    required this.exitKeys,
    required this.proxyNames,
    required this.availability,
  });

  bool get isAvailable => availability.isAvailable;
  Provenance get origin => availability.origin;

  @override
  bool operator ==(Object other) =>
      other is ProtocolRow &&
      other.key == key &&
      other.availability == availability;

  @override
  int get hashCode => Object.hash(key, availability);

  @override
  String toString() => 'ProtocolRow(${key.label}, ${availability.status.name})';
}

/// Список протоколов вместе с областью, к которой он относится.
class ProtocolSlate {
  final ProtocolScope scope;

  /// Узел, к которому относится список; `null` при [ProtocolScope.wholeFleet].
  final String? exitKey;

  final List<ProtocolRow> rows;

  /// Знает ли источник формы вообще. У импортированного тела — нет, и тогда
  /// каждая строка приходит `unknown`, а не исчезает.
  final Availability known;

  const ProtocolSlate({
    required this.scope,
    required this.rows,
    required this.known,
    this.exitKey,
  });

  static const ProtocolSlate empty = ProtocolSlate(
    scope: ProtocolScope.none,
    rows: <ProtocolRow>[],
    known: Availability.unavailable(
      OfferingReason.noProfile,
      Provenance.nothing,
    ),
  );
}

/// Полный снимок предложения.
class Offering {
  final OfferingSource source;

  /// Узлы выхода. Недоступные ОСТАЮТСЯ в списке с причиной.
  final List<ExitOffer> exits;

  /// Входы. Недоступные тоже остаются.
  final List<RelayOffer> relays;

  /// Девять пресетов ядра с выводом о применимости.
  final List<RoutePresetOffer> routePresets;

  final Capabilities capabilities;

  /// Закреплённый узел выхода ([ExitOffer.key]); `null` — автоподбор.
  final String? selectedExitKey;

  /// Закреплённая страна входа (ISO-2); `null` — вход не закреплён.
  final String? selectedRelayCountry;

  /// Инвентарь ещё грузится.
  final bool loading;

  /// Ошибка загрузки; `null` — ошибки нет.
  final Object? error;

  const Offering({
    required this.source,
    required this.exits,
    required this.relays,
    required this.routePresets,
    required this.capabilities,
    this.selectedExitKey,
    this.selectedRelayCountry,
    this.loading = false,
    this.error,
  });

  ExitOffer? get selectedExit => exitByKey(selectedExitKey);

  ExitOffer? exitByKey(String? key) {
    if (key == null) return null;
    for (final e in exits) {
      if (e.key == key) return e;
    }
    return null;
  }

  /// Выходы одной страны в порядке списка.
  List<ExitOffer> exitsIn(String countryCode) {
    final code = countryCode.trim().toUpperCase();
    return exits.where((e) => e.countryCode == code).toList(growable: false);
  }

  /// Страны выхода в порядке первого появления — «сервер = нода», страна лишь
  /// группирует их.
  List<String> get exitCountries {
    final out = <String>[];
    for (final e in exits) {
      if (!out.contains(e.countryCode)) out.add(e.countryCode);
    }
    return List<String>.unmodifiable(out);
  }

  RoutePresetOffer? routePresetById(String id) {
    for (final p in routePresets) {
      if (p.id == id) return p;
    }
    return null;
  }
}

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/data/models/subscription.dart' show AccessState;
import 'package:caramba_client/domain/offering/panel_fleet.dart'
    show panelRelayHopOf;
import 'package:caramba_client/state/access_guard.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/probe_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/state/subscription_state.dart';

/// Инвентарь выходов: единственный ответ на вопрос «какие страны и узлы этот
/// пользователь может выбрать прямо сейчас, а какие недоступны и почему».
///
/// Приложение живёт в трёх мирах — панель по REST, импортированная подписка без
/// панели и (позже) подписанный каталог CSM. Сегодня `ServersScreen` ветвит по
/// режиму всё своё тело; ещё один режим означал бы третью ветку. Провайдер
/// отсюда разрешает инвентарь ЗА экран, поэтому у экрана остаётся один список
/// [ExitLocation] независимо от того, откуда он взялся.
///
/// Состояние CSM здесь НЕ дублируется: инвентарь каталога приходит готовым
/// через [csmExitCatalogProvider], который заполняет слой CSM.

/// Инвентарь каталога CSM. `null` — каталог инвентарь не ведёт.
///
/// Это шов, а не источник истины: реализация живёт в слое CSM (`csm_state.dart`)
/// и переопределяет этот провайдер, когда каталог научится отдавать страны.
/// Пока он отдаёт `null`, и профиль разрешается прежними путями — панельным
/// REST или импортом; подменять рабочий список пустым каталогом было бы
/// потерей выбора, а не строгостью. Когда каталог уже ведёт инвентарь, но
/// конкретный тенант его не отдал, слой CSM возвращает отсюда
/// [ExitInventory.unavailable] с [ExitUnavailableReason.catalogNotReady] —
/// пустой список без причины не допускается ни в одном режиме.
final csmExitCatalogProvider = Provider<ExitInventory?>((ref) => null);

/// Право подключаться по подписке панели; `null` — панели за профилем нет и
/// спрашивать не у кого.
///
/// Живёт здесь, а не в экране: тот же ответ нужен и инвентарю (чем пометить
/// строки), и Home, и замеру. Второй источник этого знания разошёлся бы с
/// первым — а расхождение здесь означает «список говорит одно, кнопка делает
/// другое».
///
/// Проверка `isRaw` не косметическая: у импортированной подписки панельной
/// сессии нет вовсе, и запрос за состоянием ушёл бы в 401.
///
/// Живой отказ ([liveAccessRefusalProvider]) старше панельного снимка и потому
/// стоит первым. Снимок делается один раз при логине и после этого не
/// обновляется: он одинаково уверенно утверждал бы вчерашний отказ весь
/// сегодняшний день и не знал бы о лимите, который кончился десять минут назад.
/// А у импортированной подписки панели нет вовсе — и до живой проверки на этот
/// вопрос в её режиме не отвечал никто.
final subscriptionAccessProvider = Provider<AccessState?>((ref) {
  final live = ref.watch(liveAccessRefusalProvider);
  if (live != null) return live;
  final profile = ref.watch(activeConnectionProfileProvider);
  if (profile == null || profile.isRaw) return null;
  return ref.watch(subscriptionProvider).valueOrNull?.access;
});

/// Снимок инвентаря: страны, узлы, происхождение и текущий выбор.
class ExitInventory {
  final ExitInventorySource source;

  /// Страны, отсортированные: доступные раньше недоступных, внутри — по
  /// лучшему пингу, затем по имени. Недоступные ОСТАЮТСЯ в списке.
  final List<ExitLocation> locations;

  /// Плоский список всех узлов источника (в том числе недоступных).
  final List<ExitNode> nodes;

  /// Инвентарь ещё грузится (панельный `/servers` в полёте).
  final bool loading;

  /// Ошибка загрузки инвентаря; `null` — ошибки нет.
  final Object? error;

  /// Можно ли в этом режиме выбирать страну входа (цепочку relay) и почему нет.
  final ExitAvailability relayAvailability;

  /// Закреплённая страна выхода (ISO-2) или `null` — «авто».
  final String? selectedCountry;

  /// Закреплённый узел: `nodes.id` в панельном режиме, имя прокси в импорте.
  final String? selectedNodeKey;

  /// Состояние доступа по подписке; `null` — панели нет или доступ не ограничен
  /// ничем, о чём стоит говорить. Когда оно есть и закрыто, СПИСОК ОСТАЁТСЯ:
  /// исчерпанный трафик не делает флот оператора несуществующим, он делает его
  /// временно недоступным — и это разные новости.
  final AccessState? access;

  /// Список показан по памяти прошлой удачной выдачи.
  ///
  /// Старая панель выкидывает подписку с исчерпанной нормой из выдачи `/servers`
  /// целиком и отвечает пустым массивом. Показать вместо стран «серверов нет»
  /// значило бы соврать: серверы есть, к ним сейчас нельзя. Пока панель не
  /// научится отдавать их и в этом состоянии, показываем последний известный
  /// список, помеченный причиной и этим флагом.
  final bool remembered;

  const ExitInventory({
    required this.source,
    this.locations = const <ExitLocation>[],
    this.nodes = const <ExitNode>[],
    this.loading = false,
    this.error,
    this.relayAvailability = ExitAvailability.available,
    this.selectedCountry,
    this.selectedNodeKey,
    this.access,
    this.remembered = false,
  });

  /// Доступ закрыт и это стоит объяснить; `null` — объяснять нечего.
  AccessState? get blockedBy {
    final a = access;
    return (a != null && a.isBlocked) ? a : null;
  }

  /// Инвентаря нет и не будет, пока не изменится причина.
  factory ExitInventory.unavailable(
    ExitInventorySource source,
    ExitUnavailableReason reason, {
    String? detail,
  }) => ExitInventory(
    source: source,
    relayAvailability: ExitAvailability.unavailable(reason, detail: detail),
  );

  bool get isEmpty => locations.isEmpty;

  /// Узлы одной страны в порядке списка [nodes]. Пустой код — узлы, у которых
  /// страну вывести не удалось.
  List<ExitNode> nodesIn(String countryCode) {
    final code = normalizeCountryCode(countryCode);
    return nodes.where((n) => n.countryCode == code).toList(growable: false);
  }

  ExitLocation? locationOf(String? countryCode) {
    if (countryCode == null) return null;
    final code = normalizeCountryCode(countryCode);
    for (final l in locations) {
      if (l.countryCode == code) return l;
    }
    return null;
  }

  /// Страна текущего выбора или `null` («авто» либо страна исчезла из выдачи).
  ExitLocation? get selectedLocation => locationOf(selectedCountry);

  /// Лучшая доступная страна: та, куда автоподбор пойдёт при «авто».
  ExitLocation? get bestAvailable {
    for (final l in locations) {
      if (l.isAvailable) return l;
    }
    return null;
  }

  ExitInventory copyWith({
    List<ExitLocation>? locations,
    List<ExitNode>? nodes,
    bool? loading,
    Object? error,
    ExitAvailability? relayAvailability,
    String? selectedCountry,
    String? selectedNodeKey,
    AccessState? access,
    bool? remembered,
  }) => ExitInventory(
    source: source,
    locations: locations ?? this.locations,
    nodes: nodes ?? this.nodes,
    loading: loading ?? this.loading,
    error: error ?? this.error,
    relayAvailability: relayAvailability ?? this.relayAvailability,
    selectedCountry: selectedCountry ?? this.selectedCountry,
    selectedNodeKey: selectedNodeKey ?? this.selectedNodeKey,
    access: access ?? this.access,
    remembered: remembered ?? this.remembered,
  );
}

/// Разрешённый инвентарь для активного профиля.
///
/// Источник выбирается по профилю: каталог CSM (когда он его ведёт) → импорт →
/// панель → «профиля нет». Ни один режим не отдаёт пустоту без причины.
final exitInventoryProvider = Provider<ExitInventory>((ref) {
  final profile = ref.watch(activeConnectionProfileProvider);
  if (profile == null) {
    return ExitInventory.unavailable(
      ExitInventorySource.none,
      ExitUnavailableReason.noProfile,
    );
  }

  // Каталог CSM отдаёт готовый инвентарь: он знает и про план, и про политику
  // оператора, поэтому пересобирать его здесь нельзя — только принять.
  final catalog = ref.watch(csmExitCatalogProvider);
  if (catalog != null) return catalog;

  final selectedCountry = profile.selectedExitCountry;

  if (profile.isRaw) {
    return _importedInventory(ref, profile, selectedCountry);
  }

  // Профиль с закреплённым корнем оператора тоже приходит сюда, пока каталог
  // инвентарь не ведёт: панельный REST у него работает, и подменять рабочий
  // список причиной «каталог не готов» значило бы отнять выбор без нужды.
  return _panelInventory(ref, profile, selectedCountry);
});

/// Панельный REST: узлы из `GET /servers`, сгруппированные по стране.
ExitInventory _panelInventory(
  Ref ref,
  ConnectionProfile profile,
  String? selectedCountry,
) {
  final async = ref.watch(serversProvider);
  final servers = async.valueOrNull ?? const <Server>[];
  // Панель отдаёт СВОЁ число задержки; собственный замер накладывается сверху и
  // вытесняет его, как только придёт. Список при этом не ждёт замера — он
  // строится из ответа `/servers` и рисуется сразу.
  var nodes = _withMeasurements(
    servers.map(ExitNode.fromServer).toList(growable: false),
    ref.watch(clientLatencyProvider),
    ref.watch(probeRunProvider).measuring,
  );

  final blocked = ref.watch(subscriptionAccessProvider);
  final block = (blocked != null && blocked.isBlocked) ? blocked : null;

  // Память списка. Пока панель не отдаёт узлы пользователю с исчерпанной
  // нормой, единственный способ не соврать «серверов нет» — показать те, что
  // она отдавала минуту назад. Память заполняется ТОЛЬКО удачной выдачей и
  // читается только под закрытым доступом: в остальных случаях пустой ответ
  // панели — это ответ, а не повод показывать вчерашний день.
  final memory = ref.read(_panelFleetMemoryProvider);
  if (nodes.isNotEmpty) {
    memory.nodes = nodes;
    memory.profileId = profile.id;
  }
  var remembered = false;
  // Только СВОЙ профиль: узлы другого оператора здесь были бы не «последним
  // известным списком», а чужим флотом под чужой подпиской.
  if (nodes.isEmpty &&
      block != null &&
      memory.nodes.isNotEmpty &&
      memory.profileId == profile.id) {
    nodes = memory.nodes;
    remembered = true;
  }

  // Страны считаются по НЕпомеченным узлам: пометка убирает их из «живых», а
  // вместе с ними исчезли бы и числа задержки, которые пользователь уже видел.
  // Отказ подписки не портит измерения — он запрещает подключение.
  final locations = _group(
    nodes,
    ExitInventorySource.panelRest,
    blockedBy: block,
  );
  final selectedKey = profile.selectedExitNodeId?.toString();
  return ExitInventory(
    source: ExitInventorySource.panelRest,
    locations: locations,
    nodes: block == null ? nodes : _markBlocked(nodes, block),
    loading: async.isLoading,
    error: async.hasError ? async.error : null,
    relayAvailability: _panelRelayAvailability(servers),
    selectedCountry: selectedCountry,
    selectedNodeKey: selectedKey,
    access: blocked,
    remembered: remembered,
  );
}

/// Память последней непустой выдачи `/servers` в пределах контейнера.
///
/// Обычный объект, а не состояние провайдера: он читается и пишется во время
/// сборки инвентаря, и настоящее состояние здесь запустило бы перестроение на
/// собственную запись. Живёт ровно столько, сколько контейнер, поэтому тест
/// одного случая не подсматривает в другой.
final _panelFleetMemoryProvider = Provider<_FleetMemory>(
  (ref) => _FleetMemory(),
);

class _FleetMemory {
  List<ExitNode> nodes = const <ExitNode>[];

  /// Чей это список. Профиль сменился — память не годится.
  String? profileId;
}

/// Помечает узлы причиной отказа подписки, сохраняя уже названные причины.
///
/// Переполненный узел остаётся переполненным: своя причина точнее общей, и
/// затирать её значило бы потерять единственное, что отличает «узел занят» от
/// «у вас закончился трафик».
List<ExitNode> _markBlocked(List<ExitNode> nodes, AccessState access) {
  final reason = ExitAvailability.unavailable(
    ExitUnavailableReason.panelRejected,
    detail: access.shortReason,
  );
  return nodes
      .map(
        (n) => n.isAvailable
            ? ExitNode(
                key: n.key,
                name: n.name,
                countryCode: n.countryCode,
                source: n.source,
                flag: n.flag,
                panelNodeId: n.panelNodeId,
                pingMs: n.pingMs,
                measuredMs: n.measuredMs,
                measuring: n.measuring,
                proxyNames: n.proxyNames,
                protocol: n.protocol,
                load: n.load,
                availability: reason,
              )
            : n,
      )
      .toList(growable: false);
}

/// Работает ли выбор входа на панельном пути — по тому, что говорит сама
/// панель, а не по надежде.
///
/// Здесь стояло безусловное [ExitAvailability.available] с комментарием
/// «caramba-sub форвардит `?relay_country=` в конфиг». Форвардит — и на этом
/// всё: генератор Clash relay-ноды игнорирует, поэтому переключатель менял
/// параметр запроса и не менял на проводе ни байта. Теперь ответ приходит с
/// провода: `via_relay.chained_in_config` у узлов.
ExitAvailability _panelRelayAvailability(List<Server> servers) {
  var sawHop = false;
  for (final s in servers) {
    final hop = panelRelayHopOf(s.rawJson);
    if (hop == null) continue;
    sawHop = true;
    // Хотя бы один узел строит настоящую цепочку — выбор входа имеет смысл.
    if (hop.chainedInConfig) return ExitAvailability.available;
  }
  if (sawHop) {
    return const ExitAvailability.unavailable(
      ExitUnavailableReason.relayNotChainedByGenerator,
    );
  }
  // Ни один узел про вход не сказал: это молчание панели, а не отсутствие
  // релэев — они живут на отдельном эндпоинте, который узлов не называет.
  return const ExitAvailability.unavailable(
    ExitUnavailableReason.relaysReportedByCountryOnly,
  );
}

/// Импортированная подписка: узлы уже разобраны ядром и лежат на профиле.
ExitInventory _importedInventory(
  Ref ref,
  ConnectionProfile profile,
  String? selectedCountry,
) {
  final measured = ref.watch(clientLatencyProvider);
  final measuring = ref.watch(probeRunProvider).measuring;
  // Оператора на этом пути нет вовсе: всякое число здесь — собственный замер.
  final nodes = profile.servers
      .map(
        (s) => ExitNode.fromImported(
          s,
          measuredMs: measured[s.id],
          measuring: measuring && !measured.containsKey(s.id),
        ),
      )
      .toList(growable: false);
  return ExitInventory(
    source: ExitInventorySource.importedSub,
    locations: _group(nodes, ExitInventorySource.importedSub),
    nodes: nodes,
    // Цепочка на сыром sing-box/mihomo-конфиге не реализована. Контрол не
    // прячется: он виден выключенным с названной причиной, иначе пользователь
    // ищет несуществующую настройку.
    relayAvailability: const ExitAvailability.unavailable(
      ExitUnavailableReason.relayChainingUnsupported,
    ),
    selectedCountry: selectedCountry,
    selectedNodeKey: profile.selectedServerId,
  );
}

/// Накладывает СОБСТВЕННЫЕ замеры на узлы панели.
///
/// Ядро меряет прокси и называет их именами из тела конфига, а панель отдаёт
/// узел числом `nodes.id`. Мостом служит `inbounds[].proxy_name`, которое
/// панель объявляет побайтово совпадающим с телом; оно уже лежит в
/// [ExitNode.proxyNames].
///
/// У узла несколько прокси (по одному на инбаунд), и мерятся они по
/// отдельности. Узлу приписывается ЛУЧШИЙ ответивший: это одна машина, и
/// худший её инбаунд ничего не говорит о том, как далеко она от пользователя.
/// Если ответил ноль прокси, а померены были все — это таймаут узла (`-1`), а
/// не «не мерили»: разница видна и в цвете строки, и в сортировке.
///
/// Узел, у которого имён прокси нет вовсе (панель не прочитала инбаунды),
/// собственного замера получить не может НИКОГДА — и «меряю» ему не ставится:
/// строка, застрявшая в «меряю» навсегда, хуже честного операторского числа.
List<ExitNode> _withMeasurements(
  List<ExitNode> nodes,
  Map<String, int> byProxyName,
  bool measuring,
) {
  if (byProxyName.isEmpty && !measuring) return nodes;
  return nodes
      .map((n) {
        if (n.proxyNames.isEmpty) return n;
        int? best;
        var sawAny = false;
        for (final name in n.proxyNames) {
          final ms = byProxyName[name];
          if (ms == null) continue;
          sawAny = true;
          if (ms < 0) continue;
          if (best == null || ms < best) best = ms;
        }
        if (sawAny) return n.withMeasurement(measuredMs: best ?? -1);
        return measuring ? n.withMeasurement(measuring: true) : n;
      })
      .toList(growable: false);
}

/// Группирует узлы по стране и сортирует: доступные раньше недоступных, внутри
/// — по лучшему пингу (неизмеренные в конец), затем по имени.
List<ExitLocation> _group(
  List<ExitNode> nodes,
  ExitInventorySource source, {
  AccessState? blockedBy,
}) {
  final byCountry = <String, List<ExitNode>>{};
  for (final n in nodes) {
    byCountry.putIfAbsent(n.countryCode, () => <ExitNode>[]).add(n);
  }
  final locations = byCountry.entries
      .map((e) => ExitLocation.fromNodes(e.key, e.value, source: source))
      .toList();
  locations.sort((a, b) {
    if (a.isAvailable != b.isAvailable) return a.isAvailable ? -1 : 1;
    // Страна без имени уходит вниз своей группы: это «прочее», а не выбор.
    if (a.isUnknownCountry != b.isUnknownCountry) {
      return a.isUnknownCountry ? 1 : -1;
    }
    final pa = a.bestPingMs ?? 1 << 30;
    final pb = b.bestPingMs ?? 1 << 30;
    if (pa != pb) return pa.compareTo(pb);
    return a.displayName.compareTo(b.displayName);
  });
  if (blockedBy != null) {
    // Причина накладывается ПОСЛЕ сортировки: порядок по пингу — то немногое,
    // что у пользователя остаётся, пока он решает, платить или ждать полуночи.
    final reason = ExitAvailability.unavailable(
      ExitUnavailableReason.panelRejected,
      detail: blockedBy.shortReason,
    );
    return List<ExitLocation>.unmodifiable(
      locations.map(
        (l) => ExitLocation(
          countryCode: l.countryCode,
          displayName: l.displayName,
          nodeCount: l.nodeCount,
          source: l.source,
          flag: l.flag,
          bestLatency: l.bestLatency,
          protocols: l.protocols,
          availability: reason,
        ),
      ),
    );
  }
  return List<ExitLocation>.unmodifiable(locations);
}

/// Узлы одной страны — то, что показывает второй уровень пикера.
final exitNodesInCountryProvider = Provider.family<List<ExitNode>, String>(
  (ref, countryCode) => ref.watch(exitInventoryProvider).nodesIn(countryCode),
);

/// Закреплённая страна выхода активного профиля (`null` — «авто»).
final selectedExitCountryProvider = Provider<String?>(
  (ref) => ref.watch(activeConnectionProfileProvider)?.selectedExitCountry,
);

/// Страна текущего выбора как объект (или `null`, если выбран «авто» либо
/// закреплённая страна исчезла из выдачи — тогда UI показывает «авто»).
final selectedExitLocationProvider = Provider<ExitLocation?>(
  (ref) => ref.watch(exitInventoryProvider).selectedLocation,
);

/// Доступность цепочки через вход в текущем режиме — с причиной, когда её нет.
final relayAvailabilityProvider = Provider<ExitAvailability>(
  (ref) => ref.watch(exitInventoryProvider).relayAvailability,
);

/// Чем закончился выбор: применён ли он локально и синхронизирован ли с панелью.
///
/// Локальное применение и панельная синхронизация — разные события: выбор
/// сохраняется на профиле всегда, а панель может быть недоступна или отклонить
/// его. Экран показывает [sync] как причину, а не как ошибку.
class ExitSelectionOutcome {
  /// Выбор сохранён на профиле и виден провайдерам.
  final bool applied;

  /// Что закрепила панель; `null` — синхронизации не было.
  final ExitSelection? resolved;

  /// Состояние синхронизации: доступно — панель приняла; иначе причина.
  final ExitAvailability sync;

  const ExitSelectionOutcome({
    required this.applied,
    required this.sync,
    this.resolved,
  });

  bool get syncedWithPanel => resolved != null;
}

/// Мутации выбора: страна, узел, вход. Единственная точка, где выбор попадает
/// одновременно в профиль (переживает перезапуск), в панель (закрепляется на
/// подписке) и в [selectedServerProvider] (его читает connect).
class ExitSelectionController {
  final Ref _ref;

  const ExitSelectionController(this._ref);

  ConnectionProfile? get _profile => _ref.read(activeConnectionProfileProvider);

  ConnectionProfilesNotifier get _profiles =>
      _ref.read(connectionProfilesProvider.notifier);

  /// Закрепляет страну выхода. `null` — «авто».
  ///
  /// Страна ни в одном режиме не доезжает до подключения сама: панель знает
  /// `node_id`, ядро на сыром пути — имя прокси, и страны нет в контракте ни у
  /// того, ни у другого. Поэтому страна разрешается в КОНКРЕТНЫЙ узел здесь,
  /// где виден инвентарь. Локально страна тоже сохраняется — по ней автоподбор
  /// найдёт замену, когда закреплённый узел уйдёт из выдачи.
  ///
  /// Два исхода, которые раньше были одним и тем же и потому ломали живую
  /// подписку, теперь разведены:
  ///   * `null` — пользователь СОЗНАТЕЛЬНО просит дефолт оператора: пин
  ///     снимается локально и сбрасывается на панели;
  ///   * страна названа, но узла в ней не нашлось — это ОШИБКА выбора. Пин не
  ///     трогается вовсе: `node_id: null` панель читает как «вернуть дефолт» и
  ///     стирает закреплённый на подписке узел, то есть выдаёт наш сбой за
  ///     намерение пользователя.
  Future<ExitSelectionOutcome> selectCountry(String? countryCode) async {
    final profile = _profile;
    if (profile == null) {
      return const ExitSelectionOutcome(
        applied: false,
        sync: ExitAvailability.unavailable(ExitUnavailableReason.noProfile),
      );
    }
    final code = normalizeCountryCode(countryCode);
    final country = code.isEmpty ? null : code;

    // Узел ищем ДО записи страны: не найдя его, мы не должны оставить профиль в
    // состоянии «страна выбрана, а подключаться некуда».
    final node = country == null ? null : _bestNodeIn(country);
    // Панельному режиму мало самого узла: закрепить он умеет только `node_id`,
    // и узел каталога без него так же неразрешим, как отсутствующий.
    final unresolved =
        country != null &&
        (node == null || (profile.isPanel && node.panelNodeId == null));
    if (unresolved) {
      return ExitSelectionOutcome(
        applied: false,
        sync: ExitAvailability.unavailable(
          ExitUnavailableReason.allNodesBusy,
          detail: countryNameOf(country),
        ),
      );
    }

    await _profiles.setSelectedExitCountry(profile.id, country);

    if (!profile.isPanel) {
      // Импорт: панели нет, но пин обязан быть конкретным. `connectRaw` читает
      // ТОЛЬКО `selectedServerId`, и страна без имени прокси означала бы
      // галочку «Германия» при выходе через Канаду.
      _ref.read(selectedServerProvider.notifier).clear();
      await _profiles.setSelectedServer(profile.id, node?.key);
      return const ExitSelectionOutcome(
        applied: true,
        sync: ExitAvailability.unavailable(ExitUnavailableReason.panelRequired),
      );
    }

    if (node?.panelNodeId != null) {
      await _profiles.setSelectedExitNode(profile.id, node!.panelNodeId);
    }
    _syncSelectedServer(node);
    final id = node?.panelNodeId;
    final sync = await _pushToPanel(
      nodeId: id == null
          // Сюда попадает только «Авто»: страна без узла вернулась выше.
          ? const SelectionField<int>.reset()
          : SelectionField<int>.of(id),
    );
    return ExitSelectionOutcome(
      applied: true,
      resolved: sync.$2,
      sync: sync.$1,
    );
  }

  /// Закрепляет конкретный узел. `null` — снять пин (автоподбор).
  ///
  /// Ключ узла разный по режимам, поэтому берём сам [ExitNode]: у панели пин
  /// уходит в `selected_exit_node_id` и в панель, у импорта — в
  /// `selected_server_id`, который ядро отдаёт селектору CARAMBA.
  Future<ExitSelectionOutcome> selectNode(ExitNode? node) async {
    final profile = _profile;
    if (profile == null) {
      return const ExitSelectionOutcome(
        applied: false,
        sync: ExitAvailability.unavailable(ExitUnavailableReason.noProfile),
      );
    }
    if (node != null && !node.isAvailable) {
      // Недоступный узел показывается, но не выбирается: причина уже названа
      // на самом узле, и подменять её собственной было бы враньём.
      return ExitSelectionOutcome(applied: false, sync: node.availability);
    }

    if (!profile.isPanel) {
      await _profiles.setSelectedServer(profile.id, node?.key);
      if (node != null) {
        await _profiles.setSelectedExitCountry(profile.id, node.countryCode);
        // setSelectedExitCountry снимает пины, противоречащие стране: у узла
        // этой же страны пин восстанавливаем сразу после.
        await _profiles.setSelectedServer(profile.id, node.key);
      }
      return const ExitSelectionOutcome(
        applied: true,
        sync: ExitAvailability.unavailable(ExitUnavailableReason.panelRequired),
      );
    }

    await _profiles.setSelectedExitNode(profile.id, node?.panelNodeId);
    if (node != null && node.countryCode.isNotEmpty) {
      await _profiles.setSelectedExitCountry(profile.id, node.countryCode);
      await _profiles.setSelectedExitNode(profile.id, node.panelNodeId);
    }
    _syncSelectedServer(node);
    final sync = await _pushToPanel(
      nodeId: SelectionField<int>.orReset(node?.panelNodeId),
    );
    return ExitSelectionOutcome(
      applied: true,
      resolved: sync.$2,
      sync: sync.$1,
    );
  }

  /// Закрепляет страну входа (цепочку). `null` — вернуть панели дефолт.
  ///
  /// Локально ничего не пишем: вход ведёт `CoreConfig.relay`, и второй его
  /// владелец здесь означал бы два расходящихся значения одной настройки.
  Future<ExitSelectionOutcome> selectRelayCountry(String? relayCountry) async {
    final profile = _profile;
    if (profile == null) {
      return const ExitSelectionOutcome(
        applied: false,
        sync: ExitAvailability.unavailable(ExitUnavailableReason.noProfile),
      );
    }
    final relay = _ref.read(exitInventoryProvider).relayAvailability;
    if (!relay.isAvailable) {
      return ExitSelectionOutcome(applied: false, sync: relay);
    }
    final code = normalizeCountryCode(relayCountry);
    final sync = await _pushToPanel(
      relayCountry: code.isEmpty
          ? const SelectionField<String>.reset()
          : SelectionField<String>.of(code),
    );
    return ExitSelectionOutcome(
      applied: sync.$1.isAvailable,
      resolved: sync.$2,
      sync: sync.$1,
    );
  }

  /// Лучший доступный узел страны; `null` — страна «авто» или пуста.
  ExitNode? _bestNodeIn(String? countryCode) {
    if (countryCode == null) return null;
    final nodes = _ref
        .read(exitInventoryProvider)
        .nodesIn(countryCode)
        .where((n) => n.isAvailable)
        .toList();
    if (nodes.isEmpty) return null;
    nodes.sort((a, b) {
      final pa = (a.pingMs == null || a.pingMs! < 0) ? 1 << 30 : a.pingMs!;
      final pb = (b.pingMs == null || b.pingMs! < 0) ? 1 << 30 : b.pingMs!;
      return pa.compareTo(pb);
    });
    return nodes.first;
  }

  /// Держит [selectedServerProvider] в согласии с выбором: именно его читает
  /// connect, и разъехавшись, он подключал бы не туда, куда показывает UI.
  void _syncSelectedServer(ExitNode? node) {
    final notifier = _ref.read(selectedServerProvider.notifier);
    if (node?.panelNodeId == null) {
      notifier.clear();
      return;
    }
    final servers = _ref.read(serversProvider).valueOrNull ?? const <Server>[];
    for (final s in servers) {
      if (s.id == node!.panelNodeId) {
        notifier.select(s);
        return;
      }
    }
    notifier.clear();
  }

  /// Отправляет выбор на панель. Возвращает причину вместо исключения: панель
  /// может быть не подключена, и это состояние режима, а не сбой запроса.
  Future<(ExitAvailability, ExitSelection?)> _pushToPanel({
    SelectionField<int> nodeId = const SelectionField<int>.unchanged(),
    SelectionField<String> relayCountry =
        const SelectionField<String>.unchanged(),
  }) async {
    final api = _ref.read(apiClientProvider);
    if (!api.hasPanel) {
      return (
        const ExitAvailability.unavailable(ExitUnavailableReason.panelRequired),
        null,
      );
    }
    final sub = _ref.read(subscriptionProvider).valueOrNull;
    if (sub == null) {
      return (
        const ExitAvailability.unavailable(
          ExitUnavailableReason.panelUnavailable,
        ),
        null,
      );
    }
    try {
      final resolved = await api.putSubscriptionSelection(
        subscriptionId: sub.id,
        nodeId: nodeId,
        relayCountry: relayCountry,
      );
      return (ExitAvailability.available, resolved);
    } on ApiNotAvailableException {
      return (
        const ExitAvailability.unavailable(ExitUnavailableReason.panelRequired),
        null,
      );
    } on ApiException catch (e) {
      return (
        ExitAvailability.unavailable(
          ExitUnavailableReason.panelRejected,
          detail: e.message,
        ),
        null,
      );
    } catch (e) {
      return (
        ExitAvailability.unavailable(
          ExitUnavailableReason.panelUnavailable,
          detail: e.toString(),
        ),
        null,
      );
    }
  }
}

final exitSelectionControllerProvider = Provider<ExitSelectionController>(
  ExitSelectionController.new,
);

import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/domain/offering/panel_fleet.dart'
    show PanelInboundsRead;
import 'package:caramba_client/vpn/vpn_models.dart';

/// Страна выхода как ТИП, а не как глиф флага.
///
/// До этого файла страна существовала в приложении только как двухбуквенный
/// код внутри `Server.countryCode` / `ImportedServer.country` и рисовалась
/// чипом. Пикер стране привязаться не мог: не к чему было. Здесь страна
/// становится сущностью с именем, числом узлов, лучшим пингом, набором
/// протоколов и — главное — доступностью С ПРИЧИНОЙ.
///
/// Три мира дают инвентарь по-разному ([ExitInventorySource]), но экран обязан
/// видеть один и тот же тип, иначе `ServersScreen` разветвится в третий раз.

/// Откуда взят инвентарь выходов.
///
/// Различие важно не для красоты: от источника зависит, какие возможности
/// вообще существуют (цепочка через relay есть на панельном REST и нет на
/// импортированном конфиге), и какой ключ у узла.
enum ExitInventorySource {
  /// Панель по REST (`GET /servers`, `?node_id=` / `?relay_country=`).
  panelRest,

  /// Импортированная подписка: узлы разобраны ядром, панели нет вообще.
  importedSub,

  /// Подписанный каталог CSM. Инвентарь поставляет CSM-слой; здесь только
  /// маркер, чтобы UI знал происхождение.
  csmCatalog,

  /// Профиля нет — выбирать не из чего.
  none,
}

/// Почему вариант недоступен.
///
/// Домашнее правило приложения: недоступный вариант ВИДЕН и объяснён, а не
/// спрятан. Поэтому доступность — не голый bool: у неё есть машиночитаемая
/// причина, по которой UI сам решает, что показать (подсказку, кнопку
/// «обновить» или ссылку на импорт).
enum ExitUnavailableReason {
  /// Узел не принимает новые подключения (`Server.isSelectable == false`).
  nodeFull,

  /// Узел не в сети. Панельный REST этой причины не порождает — статусы там
  /// толкует один [Server.isSelectable]; её ставит источник, который про
  /// состояние узла знает больше (каталог CSM).
  nodeOffline,

  /// В стране есть узлы, но ни один сейчас не принимает подключения.
  allNodesBusy,

  /// Источник не сообщил ни одного узла (пустая подписка, пустой ответ).
  sourceEmpty,

  /// Цепочка через вход не реализована на импортированном/сыром пути sing-box.
  /// Контрол показывается выключенным с этой причиной, а не прячется.
  relayChainingUnsupported,

  /// Релэй у узла в панели назначен, а цепочки в конфиге нет: генератор Clash,
  /// чьё тело и потребляет ядро, игнорирует relay-ноды — он не выпускает ни
  /// `dialer-proxy`, ни группы `type: relay`, и в `server:` остаётся адрес
  /// выхода. Суффикс `↪` в имени прокси и группа `Auto-Relay` — ярлык.
  ///
  /// Причина отдельная от [relayChainingUnsupported]: там источник цепочку
  /// выразить не может в принципе, здесь оператор её настроил, а до провода она
  /// не доехала. Пользователю это разные новости.
  relayNotChainedByGenerator,

  /// `GET /relays` называет только страны входа; какие узлы за ними стоят и
  /// строится ли через них цепочка, панель на этом эндпоинте не сообщает.
  relaysReportedByCountryOnly,

  /// Каталог CSM ещё не ведёт инвентарь выходов.
  catalogNotReady,

  /// Профиль подключения не выбран.
  noProfile,

  /// Действие имеет смысл только с подключённой панелью.
  panelRequired,

  /// Панель есть, но вызов сейчас невозможен (нет origin/подписки/сети).
  panelUnavailable,

  /// Панель ответила отказом; текст в [ExitAvailability.detail].
  panelRejected,
}

/// Доступность варианта: либо доступен, либо недоступен С ПРИЧИНОЙ.
class ExitAvailability {
  /// `null` — вариант доступен.
  final ExitUnavailableReason? reason;

  /// Уточнение к причине (имя страны, текст отказа панели). Может быть `null`.
  final String? detail;

  const ExitAvailability._(this.reason, this.detail);

  /// Доступен.
  static const ExitAvailability available = ExitAvailability._(null, null);

  const ExitAvailability.unavailable(
    ExitUnavailableReason reason, {
    String? detail,
  }) : this._(reason, detail);

  bool get isAvailable => reason == null;

  /// Готовый текст для UI. Экран волен показать своё, но по умолчанию причина
  /// уже объяснена здесь — чтобы «недоступно» никогда не осталось без ответа
  /// на вопрос «почему».
  String get message {
    switch (reason) {
      case null:
        return 'Доступно';
      case ExitUnavailableReason.nodeFull:
        return 'Узел переполнен и не принимает новые подключения.';
      case ExitUnavailableReason.nodeOffline:
        return 'Узел не в сети.';
      case ExitUnavailableReason.allNodesBusy:
        return 'Все узлы страны сейчас недоступны.';
      case ExitUnavailableReason.sourceEmpty:
        return 'Источник не сообщил ни одного узла.';
      case ExitUnavailableReason.relayChainingUnsupported:
        return 'Вход через другую страну недоступен для импортированной '
            'подписки: цепочку такой конфиг выразить не может.';
      case ExitUnavailableReason.relayNotChainedByGenerator:
        return 'Оператор назначил узлам вход, но в конфиг, который читает '
            'приложение, цепочка не попадает: трафик идёт прямо на выход.';
      case ExitUnavailableReason.relaysReportedByCountryOnly:
        return 'Панель называет входы только странами и не сообщает, строится '
            'ли через них цепочка.';
      case ExitUnavailableReason.catalogNotReady:
        return 'Каталог оператора ещё не передаёт список стран.';
      case ExitUnavailableReason.noProfile:
        return 'Профиль подключения не выбран.';
      case ExitUnavailableReason.panelRequired:
        return 'Доступно только с подключённой панелью.';
      case ExitUnavailableReason.panelUnavailable:
        return 'Панель сейчас недоступна, выбор сохранён локально.';
      case ExitUnavailableReason.panelRejected:
        return detail?.isNotEmpty == true
            ? 'Панель отклонила выбор: $detail'
            : 'Панель отклонила выбор.';
    }
  }

  @override
  bool operator ==(Object other) =>
      other is ExitAvailability &&
      other.reason == reason &&
      other.detail == detail;

  @override
  int get hashCode => Object.hash(reason, detail);

  @override
  String toString() =>
      isAvailable ? 'ExitAvailability.available' : 'ExitAvailability($reason)';
}

/// Кто померил задержку. Числа мало: у одного и того же «42 мс» бывают разные
/// авторы, и пользователь спрашивал не про них обоих.
enum LatencySource {
  /// Никто. Показывается прочерк, не ноль и не чужое число.
  none,

  /// Замер идёт прямо сейчас; своего результата ещё нет.
  measuring,

  /// Число ОПЕРАТОРА: `nodes.last_latency` из панели. Это RTT узла до его
  /// собственной цели по heartbeat (~30 с) — расстояние УЗЛА до цели, а не
  /// расстояние пользователя до узла. Показывать его можно, выдавать за
  /// пользовательский пинг — нет.
  operator,

  /// Замер ЭТОГО устройства (ABI v2 `probe`): приложение само соединилось с
  /// узлом. Это и есть то, что просили показывать.
  client,
}

/// Задержка вместе с тем, кто её померил.
///
/// Раньше в модели лежало голое `pingMs`, и панельное число попадало в строку
/// списка как «пинг» — при том что панель меряет расстояние узла до цели, а
/// не пользователя до узла. Тип существует, чтобы такое число нельзя было
/// показать, не назвав его источник.
class Latency {
  final LatencySource source;

  /// Миллисекунды; `null` — числа нет (нет замера / идёт замер).
  /// Отрицательное значение — таймаут (контракт ABI v2: `-1`).
  final int? ms;

  const Latency._(this.source, this.ms);

  /// Никто не мерил.
  static const Latency none = Latency._(LatencySource.none, null);

  /// Замер в полёте.
  static const Latency measuring = Latency._(LatencySource.measuring, null);

  /// Число панели.
  const Latency.fromOperator(int this.ms) : source = LatencySource.operator;

  /// Число собственного замера.
  const Latency.fromClient(int this.ms) : source = LatencySource.client;

  bool get hasValue => ms != null;

  /// Узел не ответил в пределах таймаута (в отличие от «не мерили»).
  bool get isTimeout => ms != null && ms! < 0;

  /// Бакет для цвета; `null` — числа нет, красить нечего.
  PingBucket? get bucket => ms == null ? null : pingBucketOf(ms);

  /// Число для сортировки: чем меньше, тем выше. Без числа и на таймауте —
  /// в конец, но не «ноль», который отсортировался бы первым.
  int get sortKey => (ms == null || ms! < 0) ? 1 << 30 : ms!;

  @override
  bool operator ==(Object other) =>
      other is Latency && other.source == source && other.ms == ms;

  @override
  int get hashCode => Object.hash(source, ms);

  @override
  String toString() => 'Latency(${source.name}, $ms)';
}

/// Один узел выхода, приведённый к общему виду из любого источника.
///
/// Ключ узла разный по природе: у панели это `nodes.id` (число), у импорта —
/// имя прокси (строка). [key] несёт единый строковый ключ для UI, [panelNodeId]
/// заполнен только там, где число вообще существует.
class ExitNode {
  /// Стабильный ключ в пределах источника.
  final String key;

  /// `nodes.id` — только [ExitInventorySource.panelRest]; иначе `null`.
  final int? panelNodeId;

  final String name;

  /// ISO-2 в верхнем регистре; пусто — страна неизвестна.
  final String countryCode;

  /// Глиф флага ДЛЯ ПОКАЗА, уже прошедший `flagOf`/`flagOfGuessedCountry`.
  /// [kNeutralFlag] означает «страна не названа твёрдо», и это не дефект.
  final String flag;

  /// Пинг оператора в мс: `null` — панель не сообщила, отрицательное — таймаут.
  /// Это НЕ пользовательский пинг, см. [LatencySource.operator].
  final int? pingMs;

  /// Задержка, померенная САМИМ приложением; `null` — своего замера ещё нет.
  final int? measuredMs;

  /// Идёт ли замер этого узла прямо сейчас. Влияет только на показ: список
  /// рисуется сразу, а «меряю» — состояние строки, а не всего экрана.
  final bool measuring;

  /// Имена прокси, которыми узел представлен в теле конфига, — ключи, под
  /// которыми ядро возвращает результат замера.
  ///
  /// Панель отдаёт узел числом (`nodes.id`), а ядро меряет ПРОКСИ и называет
  /// их именами из тела конфига. Без этого списка результат замера некуда
  /// положить: две половины одного узла не знают друг о друге. Имена приходят
  /// из `inbounds[].proxy_name` — того самого поля, которое панель объявляет
  /// побайтово совпадающим с телом.
  final List<String> proxyNames;

  /// Протокол/тип outbound'а (`vless`, `hysteria2`, ...). Пусто — источник его
  /// не сообщает (панельный `/servers` протоколы не отдаёт).
  final String protocol;

  /// Загрузка 0..100; у импорта всегда 0 (неизвестна).
  final double load;

  final ExitInventorySource source;

  final ExitAvailability availability;

  const ExitNode({
    required this.key,
    required this.name,
    required this.countryCode,
    required this.source,
    this.flag = kNeutralFlag,
    this.panelNodeId,
    this.pingMs,
    this.measuredMs,
    this.measuring = false,
    this.proxyNames = const <String>[],
    this.protocol = '',
    this.load = 0,
    this.availability = ExitAvailability.available,
  });

  bool get isAvailable => availability.isAvailable;

  PingBucket get pingBucket => pingBucketOf(pingMs);

  /// Что показать в строке — с именем автора числа.
  ///
  /// Порядок предпочтения и есть ответ на жалобу: собственный замер вытесняет
  /// операторский, как только приходит. До этого строка не пустует и не врёт —
  /// она показывает число панели, НАЗЫВАЯ его операторским.
  ///
  /// «Меряю» стоит НИЖЕ операторского числа намеренно: заменить показанное
  /// число крутилкой значит на время замера отнять у пользователя то немногое,
  /// что у него было. Крутилка достаётся тем строкам, где числа нет вовсе, — а
  /// то, что замер идёт, экран говорит и своей шапкой.
  Latency get latency {
    final own = measuredMs;
    if (own != null) return Latency.fromClient(own);
    final panel = pingMs;
    if (panel != null) return Latency.fromOperator(panel);
    if (measuring) return Latency.measuring;
    return Latency.none;
  }

  /// Тот же узел с наложенным результатом собственного замера.
  ExitNode withMeasurement({int? measuredMs, bool? measuring}) => ExitNode(
    key: key,
    name: name,
    countryCode: countryCode,
    source: source,
    flag: flag,
    panelNodeId: panelNodeId,
    pingMs: pingMs,
    measuredMs: measuredMs ?? this.measuredMs,
    measuring: measuring ?? this.measuring,
    proxyNames: proxyNames,
    protocol: protocol,
    load: load,
    availability: availability,
  );

  /// Узел панели. Пригодность узла НЕ переопределяется здесь: её решает
  /// [Server.isSelectable] — тот же предикат, по которому узел выбирает путь
  /// подключения.
  ///
  /// Раньше пикер вёл собственный белый список статусов (`online`/`busy`), и
  /// живая панель, присылающая `active`, делала недоступной каждую страну:
  /// список рисовался, но не нажимался ни в одной строке, кроме «Авто». Второй
  /// словарь статусов — это гарантированное расхождение между тем, что показано,
  /// и тем, к чему приложение может подключиться; поэтому словарь остался один.
  factory ExitNode.fromServer(Server s) {
    final availability = s.isSelectable
        ? ExitAvailability.available
        : ExitAvailability.unavailable(
            ExitUnavailableReason.nodeFull,
            detail: s.status,
          );
    // Панель отдаёт инбаунды на том же `/servers`, и молчать о них здесь
    // значило бы оставить панельный режим «источником, который протоколы не
    // сообщает» — тем самым, из-за которого пикер показывал весь список ядра,
    // ничего не проверив. Тройки склеиваются в одну строку намеренно: разбор
    // формы принадлежит слою предложения (`domain/offering`), а этому полю
    // достаточно нести токены узла.
    final read = PanelInboundsRead.fromRow(s.rawJson);
    final protocol = read.known.isAvailable
        ? read.rows
              .where((r) => r.available)
              .map(
                (r) => <String>[
                  r.protocol,
                  r.network,
                  r.security,
                ].where((p) => p.isNotEmpty && p != 'none').join('+'),
              )
              .where((t) => t.isNotEmpty)
              .join(' ')
        // Панель инбаунды не прочитала — это молчание, и пустая строка здесь
        // означает именно его.
        : '';
    return ExitNode(
      key: s.id.toString(),
      panelNodeId: s.id,
      name: s.name,
      countryCode: normalizeCountryCode(s.countryCode),
      // Флаг панели проходит через flagOf вместе с кодом: сам по себе он не
      // годится (для узла без страны панель присылает уверенное 🇺🇸).
      flag: flagOf(s.countryCode, s.flag),
      pingMs: s.pingMs,
      // Ключи, под которыми ядро вернёт замер этого узла. Только доступные:
      // прокси, которого в теле конфига нет, ядро и не померит.
      proxyNames: read.rows
          .where((r) => r.available)
          .map((r) => r.proxyName ?? '')
          .where((n) => n.isNotEmpty)
          .toSet()
          .toList(growable: false),
      load: s.load,
      protocol: protocol,
      source: ExitInventorySource.panelRest,
      availability: availability,
    );
  }

  /// Узел импортированной подписки. Статуса у него нет: ядро отдаёт то, что
  /// разобрало, и недоступным узел делает только пустой инвентарь страны.
  /// Таймаут пинга (`-1`) — это качество, а не недоступность.
  /// Задержка здесь приходит СОБСТВЕННЫМ замером ядра ([measuredMs]), а не от
  /// оператора: на этом пути панели нет вовсе и чужого числа взяться неоткуда.
  factory ExitNode.fromImported(
    ImportedServer s, {
    int? measuredMs,
    bool measuring = false,
  }) => ExitNode(
    key: s.id,
    name: s.name.isNotEmpty ? s.name : s.id,
    countryCode: normalizeCountryCode(s.country),
    // Страна здесь ДОГАДКА ядра по имени прокси, и флаг ставится только на
    // твёрдую её половину — ту, где оператор написал флаг сам.
    flag: flagOfGuessedCountry(s.country, s.name),
    measuredMs: measuredMs,
    measuring: measuring,
    // Ключ узла и есть имя прокси: ядро вернёт замер под ним же.
    proxyNames: <String>[s.id],
    protocol: s.type,
    source: ExitInventorySource.importedSub,
  );

  @override
  bool operator ==(Object other) =>
      other is ExitNode && other.key == key && other.source == source;

  @override
  int get hashCode => Object.hash(key, source);
}

/// Страна выхода: то, к чему привязывается пикер.
class ExitLocation {
  /// ISO-2 в верхнем регистре. Пустая строка — страна неизвестна (узлы, из
  /// имени которых ядро её не вывело); такая группа не прячется.
  final String countryCode;

  /// Человекочитаемое имя («Германия»), иначе сам код.
  final String displayName;

  /// Глиф флага страны; [kNeutralFlag] — страна не названа твёрдо.
  final String flag;

  /// Сколько узлов этой страны в источнике (включая недоступные).
  final int nodeCount;

  /// Лучшая задержка среди ДОСТУПНЫХ узлов — вместе с тем, кто её померил.
  ///
  /// Страна не смешивает авторов: пока хоть один узел померен самим
  /// приложением, лучшая берётся из собственных замеров, и только если своих
  /// нет вовсе — из чисел оператора. Иначе строка страны показывала бы «12 мс»
  /// оператора рядом с честными «180 мс» её же узлов.
  final Latency bestLatency;

  /// Лучший пинг среди доступных узлов; `null` — числа нет.
  int? get bestPingMs => bestLatency.ms;

  /// Протоколы, встречающиеся в стране (отсортированы). Пусто — источник
  /// протоколы не сообщает; это НЕ значит «протоколов нет».
  final List<String> protocols;

  final ExitInventorySource source;

  final ExitAvailability availability;

  const ExitLocation({
    required this.countryCode,
    required this.displayName,
    required this.nodeCount,
    required this.source,
    this.flag = kNeutralFlag,
    this.bestLatency = Latency.none,
    this.protocols = const <String>[],
    this.availability = ExitAvailability.available,
  });

  bool get isAvailable => availability.isAvailable;

  /// Страну вывести не удалось — узлы всё равно показываются одной группой.
  bool get isUnknownCountry => countryCode.isEmpty;

  PingBucket get pingBucket => pingBucketOf(bestPingMs);

  /// Собирает страну из её узлов. Недоступной страна становится тогда и только
  /// тогда, когда недоступны ВСЕ её узлы: одного живого достаточно, чтобы
  /// пользователю было куда подключиться.
  factory ExitLocation.fromNodes(
    String countryCode,
    List<ExitNode> nodes, {
    required ExitInventorySource source,
    String? displayName,
  }) {
    final code = normalizeCountryCode(countryCode);
    final live = nodes.where((n) => n.isAvailable).toList(growable: false);
    var best = Latency.none;
    var measuring = false;
    for (final n in live) {
      final l = n.latency;
      if (l.source == LatencySource.measuring) measuring = true;
      if (!l.hasValue || l.isTimeout) continue;
      // Собственный замер вытесняет операторский на уровне страны так же, как
      // на уровне узла: смешивать авторов в одном числе нельзя.
      final better =
          !best.hasValue ||
          (best.source == LatencySource.operator &&
              l.source == LatencySource.client) ||
          (best.source == l.source && l.ms! < best.ms!);
      if (better) best = l;
    }
    // Своего числа ещё нет, но замер идёт и операторского тоже нет — страна
    // честно говорит «меряю», а не показывает прочерк, который читается как
    // «данных не будет».
    if (!best.hasValue && measuring) best = Latency.measuring;
    final protocols =
        nodes
            .map((n) => n.protocol)
            .where((p) => p.isNotEmpty)
            .toSet()
            .toList(growable: false)
          ..sort();
    final ExitAvailability availability;
    if (nodes.isEmpty) {
      availability = const ExitAvailability.unavailable(
        ExitUnavailableReason.sourceEmpty,
      );
    } else if (live.isEmpty) {
      availability = ExitAvailability.unavailable(
        ExitUnavailableReason.allNodesBusy,
        detail: countryNameOf(code),
      );
    } else {
      availability = ExitAvailability.available;
    }
    return ExitLocation(
      countryCode: code,
      displayName: displayName ?? countryNameOf(code),
      // Флаг страны берётся у её узлов, а не выводится здесь заново: узел уже
      // решил, твёрдая ли у него страна (у импорта она догадка), и второе
      // решение на том же коде разошлось бы с первым.
      flag: _flagOfNodes(code, nodes),
      nodeCount: nodes.length,
      bestLatency: best,
      protocols: protocols,
      source: source,
      availability: availability,
    );
  }

  @override
  bool operator ==(Object other) =>
      other is ExitLocation &&
      other.countryCode == countryCode &&
      other.source == source &&
      other.nodeCount == nodeCount &&
      other.bestLatency == bestLatency &&
      other.availability == availability;

  @override
  int get hashCode =>
      Object.hash(countryCode, source, nodeCount, bestLatency, availability);

  @override
  String toString() =>
      'ExitLocation($countryCode, nodes: $nodeCount, ${availability.reason})';
}

/// Флаг страны по её узлам: первый непустой глиф, который узлы сочли твёрдым.
///
/// Ни один не счёл — остаётся [kNeutralFlag]. Так страна, собранная из узлов
/// импортированной подписки со слабой догадкой о стране, показывается кодом
/// (`DE`, «Германия») и БЕЗ флага: код это то, что сказал источник, а флаг был
/// бы уверенностью, которой у нас нет.
String _flagOfNodes(String code, List<ExitNode> nodes) {
  for (final n in nodes) {
    if (n.countryCode == code && n.flag != kNeutralFlag) return n.flag;
  }
  return kNeutralFlag;
}

/// Разрешённый панелью выбор — ответ `PUT /subscriptions/{id}/selection`.
///
/// Панель отвечает тем, что применилось ФАКТИЧЕСКИ: запрошенный узел мог не
/// подойти плану, и тогда вернётся другой. Локальное состояние обязано
/// подстраиваться под этот ответ, а не под то, что мы просили.
class ExitSelection {
  /// `nodes.id` выхода; `null` — выбор сброшен в дефолт.
  final int? nodeId;

  /// ISO-2 страны входа; `null` — цепочка выключена/дефолт.
  final String? relayCountry;

  const ExitSelection({this.nodeId, this.relayCountry});

  /// Пустой выбор (панель ничего не закрепила).
  static const ExitSelection none = ExitSelection();

  factory ExitSelection.fromJson(Map<String, dynamic> json) => ExitSelection(
    nodeId: (json['node_id'] as num?)?.toInt(),
    relayCountry: _nonEmpty(json['relay_country']),
  );

  Map<String, dynamic> toJson() => <String, dynamic>{
    'node_id': nodeId,
    'relay_country': relayCountry,
  };

  static String? _nonEmpty(Object? v) {
    if (v is String && v.trim().isNotEmpty) return v.trim().toUpperCase();
    return null;
  }

  @override
  bool operator ==(Object other) =>
      other is ExitSelection &&
      other.nodeId == nodeId &&
      other.relayCountry == relayCountry;

  @override
  int get hashCode => Object.hash(nodeId, relayCountry);

  @override
  String toString() => 'ExitSelection(node: $nodeId, relay: $relayCountry)';
}

/// Поле частичного обновления: «не трогать» / «сбросить в дефолт» / «значение».
///
/// JSON различает отсутствие ключа и `null`, Dart-аргумент — нет: `null` в
/// сигнатуре значит и «не передали», и «передали null». Из-за этого без явной
/// обёртки сброс выбора неотличим от его сохранения, и одно из двух намерений
/// молча теряется по дороге к панели.
class SelectionField<T extends Object> {
  /// Ключ вообще попадёт в тело запроса.
  final bool present;

  /// Значение; `null` при [present] означает «сбросить в дефолт».
  final T? value;

  const SelectionField._(this.present, this.value);

  /// Ключ не отправляется — панель оставляет текущее значение.
  const SelectionField.unchanged() : this._(false, null);

  /// Отправляется `null` — панель возвращается к значению по умолчанию.
  const SelectionField.reset() : this._(true, null);

  /// Отправляется значение.
  const SelectionField.of(T value) : this._(true, value);

  /// Из nullable-значения: `null` читается как сброс, а не как «не трогать».
  factory SelectionField.orReset(T? value) =>
      value == null ? SelectionField<T>.reset() : SelectionField<T>.of(value);
}

/// Имя прокси, которым страна представлена на СЫРОМ пути (импортированная
/// подписка). `null` — страна не задана или её узлов в подписке нет.
///
/// У ядра на этом пути нет понятия страны: `connectRaw` получает одну строку —
/// имя прокси, и пустая строка значит «любой узел». Страна, не превращённая в
/// имя ДО connect, остаётся галочкой в интерфейсе, пока трафик выходит где
/// угодно; чтобы такое превращение было одинаковым и при выборе, и при
/// подключении, оно живёт здесь, рядом с моделью, а не в двух состояниях.
String? rawProxyNameForCountry(
  List<ImportedServer> servers,
  String? countryCode,
) {
  final code = normalizeCountryCode(countryCode);
  if (code.isEmpty) return null;
  for (final s in servers) {
    if (normalizeCountryCode(s.country) == code) return s.id;
  }
  return null;
}

/// Нейтральный глиф вместо флага: страна неизвестна ЛИБО известна слишком
/// нетвёрдо, чтобы рисовать чей-то флаг. Тот же символ выбирает и панель, когда
/// не знает кода, — расхождения между экраном и оператором нет.
const String kNeutralFlag = '🌐';

/// Первая кодовая точка Regional Indicator Symbol (U+1F1E6, «A»).
const int _kRegionalIndicatorA = 0x1F1E6;

/// Флаг страны — ГЛИФ ДЛЯ ПОКАЗА.
///
/// Владелец продукта попросил флаги (сентябрь 2026), и это разворот прежнего
/// решения «коды, не флаги» (см. `server.dart`). Разворот сознательный: часть
/// прежнего довода — «неизвестная страна не получает флага» — здесь и живёт.
///
/// Правила, и почему именно они:
///   * код не нормализуется в ISO-2 → [kNeutralFlag]. Флаг выводится ТОЛЬКО из
///     кода; кода нет — выводить не из чего, а панельный `flag` в этом случае
///     как раз и врёт (для узла без страны она подставляет `US` и уверенно
///     присылает 🇺🇸);
///   * [panelFlag] совпадает с тем, что даёт код → отдаём панельный побайтово:
///     это глиф оператора, и рисовать вместо него свой незачем;
///   * [panelFlag] пуст, равен `🌐` или ПРОТИВОРЕЧИТ коду → выводим из кода.
///     Код авторитетнее: по нему группируется список и разрешается выбор, и
///     флаг, спорящий с группировкой, — это флаг не той страны на строке.
///
/// Вывод из ISO-2 — не догадка: это та же арифметика Regional Indicator, по
/// которой панель считает своё поле (`country_flag` в app.rs).
String flagOf(String? countryCode, [String? panelFlag]) {
  final derived = _flagFromCode(normalizeCountryCode(countryCode));
  if (derived == null) return kNeutralFlag;
  final panel = (panelFlag ?? '').trim();
  return panel == derived ? panel : derived;
}

/// Флаг из канонического ISO-2; `null` — код не двухбуквенный.
String? _flagFromCode(String code) {
  if (code.length != 2) return null;
  final a = code.codeUnitAt(0);
  final b = code.codeUnitAt(1);
  if (a < 0x41 || a > 0x5A || b < 0x41 || b > 0x5A) return null;
  return String.fromCharCodes(<int>[
    _kRegionalIndicatorA + (a - 0x41),
    _kRegionalIndicatorA + (b - 0x41),
  ]);
}

/// ISO-2 из флаг-эмодзи, встреченного В ТЕКСТЕ (имя прокси импортированной
/// подписки). `''` — флага в строке нет.
///
/// Нужен там, где страна ДОГАДЫВАЕТСЯ по имени: ядро выводит её и из флага, и
/// из любого двухбуквенного слова (`subimport.CountryFromName`), а это разные
/// по надёжности вещи. Флаг в имени написал сам оператор — это не догадка;
/// слово `MY` в `my-node` даст «Малайзию» на ровном месте.
String countryOfEmbeddedFlag(String? text) {
  final runes = (text ?? '').runes.toList(growable: false);
  for (var i = 0; i + 1 < runes.length; i++) {
    final a = runes[i];
    final b = runes[i + 1];
    if (a < _kRegionalIndicatorA || a > _kRegionalIndicatorA + 25) continue;
    if (b < _kRegionalIndicatorA || b > _kRegionalIndicatorA + 25) continue;
    return String.fromCharCodes(<int>[
      0x41 + (a - _kRegionalIndicatorA),
      0x41 + (b - _kRegionalIndicatorA),
    ]);
  }
  return '';
}

/// Флаг для узла ИМПОРТИРОВАННОЙ подписки, где страны в контракте нет и её
/// выводят из имени прокси.
///
/// Флаг рисуется только тогда, когда оператор написал его в имени сам и он
/// сходится с тем, что вывело ядро. Во всех прочих случаях остаётся
/// [kNeutralFlag]: код страны на строке по-прежнему показывается (список
/// группируется по нему и без флага), а вот флаг не той страны — это ровно то
/// «уверенно неправильно», которого прежнее решение и опасалось.
String flagOfGuessedCountry(String? countryCode, String? name) {
  final code = normalizeCountryCode(countryCode);
  if (code.isEmpty) return kNeutralFlag;
  if (countryOfEmbeddedFlag(name) != code) return kNeutralFlag;
  return flagOf(code);
}

/// Приводит код страны к каноническому виду: верхний регистр без пробелов.
/// Пустая/мусорная строка становится `''` — «страна неизвестна».
String normalizeCountryCode(String? raw) {
  final v = (raw ?? '').trim().toUpperCase();
  if (v.length != 2) return '';
  return v;
}

/// Имя страны по ISO-2. Незнакомый код отдаётся как есть: неизвестная страна
/// показывается кодом, но не исчезает из списка.
String countryNameOf(String? code) {
  final c = normalizeCountryCode(code);
  if (c.isEmpty) return 'Без страны';
  return _countryNames[c] ?? c;
}

/// Названия стран, которые реально встречаются в узлах панели и в чужих
/// подписках. Список намеренно неполный: неизвестный код показывается кодом.
const Map<String, String> _countryNames = <String, String>{
  'AE': 'ОАЭ',
  'AM': 'Армения',
  'AR': 'Аргентина',
  'AT': 'Австрия',
  'AU': 'Австралия',
  'AZ': 'Азербайджан',
  'BE': 'Бельгия',
  'BG': 'Болгария',
  'BR': 'Бразилия',
  'BY': 'Беларусь',
  'CA': 'Канада',
  'CH': 'Швейцария',
  'CL': 'Чили',
  'CN': 'Китай',
  'CY': 'Кипр',
  'CZ': 'Чехия',
  'DE': 'Германия',
  'DK': 'Дания',
  'EE': 'Эстония',
  'ES': 'Испания',
  'FI': 'Финляндия',
  'FR': 'Франция',
  'GB': 'Великобритания',
  'GE': 'Грузия',
  'GR': 'Греция',
  'HK': 'Гонконг',
  'HU': 'Венгрия',
  'ID': 'Индонезия',
  'IE': 'Ирландия',
  'IL': 'Израиль',
  'IN': 'Индия',
  'IS': 'Исландия',
  'IT': 'Италия',
  'JP': 'Япония',
  'KG': 'Киргизия',
  'KR': 'Южная Корея',
  'KZ': 'Казахстан',
  'LT': 'Литва',
  'LU': 'Люксембург',
  'LV': 'Латвия',
  'MD': 'Молдова',
  'MX': 'Мексика',
  'MY': 'Малайзия',
  'NL': 'Нидерланды',
  'NO': 'Норвегия',
  'NZ': 'Новая Зеландия',
  'PL': 'Польша',
  'PT': 'Португалия',
  'RO': 'Румыния',
  'RS': 'Сербия',
  'RU': 'Россия',
  'SE': 'Швеция',
  'SG': 'Сингапур',
  'SK': 'Словакия',
  'TH': 'Таиланд',
  'TR': 'Турция',
  'TW': 'Тайвань',
  'UA': 'Украина',
  'US': 'США',
  'UZ': 'Узбекистан',
  'VN': 'Вьетнам',
  'ZA': 'ЮАР',
};

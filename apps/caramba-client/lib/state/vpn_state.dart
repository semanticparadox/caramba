import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/state/access_guard.dart';
import 'package:caramba_client/state/auto_reconnect.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_policy_mapping.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
// Сериализатор политики и разбор отказов моста живут в плагине и не входят в
// узкий re-export `lib/vpn/core_policy.dart` (тот отдаёт только модели),
// поэтому берём их напрямую из пакета.
import 'package:caramba_vpn/caramba_vpn.dart'
    show isMissingCoreBridge, jsonEncodePolicy;

/// Потолок ожидания ПОДТВЕРЖДЁННОЙ остановки ядра перед новым подъёмом.
///
/// Обычная разборка укладывается в сотни миллисекунд, так что восемь секунд —
/// запас больше чем на порядок. Сверху его ограничивает [kAttemptLimit] (25 с),
/// в который автоматика укладывает попытку ЦЕЛИКОМ: подъёму после остановки
/// обязано остаться время, иначе баннер объявит неудачу раньше, чем ядро
/// вообще начнёт поднимать туннель.
const Duration kStopConfirmationLimit = Duration(seconds: 8);

/// Снимок, приведённый к тому, что о туннеле говорит НЕ ядро.
///
/// ЗАЧЕМ. Слово «Защищено» до сих пор опиралось только на ответ ядра о самом
/// себе. Ядро ошибалось: `executor.Shutdown()` у mihomo глобален на процесс, и
/// закрытие служебного ядра валило чужой живой туннель, оставляя движок,
/// который его поднимал, в StateConnected. Корень устранён, но сама конструкция
/// осталась бы прежней — утверждение без второго источника. Наблюдение
/// платформы ([TunnelWitness]) и есть второй источник: сеть с транспортом VPN
/// плюс локальный TUN-интерфейс, то самое, чем определяется значок в
/// статус-баре.
///
/// ГЛАВНОЕ ПРАВИЛО. Вето срабатывает ТОЛЬКО на положительный ответ
/// [TunnelWitness.absent] — когда оба независимых наблюдения ответили «VPN нет»
/// и оба ответили без ошибки. Молчание, отказ в разрешении, платформа без
/// такого моста, мок, FFI-путь — всё это [TunnelWitness.unknown], и щит по нему
/// не гаснет. Ошибка в другую сторону хуже исходного дефекта: человеку,
/// которому сказали, что защиты нет, остаётся включать её заново — то есть
/// оборвать работающий туннель, а в промежутке выпустить трафик открытым.
///
/// Стадия становится [VpnStage.error], а не «отключено»: туннель не опущен, а
/// разошёлся с действительностью, и это поломка, о которой человеку надо
/// сказать, а не тихая нормальность. Момент подъёма при этом снимается —
/// таймер сессии над несуществующей защитой был половиной той же неправды.
VpnStatus witnessedStatus(VpnStatus s) {
  // Проверяем только стадии, которые ЧТО-ТО УТВЕРЖДАЮТ о защите.
  // «Подключаемся» ничего не обещает, «отключено» и «ошибка» тем более.
  if (s.stage != VpnStage.connected && s.stage != VpnStage.reconnecting) {
    return s;
  }
  if (s.witness != TunnelWitness.absent) return s;
  return VpnStatus(
    stage: VpnStage.error,
    server: s.server,
    detail: VpnFailureReason.noVpnTransport,
    witness: s.witness,
  );
}

/// Нотифаер state-машины туннеля. Подписывается на [VpnConnection.status],
/// проксирует его в Riverpod, и предоставляет toggle/connect/disconnect для UI
/// (орб на Home). Реальные переходы стадий приходят от ядра.
///
/// Активный профиль подключения ([ConnectionProfile]) задаёт путь поднятия:
///   * panelAccount → `configure` + connect к узлу подписки;
///   * rawSub       → импорт сырой подписки + connect к закреплённому узлу.
/// Профиль резолвится лениво (на момент connect) через [_activeProfile].
///
/// Перед КАЖДЫМ поднятием туннеля в ядро уходят политика ([CorePolicy], ABI v2
/// `setPolicy`) и способ захвата трафика (`setTunnelMode`): оба действуют со
/// следующего `Up`, поэтому применяются именно здесь, а не при правке настроек.
///
/// И перед каждым же поднятием спрашивается ПРАВО подключаться ([AccessGuard]).
/// Раньше не спрашивалось нигде: raw-путь поднимает туннель из кэша, не
/// обращаясь никуда вовсе, — и подписка с исчерпанным трафиком давала полностью
/// исправный туннель, через который не проходило ничего. Экран при этом говорил
/// «Защищено».
/// Комбинация, ОТДАННАЯ ядру за одно поднятие туннеля.
///
/// ЗАЧЕМ снимком, а не чтением провайдеров в момент отката. Откат после
/// неудачного автопереподключения обязан вернуть туннель туда, где он работал,
/// НЕ трогая выбор пользователя. Собирайся откат из текущих провайдеров, он
/// поднял бы ровно ту комбинацию, которая только что не поднялась, — то есть
/// был бы не откатом, а повтором отказа.
class TunnelRoute {
  /// Поднимались сырым путём (`connectRaw`), а не панельным.
  final bool raw;

  /// Узел панели; `null` на сыром пути.
  final Server? server;

  /// Имя прокси для сырого пути; `null` — «любой узел».
  final String? rawServerId;

  final CorePolicy policy;
  final TunnelMode mode;

  const TunnelRoute.panel({
    required Server this.server,
    required this.policy,
    required this.mode,
  }) : raw = false,
       rawServerId = null;

  const TunnelRoute.raw({
    required this.rawServerId,
    required this.policy,
    required this.mode,
  }) : raw = true,
       server = null;

  /// Ключ узла в терминах того пути, которым поднимались: имя прокси в импорте,
  /// `nodes.id` строкой на панели. Пусто — узел выбирало ядро.
  String get exitKey => raw ? (rawServerId ?? '') : '${server!.id}';
}

class VpnNotifier extends StateNotifier<VpnStatus> {
  final VpnConnection _conn;
  final Server? Function() _recommended;
  final ConnectionProfile? Function() _activeProfile;
  final CorePolicy Function() _policy;
  final TunnelMode Function() _tunnelMode;

  /// Сторож доступа; `null` — проверять некому (сборка без него, тест).
  /// Разрешается лениво: сторож сам смотрит на стадию туннеля.
  final AccessGuard? Function() _guard;

  StreamSubscription<VpnStatus>? _sub;

  /// Политика, реально отданная ядру при последнем УДАЧНОМ применении (JSON).
  /// `null` — туннель ещё не поднимали, либо в сборке нет моста `setPolicy`
  /// вовсе. Провал живого моста это поле НЕ обнуляет (ядро осталось на том,
  /// что в нём было) — он поднимает [corePreferencesStale]. По расхождению
  /// НАСТРОЕЧНОЙ половины с текущей политикой UI показывает «Переподключитесь,
  /// чтобы применить» ([settingsChangedProvider]); половина «путь» применяется
  /// сама ([autoReconnectProvider]).
  String? appliedPolicyJson;

  /// Та же политика объектом. Нужна отдельно от JSON: сравнение половинами
  /// ([settingsSignature]) идёт по полям, а не по готовой строке.
  CorePolicy? appliedPolicy;

  /// Способ захвата трафика, отданный ядру при последнем поднятии туннеля.
  /// Тоже действует со следующего `Up`, поэтому участвует в том же сравнении.
  TunnelMode? appliedTunnelMode;

  /// Ключ узла, к которому ушла последняя команда подъёма. Диагностика и
  /// сообщение об откате; для «что держит ядро» есть [activeProxyProvider] —
  /// это разные вопросы, и путать их нельзя.
  String? appliedExitKey;

  /// Комбинация текущей попытки: становится [lastGood], когда ядро доложит
  /// `connected`.
  TunnelRoute? _attempt;

  /// Последняя комбинация, на которой туннель РЕАЛЬНО поднялся. Единственное,
  /// на что можно откатиться после неудачной смены пути.
  TunnelRoute? lastGood;

  /// Последняя попытка отдать ядру настройки (политика, способ захвата)
  /// ПРОВАЛИЛАСЬ на живом мосту: ядро работает не на том, что выбрал человек.
  ///
  /// Отдельный признак, а не «обнулим применённое»: обнуление читалось ниже
  /// как «мы ничего не применяли и врать про расхождение не будем» — то есть
  /// провал делал экран тише, чем успех. Молчание держалось до следующего
  /// удачного применения, а применить можно только на подъёме, а подъём
  /// затевает баннер, которого больше нет: круг замыкался навсегда.
  bool corePreferencesStale = false;

  /// Потолок ожидания подтверждённой остановки. Значение — [kStopConfirmationLimit];
  /// параметром оно только ради тестов, которым нельзя ждать восемь секунд.
  final Duration stopLimit;

  /// Стадия, о которой доложило САМО ЯДРО, без поправки на наблюдение
  /// платформы.
  ///
  /// Именно она решает, остановлено ли ядро. Взять здесь [state] нельзя:
  /// [witnessedStatus] переписывает «подключено без VPN-транспорта» в
  /// [VpnStage.error], а это стадия остановки — и подъём начался бы ровно
  /// поверх работающего ядра, то есть ровно там, где и ломалось.
  VpnStage _coreStage = VpnStage.disconnected;

  /// Кто ждёт кадра об остановке. `null` — никто не ждёт. Значение ответа:
  /// `true` — остановка подтверждена кадром, `false` — ждать больше нечего
  /// (нотифаер закрыт), и подниматься тоже незачем.
  Completer<bool>? _stopWaiter;

  VpnNotifier(
    this._conn,
    this._recommended,
    this._activeProfile,
    this._policy,
    this._tunnelMode, {
    AccessGuard? Function()? guard,
    this.stopLimit = kStopConfirmationLimit,
  }) : _guard = (guard ?? _noGuard),
       // Первый кадр — тот самый, что достаётся из нативного кэша при подписке,
       // и проверять его надо наравне с остальными: именно он и рисовал
       // «Защищено» с идущим таймером на холодном старте.
       super(witnessedStatus(_conn.currentStatus)) {
    _coreStage = _conn.currentStatus.stage;
    _sub = _conn.status.listen((s) {
      // Кадр об остановке будит того, кто её ждёт, ДО всего остального:
      // подтверждение — это факт о ядре, и трогать его не должны ни вето
      // наблюдения, ни сторож доступа.
      _coreStage = s.stage;
      if (_coreHalted(s.stage)) _releaseStopWaiter();
      // ОДНО место, где стадия приводится к наблюдаемой действительности.
      // Дальше по течению её читают дайл, подпись, доступ, заголовок выхода и
      // статистика; разойтись они не могут, потому что источник один.
      final truthful = witnessedStatus(s);
      // Комбинация становится «рабочей» ТОЛЬКО по факту `connected`, и только
      // после [witnessedStatus]: туннель, которому платформа отказала в
      // подтверждении, откатом быть не может — на него незачем возвращаться.
      final attempt = _attempt;
      if (truthful.stage == VpnStage.connected && attempt != null) {
        lastGood = attempt;
      }
      state = truthful;
      // Сторож просыпается на живом туннеле: пока он поднят, право подключаться
      // может кончиться прямо в сессии (свип панели троттлит подписку и рвёт
      // соединения раз в 600 с), и узнать об этом больше неоткуда.
      _guard()?.onStage(truthful.stage);
    });
  }

  static AccessGuard? _noGuard() => null;

  /// Подключиться согласно активному профилю подключения.
  ///
  /// Если активен rawSub-профиль — поднимаем туннель из импортированной
  /// подписки, передавая её формат и закреплённый узел. Иначе (panelAccount или
  /// профиль не задан) идём панельным путём: к [server], либо к рекомендованному
  /// узлу, если сервер не передан. Возвращает `false`, если подключаться не к чему.
  Future<bool> connect([Server? server]) => _connect(server: server);

  /// Опустить и снова поднять туннель по ТЕКУЩЕМУ выбору.
  ///
  /// Живёт здесь, а не в кнопке баннера: переподключаются теперь двое —
  /// человек и автоматика, — и разойтись в том, что именно значит
  /// «переподключить», они не имеют права.
  ///
  /// БЫЛО `disconnect()` + `connect()` ВСТЫК, и это принесло пятый за сессию
  /// случай «Защищено» над мёртвым туннелем. `disconnect()` возвращает
  /// управление, когда команда ОТПРАВЛЕНА, а не когда ядро остановлено: на
  /// Android она уходит интентом в сервис, на FFI — вызовом, за которым у
  /// mihomo ещё идёт разборка. Подъём приходил в неё, и глобальный
  /// `executor.Shutdown()` гасил слушателей, которые новый подъём только что
  /// поставил: в логе «Initial configuration complete, total time: 1ms» вместо
  /// обычных тысяч и НИ ОДНОЙ строки «[TUN] Tun adapter listening».
  ///
  /// Теперь тела у метода нет вовсе: ожидание подтверждённой остановки стоит
  /// на ЕДИНСТВЕННОМ пути подъёма ([_connect]) и потому распространяется и на
  /// откат ([restoreLastGood]), и на обычный connect поверх живого туннеля.
  /// Держать его здесь значило бы починить одну дверь из трёх.
  Future<bool> reconnect() => connect();

  /// Поднять туннель на [lastGood], МИНУЯ текущий выбор пользователя.
  ///
  /// `false` — возвращаться не на что (туннель в этой сессии ни разу не
  /// поднимался нами) либо сырой профиль, которым он поднимался, исчез.
  Future<bool> restoreLastGood() {
    final route = lastGood;
    if (route == null) return Future<bool>.value(false);
    return _connect(replay: route);
  }

  /// Единственный путь подъёма. [replay] — поднять записанную комбинацию
  /// вместо текущего выбора; всё остальное в обоих случаях одинаково, потому
  /// что откат обязан идти той же дорогой, что и обычное подключение.
  Future<bool> _connect({Server? server, TunnelRoute? replay}) async {
    final profile = _activeProfile();
    final policy = replay?.policy ?? _policy();
    final mode = replay?.mode ?? _tunnelMode();

    // rawSub: явный сервер для панельного пути не передан — поднимаем raw.
    final useRaw = replay != null
        ? replay.raw
        : (server == null && profile != null && profile.isRaw);

    if (useRaw) {
      if (profile == null || !profile.isRaw) {
        // Только на откате: профиль, которым туннель поднимался, сменили.
        // Возвращать нечем, и молчать об этом нельзя.
        state = const VpnStatus(
          stage: VpnStage.error,
          detail: 'Raw profile gone',
        );
        return false;
      }
      final raw = profile.rawConfig ?? profile.source;
      if (raw.isEmpty) {
        state = const VpnStatus(stage: VpnStage.error, detail: 'Empty profile');
        return false;
      }
      if (await _refusedNow()) return false;
      if (!await _coreConfirmedStopped()) return false;
      final serverId = replay != null
          ? replay.rawServerId
          : _rawServerId(profile);
      await _applyCorePreferences(policy, mode);
      _remember(
        TunnelRoute.raw(rawServerId: serverId, policy: policy, mode: mode),
      );
      await _conn.connectRaw(
        raw: raw,
        format: profile.format,
        label: profile.displayName,
        serverId: serverId,
      );
      return true;
    }

    final target = replay?.server ?? server ?? _recommended();
    if (target == null) {
      state = const VpnStatus(
        stage: VpnStage.error,
        detail: 'No server selected',
      );
      return false;
    }
    if (await _refusedNow()) return false;
    if (!await _coreConfirmedStopped()) return false;
    await _applyCorePreferences(policy, mode);
    _remember(TunnelRoute.panel(server: target, policy: policy, mode: mode));
    await _conn.connect(target);
    return true;
  }

  /// Ядро НЕ поднято и утверждать обратное не может.
  ///
  /// Ошибка сюда входит наравне с «отключено»: `error` — тоже конец сеанса, и
  /// на Android его печатает тот же `stopTunnel`, что и `disconnected`, уже
  /// после возврата из `down()`.
  static bool _coreHalted(VpnStage stage) =>
      stage == VpnStage.disconnected || stage == VpnStage.error;

  /// Будит того, кто ждёт остановки. Идемпотентно: кадров об остановке подряд
  /// может прийти несколько (сервис печатает свой, следом переспрос платформы).
  ///
  /// [halted] `false` — «ждать больше нечего», а не «остановка подтверждена».
  /// Разница принципиальна: закрытие нотифаера освобождает ожидающего, но
  /// правом поднять туннель не является.
  void _releaseStopWaiter({bool halted = true}) {
    final waiter = _stopWaiter;
    _stopWaiter = null;
    if (waiter != null && !waiter.isCompleted) waiter.complete(halted);
  }

  /// Гарантия, ради которой всё это: подъём НЕ начинается, пока ядро не
  /// доложило, что оно остановлено.
  ///
  /// Ждём не «столько-то миллисекунд», а КАДР: на Android его печатает
  /// `stopTunnel` следующей строкой после возврата из `core.down()`, то есть
  /// после того, как `executor.Shutdown()` вернул управление; на FFI —
  /// `disconnect()` сразу за `lib.down()`. Раньше этого кадра ядро ещё живо, и
  /// любое ожидание по часам было бы угадыванием.
  ///
  /// Ядро уже остановлено — не отправляем даже команду «опустить» и не ждём
  /// ничего: обычное подключение с холодного старта обязано остаться ровно
  /// таким же быстрым, каким было.
  ///
  /// НЕ ДОЖДАЛИСЬ — ОСТАЁМСЯ В ОТКАЗЕ, а не поднимаем «всё равно». Довод
  /// один и он же главный по всей этой ветке: неподтверждённая остановка —
  /// это незнание о том, что делает ядро, а подъём в незнание и есть тот
  /// самый зелёный щит над мёртвым туннелем. Отказ при этом не хуже подъёма
  /// и по трафику: туннель либо уже опущен (тогда сеть идёт напрямую, как и
  /// после любого разрыва), либо ещё жив (тогда он и защищает), — а
  /// «поднять поверх» ломает второй случай, ничего не давая первому. Названная
  /// ошибка чинится одним нажатием человека; молчаливая ложь не чинится
  /// вообще, потому что человек не знает, что чинить.
  Future<bool> _coreConfirmedStopped() async {
    if (_coreHalted(_coreStage)) return true;
    // Прошлое ожидание, если оно было, отпускаем ОТКАЗОМ. Два подъёма разом —
    // это гонка вызывающих, а не ядра, и второй не имеет права выдать первому
    // своё подтверждение; отпущенный отказом первый тихо вернёт `false` и не
    // ляжет своей ошибкой поверх более свежего состояния.
    _releaseStopWaiter(halted: false);
    // Ожидание заводится ДО команды: кадр может прийти в тот же оборот цикла
    // событий, и подписчик, созданный после, его бы уже не увидел.
    final waiter = _stopWaiter = Completer<bool>();
    await disconnect();
    try {
      // `false` бывает только от закрытия нотифаера: подниматься некому и
      // некуда, а состояние писать уже нельзя.
      return await waiter.future.timeout(stopLimit);
    } on TimeoutException {
      if (identical(_stopWaiter, waiter)) _stopWaiter = null;
      if (mounted) {
        state = const VpnStatus(
          stage: VpnStage.error,
          detail: VpnFailureReason.stopNotConfirmed,
        );
      }
      return false;
    }
  }

  /// Запоминает комбинацию ПОПЫТКИ. Рабочей она станет только по `connected`.
  void _remember(TunnelRoute route) {
    _attempt = route;
    appliedExitKey = route.exitKey;
  }

  /// Спрашивает право подключаться и, если в нём отказано, переводит состояние
  /// в ошибку с НАЗВАННОЙ причиной вместо поднятия туннеля.
  ///
  /// Отказом считается только явный ответ оператора. Молчащая сеть, таймаут,
  /// упавшая панель — не отказ: подключаемся. Отказать по неответу значило бы
  /// запретить человеку VPN ровно тогда, когда сеть плохая, — то есть тогда,
  /// когда VPN и нужен.
  Future<bool> _refusedNow() async {
    final guard = _guard();
    if (guard == null) return false;
    final verdict = await guard.checkBeforeConnect();
    if (!verdict.blocked) return false;
    // Причина уходит в detail уликой; человеческий текст экран берёт из
    // состояния доступа, которое сторож только что записал.
    state = VpnStatus(stage: VpnStage.error, detail: verdict.detail);
    return true;
  }

  /// Имя прокси для сырого подключения: явный пин, а если его нет — узел
  /// закреплённой страны.
  ///
  /// Страна ядру на этом пути не передаётся вовсе: `connectRaw` знает одну
  /// строку — имя прокси, и пустая значит «любой узел». Пин обычно уже
  /// конкретный (его ставит выбор страны), но пережить он может не всё:
  /// «Обновить подписку» снимает пин, узла которого в новом составе нет, а
  /// закреплённая страна остаётся. Без этого запасного разрешения такой профиль
  /// показывал бы «Германия» с галочкой, выпуская трафик через любой узел.
  static String? _rawServerId(ConnectionProfile p) {
    final pin = p.selectedServerId;
    if (pin != null && pin.isNotEmpty) return pin;
    return rawProxyNameForCountry(p.servers, p.selectedExitCountry);
  }

  /// Отдаёт ядру политику и режим захвата трафика. Ни та, ни другая неудача не
  /// фатальна для подключения: туннель на умолчаниях всё равно лучше, чем
  /// отказ подключаться.
  ///
  /// НО НЕУДАЧА РАЗНАЯ, И РАЗЛИЧАТЬ ЕЁ ОБЯЗАТЕЛЬНО.
  ///
  /// Моста нет в сборке ([isMissingCoreBridge]) — это свойство сборки, а не
  /// поломка: применённого нет и не будет, приложение об этом молчит, и ниже
  /// [settingsAwaitReconnect] по тому же `null` честно не утверждает никакого
  /// расхождения. Иначе человек получил бы вечный баннер «переподключитесь,
  /// чтобы применить» — предложение, которое ничего не меняет.
  ///
  /// Мост есть, а вызов не прошёл — отказ ЗДЕСЬ И СЕЙЧАС (ядро в разборке,
  /// платформа ответила ошибкой). Тогда: применённое НЕ ТЕРЯЕМ (ядро осталось
  /// на том, что в нём было, и забыть это значит потерять состояние), а факт
  /// провала поднимаем признаком [corePreferencesStale] — по нему баннер
  /// появляется сразу, а не ждёт следующей правки, которая до этого исправно
  /// проваливалась в тишину.
  ///
  /// Признак снимает только УДАЧНОЕ применение — то есть следующий подъём.
  Future<void> _applyCorePreferences(CorePolicy policy, TunnelMode mode) async {
    var stale = false;
    try {
      await _conn.setTunnelMode(mode, mixedPort: kMixedPort);
      appliedTunnelMode = mode;
    } catch (e) {
      // Раньше здесь не было ловли вовсе, и отказ моста улетал наружу из
      // `connect`: попытка автоматики падала исключением, а её баннер оставался
      // со словом «Переподключаюсь» навсегда.
      if (isMissingCoreBridge(e)) {
        appliedTunnelMode = null;
      } else {
        stale = true;
      }
    }
    try {
      await _conn.setPolicy(policy);
      appliedPolicy = policy;
      appliedPolicyJson = jsonEncodePolicy(policy);
    } catch (e) {
      if (isMissingCoreBridge(e)) {
        appliedPolicy = null;
        appliedPolicyJson = null;
      } else {
        stale = true;
      }
    }
    corePreferencesStale = stale;
  }

  Future<void> disconnect() => _conn.disconnect();

  /// Переспросить платформу о подлинной стадии туннеля.
  ///
  /// Зовётся при появлении экрана и при возвращении приложения из фона —
  /// в двух моментах, когда состояние в памяти могло разойтись с
  /// действительностью, а разойтись оно может ТОЛЬКО в опасную сторону.
  /// Сервис Android переживает активность на законных основаниях, но и умереть
  /// он может без нас: приложение закрыли кнопкой «Назад», процесс остался в
  /// памяти, туннель свернулся — а последний кадр в нативном кэше так и остался
  /// «connected» с моментом подъёма. Следующий запуск получал его первым же
  /// кадром и рисовал щит с идущим таймером над мёртвым интерфейсом.
  ///
  /// Ответ приходит обычным снимком в общий поток, поэтому отдельной ветки в
  /// состоянии не появляется: правда доезжает тем же путём, что и всегда.
  Future<void> refreshStage() async {
    await _conn.refreshStatus();
  }

  /// Тап по орбу: connected/connecting → отключение, иначе → подключение.
  Future<void> toggle([Server? server]) {
    if (state.isConnected || state.isBusy) return disconnect();
    return connect(server);
  }

  @override
  void dispose() {
    _sub?.cancel();
    // Кадров больше не будет, а ждущий висел бы до таймаута и писал состояние
    // в закрытый нотифаер. Отпускаем сразу и именно отказом: подтверждением
    // остановки закрытие подписки не является.
    _releaseStopWaiter(halted: false);
    super.dispose();
  }
}

final vpnProvider = StateNotifierProvider<VpnNotifier, VpnStatus>((ref) {
  return VpnNotifier(
    ref.watch(vpnConnectionProvider),
    () => ref.read(recommendedServerProvider),
    () => ref.read(activeConnectionProfileProvider),
    () => ref.read(corePolicyProvider),
    () => ref.read(tunnelModeProvider),
    // Через замыкание, а не значением: сторож читает активный профиль и сессию
    // на момент вопроса, и создавать его раньше, чем он понадобится, незачем.
    guard: () => ref.read(accessGuardProvider.notifier),
  );
});

/// Что Home обязана сказать про выход: страна, через которую трафик выходит НА
/// САМОМ ДЕЛЕ, и — отдельно — закреплённая страна, мимо которой он уходит.
///
/// Заголовок строки «Сервер» читается как утверждение о выходе, а не как
/// напоминание о настройке, поэтому источником для него не может быть пин.
/// Живых узлов в закреплённой стране может не быть (на боевом флоте в DE узел
/// один, и одного переполнения хватает), автоподбор тогда уходит в другую
/// страну — и пин, оставшийся заголовком, называл бы канадский выход
/// «Германией».
class ExitHeadline {
  /// ISO-2 страны узла, через который идёт (или пойдёт) трафик. Пусто — узла
  /// нет или страну по нему не определить: заголовок тогда «Авто».
  final String countryCode;

  /// Закреплённая страна, мимо которой уходит трафик; `null` — расхождения нет.
  final String? unavailableCountry;

  const ExitHeadline({this.countryCode = '', this.unavailableCountry});

  /// Заголовок строки «Сервер».
  String get title => countryCode.isEmpty ? 'Авто' : countryNameOf(countryCode);

  /// Автоподбор увёл трафик из закреплённой страны.
  bool get diverged => unavailableCountry != null;

  /// Готовая причина для баннера. Пусто — расхождения нет, и говорить нечего.
  String get divergenceMessage => unavailableCountry == null
      ? ''
      : 'В стране «${countryNameOf(unavailableCountry)}» сейчас нет свободных '
            'узлов. Подключение идёт через $title.';

  @override
  bool operator ==(Object other) =>
      other is ExitHeadline &&
      other.countryCode == countryCode &&
      other.unavailableCountry == unavailableCountry;

  @override
  int get hashCode => Object.hash(countryCode, unavailableCountry);
}

/// Разрешённый заголовок выхода.
///
/// Узел берётся тот, что держит ЯДРО, и лишь вне сессии — тот, к которому
/// пойдёт connect: во время сессии правда о выходе принадлежит ядру, и
/// пересчёт рекомендации (узел ушёл из выдачи, пришли новые пинги) не имеет
/// права переименовать страну живого туннеля.
final exitHeadlineProvider = Provider<ExitHeadline>((ref) {
  final live = ref.watch(vpnProvider.select((s) => s.server));
  final server = live ?? ref.watch(recommendedServerProvider);
  final node = normalizeCountryCode(server?.countryCode);
  final pinned = normalizeCountryCode(
    ref.watch(activeConnectionProfileProvider)?.selectedExitCountry,
  );
  // Узла нет вовсе (список ещё не приехал, подписка пуста) — трафик никуда не
  // идёт, и закреплённая страна остаётся честным заголовком намерения.
  if (node.isEmpty) return ExitHeadline(countryCode: pinned);
  return ExitHeadline(
    countryCode: node,
    unavailableCountry: (pinned.isEmpty || pinned == node) ? null : pinned,
  );
});

/// Текущая политика ядра, собранная из пользовательского выбора. Отдельный
/// провайдер, чтобы её видели и connect, и баннер «нужно переподключение».
final corePolicyProvider = Provider<CorePolicy>((ref) {
  return corePolicyFrom(
    ref.watch(coreConfigProvider),
    ref.watch(relaysProvider),
  );
});

/// НАСТРОЕЧНАЯ половина расхождения: правки, которые ждут человека.
///
/// Реклама, правила по сайтам, DNS, стек, MTU, IPv6, FakeIP, kill switch и
/// способ захвата трафика. Их правят сериями и часть из них диагностическая —
/// момент разрыва там выбирает человек. Половина «путь» (протокол, режим,
/// relay, узел выхода) сюда НЕ входит: она применяется сама
/// ([autoReconnectProvider]).
final settingsChangedProvider = Provider<bool>((ref) {
  final status = ref.watch(vpnProvider);
  final notifier = ref.read(vpnProvider.notifier);
  return settingsAwaitReconnect(
    connected: status.isConnected,
    preferencesStale: notifier.corePreferencesStale,
    applied: notifier.appliedPolicy,
    appliedMode: notifier.appliedTunnelMode,
    current: () => ref.watch(corePolicyProvider),
    currentMode: () => ref.watch(tunnelModeProvider),
  );
});

/// Решение «настройки ждут переподключения» ОДНОЙ функцией, без контейнера.
///
/// Вынесено из провайдера по той же причине, что и [witnessedStatus]: у
/// правила три входа и одно из них — след неудачи, который иначе виден только
/// на живом устройстве.
///
/// ПОРЯДОК ПРОВЕРОК ЗДЕСЬ — ЭТО И ЕСТЬ ИСПРАВЛЕНИЕ.
///
/// [preferencesStale] спрашивается РАНЬШЕ, чем `applied == null`. Провал
/// применения раньше приводил к `applied == null`, а `null` читался как «мы
/// ничего не применяли, врать про расхождение не будем» — и признак умолкал
/// навсегда: баннера нет, значит нет и переподключения, значит настройки
/// никогда не применяются заново. Провал обязан ПОДНИМАТЬ признак, а не
/// опускать его: ядро работает не на том, что выбрал человек, и это ровно то,
/// о чём баннер и говорит.
///
/// `applied == null` без провала — это по-прежнему честное «не знаем»: туннель
/// поднимали не мы, либо в сборке нет моста настроек вовсе.
///
/// ТЕКУЩИЙ ВЫБОР ПРИХОДИТ ЗАМЫКАНИЯМИ, А НЕ ЗНАЧЕНИЯМИ, и это не украшение.
/// В провайдере за ними стоит `ref.watch`, то есть ПОДПИСКА, а сборка текущей
/// политики тянет список релеев с панели. Спроси мы их до ранних выходов —
/// экран, ответ для которого заведомо «нет», заводил бы сетевой запрос ради
/// выброшенного результата.
bool settingsAwaitReconnect({
  required bool connected,
  required bool preferencesStale,
  required CorePolicy? applied,
  required TunnelMode? appliedMode,
  required CorePolicy Function() current,
  required TunnelMode Function() currentMode,
}) {
  // Вне сессии применять нечего: следующий подъём и так возьмёт текущий выбор.
  if (!connected) return false;
  if (preferencesStale) return true;
  if (applied == null) return false;
  if (appliedMode != currentMode()) return true;
  return settingsSignature(applied) != settingsSignature(current());
}

/// Ядро держит НЕ тот узел, который закреплён выбором.
///
/// Жил на экране «Серверы» приватным методом и был виден только оттуда. Теперь
/// общий: расхождение выхода — это состояние подключения, а не свойство одного
/// экрана, и человек, стоящий на Главной, обязан его видеть так же.
///
/// Это НЕ то же самое, что смена выбора: сюда попадает и холодный старт поверх
/// живого туннеля, где выбор никто не менял, а расхождение уже есть. Поэтому
/// он поднимает баннер, но не заводит автопереподключение — рвать туннель
/// имеет право только действие человека.
final exitDisagreesProvider = Provider<bool>((ref) {
  final status = ref.watch(vpnProvider);
  if (!status.isConnected) return false;
  final inventory = ref.watch(exitInventoryProvider);
  final key = inventory.selectedNodeKey;
  if (key == null || key.isEmpty) return false;

  if (inventory.source == ExitInventorySource.panelRest) {
    final id = int.tryParse(key);
    final live = status.server?.id;
    return id != null && live != null && live != id;
  }

  final active = ref.watch(activeProxyProvider);
  if (active == null || active.isEmpty) return false;
  for (final n in inventory.nodes) {
    if (n.key != key) continue;
    // Сравниваем по идентификатору узла, а не по отображаемому имени.
    // activeProxy это то, что доложило ядро про селектор CARAMBA, и оно
    // вправе доложить метку группы или дедуплицированное имя; сравнение по
    // имени тогда залипает баннером, который нельзя закрыть, потому что
    // Reconnect его не снимает.
    if (active == key) return false;
    return n.name.isNotEmpty && n.name != active;
  }
  return false;
});

/// `true`, когда баннер переподключения обязан быть на экране.
///
/// Имя и смысл прежние — «баннер показывать», — а поводов теперь три:
/// автоматика что-то делает или уже сделала ([autoReconnectProvider]),
/// настройки ждут человека, ядро держит чужой узел. Внутрь баннера уходит
/// разбор, ЧТО именно показать; экраны как гейтили на этом провайдере, так и
/// гейтят.
final reconnectRequiredProvider = Provider<bool>((ref) {
  if (ref.watch(autoReconnectProvider).visible) return true;
  if (ref.watch(settingsChangedProvider)) return true;
  return ref.watch(exitDisagreesProvider);
});

/// Поток статистики трафика для Home (тикает в connected). Эмитит нули вне сессии.
final trafficProvider = StreamProvider.autoDispose<TrafficStats>((ref) {
  return ref.watch(vpnConnectionProvider).traffic;
});

/// Удобный булев селектор «подключены ли мы» (для бейджей/иконок).
final isConnectedProvider = Provider<bool>(
  (ref) => ref.watch(vpnProvider).isConnected,
);

/// Узел, на который ядро сейчас указывает селектором CARAMBA (ABI v2).
/// `null` вне сессии или когда ядро поле не прислало.
final activeProxyProvider = Provider<String?>(
  (ref) => ref.watch(vpnProvider).activeProxy,
);

/// Способ захвата трафика, о котором отчиталось ЯДРО (в отличие от
/// [tunnelModeProvider] — это выбор пользователя, применяемый со следующего Up).
final activeTunnelModeProvider = Provider<TunnelMode?>(
  (ref) => ref.watch(vpnProvider).mode,
);

/// Адрес локального прокси (`127.0.0.1:7890`), когда ядро работает в
/// proxy-режиме. `null` в tun-режиме и вне сессии — строку показывать нечего.
final proxyEndpointProvider = Provider<String?>((ref) {
  final status = ref.watch(vpnProvider);
  if (status.mode != TunnelMode.proxy) return null;
  final port = status.mixedPort;
  if (port == null || port <= 0) return null;
  return '127.0.0.1:$port';
});

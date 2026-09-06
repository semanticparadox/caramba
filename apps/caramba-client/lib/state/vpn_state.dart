import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/state/access_guard.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_policy_mapping.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
// Сериализатор политики живёт в плагине и не входит в узкий re-export
// `lib/vpn/core_policy.dart` (тот отдаёт только модели), поэтому берём его
// напрямую из пакета.
import 'package:caramba_vpn/caramba_vpn.dart' show jsonEncodePolicy;

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

  /// Политика, реально отданная ядру при последнем поднятии туннеля (JSON).
  /// `null` — туннель ещё не поднимали. По расхождению с текущей политикой UI
  /// показывает «Переподключитесь, чтобы применить» ([reconnectRequiredProvider]);
  /// авто-переподключения нет намеренно: обрыв туннеля решает пользователь.
  String? appliedPolicyJson;

  /// Способ захвата трафика, отданный ядру при последнем поднятии туннеля.
  /// Тоже действует со следующего `Up`, поэтому участвует в том же сравнении.
  TunnelMode? appliedTunnelMode;

  VpnNotifier(
    this._conn,
    this._recommended,
    this._activeProfile,
    this._policy,
    this._tunnelMode, {
    AccessGuard? Function()? guard,
  }) : _guard = (guard ?? _noGuard),
       // Первый кадр — тот самый, что достаётся из нативного кэша при подписке,
       // и проверять его надо наравне с остальными: именно он и рисовал
       // «Защищено» с идущим таймером на холодном старте.
       super(witnessedStatus(_conn.currentStatus)) {
    _sub = _conn.status.listen((s) {
      // ОДНО место, где стадия приводится к наблюдаемой действительности.
      // Дальше по течению её читают дайл, подпись, доступ, заголовок выхода и
      // статистика; разойтись они не могут, потому что источник один.
      final truthful = witnessedStatus(s);
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
  Future<bool> connect([Server? server]) async {
    final profile = _activeProfile();

    // rawSub: явный сервер для панельного пути не передан — поднимаем raw.
    if (server == null && profile != null && profile.isRaw) {
      final raw = profile.rawConfig ?? profile.source;
      if (raw.isEmpty) {
        state = const VpnStatus(stage: VpnStage.error, detail: 'Empty profile');
        return false;
      }
      if (await _refusedNow()) return false;
      await _applyCorePreferences();
      await _conn.connectRaw(
        raw: raw,
        format: profile.format,
        label: profile.displayName,
        serverId: _rawServerId(profile),
      );
      return true;
    }

    final target = server ?? _recommended();
    if (target == null) {
      state = const VpnStatus(
        stage: VpnStage.error,
        detail: 'No server selected',
      );
      return false;
    }
    if (await _refusedNow()) return false;
    await _applyCorePreferences();
    await _conn.connect(target);
    return true;
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

  /// Отдаёт ядру политику и режим захвата трафика. Ошибку setPolicy не считаем
  /// фатальной для подключения: старое ядро (до ABI v2) её не знает, а туннель
  /// на дефолтной политике всё равно лучше, чем отказ подключаться.
  Future<void> _applyCorePreferences() async {
    final mode = _tunnelMode();
    await _conn.setTunnelMode(mode, mixedPort: kMixedPort);
    appliedTunnelMode = mode;
    final policy = _policy();
    try {
      await _conn.setPolicy(policy);
      appliedPolicyJson = jsonEncodePolicy(policy);
    } catch (_) {
      appliedPolicyJson = null;
    }
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

/// `true`, когда настройки правились при поднятом туннеле и ещё не применены.
/// UI показывает баннер «Переподключитесь, чтобы применить»; переподключение
/// инициирует пользователь, приложение туннель само не рвёт.
final reconnectRequiredProvider = Provider<bool>((ref) {
  final status = ref.watch(vpnProvider);
  if (!status.isConnected) return false;
  final notifier = ref.read(vpnProvider.notifier);
  final applied = notifier.appliedPolicyJson;
  if (applied == null) return false;
  if (applied != jsonEncodePolicy(ref.watch(corePolicyProvider))) return true;
  return notifier.appliedTunnelMode != ref.watch(tunnelModeProvider);
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

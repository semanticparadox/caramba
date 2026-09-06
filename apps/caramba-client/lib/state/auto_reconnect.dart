/// Автоматическое переподключение при смене ПУТИ трафика.
///
/// ПРАВИЛО ОДНОЙ ФРАЗОЙ. Всё, что выбирается в группе Главной — страна/узел
/// выхода, relay, тип подключения, режим, — применяется само; всё, что живёт в
/// Настройках (реклама, правила по сайтам, DNS, стек, MTU, IPv6, FakeIP, режим
/// захвата, kill switch), ждёт кнопку «Переподключить».
///
/// ПОЧЕМУ ИМЕННО ЭТИ ЧЕТЫРЕ. Страну и тип назвал владелец. Relay и режим
/// оказались с ними в одной группе, отвечают на тот же вопрос «куда и как идёт
/// трафик», и один контрол из четырёх, ведущий себя иначе, — сюрприз. Режим
/// особенно: человек переключает «Полный обход» и смотрит в браузер, а не в
/// баннер под дайлом.
///
/// ПОЧЕМУ НЕ НАСТРОЙКИ. Их правят сериями и часть из них диагностическая —
/// момент разрыва там выбирает человек, а не таймер.
///
/// ОКНО ТИШИНЫ. Смена не рвёт туннель сразу: запускается окно в [kQuietWindow],
/// и каждая следующая смена его перезапускает. Перебрал три варианта подряд —
/// разрыв ОДИН, на итоговой комбинации. Без окна перебор превращается в череду
/// обрывов, что хуже ожидания баннера.
///
/// НЕУДАЧА. Автоматически возвращается ТУННЕЛЬ на последнюю рабочую комбинацию
/// ([VpnNotifier.lastGood]); ВЫБОР пользователя не трогается. Довод против
/// «остаться в отказе»: с kill switch отказ означает вообще без сети. Довод
/// против отката выбора: интерфейс не имеет права молча менять то, что человек
/// выбрал. Повторных попыток по таймеру нет — это и есть череда разрывов.
library;

import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/core_policy.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Окно тишины: столько ждём последней правки, прежде чем рвать туннель.
const Duration kQuietWindow = Duration(seconds: 3);

/// Сколько держится строка «Переподключил» перед тем, как баннер исчезнет.
const Duration kDoneLinger = Duration(milliseconds: 2500);

/// Потолок ожидания одной попытки. Ядро обязано ответить `connected` или
/// `error`, но зависшая попытка оставила бы баннер со словом «Переподключаюсь»
/// навсегда, а это ложь о происходящем.
const Duration kAttemptLimit = Duration(seconds: 25);

/// Текст РУЧНОГО баннера. Живёт рядом с машиной, а не в виджете: его же
/// произносит `dismiss()`, когда человек отказался от автоматики, и разойтись
/// этим двум строкам нельзя.
const String kManualReconnectText =
    'Новые настройки применятся после переподключения.';

// ---------------------------------------------------------------- план пути

/// Ответ на вопрос «куда и как пойдёт трафик при следующем поднятии».
///
/// ХРАНИМ ИНДЕКСЫ ПИКЕРОВ, А НЕ РАЗРЕШЁННЫЕ ЗНАЧЕНИЯ ПРОВОДА, и это главное в
/// классе. `CorePolicy.relay` — код страны, полученный индексацией списка
/// релеев, а список приезжает с панели АСИНХРОННО и до этого равен
/// [Relay.defaults]. Сравнивай мы разрешённые значения, приезд списка сам по
/// себе выглядел бы как «пользователь сменил relay» и рвал бы живой туннель без
/// единого нажатия. Индексы меняет только запись в [CoreConfig] — то есть
/// человек или операторская синхронизация, и оба случая переподключения
/// заслуживают.
@immutable
class RoutePlan {
  /// Профиль подключения. Ключи узлов разных профилей несравнимы, поэтому он
  /// входит в план: без него смена профиля читалась бы как «ничего не
  /// изменилось».
  final String profileId;

  /// Закреплённая страна выхода (ISO-2); пусто — «авто».
  final String exitCountry;

  /// Закреплённый узел: имя прокси в импорте, `nodes.id` строкой на панели.
  final String exitNode;

  /// Индекс в [ProtocolOption.defaults].
  final int protocol;

  /// Индекс в [RoutingMode.defaults].
  final int route;

  /// Индекс в списке релеев.
  final int relay;

  const RoutePlan({
    this.profileId = '',
    this.exitCountry = '',
    this.exitNode = '',
    this.protocol = 0,
    this.route = 0,
    this.relay = 0,
  });

  /// Профиль уже известен. План без профиля — не «выбор пользователя», а
  /// состояние «ещё не загрузились», и разница между ними не событие.
  bool get resolved => profileId.isNotEmpty;

  /// Человеческое имя комбинации для баннера: «Канада · Hysteria2 · Режим».
  String get summary {
    final parts = <String>[
      exitCountry.isEmpty ? 'Авто' : countryNameOf(exitCountry),
      _nameAt(protocol, ProtocolOption.defaults.map((o) => o.name)),
      _nameAt(route, RoutingMode.defaults.map((m) => m.name)),
    ]..removeWhere((s) => s.isEmpty);
    return parts.join(' · ');
  }

  static String _nameAt(int index, Iterable<String> names) {
    final list = names.toList(growable: false);
    if (index < 0 || index >= list.length) return '';
    return list[index];
  }

  @override
  bool operator ==(Object other) =>
      other is RoutePlan &&
      other.profileId == profileId &&
      other.exitCountry == exitCountry &&
      other.exitNode == exitNode &&
      other.protocol == protocol &&
      other.route == route &&
      other.relay == relay;

  @override
  int get hashCode =>
      Object.hash(profileId, exitCountry, exitNode, protocol, route, relay);
}

/// Текущий план пути. Смена его значения — единственное, что заводит
/// автопереподключение.
final routePlanProvider = Provider<RoutePlan>((ref) {
  final profile = ref.watch(activeConnectionProfileProvider);
  final cfg = ref.watch(coreConfigProvider);
  final node = profile == null
      ? ''
      : (profile.isRaw
            ? (profile.selectedServerId ?? '')
            : (profile.selectedExitNodeId?.toString() ?? ''));
  return RoutePlan(
    profileId: profile?.id ?? '',
    exitCountry: normalizeCountryCode(profile?.selectedExitCountry),
    exitNode: node,
    protocol: cfg.protocol,
    route: cfg.route,
    relay: cfg.relay,
  );
});

/// Ключи политики, отвечающие на вопрос «куда и как идёт трафик».
///
/// Живут здесь, а не в `core_policy_mapping.dart`: там перевод выбора в
/// контракт ядра, а это — деление контракта на «применяется само» и «ждёт
/// человека», то есть решение об интерфейсе.
const Set<String> kPathPolicyKeys = <String>{'protocol', 'preset', 'relay'};

/// Отпечаток НАСТРОЕЧНОЙ половины политики: всё, кроме [kPathPolicyKeys].
///
/// Считается вычитанием, а не перечислением: поле, добавленное в политику
/// завтра, попадёт в настройки — то есть в сторону «спросить человека», а не в
/// сторону «порвать туннель молча».
String settingsSignature(CorePolicy policy) {
  final map = Map<String, Object?>.from(policy.toJson())
    ..removeWhere((k, _) => kPathPolicyKeys.contains(k));
  return jsonEncode(map);
}

// -------------------------------------------------------------- состояние

enum AutoReconnectPhase {
  /// Ничего не ждём и ничего не делаем.
  idle,

  /// Идёт окно тишины: смена принята, туннель ещё не тронут.
  pending,

  /// Туннель поднимается заново (или возвращается на прежнюю комбинацию).
  reconnecting,

  /// Подняли; строка живёт [kDoneLinger] и гаснет сама.
  done,

  /// Не поднялось. Что стало с туннелем — сказано в [AutoReconnectState.message].
  failed,

  /// Человек отказался от автоматики («Не сейчас»). Баннер переходит в ручной
  /// режим, выбор при этом НЕ откатывается.
  manual,
}

@immutable
class AutoReconnectState {
  final AutoReconnectPhase phase;

  /// Комбинация, ради которой всё затевается («Канада · Hysteria2 · Режим»).
  final String summary;

  /// Готовая строка баннера. Формулировки живут здесь, а не в виджете: одно
  /// событие — одна фраза, и её видно в тесте состояния.
  final String message;

  const AutoReconnectState({
    this.phase = AutoReconnectPhase.idle,
    this.summary = '',
    this.message = '',
  });

  static const AutoReconnectState idle = AutoReconnectState();

  /// Баннер обязан быть на экране: что-то ждёт, идёт или уже случилось.
  bool get visible => phase != AutoReconnectPhase.idle;

  @override
  bool operator ==(Object other) =>
      other is AutoReconnectState &&
      other.phase == phase &&
      other.summary == summary &&
      other.message == message;

  @override
  int get hashCode => Object.hash(phase, summary, message);
}

// --------------------------------------------------------------- нотифаер

/// Машина автопереподключения. Ничего не знает про виджеты и про Riverpod:
/// снаружи приходят четыре замыкания и два наблюдения (план и стадия).
class AutoReconnectNotifier extends StateNotifier<AutoReconnectState> {
  /// Опустить и снова поднять туннель по ТЕКУЩЕМУ выбору.
  final Future<bool> Function() _reconnect;

  /// Поднять туннель на последней рабочей комбинации, не трогая выбор.
  final Future<bool> Function() _restore;

  /// Есть ли вообще куда возвращаться.
  final bool Function() _hasLastGood;

  /// Стадия туннеля на момент вопроса.
  final VpnStage Function() _stage;

  final Duration window;
  final Duration linger;
  final Duration attemptLimit;

  /// Последний увиденный план. Сравнивается со следующим — так отличается
  /// СОБЫТИЕ «человек сменил» от стоячего расхождения, которое могло приехать
  /// с холодного старта поверх живого туннеля.
  RoutePlan? _plan;

  /// Стадия прошлого кадра: `_lastGoodSummary` обновляется только на ПЕРЕХОДЕ
  /// в connected. Повторный кадр той же стадии (его шлёт `refreshStage()` при
  /// появлении экрана) иначе переписал бы «прежнюю рабочую» комбинацию уже
  /// новым, ещё не применённым выбором.
  VpnStage? _prevStage;

  /// Имя комбинации, на которой туннель в последний раз реально поднялся.
  String _lastGoodSummary = '';

  /// Комбинация, ради которой затеяно текущее переподключение.
  String _target = '';

  Timer? _quiet;
  Timer? _lingerTimer;
  Timer? _guard;

  /// Окно истекло, но туннель был в подъёме: ждём `connected`.
  bool _dueWhenUp = false;

  /// Ждёт результата текущей попытки. Завершается наблюдением стадии.
  Completer<bool>? _settle;

  /// Страна выхода, на котором туннель стоит СЕЙЧАС (ISO-2); пусто — неизвестна.
  final String Function() _liveCountry;

  /// Машина живого выхода. Ключ машины, а не прокси: у одной машины несколько
  /// входов, и выход у них общий.
  final String Function() _liveMachine;

  /// Машина, которой принадлежит узел плана. Пусто — узнать не удалось.
  final String Function(String nodeKey) _machineOf;

  AutoReconnectNotifier({
    required Future<bool> Function() reconnect,
    required Future<bool> Function() restoreLastGood,
    required bool Function() hasLastGood,
    required VpnStage Function() stage,
    String Function()? liveCountry,
    String Function()? liveMachine,
    String Function(String nodeKey)? machineOf,
    this.window = kQuietWindow,
    this.linger = kDoneLinger,
    this.attemptLimit = kAttemptLimit,
  }) : _reconnect = reconnect,
       _restore = restoreLastGood,
       _hasLastGood = hasLastGood,
       _stage = stage,
       _liveCountry = liveCountry ?? _emptyString,
       _liveMachine = liveMachine ?? _emptyString,
       _machineOf = machineOf ?? _noMachine,
       super(AutoReconnectState.idle);

  static String _emptyString() => '';
  static String _noMachine(String _) => '';

  /// Туннель есть или вот-вот будет: только тогда смена что-то рвёт. Вне
  /// сессии применять нечего — следующий connect и так возьмёт новый выбор.
  bool get _live {
    final s = _stage();
    return s == VpnStage.connected ||
        s == VpnStage.connecting ||
        s == VpnStage.reconnecting;
  }

  /// Меняет ли новый план то, что туннель делает ПРЯМО СЕЙЧАС.
  ///
  /// План и результат — разные вещи, и мерить надо результат. Закрепление
  /// страны, на которой ядро уже стоит, меняет план и не меняет ровно ничего в
  /// проводе. Рвать ради этого живой туннель — чистая потеря связи; само
  /// закрепление сохраняется и вступит в силу на следующем подъёме.
  ///
  /// Сравнение идёт по МАШИНЕ, а не по стране, и это не педантизм. Выбор
  /// страны в приложении разрешается в конкретный узел (иначе на сыром пути
  /// «Канада» с галочкой уживалась бы с выходом через Германию — `connectRaw`
  /// знает имя прокси и не знает стран). Поэтому нажатие «Канада» при живом
  /// «🇨🇦 Stream» закрепляет, скажем, «🇨🇦 Stealth»: ДРУГОЙ ВХОД ТОЙ ЖЕ МАШИНЫ.
  /// Выход при этом байт в байт тот же самый — снято на устройстве, адрес до и
  /// после разрыва совпадал. Сравнение по стране этот случай пропускало, потому
  /// что вместе со страной менялся и узел.
  ///
  /// Всё остальное — тип, режим, релэй, профиль — рвёт как раньше: там меняется
  /// именно то, ради чего туннель и поднимают заново.
  bool _outcomeUnchanged(RoutePlan prev, RoutePlan next) {
    final sameEverythingElse =
        prev.profileId == next.profileId &&
        prev.protocol == next.protocol &&
        prev.route == next.route &&
        prev.relay == next.relay;
    if (!sameEverythingElse) return false;

    // Пустой узел это «авто»: ядро остаётся на том, на чём стоит.
    if (next.exitNode.isEmpty) {
      if (next.exitCountry.isEmpty) return true;
      final live = _liveCountry().trim().toUpperCase();
      return live.isNotEmpty && live == next.exitCountry.trim().toUpperCase();
    }

    final liveMachine = _liveMachine().trim();
    if (liveMachine.isEmpty) return false;
    final target = _machineOf(next.exitNode).trim();
    // Машину узла определить не удалось — рвём, а не гадаем.
    return target.isNotEmpty && target == liveMachine;
  }

  /// Новый план пути.
  void observePlan(RoutePlan next) {
    final prev = _plan;
    _plan = next;
    // База, а не событие: до профиля сравнивать не с чем, и первое же
    // разрешение профиля на холодном старте порвало бы живой туннель.
    if (prev == null || !prev.resolved || !next.resolved) return;
    if (prev == next) return;
    if (!_live) return;
    if (_outcomeUnchanged(prev, next)) return;
    // Пока попытка в полёте, план не трогаем: её результат важнее, а новая
    // смена в этот момент означала бы разрыв поверх разрыва.
    if (state.phase == AutoReconnectPhase.reconnecting) return;

    _target = next.summary;
    _set(
      AutoReconnectState(
        phase: AutoReconnectPhase.pending,
        summary: _target,
        message: 'Применю через ${window.inSeconds} с: $_target',
      ),
    );
    _dueWhenUp = false;
    _quiet?.cancel();
    _quiet = Timer(window, _onWindowClosed);
  }

  /// Новая стадия туннеля.
  void observeStage(VpnStage stage) {
    final prev = _prevStage;
    _prevStage = stage;
    if (stage == VpnStage.connected && prev != VpnStage.connected) {
      _lastGoodSummary = _plan?.summary ?? _lastGoodSummary;
    }

    switch (state.phase) {
      case AutoReconnectPhase.pending:
        if (stage == VpnStage.connected && _dueWhenUp) {
          _dueWhenUp = false;
          unawaited(_run(_reconnect));
        } else if (stage == VpnStage.disconnected || stage == VpnStage.error) {
          // Туннель ушёл сам (человек нажал орб, ядро упало) — переподключать
          // нечего, а поднимать его без спроса мы не имеем права.
          _cancelQuiet();
          _set(AutoReconnectState.idle);
        }
      case AutoReconnectPhase.reconnecting:
        // Промежуточные `disconnected`/`connecting` — это наш собственный
        // разрыв внутри попытки, и результатом они не являются.
        if (stage == VpnStage.connected) {
          _finish(true);
        } else if (stage == VpnStage.error) {
          _finish(false);
        }
      case AutoReconnectPhase.manual:
      case AutoReconnectPhase.failed:
        // Обе фразы говорят о ЖИВОМ туннеле: «применится после
        // переподключения» и «вернул прежнее». Туннель опущен — говорить не о
        // чем, а оставленный баннер обещал бы применить что-то к тому, чего
        // нет; следующий connect и так возьмёт текущий выбор.
        if (stage == VpnStage.disconnected || stage == VpnStage.error) {
          _set(AutoReconnectState.idle);
        }
      case AutoReconnectPhase.idle:
      case AutoReconnectPhase.done:
        break;
    }
  }

  /// «Не сейчас»: автоматику отменяем, выбор оставляем. Баннер становится
  /// обычным ручным — так человек не теряет ни выбор, ни возможность применить.
  void dismiss() {
    _cancelQuiet();
    _dueWhenUp = false;
    _set(
      const AutoReconnectState(
        phase: AutoReconnectPhase.manual,
        message: kManualReconnectText,
      ),
    );
  }

  /// «Переподключить» / «Повторить». Одна дорога для ручной кнопки и для
  /// повтора после отказа: иначе у одного действия было бы два поведения.
  Future<void> reconnectNow() {
    _cancelQuiet();
    _dueWhenUp = false;
    if (_target.isEmpty) _target = _plan?.summary ?? '';
    return _run(_reconnect);
  }

  void _onWindowClosed() {
    _quiet = null;
    final s = _stage();
    if (s == VpnStage.connecting || s == VpnStage.reconnecting) {
      // Рвать поднимающийся туннель бессмысленно: дождёмся `Up` и применим.
      _dueWhenUp = true;
      return;
    }
    if (s != VpnStage.connected) {
      _set(AutoReconnectState.idle);
      return;
    }
    unawaited(_run(_reconnect));
  }

  /// Одна попытка: перевести в [AutoReconnectPhase.reconnecting], дождаться
  /// стадии и, если не вышло, вернуть туннель на прежнюю комбинацию.
  Future<void> _run(Future<bool> Function() attempt) async {
    _set(
      AutoReconnectState(
        phase: AutoReconnectPhase.reconnecting,
        summary: _target,
        message: _target.isEmpty
            ? 'Переподключаюсь…'
            : 'Переподключаюсь: $_target',
      ),
    );
    final ok = await _await(attempt);
    if (!mounted) return;
    if (ok) {
      _set(
        AutoReconnectState(
          phase: AutoReconnectPhase.done,
          summary: _target,
          message: _target.isEmpty
              ? 'Переподключил.'
              : 'Переподключил: $_target',
        ),
      );
      _lingerTimer?.cancel();
      _lingerTimer = Timer(linger, () {
        if (state.phase == AutoReconnectPhase.done) {
          _set(AutoReconnectState.idle);
        }
      });
      return;
    }

    final head = _target.isEmpty ? 'Подключение' : _target;
    if (!_hasLastGood()) {
      _set(
        AutoReconnectState(
          phase: AutoReconnectPhase.failed,
          summary: _target,
          message: '$head не поднялось. Прежнее подключение вернуть нечем.',
        ),
      );
      return;
    }

    // Возвращаем ТУННЕЛЬ, а не выбор: выбор остаётся тем, что сделал человек,
    // и расхождение теперь называется словами, а не молчаливым откатом.
    final back = _lastGoodSummary.isEmpty
        ? 'прежнее подключение'
        : _lastGoodSummary;
    _set(
      AutoReconnectState(
        phase: AutoReconnectPhase.reconnecting,
        summary: _target,
        message: 'Возвращаю: $back',
      ),
    );
    final restored = await _await(_restore);
    if (!mounted) return;
    _set(
      AutoReconnectState(
        phase: AutoReconnectPhase.failed,
        summary: _target,
        message: restored
            ? '$head не поднялось — вернул $back. Выбор сохранён.'
            : '$head не поднялось. Вернуть прежнее подключение тоже не вышло.',
      ),
    );
  }

  /// Запускает попытку и ждёт, чем она кончилась. Ответ приходит наблюдением
  /// стадии ([observeStage]), потому что `connect` возвращает управление
  /// сразу — он лишь ОТПРАВЛЯЕТ команду ядру.
  Future<bool> _await(Future<bool> Function() attempt) async {
    final done = Completer<bool>();
    _settle = done;
    _guard?.cancel();
    _guard = Timer(attemptLimit, () {
      if (!done.isCompleted) done.complete(false);
    });
    final started = await attempt();
    if (!started && !done.isCompleted) done.complete(false);
    final ok = await done.future;
    _guard?.cancel();
    _guard = null;
    if (identical(_settle, done)) _settle = null;
    return ok;
  }

  void _finish(bool ok) {
    final settle = _settle;
    if (settle != null && !settle.isCompleted) settle.complete(ok);
  }

  void _cancelQuiet() {
    _quiet?.cancel();
    _quiet = null;
  }

  /// Попытка живёт дольше нотифаера: команда уже у ядра, а контейнер могли
  /// закрыть (тест, смена аккаунта). Писать состояние в мёртвый объект —
  /// исключение на ровном месте, поэтому запись идёт только через это.
  void _set(AutoReconnectState next) {
    if (mounted) state = next;
  }

  @override
  void dispose() {
    _cancelQuiet();
    _lingerTimer?.cancel();
    _guard?.cancel();
    final settle = _settle;
    if (settle != null && !settle.isCompleted) settle.complete(false);
    super.dispose();
  }
}

/// Машина автопереподключения приложения.
///
/// Наблюдения подписываются ЗДЕСЬ, а не в конструкторе нотифаера: так нотифаер
/// остаётся проверяемым без контейнера, а провайдер — единственным местом, где
/// он склеен с остальным состоянием.
final autoReconnectProvider =
    StateNotifierProvider<AutoReconnectNotifier, AutoReconnectState>((ref) {
      final notifier = AutoReconnectNotifier(
        reconnect: () => ref.read(vpnProvider.notifier).reconnect(),
        restoreLastGood: () => ref.read(vpnProvider.notifier).restoreLastGood(),
        hasLastGood: () => ref.read(vpnProvider.notifier).lastGood != null,
        stage: () => ref.read(vpnProvider).stage,
        // Страна ЖИВОГО выхода: имя активного прокси разрешается в факт флота.
        // Нужна ровно для того, чтобы не рвать туннель ради выбора, который
        // ничего в нём не меняет.
        liveCountry: () {
          final active = ref.read(activeProxyProvider);
          if (active == null || active.isEmpty) return '';
          return ref.read(fleetFactsProvider)[active]?.countryCode ?? '';
        },
        liveMachine: () {
          final active = ref.read(activeProxyProvider);
          if (active == null || active.isEmpty) return '';
          return ref.read(fleetFactsProvider)[active]?.exitKey ?? '';
        },
        // Ключ узла в плане разного рода на разных путях: на импортированной
        // подписке это ИМЯ ПРОКСИ, на панельном профиле — `nodes.id`, то есть
        // сразу ключ машины. Ищем сперва по имени, потом среди самих машин; не
        // нашли ни там ни там — возвращаем пусто, и правило честно рвёт вместо
        // того, чтобы сравнить ключ неизвестного рода с чем попало.
        machineOf: (nodeKey) {
          final facts = ref.read(fleetFactsProvider);
          final byProxy = facts[nodeKey]?.exitKey ?? '';
          if (byProxy.isNotEmpty) return byProxy;
          for (final f in facts.values) {
            if (f.exitKey == nodeKey) return f.exitKey;
          }
          return '';
        },
      );
      ref.listen<RoutePlan>(
        routePlanProvider,
        (_, next) => notifier.observePlan(next),
        fireImmediately: true,
      );
      ref.listen<VpnStage>(
        vpnProvider.select((s) => s.stage),
        (_, next) => notifier.observeStage(next),
        fireImmediately: true,
      );
      return notifier;
    });

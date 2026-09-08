/// Состояние CSM/1 активного профиля и провайдеры, за которыми смотрит UI.
///
/// Нормативно: 02-SPEC.md 2.1 (автомат профиля), 6 (возможности), 7
/// (настройки, карточки), 8.1 и 8.3 (лестница), 8.8 (что пользователь обязан
/// видеть: INV-17..INV-21), 9 (энроллмент), INV-13 (липкое правило).
///
/// Разделение обязанностей: этот файл ВЛАДЕЕТ состоянием и правилами перехода;
/// экраны только читают провайдеры ниже и вызывают методы нотифаера. Ни один
/// экран не собирает состояние CSM сам.
library;

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_enrollment.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/csm_write.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_policy_mapping.dart';
import 'package:caramba_client/state/providers.dart';

/// Исход одной попытки отдать очередь записей оператору.
enum CsmWriteFlushOutcome {
  /// Отдавать нечего.
  idle,

  /// Профиль не закреплял корневой ключ: записывать некуда.
  notEnrolled,

  /// Бит 3 снят: оператор записи настроек не предлагает.
  notOffered,

  /// Запрос ушёл и подписанный ответ принят ядром.
  sent,

  /// Сеть или панель отказали. Очередь цела, локальное значение цело.
  failed,
}

/// Чем закончилось применение порядка и переключателей ступеней.
enum CsmLadderApplyOutcome {
  /// Порядок записан локально И принят ядром.
  applied,

  /// Профиль не закреплял корневой ключ: применять некуда.
  notEnrolled,

  /// Локально записано, а ядро отказало. Экран обязан сказать об этом:
  /// показывать порядок, по которому ядро не ходит, значит врать.
  coreRefused,
}

/// Что видно на экране настроек про судьбу последней записи.
///
/// Четыре исхода [CsmWriteFlushOutcome] раньше не доходили ни до одного
/// виджета: `setByUser` отдавал их в `unawaited`, и пользователь менял
/// настройку, локальное значение применялось, запись молча не уходила, и
/// ничто на экране об этом не говорило.
class CsmWriteStatus {
  const CsmWriteStatus({this.outcome, this.pending = 0, this.atMs = 0});

  static const CsmWriteStatus none = CsmWriteStatus();

  /// Исход последней попытки отдачи. `null` до первой попытки.
  final CsmWriteFlushOutcome? outcome;

  /// Сколько записей ещё лежит в очереди.
  final int pending;

  /// Когда исход был получен.
  final int atMs;

  /// Есть ли что показывать пользователю.
  ///
  /// `idle` и `sent` при пустой очереди это нормальная тишина: показывать
  /// «всё хорошо» после каждого движения ползунка незачем.
  bool get isNoteworthy =>
      outcome == CsmWriteFlushOutcome.notOffered ||
      outcome == CsmWriteFlushOutcome.failed ||
      pending > 0;

  /// Отказ постоянный (оператор записи не предлагает вовсе), а не временный.
  bool get isPermanent => outcome == CsmWriteFlushOutcome.notOffered;
}

// ------------------------------------------------------- модели для UI

/// Личность оператора, INV-18. Всё, что здесь есть, обязано быть на экране:
/// имя, отпечаток корневого ключа группами по четыре, дата энроллмента, как
/// установлен пин и менялся ли он когда-нибудь.
class CsmOperatorIdentity {
  const CsmOperatorIdentity({
    required this.displayName,
    required this.pid,
    required this.fingerprint,
    required this.pinOrigin,
    required this.enrolledAtMs,
    required this.pinEverChanged,
    required this.hardwareTier,
    required this.stage,
  });

  /// Инертный текст из бутстрап-блоба либо имя профиля. Никогда не ключ и
  /// никогда не эхо (INV-10, INV-11).
  final String displayName;

  final String pid;

  /// `link_pin` группами по четыре.
  final String fingerprint;

  final CsmPinOrigin pinOrigin;
  final int enrolledAtMs;

  /// Непустая история пина. В CSM/1 законный способ сменить пин один, удалить
  /// профиль и завести заново, поэтому истина здесь это то, что пользователь
  /// обязан увидеть.
  final bool pinEverChanged;

  final CsmHardwareTier hardwareTier;
  final CsmProfileStage stage;
}

/// Состояние проверки документов в работе, INV-19: версия, выпущен, истекает,
/// отпечаток подписавшего, результат проверки.
class CsmDocumentState {
  const CsmDocumentState({
    required this.nowSec,
    this.keyDocument,
    this.catalog,
    this.directive,
    this.capabilitiesDisagree = false,
    this.fleetRootAnchored = true,
  });

  final int nowSec;
  final CsmDocumentRecord? keyDocument;
  final CsmDocumentRecord? catalog;
  final CsmDocumentRecord? directive;

  /// `cap` каталога и директивы разошлись: устройство живёт на старом
  /// каталоге, и оператор обязан это видеть (02-SPEC.md 6.5).
  final bool capabilitiesDisagree;

  /// Ключевой документ несёт запись `tiers` для тира этой директивы. Ложь
  /// рендерится как `fleet not root-anchored` (02-SPEC.md 8.8.2).
  final bool fleetRootAnchored;

  /// Просроченный доверенный ключевой документ ОСТАЁТСЯ якорем авторизации.
  /// `exp` на нём говорит, когда его следовало перезапросить, а не когда он
  /// перестаёт быть якорем (02-SPEC.md 2.2).
  bool get anchorPastExpiry {
    final k = keyDocument;
    return k != null && k.isExpiredAt(nowSec);
  }

  bool get hasAnything =>
      keyDocument != null || catalog != null || directive != null;
}

/// Одна ступень лестницы для экрана транспортов, INV-17.
///
/// Недоступная ступень рендерится ВИДИМОЙ И ВЫКЛЮЧЕННОЙ с причиной, никогда не
/// прячется: ступень это обещание, которое приложение даёт о себе, и
/// пользователь вправе проверить весь список. Контрол, завязанный на бит
/// возможности, наоборот прячется, и это различие намеренное (02-SPEC.md 6.2).
class CsmLadderRung {
  const CsmLadderRung({
    required this.rung,
    required this.position,
    required this.enabled,
    this.reason,
  });

  final CsmRung rung;

  /// Место в действующем порядке. R0 всегда 0.
  final int position;

  final bool enabled;

  /// Почему ступень недоступна. `null`, когда она доступна.
  final CsmUnavailableReason? reason;

  bool get available => reason == null;
}

/// Лестница целиком: порядок, включённость и причины.
class CsmLadderState {
  const CsmLadderState({required this.rungs, required this.userTouched});

  static const CsmLadderState empty = CsmLadderState(
    rungs: <CsmLadderRung>[],
    userTouched: false,
  );

  /// Все семь ступеней, в действующем порядке. Список полный всегда: скрывать
  /// ступень нельзя (INV-17).
  final List<CsmLadderRung> rungs;

  /// Пользователь трогал порядок или включённость. С этого момента подписанные
  /// умолчания каталога его выбор не переписывают (02-SPEC.md 8.3).
  final bool userTouched;
}

/// Возраст конфигурации и её источник, INV-21.
class CsmConfigurationAge {
  const CsmConfigurationAge({
    required this.ageSec,
    required this.source,
    required this.runningOnCache,
    required this.stage,
  });

  static const CsmConfigurationAge unknown = CsmConfigurationAge(
    ageSec: null,
    source: null,
    runningOnCache: false,
    stage: CsmProfileStage.unenrolled,
  );

  /// Сколько секунд назад проверена самая свежая директива. `null`, когда её
  /// не было ни разу.
  final int? ageSec;

  /// Ступень, принёсшая конфигурацию.
  final CsmRung? source;

  /// Клиент работает на кэше. Это НЕ ошибка: это нормальное состояние
  /// устройства в блокированной сети, и рендерится оно как «работает на
  /// сохранённой конфигурации, N часов» (02-SPEC.md 2.1 правило 2).
  final bool runningOnCache;

  final CsmProfileStage stage;

  int? get ageHours => ageSec == null ? null : ageSec! ~/ 3600;
}

/// Жёсткая ошибка липкого правила, INV-13.
class CsmStickyError {
  const CsmStickyError({required this.pid, required this.stage});

  final String pid;
  final CsmProfileStage stage;

  /// Недиссмиссабельна по определению: это не баннер, который можно закрыть.
  bool get dismissible => false;
}

// ------------------------------------------------------------ нотифаер

/// Единственная точка мутации состояния CSM активного профиля.
///
/// Пишет через [ConnectionProfilesNotifier], поэтому состояние CSM переживает
/// перезапуск ровно так же, как остальной профиль, и не заводит второго дома
/// для отметок максимума версий (02-SPEC.md 5.1).
class CsmNotifier {
  CsmNotifier(this._ref);

  final Ref _ref;

  ConnectionProfile? get _profile => _ref.read(activeConnectionProfileProvider);

  CsmProfileState? get state => _profile?.csm;

  /// Ворота, через которые проходит КАЖДАЯ мутация состояния CSM.
  ///
  /// Каждая из них читает состояние профиля целиком и целиком возвращает.
  /// Две такие, начатые в одном кадре, читают одно и то же, вторая пишет
  /// поверх первой, и первая пропадает вместе с очередью, в которой стояла её
  /// запись. Сегодня это не проявляется только потому, что нижний нотифаер
  /// успевает поставить состояние синхронно; правильность, зависящая от такого
  /// совпадения, ломается в тот день, когда между чтением и записью появляется
  /// await. Ворота делают участок «прочитал, посчитал, записал» неделимым.
  ///
  /// Сеть в ворота НЕ ЗАХОДИТ: изменение настроек не блокируется на сети
  /// (02-SPEC.md 7.8), поэтому [flushWrites] держит их только на своих двух
  /// записях, а раунд-трип идёт снаружи.
  Future<void> _gate = Future<void>.value();

  Future<T> _serial<T>(Future<T> Function() body) {
    final previous = _gate;
    final done = Completer<void>();
    // Ворота никогда не завершаются ошибкой: отказ одной мутации не имеет
    // права остановить очередь остальных.
    _gate = done.future;
    return previous.then((_) => body()).whenComplete(done.complete);
  }

  Future<void> _write(CsmProfileState next) async {
    final profile = _profile;
    if (profile == null) {
      return;
    }
    await _ref
        .read(connectionProfilesProvider.notifier)
        .setCsm(profile.id, next);
  }

  /// Устанавливает пин из проверенного бутстрап-блоба. Единственный момент,
  /// когда доверие создаётся, а не проверяется.
  ///
  /// Пин можно установить ТОЛЬКО пока профиль в `unenrolled` или `pinning`.
  /// После этого он неизменен на всю жизнь профиля, и смена пина это удаление
  /// профиля с новым энроллментом (02-SPEC.md 2.1 правило 1). Метод возвращает
  /// `false`, когда пин уже закреплён, и НЕ трогает состояние.
  Future<bool> establishPin(CsmBootstrap bootstrap, {int? nowMs}) =>
      _serial(() async {
        final profile = _profile;
        if (profile == null) {
          return false;
        }
        final current = profile.csm;
        if (current != null && current.stage.isPinned) {
          return false;
        }
        final at = nowMs ?? DateTime.now().millisecondsSinceEpoch;
        await _write(csmProfileFromBootstrap(bootstrap, nowMs: at));
        return true;
      });

  /// Устанавливает пин из ссылки энроллмента, несущей `k`.
  ///
  /// Слабее, чем блоб, продиктованный вне полосы: пин приехал с того же origin,
  /// что и документы, и экран личности оператора обязан это говорить (INV-18).
  /// Как и [establishPin], работает только пока профиль не закрепил корень.
  Future<bool> establishPinFromLink(CsmEnrollLink link, {int? nowMs}) =>
      _serial(() async {
        final profile = _profile;
        if (profile == null) {
          return false;
        }
        final current = profile.csm;
        if (current != null && current.stage.isPinned) {
          return false;
        }
        final at = nowMs ?? DateTime.now().millisecondsSinceEpoch;
        final next = csmProfileFromLink(link, nowMs: at);
        if (next == null) {
          return false;
        }
        await _write(next);
        return true;
      });

  /// Профиль перешёл в `anchored`: первый ключевой документ проверен против
  /// пина, `pid` посчитан и закреплён, временной пол установлен.
  Future<void> anchor({
    required String pid,
    required CsmDocumentRecord keyDocument,
    required int timeFloorSec,
  }) => _serial(() async {
    final current = state;
    if (current == null) {
      return;
    }
    await _write(
      current.copyWith(
        pin: CsmPin(
          pid: pid,
          linkPin: current.pin.linkPin,
          origin: current.pin.origin,
          establishedMs: current.pin.establishedMs,
        ),
        stage: CsmProfileStage.anchored,
        keyDocument: keyDocument,
        // Временной пол монотонен и НИКОГДА не уменьшается.
        timeFloorSec: timeFloorSec > current.timeFloorSec
            ? timeFloorSec
            : current.timeFloorSec,
      ),
    );
  });

  /// Отмечает, что пришедший документ не нёс `cap`. На закреплённом профиле
  /// это жёсткая недиссмиссабельная ошибка, а не откат (INV-13).
  Future<void> markMissingCapability() => _serial(() async {
    final current = state;
    if (current == null) {
      return;
    }
    await _write(csmMarkMissingCapability(current));
  });

  /// Пользователь ответил «Оставить»: локальное значение удерживается, ключ
  /// помечается пользовательским, и запись, переутверждающая его, встаёт в
  /// очередь (02-SPEC.md 7.7).
  Future<void> keepCard(String cardId, {int? nowMs}) => _serial(() async {
    final current = state;
    if (current == null) {
      return;
    }
    final card = _cardById(current, cardId);
    if (card == null) {
      return;
    }
    final at = nowMs ?? DateTime.now().millisecondsSinceEpoch;
    var settings = current.settings;
    var queue = current.writeQueue;
    for (final item in card.items) {
      settings = settings.setByUser(item.key, item.current);
      queue = queue.enqueue(
        CsmQueuedWrite(
          key: item.key,
          op: CsmWantSet(item.current),
          queuedMs: at,
        ),
      );
    }
    await _write(
      current.copyWith(
        settings: settings,
        writeQueue: queue,
        pendingChanges: _without(current.pendingChanges, cardId),
      ),
    );
  });

  /// Пользователь ответил «Вернуть»: применяется значение оператора и отметка
  /// пользователя по этому ключу снимается.
  Future<void> revertCard(String cardId) => _serial(() async {
    final current = state;
    if (current == null) {
      return;
    }
    final card = _cardById(current, cardId);
    if (card == null) {
      return;
    }
    final entries = Map<CsmSettingKey, CsmSettingEntry>.from(
      current.settings.entries,
    );
    for (final item in card.items) {
      entries[item.key] = CsmSettingEntry(
        value: item.proposed,
        src: item.src,
        userSet: false,
      );
    }
    final merged = CsmSettings(entries: entries);
    await _write(
      current.copyWith(
        settings: merged,
        pendingChanges: _without(current.pendingChanges, cardId),
      ),
    );
    // Карточка обещает пользователю "пока вы не ответите, действует ваше
    // значение". Ответ "принять новое" обязан это обещание закрыть: без
    // пересборки локального конфига провенанс и состояние CSM говорят, что
    // действует значение оператора, а ядро, пикеры настроек и баннер
    // переподключения продолжают жить на старом. Одно значение на настройку.
    _applyToCoreConfig(merged);
  });

  /// Переносит принятые значения CSM в локальную политику ядра.
  ///
  /// Прямое отображение уже написано (`corePolicyFromCsm`), обратное тоже
  /// (`coreConfigFromPolicy`); здесь они соединяются, чтобы пикер, политика и
  /// баннер переподключения двигались вместе.
  void _applyToCoreConfig(CsmSettings merged) {
    final notifier = _ref.read(coreConfigProvider.notifier);
    final current = _ref.read(coreConfigProvider);
    final next = coreConfigFromPolicy(
      current,
      corePolicyFromCsm(merged, current),
    );
    if (next == current) {
      return;
    }
    notifier.state = next;
  }

  /// Пользователь поменял настройку сам: принимается локально и немедленно,
  /// запись встаёт в очередь и уходит по любой доступной ступени. Ни одно
  /// изменение настроек не блокируется на сети (02-SPEC.md 7.8).
  ///
  /// «Принято немедленно» это не «действует немедленно»: ядро применяет
  /// политику на СЛЕДУЮЩЕМ `Up`, поэтому изменение ключа, от которого зависит
  /// работающий туннель, вступит в силу после переподключения, и приложение
  /// поднимает баннер переподключения, а не рвёт туннель само.
  Future<void> setByUser(
    CsmSettingKey key,
    CsmSettingValue value, {
    int? nowMs,
  }) async {
    final at = nowMs ?? DateTime.now().millisecondsSinceEpoch;
    final accepted = await _serial(() async {
      // Состояние читается ВНУТРИ ворот: правка, начатая в том же кадре, что и
      // соседняя, обязана видеть её результат, а не то, что было до неё.
      final current = state;
      if (current == null || !csmValueInVocabulary(key, value)) {
        return false;
      }
      await _write(
        current.copyWith(
          settings: current.settings.setByUser(key, value),
          writeQueue: current.writeQueue.enqueue(
            CsmQueuedWrite(key: key, op: CsmWantSet(value), queuedMs: at),
          ),
        ),
      );
      return true;
    });
    if (!accepted) {
      return;
    }
    // Немедленно пробуем отдать очередь оператору. Ошибка сети не откатывает
    // локальное значение и не теряет запись: она остаётся в очереди и уйдёт по
    // любой доступной ступени позже (02-SPEC.md 7.8, инвариант 16).
    //
    // Отдача идёт ВНЕ ворот: они держатся только на записях состояния, потому
    // что изменение настроек не имеет права ждать сеть. Исход при этом не
    // выбрасывается: он публикуется в csmWriteStatusProvider, и экран
    // настроек показывает "не доставлено" вместо тишины.
    unawaited(flushWrites(nowMs: at));
  }

  /// Отдаёт очередь записей оператору одним подписанным запросом.
  ///
  /// Весь round trip живёт в ядре: оно собирает тело CBOR, подписывает прообраз
  /// 03-WIRE.md 13.6 ключом устройства, ставит `X-CSM-Proof` и `If-Match`, идёт
  /// по лестнице транспортов и ПРОВЕРЯЕТ подписанный запечатанный ответ прежде,
  /// чем что-либо применить. Слой Dart не открывает сокетов к оператору и не
  /// проверяет подпись второй раз по перекодированным байтам.
  ///
  /// Возвращает исход попытки. Отказ НЕ очищает очередь и НЕ трогает локально
  /// принятое значение: изменение настроек не блокируется на сети.
  /// Идёт ли отдача прямо сейчас.
  ///
  /// Две быстрые правки подряд давали две одновременные отдачи, каждая со
  /// своим nonce и с одним и тем же снимком очереди: панели приходилось гасить
  /// дубликат своей идемпотентностью по nonce, а лестница тратила два запроса
  /// там, где нужен один. Отдача сериализуется, и хвост, поставленный в
  /// очередь во время раунд-трипа, уходит следующей отдачей.
  Future<CsmWriteFlushOutcome>? _inFlight;

  Future<CsmWriteFlushOutcome> flushWrites({int? nowMs}) {
    final running = _inFlight;
    if (running != null) {
      return running;
    }
    final started = _flushOnce(nowMs);
    _inFlight = started;
    return started.whenComplete(() {
      if (identical(_inFlight, started)) {
        _inFlight = null;
      }
    });
  }

  Future<CsmWriteFlushOutcome> _flushOnce(int? nowMs) async {
    final at = nowMs ?? DateTime.now().millisecondsSinceEpoch;
    final outcome = await _flushWrites(at);
    // Исход публикуется ВСЕГДА, включая успех: экран настроек читает его и
    // показывает "не доставлено" там, где раньше была тишина.
    _publishWriteStatus(outcome, at);
    return outcome;
  }

  void _publishWriteStatus(CsmWriteFlushOutcome outcome, int atMs) {
    _ref.read(csmWriteStatusProvider.notifier).state = CsmWriteStatus(
      outcome: outcome,
      pending: state?.writeQueue.length ?? 0,
      atMs: atMs,
    );
  }

  Future<CsmWriteFlushOutcome> _flushWrites(int at) async {
    // Первый участок под воротами: прополка очереди и снятие снимка того, что
    // уходит. Запись, не доставленная 7 суток, выбрасывается: она описывает
    // желание, которого пользователь давно не помнит (02-SPEC.md 7.8).
    final (outcome, sending) = await _serial(() async {
      final current = state;
      if (current == null || !current.stage.isPinned) {
        return (CsmWriteFlushOutcome.notEnrolled, const <CsmQueuedWrite>[]);
      }
      final pruned = current.writeQueue.prune(at);
      if (pruned.length != current.writeQueue.length) {
        await _write(current.copyWith(writeQueue: pruned));
      }
      if (pruned.isEmpty) {
        return (CsmWriteFlushOutcome.idle, const <CsmQueuedWrite>[]);
      }
      // Бит 3 снят означает, что оператор записи настроек не предлагает. Слать
      // запрос всё равно значило бы дать пользователю обещание, которого панель
      // не давала.
      final nowSec = at ~/ 1000;
      final caps = current
          .effectiveOperatorCapabilities(nowSec)
          .intersectWithClient();
      if (!caps.has(CsmCapability.settingsWrite)) {
        return (CsmWriteFlushOutcome.notOffered, const <CsmQueuedWrite>[]);
      }
      return (
        CsmWriteFlushOutcome.sent,
        List<CsmQueuedWrite>.unmodifiable(pruned.entries),
      );
    });
    if (outcome != CsmWriteFlushOutcome.sent) {
      return outcome;
    }

    final want = csmWantMapFromQueue(sending);
    if (want.isEmpty) {
      return CsmWriteFlushOutcome.idle;
    }
    // Сеть ВНЕ ворот: пока идёт раунд-трип, пользователь продолжает менять
    // настройки, и его правки не имеют права ждать оператора.
    try {
      await _ref.read(vpnConnectionProvider).csmRequestSettings(want: want);
    } on Object {
      // Ровно инвариант 16: сетевой отказ не очищает кеш, не гасит туннель и не
      // понижает доверенное состояние. Очередь остаётся как была.
      return CsmWriteFlushOutcome.failed;
    }
    // Доставленное снимается с очереди. Состояние настроек НЕ переписывается
    // здесь: авторитетным его делает проверенная директива, а её принимает
    // ядро, и приложение перечитает её обычным путём.
    await _serial(() async {
      final after = state;
      if (after == null) {
        return;
      }
      // Снимается ИМЕННО отправленное, а не всё с этим ключом. Пока шёл
      // раунд-трип, пользователь мог ответить на карточку «Оставить моё», и та
      // поставила в очередь новую запись по тому же ключу; снять её здесь
      // значило бы потерять ответ пользователя, который никуда не уходил.
      bool delivered(CsmQueuedWrite e) => sending.any(
        (sent) => sent.key == e.key && sent.queuedMs == e.queuedMs,
      );
      await _write(
        after.copyWith(
          writeQueue: CsmWriteQueue(
            List<CsmQueuedWrite>.unmodifiable(
              after.writeQueue.entries.where((e) => !delivered(e)),
            ),
          ),
        ),
      );
    });
    return CsmWriteFlushOutcome.sent;
  }

  /// Пользователь переставил или переключил ступени. С этого момента его
  /// порядок и набор побеждают подписанные умолчания навсегда.
  ///
  /// Порядок применяется И К ЯДРУ. Лестницей ходит оно, и порядок берёт из
  /// своего хранилища; запись, оставшаяся здесь, меняла бы только картинку:
  /// пользователь переставляет ступени, экран показывает новый порядок, а
  /// выборка идёт по старому. Отказ ядра возвращается наверх, а не гасится:
  /// экран, показывающий порядок, который ядро отвергло, врёт.
  Future<CsmLadderApplyOutcome> setLadder({
    List<int>? order,
    List<int>? enabled,
  }) async {
    final applied = await _serial(() async {
      final current = state;
      if (current == null) {
        return null;
      }
      final next = CsmLadderPrefs(
        order: order ?? current.ladder.order,
        // R0 и R6 отключить нельзя.
        enabled: <int>{
          ...(enabled ?? current.ladder.enabled),
          0,
          6,
        }.toList(growable: false)..sort(),
        userTouched: true,
      );
      await _write(current.copyWith(ladder: next));
      return next;
    });
    if (applied == null) {
      return CsmLadderApplyOutcome.notEnrolled;
    }
    // Вне ворот: ворота держатся только на записях состояния, а этот вызов
    // уходит в ядро.
    try {
      await _ref
          .read(vpnConnectionProvider)
          .csmSetLadder(
            order: applied.effectiveOrder
                .map((r) => r.id)
                .toList(growable: false),
            enabled: <int, bool>{
              for (final rung in applied.effectiveOrder)
                rung.id: applied.isEnabled(rung),
            },
          );
    } on Object {
      return CsmLadderApplyOutcome.coreRefused;
    }
    return CsmLadderApplyOutcome.applied;
  }

  static CsmPendingChange? _cardById(CsmProfileState s, String id) {
    for (final c in s.pendingChanges) {
      if (c.id == id) {
        return c;
      }
    }
    return null;
  }

  static List<CsmPendingChange> _without(
    List<CsmPendingChange> cards,
    String id,
  ) => cards.where((c) => c.id != id).toList(growable: false);
}

// -------------------------------------------------------- провайдеры

/// Точка мутации состояния CSM активного профиля.
final csmNotifierProvider = Provider<CsmNotifier>(CsmNotifier.new);

/// Судьба последней отдачи очереди записей.
///
/// Существует потому, что без него исход отдачи не доходил ни до одного
/// виджета: пользователь менял настройку, локальное значение применялось,
/// запись молча не уходила, и экран настроек об этом не говорил. Особенно это
/// важно для `notOffered`: оператор не предлагает запись настроек ВООБЩЕ, и
/// это постоянное свойство, а не временный отказ сети.
final csmWriteStatusProvider = StateProvider<CsmWriteStatus>(
  (ref) => CsmWriteStatus.none,
);

/// Состояние CSM активного профиля. `null` означает «профиль никогда не
/// закреплял корневой ключ»: legacy-импорт таким и остаётся.
final csmProfileStateProvider = Provider<CsmProfileState?>(
  (ref) => ref.watch(activeConnectionProfileProvider)?.csm,
);

/// Личность оператора для экрана INV-18.
final csmOperatorIdentityProvider = Provider<CsmOperatorIdentity?>((ref) {
  final profile = ref.watch(activeConnectionProfileProvider);
  final csm = profile?.csm;
  if (csm == null) {
    return null;
  }
  return CsmOperatorIdentity(
    displayName: csm.operatorName ?? profile?.displayName ?? '',
    pid: csm.pin.pid,
    fingerprint: csm.pin.fingerprint,
    pinOrigin: csm.pin.origin,
    enrolledAtMs: csm.enrolledAtMs,
    pinEverChanged: csm.pinHistory.isNotEmpty,
    hardwareTier: csm.hardwareTier,
    stage: csm.stage,
  );
});

/// Часы для чистых провайдеров ниже. Отдельным провайдером, чтобы тест мог их
/// подменить, не подменяя состояние.
final csmClockProvider = Provider<DateTime Function()>((ref) => DateTime.now);

/// Состояние проверки документов в работе, INV-19.
final csmDocumentStateProvider = Provider<CsmDocumentState>((ref) {
  final csm = ref.watch(csmProfileStateProvider);
  final nowSec = ref.watch(csmClockProvider)().millisecondsSinceEpoch ~/ 1000;
  if (csm == null) {
    return CsmDocumentState(nowSec: nowSec);
  }
  return CsmDocumentState(
    nowSec: nowSec,
    keyDocument: csm.keyDocument,
    catalog: csm.catalog,
    directive: csm.directive,
    capabilitiesDisagree: csm.capabilitiesDisagree,
    fleetRootAnchored: csm.fleetRootAnchored,
  );
});

/// Действующий набор возможностей: разрешение разногласия каталога и
/// директивы, затем пересечение с вкомпилированным полем клиента.
///
/// Контрол, завязанный на снятый бит, ОБЯЗАН быть спрятан, а не отрисован
/// включённым и молча ничего не делающим (02-SPEC.md 6.2, INV B1).
final csmCapabilitiesProvider = Provider<CsmCapabilitySet>((ref) {
  final csm = ref.watch(csmProfileStateProvider);
  if (csm == null) {
    return CsmCapabilitySet.none;
  }
  final nowSec = ref.watch(csmClockProvider)().millisecondsSinceEpoch ~/ 1000;
  // Карве-аут по содержимому применяется ЗДЕСЬ, на единственном пути, который
  // читают все закрытые контролы. Без него бит 4 при пустом `mir` предлагает
  // ступень зеркал, за которой зеркал нет, а бит 6 при пустых `rs` и `geo`
  // снова открывает выборку ресурсов (02-SPEC.md 6.2).
  return csm.effectiveOperatorCapabilities(nowSec).intersectWithClient();
});

/// Факты о транспорте, которыми владеет ядро на Go, а не слой Dart.
///
/// Эти два значения приходят из `CsmLadderJSON` через `CsmLadderSync.pump`:
/// иначе `csmRungReason` берёт их из умолчаний собственных параметров и рисует
/// R4 как `platform_unsupported`, а R5 как `not_configured` независимо от того,
/// что на самом деле настроено. До первого подъёма из ядра оба ложны, и это
/// правильная сторона отказа: назвать ступень доступной, не спросив ядро,
/// значит обещать путь, которого может не быть.
class CsmTransportFacts {
  const CsmTransportFacts({
    this.tunnelFetchSupported = false,
    this.proxyConfigured = false,
  });

  final bool tunnelFetchSupported;
  final bool proxyConfigured;
}

final csmTransportFactsProvider = StateProvider<CsmTransportFacts>(
  (ref) => const CsmTransportFacts(),
);

/// Лестница для экрана транспортов, INV-17.
final csmLadderProvider = Provider<CsmLadderState>((ref) {
  final csm = ref.watch(csmProfileStateProvider);
  if (csm == null) {
    return CsmLadderState.empty;
  }
  final caps = ref.watch(csmCapabilitiesProvider);
  final transport = ref.watch(csmTransportFactsProvider);
  final prefs = csm.ladder;
  final order = prefs.effectiveOrder;
  final rungs = <CsmLadderRung>[];
  for (var i = 0; i < order.length; i++) {
    final rung = order[i];
    final enabled = prefs.isEnabled(rung);
    rungs.add(
      CsmLadderRung(
        rung: rung,
        position: i,
        enabled: enabled,
        reason: csmRungReason(
          rung,
          enabled: enabled,
          capabilities: caps,
          tunnelFetchSupported: transport.tunnelFetchSupported,
          proxyConfigured: transport.proxyConfigured,
        ),
      ),
    );
  }
  return CsmLadderState(rungs: rungs, userTouched: prefs.userTouched);
});

/// Почему ступень недоступна. Словарь закрытый (02-SPEC.md 8.1).
///
/// R4 на Android в обычном режиме TUN пути не имеет: приложение исключено из
/// собственного туннеля по замыслу, а слушателя на петле в этом режиме нет.
/// Пока это не поедет, R4 рендерится видимой и выключенной с причиной
/// `platform_unsupported` (02-SPEC.md 8.2).
CsmUnavailableReason? csmRungReason(
  CsmRung rung, {
  required bool enabled,
  required CsmCapabilitySet capabilities,
  bool tunnelFetchSupported = false,
  bool proxyConfigured = false,
}) {
  if (!enabled && !rung.isMandatory) {
    return CsmUnavailableReason.userDisabled;
  }
  switch (rung) {
    case CsmRung.mirrors:
      return capabilities.has(CsmCapability.mirrorPool)
          ? null
          : CsmUnavailableReason.notOfferedByOperator;
    case CsmRung.doh:
      return capabilities.has(CsmCapability.dohEndpoints)
          ? null
          : CsmUnavailableReason.notOfferedByOperator;
    case CsmRung.tunnel:
      return tunnelFetchSupported
          ? null
          : CsmUnavailableReason.platformUnsupported;
    case CsmRung.userProxy:
      return proxyConfigured ? null : CsmUnavailableReason.notConfigured;
    case CsmRung.cached:
    case CsmRung.direct:
    case CsmRung.outOfBand:
      return null;
  }
}

/// Индексы пресетов маршрутизации, которые видны, но не выбираемы, и причина
/// к каждому.
///
/// 02-SPEC.md 7.2: `ro[].id` каталога берётся из того же словаря, что и
/// локальные пресеты. Маршрут, чей `id` эта версия приложения не реализует,
/// обязан отрисоваться видимым и выключенным с причиной
/// `app_version_unsupported`; маршрут, которого оператор не предлагает вовсе,
/// это строка 7.9 "у оператора есть возможность, но нет этого значения".
/// Молча подставить другой нельзя ни в том, ни в другом случае: пользователь
/// закрепил значение по причине, которой клиент не знает.
///
/// Пока каталог не проверялся, список не сужается: сузить его по незнанию
/// значит спрятать то, что оператор на самом деле предлагает.
final csmDisabledRoutePresetsProvider = Provider<Map<int, String>>((ref) {
  final csm = ref.watch(csmProfileStateProvider);
  final content = csm?.catalogContent;
  if (content == null || !content.known || content.offeredRoutes.isEmpty) {
    return const <int, String>{};
  }
  final offered = content.offeredRoutes.toSet();
  final modes = ref.watch(routingModesProvider);
  final out = <int, String>{};
  for (var i = 0; i < modes.length; i++) {
    if (!offered.contains(modes[i].id)) {
      out[i] = csmUnavailableReasonText(
        CsmUnavailableReason.notOfferedByOperator,
      );
    }
  }
  return out;
});

/// Идентификаторы маршрутов, которые оператор предлагает, а это приложение не
/// реализует. Экран диагностики называет их, а не прячет.
final csmUnimplementedRoutesProvider = Provider<List<String>>((ref) {
  final content = ref.watch(csmProfileStateProvider)?.catalogContent;
  if (content == null || !content.known) {
    return const <String>[];
  }
  final local = ref.watch(routingModesProvider).map((m) => m.id).toSet();
  return content.offeredRoutes
      .where((id) => !local.contains(id))
      .toList(growable: false);
});

/// Нерешаемый выбор: то, что назвала директива, в связанном каталоге не
/// находится.
///
/// 02-SPEC.md 7.4, два предиката, зависящих от каталога. Они проверяются
/// ПОСЛЕ того, как связанный каталог дошёл до `verified`, и НЕ на разборе:
/// на разборе связанного каталога может ещё не быть, а предикат, который
/// нельзя вычислить, не является критерием отказа.
///
/// Ни один из них не отвергает директиву. Клиент откатывается к умолчанию
/// оператора и поднимает ИНФОРМАЦИОННОЕ уведомление, никогда карточку, и
/// НИКОГДА не подставляет другой узел молча: пользователь закрепил сервер по
/// причине, которой клиент не знает (02-SPEC.md 7.9, 06-MIGRATION.md 7.5).
class CsmUnresolvableSelection {
  const CsmUnresolvableSelection({required this.field, required this.value});

  /// `sel.exit` или `sel.relay`.
  final String field;

  /// Значение как оно пришло. Показывается ИНЕРТНЫМ текстом.
  final String value;
}

final csmUnresolvableSelectionsProvider =
    Provider<List<CsmUnresolvableSelection>>((ref) {
      final csm = ref.watch(csmProfileStateProvider);
      if (csm == null) {
        return const <CsmUnresolvableSelection>[];
      }
      final content = csm.catalogContent;
      // Пока каталог не проверялся, предикат не вычисляется, а не считается
      // нарушенным.
      if (!content.known) {
        return const <CsmUnresolvableSelection>[];
      }
      final out = <CsmUnresolvableSelection>[];
      final exit = csm.selection.exit;
      if (exit != null &&
          content.offeredExits.isNotEmpty &&
          !content.offeredExits.contains(exit)) {
        out.add(CsmUnresolvableSelection(field: 'sel.exit', value: exit));
      }
      final relay = csm.selection.relay;
      if (relay != null &&
          content.offeredRelays.isNotEmpty &&
          !content.offeredRelays.contains(relay)) {
        out.add(CsmUnresolvableSelection(field: 'sel.relay', value: relay));
      }
      return List<CsmUnresolvableSelection>.unmodifiable(out);
    });

/// Висящие изменения оператора: карточки «Оставить или Вернуть», INV-22.
///
/// Карточка живёт, пока пользователь не ответит: не истекает по таймеру, не
/// закрывается навигацией, не отвечается молчанием.
final csmPendingChangesProvider = Provider<List<CsmPendingChange>>(
  (ref) =>
      ref.watch(csmProfileStateProvider)?.pendingChanges ??
      const <CsmPendingChange>[],
);

/// Возраст конфигурации и её источник, INV-21.
final csmConfigurationAgeProvider = Provider<CsmConfigurationAge>((ref) {
  final csm = ref.watch(csmProfileStateProvider);
  if (csm == null) {
    return CsmConfigurationAge.unknown;
  }
  final nowMs = ref.watch(csmClockProvider)().millisecondsSinceEpoch;
  final rung = csm.directive?.viaRung ?? csm.catalog?.viaRung;
  return CsmConfigurationAge(
    ageSec: csm.configurationAgeSec(nowMs),
    // Источник остаётся неизвестным, когда ни одна ступень ничего не приносила:
    // сказать R0 про конфигурацию, которой нет, значит назвать источник того,
    // чего не происходило. Карточка умеет рисовать "источник неизвестен".
    source: rung == null ? null : CsmRung.fromId(rung),
    runningOnCache:
        csm.stage == CsmProfileStage.trustedStale ||
        csm.stage == CsmProfileStage.grace ||
        csm.stage == CsmProfileStage.graceExhausted,
    stage: csm.stage,
  );
});

/// Жёсткая ошибка липкого правила: корень закреплён, а `cap` не пришёл.
/// Отката к непроверяемому legacy-режиму нет ни по какой причине (INV-13).
final csmStickyErrorProvider = Provider<CsmStickyError?>((ref) {
  final csm = ref.watch(csmProfileStateProvider);
  if (!csmHardCapabilityError(csm)) {
    return null;
  }
  return CsmStickyError(pid: csm!.pin.pid, stage: csm.stage);
});

/// Настройки активного профиля с происхождением по каждому ключу.
final csmSettingsProvider = Provider<CsmSettings>(
  (ref) => ref.watch(csmProfileStateProvider)?.settings ?? CsmSettings.empty,
);

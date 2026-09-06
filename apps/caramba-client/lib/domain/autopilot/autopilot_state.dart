/// Автопилот: ход подбора, его результат и подписи «Авто» для всех контролов.
///
/// Здесь ровно две вещи, и обе — состояние, а не решение. РЕШЕНИЕ (кого
/// выбрать) живёт в чистой [autoPick] в auto_pick.dart и проверяется
/// табличными тестами. Здесь — как этот выбор запускается, что с ним делают
/// (закрепляют на профиле) и как он называется в строках интерфейса.
///
/// Инвалидация — по СОБЫТИЯМ, без таймеров: приложение не имеет права держать
/// фоновый цикл замеров ради строки на экране.
library;

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/domain/autopilot/auto_pick.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/servers/fleet_alignment.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_error.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/probe_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/vpn_state.dart';

/// Возраст, после которого прошлый подбор считается устаревшим.
///
/// Шесть часов — не круглое число ради круглости: это примерный срок, за
/// который телефон успевает сменить сеть (дом → дорога → работа), а оператор —
/// пересобрать флот. Меньше значило бы гонять замер зря, больше — обещать
/// выбор, сделанный в другой сети.
const Duration kAutoPickMaxAge = Duration(hours: 6);

/// Почему прошлый выбор больше не описывает действительность.
enum AutoStaleReason {
  /// Не устарел.
  none,

  /// Замер старше [kAutoPickMaxAge].
  age,

  /// Состав узлов подписки обновился после замера.
  fleetChanged,

  /// Туннель поднят, и ядро стоит НЕ на выбранном узле.
  tunnelDisagrees,

  /// Туннеля нет, но «Сервер» закреплён НЕ на выбранном узле: следующее
  /// подключение пойдёт мимо выбора, и обещать его как действующий нельзя.
  pinDisagrees,
}

/// Подпись контрола в режиме «Авто»: что именно авто выбрал и откуда это
/// известно.
///
/// Три состояния из замысла ровно здесь: выбора нет ([choice] пусто), выбор
/// сделан ([choice] + [source]), выбор устарел ([stale]).
class AutoLabel {
  /// Что выбрано; пусто — выбора нет.
  final String choice;

  /// Откуда это известно: «сейчас в туннеле» / «по замеру 12 мин назад».
  final String source;

  final AutoStaleReason stale;

  /// Приписка про узел, чьё имя обещает вход, которого конфиг не строит;
  /// пусто — расхождения нет.
  ///
  /// Живёт отдельно от [source]: источник отвечает на «откуда мы это знаем», а
  /// это — на «почему имя узла говорит не то, что провод». Склеить их значило
  /// бы потерять одно из двух при устаревшем выборе, где [source] уступает
  /// место причине устаревания.
  final String claimNote;

  const AutoLabel({
    this.choice = '',
    this.source = '',
    this.stale = AutoStaleReason.none,
    this.claimNote = '',
  });

  static const AutoLabel unknown = AutoLabel();

  bool get hasChoice => choice.isNotEmpty;
  bool get isStale => stale != AutoStaleReason.none;

  /// Значение строки: «Авто» или «Авто · CA Canada».
  String get value => hasChoice ? 'Авто · $choice' : 'Авто';

  /// Подпись под строкой. Пустой не бывает: контрол, который не говорит, что
  /// он сделал, — это и есть жалоба владельца.
  String get subtitle {
    if (!hasChoice) return 'Выберется при подключении';
    final head = isStale ? _staleText : source;
    return claimNote.isEmpty ? head : '$head · $claimNote';
  }

  /// Слово для бейджа рядом с заголовком; пусто — бейджа нет.
  ///
  /// Слов ровно два, и они про РАЗНОЕ. «Устарело» отправляет перезамерять, и
  /// это правда только когда состарился сам замер. При расхождении с
  /// держателем замер как раз свежий — перезамер не изменит ничего: не в силе
  /// сам ВЫБОР, потому что трафик идёт (или пойдёт) мимо него. Одно слово на
  /// оба состояния и было той неточностью, из-за которой строка «Авто»
  /// отправляла человека перезамерять там, где надо снять пин.
  ///
  /// Словарь общий с Главной (home_screen.dart, `_autopilotRow`): два экрана,
  /// называющие одно состояние разными словами, — это два разных состояния для
  /// того, кто на них смотрит.
  String get badge => switch (stale) {
    AutoStaleReason.tunnelDisagrees ||
    AutoStaleReason.pinDisagrees => 'не в силе',
    AutoStaleReason.age || AutoStaleReason.fleetChanged => 'устарело',
    AutoStaleReason.none => '',
  };

  /// Причина устаревания одной строкой — столько, сколько влезает под контрол.
  ///
  /// Имени узла-соперника здесь нет намеренно: подпись контрола отвечает за
  /// СВОЙ контрол, а разбор «выбрано одно, в силе другое» с именами и странами
  /// ведёт [autopilotBannerText] — там на это есть место и там же стоит вторая
  /// половина пары фактов.
  String get _staleText => switch (stale) {
    AutoStaleReason.tunnelDisagrees =>
      'Ядро сейчас стоит на другом узле. Пересчитается при переподключении.',
    AutoStaleReason.pinDisagrees =>
      'Сервер закреплён на другом узле. Пересчитается при переподключении.',
    AutoStaleReason.fleetChanged =>
      'Состав узлов обновился после замера. Пересчитается при переподключении.',
    AutoStaleReason.age =>
      'Замеру больше 6 часов. Пересчитается при переподключении.',
    AutoStaleReason.none => source,
  };
}

/// «12 мин назад» — тем же способом, каким это делает экран серверов.
String autoAgeText(DateTime at, {DateTime? now}) {
  final diff = (now ?? DateTime.now()).difference(at);
  if (diff.inMinutes < 1) return 'только что';
  if (diff.inMinutes < 60) return '${diff.inMinutes} мин назад';
  if (diff.inHours < 24) return '${diff.inHours} ч назад';
  return '${diff.inDays} дн назад';
}

/// Что предложение знает про каждое имя прокси.
///
/// Ключ — имя прокси в теле конфига: ровно то, чем ядро называет узлы в
/// замере и в `activeProxy`. Другого общего ключа между замером и предложением
/// не существует.
final fleetFactsProvider = Provider<Map<String, FleetFact>>((ref) {
  final offering = ref.watch(offeringProvider);
  return fleetFactsOf(offering);
});

/// Чистая половина [fleetFactsProvider] — её и проверяет тест.
Map<String, FleetFact> fleetFactsOf(Offering offering) {
  final titles = disambiguateTitles(
    offering.exits.map(machineTitleOf).toList(growable: false),
  );
  final out = <String, FleetFact>{};
  for (var i = 0; i < offering.exits.length; i++) {
    final exit = offering.exits[i];
    for (final inbound in exit.inbounds) {
      final proxy = inbound.proxyName;
      // Инбаунд без имени прокси в конфиг не доезжает: закрепить его нечем, и
      // замер его не видел.
      if (proxy == null || proxy.isEmpty) continue;
      out[proxy] = FleetFact(
        proxyName: proxy,
        exitKey: exit.key,
        countryCode: exit.countryCode,
        machineTitle: titles[i],
        protocol: inbound.key.protocol,
        transport: inbound.key.transport,
        security: inbound.key.security,
        protocolLabel: inbound.key.label,
        // Порт доезжает до автоподбора не ради показа: им отличается ПРОВОД,
        // и без него два имени одного инбаунда неотличимы от двух инбаундов.
        port: inbound.port ?? 0,
        loadPct: exit.loadPct ?? -1,
        // ЕДИНСТВЕННЫЙ источник, который вправе утверждать цепочку. Панель
        // смотрит в тело, которое сама же и сгенерировала, и отвечает
        // `via_relay.chained_in_config`. Импортированное тело такого поля не
        // несёт вовсе — там останется false, и суффикс в имени будет назван
        // ярлыком, каким он и является.
        wireChained: exit.viaRelay?.chainedInConfig ?? false,
        // Роль берётся у ПРОКСИ, а не у машины: ядро меряет и закрепляет
        // именно прокси, и на машине со смешанным набором запрет обязан быть
        // точечным. Роль машины остаётся на [ExitOffer.role] — её читает
        // список серверов, где строка это машина.
        isRelay: inbound.role.isRelay || exit.role.isRelay,
      );
    }
  }
  return out;
}

/// Ограничения пользователя: закреплённая страна и закреплённое семейство
/// протокола. Автоподбор обязан их уважать — иначе «Авто» переигрывало бы
/// явный выбор человека.
final autoConstraintsProvider = Provider<AutoConstraints>((ref) {
  final profile = ref.watch(activeConnectionProfileProvider);
  final protocols = ref.watch(protocolsProvider);
  final cfg = ref.watch(coreConfigProvider);
  final option = (cfg.protocol >= 0 && cfg.protocol < protocols.length)
      ? protocols[cfg.protocol]
      : null;
  return AutoConstraints(
    pinnedCountry: (profile?.selectedExitCountry ?? '').toUpperCase(),
    // «Авто» у протокола — это отказ от ограничения, а не ограничение с
    // пустым именем.
    protocolFamily: (option == null || option.auto) ? '' : option.coreFamily,
  );
});

/// Прошлый выбор автоподбора активного профиля.
final autoPickRecordProvider = Provider<AutoPickRecord?>(
  (ref) => ref.watch(activeConnectionProfileProvider)?.autoPick,
);

/// На каком основании известно, какой узел в силе.
enum AutoHolderSource {
  /// Ни туннеля, ни пина: узел выберет ядро в момент подключения.
  none,

  /// Ядро держит сессию на этом узле — факт о происходящем прямо сейчас.
  tunnel,

  /// Туннеля нет, но узел закреплён: именно к нему пойдёт следующий connect.
  pin,
}

/// Узел, который В СИЛЕ, — вторая половина пары фактов на Главной.
///
/// Первая половина — [AutoPickRecord]: что автоподбор ВЫБРАЛ, и это факт про
/// прошлое (замер). Здесь — куда трафик идёт или пойдёт, и это факт про
/// настоящее. Две половины расходятся штатно (человек закрепил другую страну,
/// ядро встало на соседний узел), и ровно поэтому у каждой должен быть свой
/// носитель: пока «выбор» брал подпись у «в силе», экран объявлял немецкий
/// выход канадским.
class AutoHolder {
  final AutoHolderSource source;

  /// Имя прокси; пусто — держателя назвали только машиной (панельный пин
  /// закрепляет `nodes.id`, а инбаунд внутри машины выбирает ядро).
  final String proxyName;

  /// Ключ машины; пусто — держателя назвали только именем прокси и разрешить
  /// его в машину не удалось.
  final String exitKey;

  final String countryCode;

  /// Как назвать держателя человеку. Правило то же, что у
  /// [AutoPickRecord.shortLabel]: имена по разным правилам в одной фразе
  /// сравнить нельзя.
  final String title;

  /// Имя узла так, как оно стоит в строке «Сервер» (имя прокси без ярлыка
  /// цепочки); пусто — совпадает с [title] или неизвестно. Нужно, чтобы
  /// названный в баннере узел человек нашёл на экране глазами.
  final String nodeTitle;

  const AutoHolder({
    this.source = AutoHolderSource.none,
    this.proxyName = '',
    this.exitKey = '',
    this.countryCode = '',
    this.title = '',
    this.nodeTitle = '',
  });

  static const AutoHolder none = AutoHolder();

  bool get isKnown => source != AutoHolderSource.none;

  /// В силе НЕ тот узел, что выбрал автоподбор.
  ///
  /// Ключи у путей разные: ядро и raw-пин называют узел именем прокси,
  /// панельный пин — только машиной. Сравниваем по тому ключу, который назвали
  /// ОБЕ стороны, и на этом останавливаемся: имя прокси точнее машины, и если
  /// оно есть у обоих, машина уже ничего не добавит. Общего ключа нет —
  /// расхождения не утверждаем: молчание источника это не «другой узел».
  bool disagreesWith(AutoPickRecord pick) {
    if (!isKnown) return false;
    if (proxyName.isNotEmpty && pick.proxyName.isNotEmpty) {
      return proxyName != pick.proxyName;
    }
    if (exitKey.isNotEmpty && pick.exitKey.isNotEmpty) {
      return exitKey != pick.exitKey;
    }
    return false;
  }
}

/// Держатель, названный именем прокси (ядро и raw-пин говорят именно так).
AutoHolder holderOfProxy(
  AutoHolderSource source,
  String proxyName,
  Map<String, FleetFact> facts,
) {
  final fact = facts[proxyName];
  final naming = namingOfProxy(proxyName, facts);
  final machine = fact?.machineTitle ?? '';
  return AutoHolder(
    source: source,
    proxyName: proxyName,
    exitKey: fact?.exitKey ?? '',
    countryCode: fact?.countryCode ?? '',
    title: machine.isNotEmpty ? machine : naming.title,
    nodeTitle: naming.title,
  );
}

/// Держатель, названный машиной (панельный пин знает только `nodes.id`).
///
/// Имя прокси здесь НЕ восстанавливается по первому подходящему факту: у
/// машины инбаундов несколько, и назвать один из них закреплённым значило бы
/// выдумать выбор, которого человек не делал, — его сделает ядро.
AutoHolder holderOfExitKey(
  AutoHolderSource source,
  String exitKey,
  Map<String, FleetFact> facts,
) {
  for (final fact in facts.values) {
    if (fact.exitKey != exitKey) continue;
    return AutoHolder(
      source: source,
      exitKey: exitKey,
      countryCode: fact.countryCode,
      title: fact.machineTitle,
    );
  }
  return AutoHolder(source: source, exitKey: exitKey);
}

/// Какой узел в силе сейчас: ядро держит сессию на нём, а вне сессии — к нему
/// пойдёт следующее подключение.
///
/// Пин читается по тому полю, в которое его КЛАДЁТ этот путь (см. `_apply`
/// ниже и `ExitInventory.selectNode`): импорт пинит имя прокси в
/// `selectedServerId`, панель — `nodes.id` в `selectedExitNodeId`.
/// Читать оба подряд нельзя: на панельном профиле может лежать пин прокси от
/// прежнего импорта, и он назвал бы закреплённым узел, к которому connect не
/// пойдёт.
final autoHolderProvider = Provider<AutoHolder>((ref) {
  final facts = ref.watch(fleetFactsProvider);
  final activeProxy = ref.watch(activeProxyProvider);
  if (activeProxy != null && activeProxy.isNotEmpty) {
    return holderOfProxy(AutoHolderSource.tunnel, activeProxy, facts);
  }
  final profile = ref.watch(activeConnectionProfileProvider);
  if (profile == null) return AutoHolder.none;
  if (profile.isRaw) {
    final pinned = profile.selectedServerId ?? '';
    return pinned.isEmpty
        ? AutoHolder.none
        : holderOfProxy(AutoHolderSource.pin, pinned, facts);
  }
  final nodeId = profile.selectedExitNodeId;
  return nodeId == null
      ? AutoHolder.none
      : holderOfExitKey(AutoHolderSource.pin, '$nodeId', facts);
});

/// Устарел ли прошлый выбор и почему.
final autoStaleProvider = Provider<AutoStaleReason>((ref) {
  final pick = ref.watch(autoPickRecordProvider);
  if (pick == null) return AutoStaleReason.none;
  final profile = ref.watch(activeConnectionProfileProvider);
  return autoStaleReasonOf(
    pick: pick,
    serversUpdatedMs: profile?.serversUpdatedMs ?? 0,
    holder: ref.watch(autoHolderProvider),
  );
});

/// Чистая половина [autoStaleProvider].
///
/// Порядок веток — от самого доказательного к самому косвенному. Узел, который
/// в силе, — это ФАКТ о том, куда идёт (или пойдёт) трафик, и он важнее и
/// возраста замера, и состава флота: устаревший замер всё ещё описывает нужный
/// узел, а расхождение означает, что выбор не исполняется вовсе.
AutoStaleReason autoStaleReasonOf({
  required AutoPickRecord pick,
  required int serversUpdatedMs,
  AutoHolder holder = AutoHolder.none,
  DateTime? now,
}) {
  if (holder.disagreesWith(pick)) {
    return holder.source == AutoHolderSource.tunnel
        ? AutoStaleReason.tunnelDisagrees
        : AutoStaleReason.pinDisagrees;
  }
  if (serversUpdatedMs > 0 && serversUpdatedMs != pick.serversUpdatedMs) {
    return AutoStaleReason.fleetChanged;
  }
  final age = (now ?? DateTime.now()).difference(pick.updatedAt);
  if (age > kAutoPickMaxAge) return AutoStaleReason.age;
  return AutoStaleReason.none;
}

/// Текст баннера автоподбора на Главной.
///
/// Здесь два разных факта СВОДЯТСЯ, но не склеиваются. Первая фраза говорит
/// только про выбор и берёт всё из записи выбора; вторая — про то, что в силе,
/// и берёт всё из [holder]. Раньше вторую фразу давал `autoServerLabelProvider`
/// — подпись контрола «Сервер», которая при живом туннеле описывает АКТИВНЫЙ
/// узел, — и получалось «Автоподбор выбрал Канада … Сейчас в туннеле» над
/// немецким выходом: подпись одного источника стояла под утверждением другого.
String autopilotBannerText({
  required AutoPickRecord pick,
  required AutoStaleReason stale,
  AutoHolder holder = AutoHolder.none,
  DateTime? now,
}) {
  final chose = <String>[
    'Автоподбор выбрал ${pick.shortLabel}',
    if (pick.protocolLabel.isNotEmpty) pick.protocolLabel,
    '${pick.latencyMs} мс',
    'работает ${pick.working} из ${pick.checked}',
  ].join(' · ');
  return '$chose. ${_standingText(pick, stale, holder, now)}';
}

const String _recount = 'Пересчитается при переподключении.';

/// Что с этим выбором сейчас: он в силе — или в силе кто-то другой.
String _standingText(
  AutoPickRecord pick,
  AutoStaleReason stale,
  AutoHolder holder,
  DateTime? now,
) {
  final measured = 'Замер ${autoAgeText(pick.updatedAt, now: now)}';
  return switch (stale) {
    AutoStaleReason.none => switch (holder.source) {
      // Туннель стоит на выбранном узле — единственный случай, когда «выбрал»
      // и «в силе» это про один и тот же узел, и сказать так можно.
      AutoHolderSource.tunnel => '$measured, сейчас в туннеле.',
      AutoHolderSource.pin => '$measured, узел закреплён.',
      AutoHolderSource.none => '$measured, подключение пойдёт через него.',
    },
    AutoStaleReason.age => 'Замеру больше 6 часов. $_recount',
    AutoStaleReason.fleetChanged =>
      'Состав узлов обновился после замера. $_recount',
    AutoStaleReason.tunnelDisagrees || AutoStaleReason.pinDisagrees =>
      '$measured, но ${_disagreementText(pick, holder)} $_recount',
  };
}

/// Расхождение словами: кто в силе вместо выбора и чем это грозит.
String _disagreementText(AutoPickRecord pick, AutoHolder holder) {
  final who = holder.source == AutoHolderSource.tunnel
      ? 'ядро сейчас стоит'
      : '«Сервер» закреплён';
  // Заголовки совпали — расходятся инбаунды ОДНОЙ машины: страна выхода та же,
  // и назвать «другим узлом» имя, которое уже стоит в начале фразы, значило бы
  // отправить человека искать разницу там, где её не видно.
  final String name;
  if (holder.title.isEmpty) {
    // Держателя нечем назвать (машины нет в предложении). «Другой узел» без
    // имени — всё ещё правда; выдумывать имя ради складности фразы нельзя.
    name = 'на другом узле';
  } else if (holder.title == pick.shortLabel) {
    name = 'на другом входе той же машины';
  } else {
    name = 'на другом узле — ${_holderName(holder)}';
  }
  return '$who $name.${_countryClause(pick, holder)}';
}

/// Имя держателя для фразы.
///
/// Имя узла из строки «Сервер» ([AutoHolder.nodeTitle]) стоит здесь ради
/// узнавания: по нему человек находит узел на экране. Заголовок машины
/// приписывается к нему, только когда он добавляет что-то сверх страны — на
/// импортированном пути заголовком машины СТАНОВИТСЯ название страны, и без
/// этой проверки фраза называла бы страну дважды подряд.
String _holderName(AutoHolder holder) {
  final node = holder.nodeTitle;
  if (node.isEmpty || node == holder.title) return holder.title;
  if (holder.title == countryNameOf(holder.countryCode)) return '«$node»';
  return '${holder.title} («$node»)';
}

/// Разница в СТРАНЕ выхода — не подробность: её человек и выбирал. Страны
/// названы в именительном падеже намеренно: склонять названия из справочника
/// нечем, а «выход из Германия» хуже, чем чуть более сухая формулировка.
String _countryClause(AutoPickRecord pick, AutoHolder holder) {
  final ours = normalizeCountryCode(pick.countryCode);
  final theirs = normalizeCountryCode(holder.countryCode);
  if (ours.isEmpty || theirs.isEmpty || ours == theirs) return '';
  return ' Страна выхода — ${countryNameOf(theirs)}, '
      'а выбрана ${countryNameOf(ours)}.';
}

/// Подпись «Авто» для строки СЕРВЕРА.
///
/// ЗАГОЛОВОК ЭТОЙ СТРОКИ — ВСЕГДА ВЫБОР АВТОПОДБОРА, и другого источника у него
/// нет. Раньше при живом туннеле сюда подставлялся ДЕРЖАТЕЛЬ — узел, на котором
/// ядро стоит прямо сейчас, — и получалось, что контрол автоподбора назван
/// чужим именем: с пином 🇩🇪 Stream и выбором Канады «Серверы» говорили
/// «Авто · DE · Германия — Сейчас в туннеле», а Главная в ту же секунду —
/// «Автоподбор выбрал Канада… не в силе». Два экрана называли один выбор
/// по-разному, потому что смотрели в разные источники.
///
/// Держатель с этого экрана и так виден: галочка стоит на строке своего узла, а
/// страну называет заголовок. Дублировать его в подписи контрола, который за
/// него не отвечает, незачем — расхождение выражается словом «не в силе»
/// ([AutoLabel.badge]) и подписью из [AutoLabel._staleText].
///
/// Обещание входа тянется за УЗЛОМ, а не за заголовком строки: заголовок на
/// панельном пути — имя машины («Germany»), и обещания в нём нет, а врёт при
/// этом имя прокси («🇩🇪 Secure via 🇷🇺»). Поэтому [claimNote] ищется по имени
/// прокси из записи выбора.
final autoServerLabelProvider = Provider<AutoLabel>((ref) {
  final pick = ref.watch(autoPickRecordProvider);
  if (pick == null) return AutoLabel.unknown;
  final facts = ref.watch(fleetFactsProvider);
  final naming = namingOfProxy(pick.proxyName, facts);
  final cc = pick.countryCode;
  final title = pick.machineTitle.isNotEmpty ? pick.machineTitle : naming.title;
  return AutoLabel(
    choice: cc.isEmpty || cc == title ? title : '$cc · $title',
    source: _sourceOf(
      pick,
      ref.watch(autoStaleProvider),
      ref.watch(autoHolderProvider),
    ),
    stale: ref.watch(autoStaleProvider),
    claimNote: naming.shortNote,
  );
});

/// Подпись «Авто» для строки ТИПА ПОДКЛЮЧЕНИЯ (бывший «Протокол»).
///
/// Правило то же, что у строки сервера, и по той же причине: «Авто» — контрол
/// автоподбора, и называть он обязан СВОЙ выбор. Форму инбаунда, поднятого
/// ядром, показывает не он: расхождение закреплённого типа с проводом разбирает
/// `protocolTruthOf` (protocol_truth.dart), у которого для этого есть и место,
/// и обе половины факта.
final autoProtocolLabelProvider = Provider<AutoLabel>((ref) {
  final pick = ref.watch(autoPickRecordProvider);
  if (pick == null || pick.protocolLabel.isEmpty) return AutoLabel.unknown;
  return AutoLabel(
    choice: pick.protocolLabel,
    source: _sourceOf(
      pick,
      ref.watch(autoStaleProvider),
      ref.watch(autoHolderProvider),
    ),
    stale: ref.watch(autoStaleProvider),
  );
});

/// Откуда известно, что выбор — не только запись в хранилище.
///
/// Держатель здесь отвечает ровно на «откуда мы это знаем», а не «кто в силе»:
/// когда ядро стоит НА ВЫБРАННОМ узле, самое сильное свидетельство — туннель, а
/// не возраст замера. Разошлись — свидетельства нет вовсе, и подпись уступает
/// место причине расхождения ([AutoLabel._staleText]); [_pickSource] здесь
/// остаётся запасным ответом, который эту причину не перекрывает.
String _sourceOf(
  AutoPickRecord pick,
  AutoStaleReason stale,
  AutoHolder holder,
) {
  if (stale == AutoStaleReason.none &&
      holder.source == AutoHolderSource.tunnel) {
    return 'Сейчас в туннеле';
  }
  return _pickSource(pick);
}

String _pickSource(AutoPickRecord pick) {
  final age = autoAgeText(pick.updatedAt);
  if (!pick.confirmed) {
    // Число есть, а подтверждения нет: сборка без ядра или ядро старше
    // вердиктов. Выдать это за проверенный выбор нельзя.
    return 'По замеру $age (проверен только адрес)';
  }
  return 'По замеру $age · ${pick.latencyMs} мс';
}

/// Ход подбора.
class AutopilotRun {
  final bool running;

  /// Когда начали — из этого экран считает секунды.
  final DateTime? startedAt;

  /// Сколько узлов ушло в замер; 0 — ещё не знаем.
  final int nodeCount;

  /// Итог последнего прохода; `null` — прохода не было.
  final AutoPickOutcome? outcome;

  /// Отказ ядра (не отказ подбора): текст уже человеческий.
  final String? error;

  /// Сырой текст под «Подробности».
  final String? errorDetail;

  /// Отказ объясняется подпиской: вместо «повторить» нужна оплата.
  final bool errorPayable;

  const AutopilotRun({
    this.running = false,
    this.startedAt,
    this.nodeCount = 0,
    this.outcome,
    this.error,
    this.errorDetail,
    this.errorPayable = false,
  });

  static const AutopilotRun idle = AutopilotRun();

  bool get finished => !running && (outcome != null || error != null);
}

/// Запускает подбор и закрепляет его результат.
class AutopilotController extends StateNotifier<AutopilotRun> {
  final Ref _ref;

  AutopilotController(this._ref) : super(AutopilotRun.idle);

  /// Полный проход: замер всех узлов, ранжирование, закрепление лучшего.
  ///
  /// Возвращает `true`, если рабочая комбинация нашлась.
  Future<bool> run() async {
    if (state.running) return false;
    final profile = await _resolveProfile();
    if (profile == null) {
      state = const AutopilotRun(
        error:
            'Нет активного профиля. Импортируйте подписку или войдите в панель.',
      );
      return false;
    }

    // Замер под закрытым доступом бессмыслен и вреден: узлы ответят отказом
    // ключа, и автоподбор обвинит сеть в том, в чём виновата исчерпанная
    // подписка. Причину показывает AccessCard, а не мы.
    final access = _ref.read(subscriptionAccessProvider);
    if (access != null && access.isBlocked) {
      state = AutopilotRun(error: access.shortReason, errorPayable: true);
      return false;
    }

    state = AutopilotRun(
      running: true,
      startedAt: DateTime.now(),
      nodeCount: _ref.read(fleetFactsProvider).length,
    );
    try {
      final results = await probeProfile(
        _ref.read(vpnConnectionProvider),
        profile,
        seam: _ref.read(probeSeamResolverProvider),
      );
      final profiles = _ref.read(connectionProfilesProvider.notifier);
      await profiles.setProbe(profile.id, ProbeSnapshot.fromResults(results));

      final outcome = autoPick(
        results: results,
        facts: _ref.read(fleetFactsProvider),
        constraints: _ref.read(autoConstraintsProvider),
        previous: profile.autoPick,
        serversUpdatedMs: profile.serversUpdatedMs,
      );
      if (outcome.pick != null) {
        await _apply(profile, outcome.pick!);
      }
      if (!mounted) return outcome.hasPick;
      state = AutopilotRun(outcome: outcome, nodeCount: results.length);
      return outcome.hasPick;
    } catch (e) {
      if (!mounted) return false;
      // Состояние подписки, если панель его уже назвала, точнее любого разбора
      // текста ошибки: в нём есть числа и срок возврата нормы.
      final failure = describeFailure(
        e,
        access: _ref.read(subscriptionAccessProvider),
      );
      state = AutopilotRun(
        error: failure?.text ?? 'Не удалось замерить узлы.',
        errorDetail: failure?.technical,
        errorPayable: failure?.payable ?? false,
      );
      return false;
    }
  }

  /// Активный профиль, дождавшись чтения хранилища.
  ///
  /// Ждать приходится потому, что экран автоподбора открывается ПЕРВЫМ на
  /// первом запуске — раньше, чем secure storage отдаст профили. Прочитать
  /// провайдер сразу значило бы получить `null` и объявить человеку «нет
  /// активного профиля» ровно в тот момент, когда профиль у него есть и
  /// читается. Ожидание событийное и с потолком: молчащее хранилище не имеет
  /// права держать экран вечно.
  Future<ConnectionProfile?> _resolveProfile() async {
    final direct = _ref.read(activeConnectionProfileProvider);
    if (direct != null) return direct;
    if (!_ref.read(connectionProfilesProvider).loading) return null;

    final done = Completer<ConnectionProfile?>();
    final sub = _ref.listen<ConnectionProfilesState>(
      connectionProfilesProvider,
      (previous, next) {
        if (!next.loading && !done.isCompleted) done.complete(next.active);
      },
    );
    try {
      return await done.future.timeout(
        const Duration(seconds: 3),
        onTimeout: () => _ref.read(activeConnectionProfileProvider),
      );
    } finally {
      sub.close();
    }
  }

  /// Закрепляет выбор так, как это умеет ЭТОТ путь.
  ///
  /// Raw-путь пинит имя прокси — `connectRaw` знает ровно эту строку.
  /// Панельный пинит МАШИНУ (`nodes.id`): конкретный инбаунд внутри машины
  /// выбирает url-test ядра, а пин инбаунда потребовал бы общей правки
  /// `api.go`, которую делят два исполнителя. Результат для человека тот же —
  /// какой инбаунд взят, видно в `activeProxy` (строка «Тип подключения»).
  Future<void> _apply(ConnectionProfile profile, AutoPickRecord pick) async {
    final profiles = _ref.read(connectionProfilesProvider.notifier);
    if (profile.isRaw) {
      await profiles.setSelectedServer(profile.id, pick.proxyName);
    } else {
      final nodeId = int.tryParse(pick.exitKey);
      if (nodeId != null) {
        await profiles.setSelectedExitNode(profile.id, nodeId);
      }
    }
    await profiles.setAutoPick(profile.id, pick);
  }

  /// Убирает результат с экрана, не трогая закрепление.
  void reset() => state = AutopilotRun.idle;
}

final autopilotProvider =
    StateNotifierProvider<AutopilotController, AutopilotRun>(
      AutopilotController.new,
    );

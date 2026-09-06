/// Автоподбор: выбор ЛУЧШЕЙ РАБОЧЕЙ комбинации из результатов замера.
///
/// Владелец: «не вижу режима который автоматически подбирает наилучший
/// работающий комбинацию страны + типа конфига + релэй + маршрут». Три из
/// четырёх осей решаются здесь; про четвёртую (релэй) честный ответ — что на
/// этом пути её выбор не меняет ни байта на проводе, и волны релэя нет
/// намеренно (см. [AutoPickOutcome.relayNote]).
///
/// Ранжировщик живёт В DART, а не в Go, и это осознанно: все входы — выбор
/// пользователя, загрузка узла из `/servers`, форма инбаунда из предложения —
/// уже здесь. Тащить их в ядро значило бы дублировать слой предложения на Go.
///
/// Файл ЧИСТЫЙ: ни Riverpod, ни Flutter, ни ввода-вывода. Всё, что решает, кого
/// выбрать, проверяется табличными тестами без эмулятора.
library;

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

/// Ограничения пользователя. Сужают набор ДО замера, а не после: мерить узлы,
/// которые всё равно не будут выбраны, значит тратить секунды человека.
class AutoConstraints {
  /// Закреплённая страна выхода (ISO-2 верхнего регистра); пусто — любая.
  final String pinnedCountry;

  /// Закреплённое семейство протокола (`vless`, `hysteria2`); пусто — любое.
  final String protocolFamily;

  const AutoConstraints({this.pinnedCountry = '', this.protocolFamily = ''});

  static const AutoConstraints none = AutoConstraints();

  bool get isEmpty => pinnedCountry.isEmpty && protocolFamily.isEmpty;
}

/// Что имя узла обещает про вход — и что за этим стоит на проводе.
///
/// Разница не косметическая. Панельный генератор Clash relay-ноды ИГНОРИРУЕТ:
/// в теле, которое читает ядро, нет ни `dialer-proxy`, ни группы `type: relay`,
/// а в `server:` остаётся адрес выхода. Живая подписка прямо сейчас:
/// `🇩🇪 Secure ↪` → `server: 85.215.196.151` и никакого `dialer-proxy`.
/// Sing-box-тело той же подписки цепочку строит (`detour: "relay 🇷🇺"`,
/// тег `🇩🇪 Secure via 🇷🇺`), но переводчик sing-box → clash в ядре
/// (`subimport/singboxOutboundToClash`) `detour` не переносит — остаётся имя.
///
/// Значит, суффикс в имени сам по себе НИЧЕГО не доказывает, и подтвердить
/// цепочку может только источник отдельным полем.
enum ChainClaim {
  /// Имя входа не обещает.
  none,

  /// Цепочка есть на проводе: источник подтвердил её полем, а не именем.
  wired,

  /// Имя обещает вход, а конфиг цепочки не строит. Суффикс — ярлык.
  labelOnly,
}

/// Глифы, которыми генераторы рисуют цепочку прямо в имени прокси.
const List<String> _kChainGlyphs = <String>['↪', '→', '⇢', '⤳', '»', '->'];

/// Слово «via» отдельным словом.
///
/// Границы по НЕ-букве обязательны: `Bolivia` тоже содержит «via», и без
/// границ честный узел был бы объявлен врущим.
final RegExp _kViaWord = RegExp(
  r'(?:^|[^\p{L}])via(?:$|[^\p{L}])',
  caseSensitive: false,
  unicode: true,
);

/// Где в имени начинается обещание входа; -1 — обещания нет.
///
/// Список признаков намеренно КОРОТКИЙ и проверен на живом флоте (`↪` в
/// clash-теле, ` via 🇷🇺` в sing-box-теле). Расширять его догадками нельзя:
/// ложное срабатывание переименовывает честный узел и врёт про него ровно так
/// же, как ярлык врёт про цепочку.
int chainClaimIndexIn(String name) {
  var at = -1;
  for (final glyph in _kChainGlyphs) {
    final i = name.indexOf(glyph);
    if (i >= 0 && (at < 0 || i < at)) at = i;
  }
  final m = _kViaWord.firstMatch(name);
  if (m != null && (at < 0 || m.start < at)) at = m.start;
  return at;
}

/// Обещает ли имя вход через другую страну.
bool nameClaimsChain(String name) => chainClaimIndexIn(name) >= 0;

/// Имя без обещания входа: `🇨🇦 Secure via 🇷🇺` → `🇨🇦 Secure`,
/// `🇩🇪 Stream ↪` → `🇩🇪 Stream`. Имя, от которого не осталось ничего, не
/// режется: пустая строка хуже неточной.
String nameWithoutChainClaim(String name) {
  final at = chainClaimIndexIn(name);
  if (at < 0) return name;
  final head = name.substring(0, at).trim();
  return head.isEmpty ? name : head;
}

/// Честный вид имени узла: что показать, что не потерять и что объяснить.
///
/// Два очевидных решения здесь неверны оба. Показать `Secure via 🇷🇺` при
/// выключенном входе — соврать: вход не работает. Молча переименовать в
/// `Secure` — потерять имя, под которым узел лежит в конфиге и под которым его
/// знает поддержка. Поэтому в строку идёт то, что НА ПРОВОДЕ, а имя оператора
/// остаётся рядом целиком, с объяснением расхождения.
class NodeNaming {
  /// Имя у оператора целиком, как оно стоит в конфиге.
  final String operatorName;

  final ChainClaim claim;

  const NodeNaming({required this.operatorName, required this.claim});

  bool get overPromises => claim == ChainClaim.labelOnly;

  /// Что показать в строке.
  String get title =>
      overPromises ? nameWithoutChainClaim(operatorName) : operatorName;

  /// Строка для списка: имя оператора целиком плюс то, чем оно не является.
  ///
  /// Имя стоит ЗДЕСЬ, а не значком рядом с заголовком: значок сообщил бы
  /// расхождение, но потерял бы само имя, а в списке потерять его негде —
  /// второй строки у элемента больше нет. Пусто — расхождения нет.
  String get listNote =>
      overPromises ? '«$operatorName» — вход только в имени' : '';

  /// Объяснение расхождения; пусто — расхождения нет.
  String get note => overPromises
      ? 'Оператор назвал узел «$operatorName», но в конфиге, который читает '
            'приложение, цепочки нет: трафик идёт прямо на выход. Вход через '
            'другую страну этим именем не включается.'
      : '';

  /// Короткая приписка для однострочных подписей, где на объяснение нет места.
  String get shortNote =>
      overPromises ? 'вход обещан именем, но не построен' : '';
}

/// Честный вид имени УЖЕ ЗАКРЕПЛЁННОГО выбора.
///
/// В записи выбора поля про цепочку нет — её формат общий и правится не
/// здесь, — поэтому подтверждение ищется в живых фактах по имени прокси. Факта
/// нет (флот сменился, предложение ещё не загрузилось) — цепочка НЕ
/// подтверждена: правило то же, что и везде, молчание источника не «да».
NodeNaming namingOfProxy(String proxyName, Map<String, FleetFact> facts) {
  final fact = facts[proxyName];
  if (fact != null) return fact.naming;
  return NodeNaming(
    operatorName: proxyName,
    claim: nameClaimsChain(proxyName) ? ChainClaim.labelOnly : ChainClaim.none,
  );
}

/// Что предложение знает про один инбаунд. Всё, кроме задержки: её приносит
/// замер.
class FleetFact {
  /// Имя прокси в теле конфига — ключ, по которому факт сходится с замером.
  final String proxyName;

  /// Ключ машины в предложении; пусто — машину не разрешили.
  final String exitKey;

  final String countryCode;

  /// Заголовок машины, как он показан на экране серверов.
  final String machineTitle;

  /// Семейство: `vless`, `hysteria2`, `shadowsocks`.
  final String protocol;

  final String transport;

  /// `reality`, `tls`, `none`; пусто — источник не называет.
  final String security;

  /// Подпись типа подключения целиком (`vless · tcp · reality`).
  final String protocolLabel;

  /// Порт инбаунда; 0 — источник его не называет.
  ///
  /// Порт здесь не для показа: он единственное, чем два прокси одной машины
  /// отличаются как ПРОВОД. Пара `🇨🇦 Stream` / `🇨🇦 Stream via 🇷🇺` в живом
  /// теле — это `158.69.213.88:10400` дважды, и без порта отличить «два имени
  /// одного провода» от «два разных инбаунда» нечем.
  final int port;

  /// Загрузка машины 0..100; отрицательное — источник её не сообщает.
  final double loadPct;

  /// Подтвердил ли ИСТОЧНИК, что генератор строит цепочку через вход
  /// (`GET /app/servers[].via_relay.chained_in_config`).
  ///
  /// Молчание источника — не «да». Утверждать цепочку может только тот, кто
  /// видел тело конфига: у приложения на импортированном пути в руках вообще
  /// нет поля про цепочку (ядро отдаёт `id/name/type/server/port/country`), а
  /// в clash-теле цепочки не бывает по построению.
  final bool wireChained;

  const FleetFact({
    required this.proxyName,
    this.exitKey = '',
    this.countryCode = '',
    this.machineTitle = '',
    this.protocol = '',
    this.transport = '',
    this.security = '',
    this.protocolLabel = '',
    this.port = 0,
    this.loadPct = -1,
    this.wireChained = false,
  });

  /// Что имя обещает и что за ним стоит.
  ChainClaim get chainClaim {
    if (wireChained) return ChainClaim.wired;
    return nameClaimsChain(proxyName) ? ChainClaim.labelOnly : ChainClaim.none;
  }

  /// Честный вид имени этого инбаунда.
  NodeNaming get naming =>
      NodeNaming(operatorName: proxyName, claim: chainClaim);
}

/// Один и тот же провод под двумя именами: [plain] — то же подключение, что и
/// [labelled], но без обещания входа в имени.
///
/// Это и есть ответ на «победил вариант с пустым обещанием». Генератор
/// оператора выпускает КАЖДЫЙ инбаунд дважды — плоским и «via 🇷🇺», — и
/// переводчик sing-box → clash в ядре теряет `detour`, после чего обе записи
/// становятся байт в байт одним подключением. Живое тело `sub 34`, пропущенное
/// через сам импортёр ядра, даёт 14 таких пар, и в каждой совпадают адрес,
/// порт и тип: `158.69.213.88:10400/vless` → `🇨🇦 Stream` и `🇨🇦 Stream via 🇷🇺`.
///
/// Раз провод один, разница их замеров (154 мс против 138 мс) — это шум одного
/// и того же канала, а не качество. Поэтому близнецов НЕ сравнивают по
/// миллисекундам: любая полоса ничьей рано или поздно разойдётся на шуме и
/// выпустит вперёд имя, которое врёт.
///
/// Признаков два, и работает любой — источники называют разное. Панель и тело
/// подписки дают порт, и тогда провод сверяется целиком. Порта нет (источник
/// его не сообщил) — остаётся имя, из которого убрано обещание: у живого
/// генератора плоский близнец назван ровно так, и совпадение имени при той же
/// машине и том же протоколе значит то же самое.
bool isPlainTwin({required FleetFact labelled, required FleetFact plain}) {
  if (identical(labelled, plain)) return false;
  if (labelled.proxyName.isEmpty || labelled.proxyName == plain.proxyName) {
    return false;
  }
  // Опровергать нечего там, где обещания нет, и НЕЛЬЗЯ там, где источник
  // цепочку подтвердил: подтверждённая цепочка — это другой провод, а не
  // второе имя того же.
  if (labelled.chainClaim != ChainClaim.labelOnly) return false;
  // Близнец обязан быть честным сам, иначе он не альтернатива, а второй
  // такой же ярлык.
  if (plain.wireChained || nameClaimsChain(plain.proxyName)) return false;
  // Разные машины близнецами не бывают. Пустой ключ равен пустому: источник
  // молчит про обе стороны одинаково, и это не повод считать их разными.
  if (labelled.exitKey != plain.exitKey) return false;

  // Провод сверен целиком: машина, порт и форма подключения.
  if (labelled.port > 0 &&
      labelled.port == plain.port &&
      labelled.protocol == plain.protocol &&
      labelled.transport == plain.transport &&
      labelled.security == plain.security) {
    return true;
  }

  // Порта источник не назвал. Тогда близнеца называет само имя — но только
  // при согласии формы: имя без обещания у другого протокола это уже другой
  // инбаунд, как бы похоже он ни назывался.
  if (labelled.protocol.isNotEmpty &&
      plain.protocol.isNotEmpty &&
      labelled.protocol != plain.protocol) {
    return false;
  }
  return plain.proxyName == nameWithoutChainClaim(labelled.proxyName);
}

/// Кандидат после сведения замера с фактами флота.
class AutoCandidate {
  final ProbeResult probe;
  final FleetFact fact;

  /// Счёт: чем меньше, тем лучше. `latency × k_proto × k_load`.
  final double score;

  /// Имя ЖИВОГО плоского близнеца — того же провода без обещания в имени.
  /// Пусто: обещания нет либо близнеца в этом проходе не нашлось.
  ///
  /// Заполняется [autoPick] вторым проходом: пока не собран весь список
  /// кандидатов, ответить на «есть ли у этого имени честный двойник» нечем.
  final String plainTwin;

  const AutoCandidate({
    required this.probe,
    required this.fact,
    required this.score,
    this.plainTwin = '',
  });

  AutoCandidate withPlainTwin(String twin) =>
      AutoCandidate(probe: probe, fact: fact, score: score, plainTwin: twin);

  bool get confirmed => probe.verdict == ProbeVerdict.ok;

  /// Адрес жив, а протокол проверить было нечем (сборка без ядра, старое
  /// ядро). Выбирать можно, называть проверенным — нет.
  bool get unconfirmed => probe.verdict.isUnconfirmed && probe.latencyMs >= 0;

  String get name => probe.id;

  /// Имя обещает вход, которого конфиг не строит.
  bool get overPromisesChain => fact.chainClaim == ChainClaim.labelOnly;

  /// Обещание в имени опровергнуто НЕ рассуждением, а живым близнецом: тот же
  /// провод в этом же проходе есть под честным именем.
  ///
  /// Такой кандидат из победы исключается целиком — см. [autoPick]. Из списка
  /// он не исчезает: человек мерил его собственными секундами и вправе видеть
  /// и число, и причину, по которой оно ничего не решило.
  bool get chainDisprovedByTwin => overPromisesChain && plainTwin.isNotEmpty;

  /// Строка для списка: чем имя не является.
  ///
  /// Там, где близнец найден, приписка называет ЕГО: «то же самое, что …» —
  /// это единственное, что объясняет, почему быстрый на вид узел не выбран, и
  /// объясняет фактом, а не порогом.
  String get listNote => chainDisprovedByTwin
      ? '«${fact.proxyName}» — вход только в имени: тот же узел и порт, '
            'что у «$plainTwin»'
      : naming.listNote;

  /// Честный вид имени — тот же для строки списка и для карточки итога.
  ///
  /// Узел, которого предложение не знает, обещание в имени НЕ теряет:
  /// [autoPick] заводит такому кандидату факт с именем из замера, и разбор
  /// имени работает и без панели.
  NodeNaming get naming => fact.naming;
}

/// Класс отказа: ПОЧЕМУ рабочей комбинации не нашлось. Он и определяет, что
/// человеку делать дальше, — «повторить» здесь почти всегда бесполезный совет.
enum AutoFailureKind {
  /// Мерить было нечего: узлов нет.
  noNodes,

  /// Ограничения пользователя не оставили ни одного узла.
  constrainedOut,

  /// Все узлы отвергли ключ подписки. Дело не в сети.
  keyRejected,

  /// До узлов есть TCP, но запросы сквозь них не проходят: похоже на
  /// фильтрацию протоколов в этой сети.
  protocolsBlocked,

  /// Сертификаты и закрытые порты: сторона оператора.
  operatorSide,

  /// Смешанная картина без явного большинства.
  mixed,
}

/// Отказ подбора вместе с уже готовым человеческим текстом.
class AutoFailure {
  final AutoFailureKind kind;

  /// Сколько узлов дало каждый вердикт — то, из чего сделан вывод.
  final Map<ProbeVerdict, int> tally;

  const AutoFailure({required this.kind, this.tally = const {}});

  /// Короткая причина для экрана.
  String get message => switch (kind) {
    AutoFailureKind.noNodes =>
      'Узлов для проверки нет. Обновите подписку или импортируйте её заново.',
    AutoFailureKind.constrainedOut =>
      'Под ваши ограничения не подошёл ни один узел. Снимите закрепление '
          'страны или типа подключения.',
    AutoFailureKind.keyRejected =>
      'Ключ подписки не принят ни одним узлом. Сеть тут ни при чём: адреса '
          'отвечают, а доступ по подписке закрыт.',
    AutoFailureKind.protocolsBlocked =>
      'До узлов есть связь, но трафик сквозь них не проходит. Похоже, в этой '
          'сети режут протоколы.',
    AutoFailureKind.operatorSide =>
      'Узлы отвечают неправильно (сертификат или закрытый порт). Это сторона '
          'оператора, и починить это с телефона нельзя.',
    AutoFailureKind.mixed =>
      'Ни один узел не пропустил проверочный запрос. Причины у разных узлов '
          'разные.',
  };

  /// Стоит ли предлагать повтор. Там, где причина в подписке или у оператора,
  /// кнопка «повторить» вернёт ровно то же самое.
  bool get retryable =>
      kind == AutoFailureKind.protocolsBlocked || kind == AutoFailureKind.mixed;
}

/// Итог подбора.
class AutoPickOutcome {
  /// Выбор; `null` — рабочей комбинации не нашлось.
  final AutoPickRecord? pick;

  /// Причина, когда выбора нет.
  final AutoFailure? failure;

  /// Все кандидаты в порядке предпочтения — из них строится итоговый экран.
  final List<AutoCandidate> ranked;

  /// Узлы, которые проверку не прошли, с их вердиктами.
  ///
  /// Они ОСТАЮТСЯ в итоге, а не выбрасываются: «девять узлов из тринадцати
  /// мертвы» — это самое важное, что человек может узнать про свой флот, и
  /// молчание об этом выглядит как «у оператора всего четыре сервера».
  final List<AutoCandidate> rejected;

  /// Прошлый выбор, который можно оставить, когда новый проход пуст.
  final AutoPickRecord? previous;

  const AutoPickOutcome({
    this.pick,
    this.failure,
    this.ranked = const <AutoCandidate>[],
    this.rejected = const <AutoCandidate>[],
    this.previous,
  });

  bool get hasPick => pick != null;

  /// Почему волны релэя нет. Это не «не успели»: пока конфиг оператора не
  /// несёт цепочки, переключение входа не меняет на проводе ни байта, и
  /// подбирать по нему нечего.
  ///
  /// Утверждение проверяется по ЭТОМУ проходу, а не зашито навсегда: как
  /// только источник подтвердит цепочку хоть у одного узла, фраза «цепочку не
  /// строит» станет таким же враньём, каким сейчас является суффикс в имени.
  String get relayNote =>
      ranked.any((c) => c.fact.chainClaim == ChainClaim.wired)
      ? 'Вход (relay) в подборе не участвует: там, где оператор цепочку '
            'построил, она вшита в сам узел, и отдельного выбора входа на '
            'этом пути нет.'
      : kRelayNotChainedNote;
}

/// Ответ по умолчанию: цепочки в конфиге нет, и выбирать вход нечем.
///
/// Живёт снаружи класса, чтобы им можно было пользоваться там, где итога
/// прохода ещё нет.
const String kRelayNotChainedNote =
    'Вход (relay) в подборе не участвует: на этом пути конфиг оператора '
    'цепочку не строит, и выбор входа ничего не меняет.';

/// Как назвать вердикт человеку. Короткая форма — для строки списка.
///
/// Живёт рядом с классами отказа намеренно: это один словарь, и вторая его
/// копия в экране протокола рано или поздно начала бы говорить про тот же
/// вердикт другое.
String probeVerdictShort(ProbeVerdict v) => switch (v) {
  ProbeVerdict.ok => 'работает',
  ProbeVerdict.tlsUntrusted => 'сертификат не принят',
  ProbeVerdict.authRejected => 'ключ не принят',
  ProbeVerdict.portClosed => 'адрес не отвечает',
  ProbeVerdict.timeout => 'не проходит',
  ProbeVerdict.unsupported => 'ядро не знает этот тип',
  ProbeVerdict.tcpOnly => 'проверен только адрес',
  ProbeVerdict.skipped => 'не проверяли',
  ProbeVerdict.unknown => 'не проверяли',
};

/// Полная форма — для карточки итога, где есть место на объяснение.
String probeVerdictText(ProbeVerdict v) => switch (v) {
  ProbeVerdict.ok => 'Сквозь узел прошёл проверочный запрос.',
  ProbeVerdict.tlsUntrusted =>
    'Узел отдаёт сертификат, которому клиент не верит. Это сторона оператора.',
  ProbeVerdict.authRejected =>
    'Адрес отвечает, но узел рвёт соединение: ключ подписки он не принял.',
  ProbeVerdict.portClosed => 'Адрес узла не отвечает вовсе.',
  ProbeVerdict.timeout =>
    'До узла есть связь, а запрос сквозь него не прошёл за отведённое время.',
  ProbeVerdict.unsupported =>
    'Ядро не собрало подключение из полей этого узла, проверить его нечем.',
  ProbeVerdict.tcpOnly =>
    'Проверен только адрес: в этой сборке проверить протокол нечем.',
  ProbeVerdict.skipped => 'До этого узла проверка не дошла.',
  ProbeVerdict.unknown =>
    'Ядро не сообщило, чем кончилась проверка (сборка старше вердиктов).',
};

/// Коэффициент протокола: во что обходится выбор формы подключения.
///
/// Это не «скорость», а стойкость к фильтрации: reality маскируется под чужой
/// сайт, голый TLS на нестандартном порту заметен, отсутствие TLS заметно
/// сильнее всего. Числа умеренные (±15%), чтобы форма НЕ перебивала разницу в
/// задержке между континентами.
double protocolFactor({required String protocol, required String security}) {
  final sec = security.toLowerCase();
  final proto = protocol.toLowerCase();
  if (sec == 'reality') return 0.85;
  if (proto == 'hysteria2' || proto == 'hysteria' || proto == 'tuic') {
    return 0.9;
  }
  if (sec == 'tls') return 1.0;
  if (sec == 'none') return 1.15;
  return 1.0;
}

/// Коэффициент загрузки: 0% не трогает счёт, 100% удваивает его на треть.
/// Неизвестная загрузка — множитель 1: догадываться о ней нечем.
double loadFactor(double loadPct) {
  if (loadPct.isNaN || loadPct < 0) return 1.0;
  final capped = loadPct > 100 ? 100.0 : loadPct;
  return 1.0 + capped / 300.0;
}

/// Порог гистерезиса: новый лидер должен выиграть И по счёту (на 20%), И по
/// задержке (на 30 мс), чтобы прошлый выбор сменили.
///
/// Волны «подтверждение top-3» здесь нет намеренно: у ядра нет замера
/// подмножества узлов, а второй полный проход стоит человеку тех же секунд.
/// Гистерезис даёт ту же устойчивость — выбор не прыгает от шума измерения —
/// и не стоит ничего.
const double kHysteresisScoreGain = 0.8;
const int kHysteresisLatencyGainMs = 30;

/// Полоса «всё остальное равно» для выбора между ярлыком и плоским узлом.
///
/// Ярлык цепочки НЕ делает узел хуже на проводе: пакеты у него ровно те же,
/// что у плоского соседа. Поэтому имя не трогает счёт — множитель за имя
/// выдумал бы разницу в качестве, которой нет, и увёл бы человека с быстрого
/// узла на медленный ради строки на экране.
///
/// Имя решает только НИЧЬЮ: когда разница между двумя узлами укладывается в
/// шум одного замера, берём тот, чьё имя не обещает больше, чем даёт конфиг.
/// Полоса по задержке та же, что у гистерезиса: второе число для «это уже шум»
/// неизбежно разошлось бы с первым.
///
/// Полоса работает только между РАЗНЫМИ проводами, где сравнивать по
/// миллисекундам осмысленно. Два имени одного провода она не судит вовсе — там
/// решает [isPlainTwin], и решает без порогов: полоса на такой паре всегда
/// была спорной (154 против 138 мс — это ×1.116, шаг за границу ×1.1), потому
/// что мерила шум одного канала как разницу качества.
const int kChainClaimTieBandMs = kHysteresisLatencyGainMs;

/// И по счёту: 10% — та же величина, только в долях, потому что счёт несёт ещё
/// форму подключения и загрузку.
const double kChainClaimTieBandRatio = 1.1;

/// Выбирает лучшую рабочую комбинацию.
///
/// [results] — ответ ядра, [facts] — что предложение знает про те же имена
/// прокси, [constraints] — ограничения пользователя, [previous] — прошлый
/// выбор (для гистерезиса и для «оставить прошлый»).
AutoPickOutcome autoPick({
  required List<ProbeResult> results,
  required Map<String, FleetFact> facts,
  AutoConstraints constraints = AutoConstraints.none,
  AutoPickRecord? previous,
  int serversUpdatedMs = 0,
  String networkKey = '',
  DateTime? at,
}) {
  final now = at ?? DateTime.now();
  if (results.isEmpty) {
    return AutoPickOutcome(
      failure: const AutoFailure(kind: AutoFailureKind.noNodes),
      previous: previous,
    );
  }

  // Ограничения применяются к тому, что померили: сузить набор ДО замера —
  // работа вызывающего, но ядро меряет весь конфиг, и второй фильтр здесь
  // обязателен, иначе закреплённая страна ничего не значила бы.
  final considered = <ProbeResult>[];
  for (final r in results) {
    final f = facts[r.id];
    if (!_passesConstraints(r, f, constraints)) continue;
    considered.add(r);
  }
  if (considered.isEmpty) {
    return AutoPickOutcome(
      failure: AutoFailure(
        kind: AutoFailureKind.constrainedOut,
        tally: tallyVerdicts(results),
      ),
      previous: previous,
    );
  }

  final ranked = <AutoCandidate>[];
  final rejected = <AutoCandidate>[];
  for (final r in considered) {
    final fact = facts[r.id] ?? FleetFact(proxyName: r.id);
    if (r.latencyMs < 0 || r.verdict.isFailure) {
      // Счёт непригодному узлу не считается: он не участвует в сравнении, а
      // число рядом с ним читалось бы как «почти подошёл».
      rejected.add(AutoCandidate(probe: r, fact: fact, score: double.infinity));
      continue;
    }
    final score =
        r.latencyMs *
        protocolFactor(protocol: fact.protocol, security: fact.security) *
        loadFactor(fact.loadPct);
    ranked.add(AutoCandidate(probe: r, fact: fact, score: score));
  }
  rejected.sort((a, b) => a.name.compareTo(b.name));

  // Подтверждённые всегда впереди непроверенных: «адрес жив» не равно
  // «трафик идёт», и смешивать их в одном порядке значило бы вернуть тот же
  // фолбэк, который ядро только что перестало делать.
  ranked.sort((a, b) {
    if (a.confirmed != b.confirmed) return a.confirmed ? -1 : 1;
    final c = a.score.compareTo(b.score);
    return c != 0 ? c : a.name.compareTo(b.name);
  });

  // Второй проход по уже собранному списку: у кого из обещающих имён нашёлся
  // живой плоский близнец. Раньше этого прохода не было, и вопрос «то же ли
  // это подключение» подменялся вопросом «на сколько разошлись замеры».
  final twinned = ranked
      .map((c) => c.withPlainTwin(_plainTwinNameOf(c, ranked)))
      .toList(growable: false);
  ranked
    ..clear()
    ..addAll(twinned);

  if (ranked.isEmpty) {
    return AutoPickOutcome(
      failure: AutoFailure(
        kind: _failureKindOf(considered),
        tally: tallyVerdicts(considered),
      ),
      rejected: rejected,
      previous: previous,
    );
  }

  final working = ranked.where((c) => c.confirmed).length;
  final checked = considered
      .where((r) => r.verdict != ProbeVerdict.skipped)
      .length;

  // Кандидат, чьё обещание опровергнуто близнецом, не участвует в ПОБЕДЕ
  // вовсе: он не «чуть хуже» честного соседа, он и есть тот же провод под
  // именем, которое врёт. Выбирать между ними по миллисекундам нечего.
  //
  // Пусто быть не может (у каждого исключённого близнец в этом же списке), но
  // страховка стоит строки: остаться без выбора там, где рабочие узлы есть, —
  // худший из возможных исходов.
  final eligible = ranked
      .where((c) => !c.chainDisprovedByTwin)
      .toList(growable: false);
  final pool = eligible.isEmpty ? ranked : eligible;
  final best = _preferHonestName(pool);
  final String bestReason;
  if (identical(best, ranked.first)) {
    bestReason = 'best_score';
  } else if (!identical(pool.first, ranked.first)) {
    // Лидера сменил именно близнец: это решение факта, а не порога, и
    // называться оно обязано иначе, чем поправка на полосу ничьей.
    bestReason = 'plain_twin';
  } else {
    bestReason = 'plain_over_labelled';
  }
  final winner = _applyHysteresis(pool, previous, best, bestReason);
  return AutoPickOutcome(
    pick: AutoPickRecord(
      proxyName: winner.candidate.name,
      exitKey: winner.candidate.fact.exitKey,
      countryCode: winner.candidate.fact.countryCode.isNotEmpty
          ? winner.candidate.fact.countryCode
          : winner.candidate.probe.country,
      machineTitle: winner.candidate.fact.machineTitle,
      protocolLabel: winner.candidate.fact.protocolLabel,
      latencyMs: winner.candidate.probe.latencyMs,
      confirmed: winner.candidate.confirmed,
      checked: checked,
      working: working,
      total: results.length,
      updatedMs: now.millisecondsSinceEpoch,
      serversUpdatedMs: serversUpdatedMs,
      networkKey: networkKey,
      reasonCode: winner.reasonCode,
    ),
    ranked: ranked,
    rejected: rejected,
    previous: previous,
  );
}

class _Winner {
  final AutoCandidate candidate;
  final String reasonCode;
  const _Winner(this.candidate, this.reasonCode);
}

/// Имя живого плоского близнеца [c] среди [all]; пусто — близнеца нет.
///
/// Близнец, который сам проверку не прошёл, заменой не становится: провод у
/// них общий, но ответил он по-разному, и подставлять вместо работающего узла
/// его неотвечающего двойника значит чинить подпись ценой подключения.
String _plainTwinNameOf(AutoCandidate c, List<AutoCandidate> all) {
  if (!c.overPromisesChain) return '';
  for (final o in all) {
    if (identical(o, c) || o.overPromisesChain) continue;
    if (c.confirmed && !o.confirmed) continue;
    if (isPlainTwin(labelled: c.fact, plain: o.fact)) return o.name;
  }
  return '';
}

/// Лидер с поправкой на честность имени.
///
/// Возвращает `ranked.first`, если тот не обещает лишнего. Если обещает —
/// ищет в пределах полосы шума ближайшего кандидата, чьё имя ничего не
/// обещает, и отдаёт его. Дальше полосы не смотрит: там узел уже ощутимо
/// хуже, и менять качество на подпись нельзя.
///
/// Подтверждённость имени не уступает: непроверенный узел с честным именем
/// хуже проверенного с ярлыком, и ради подписи мы не вернём тот самый фолбэк,
/// который ядро только что перестало делать.
AutoCandidate _preferHonestName(List<AutoCandidate> ranked) {
  final best = ranked.first;
  if (!best.overPromisesChain) return best;
  for (final c in ranked) {
    if (identical(c, best)) continue;
    if (c.confirmed != best.confirmed) break;
    if (c.overPromisesChain) continue;
    final withinLatency =
        c.probe.latencyMs - best.probe.latencyMs <= kChainClaimTieBandMs;
    final withinScore = c.score <= best.score * kChainClaimTieBandRatio;
    // Список отсортирован по возрастанию счёта: первый, кто вышел из полосы,
    // закрывает и всех, кто за ним.
    if (!withinLatency || !withinScore) break;
    return c;
  }
  return best;
}

/// Удерживает прошлый выбор, пока новый лидер не выиграл ощутимо.
///
/// Без этого выбор прыгал бы между двумя узлами, разница между которыми
/// укладывается в шум одного замера, — и каждый прыжок это переподключение,
/// которое человек видит.
///
/// [best] — уже выбранный лидер прохода (см. [_preferHonestName]), а не
/// `ranked.first`: иначе поправка на честность имени пропадала бы всякий раз,
/// когда есть прошлый выбор.
///
/// [pool] — кандидаты, которым победа вообще разрешена. Прошлый выбор ищется
/// ТОЛЬКО в нём: узел, чьё обещание опровергнуто близнецом, гистерезис иначе
/// удерживал бы вечно — один раз закрепившись, он не проиграл бы уже никогда.
_Winner _applyHysteresis(
  List<AutoCandidate> pool,
  AutoPickRecord? previous,
  AutoCandidate best,
  String bestReason,
) {
  if (previous == null || previous.proxyName == best.name) {
    return _Winner(best, bestReason);
  }
  AutoCandidate? held;
  for (final c in pool) {
    if (c.name == previous.proxyName) {
      held = c;
      break;
    }
  }
  // Прошлый выбор в этом проходе не работает — держать нечего.
  if (held == null || held.confirmed != best.confirmed) {
    return _Winner(best, bestReason);
  }
  final scoreGain = best.score <= held.score * kHysteresisScoreGain;
  final latencyGain =
      held.probe.latencyMs - best.probe.latencyMs > kHysteresisLatencyGainMs;
  if (scoreGain && latencyGain) return _Winner(best, bestReason);
  return _Winner(held, 'kept_previous');
}

bool _passesConstraints(
  ProbeResult r,
  FleetFact? fact,
  AutoConstraints constraints,
) {
  if (constraints.isEmpty) return true;
  final country = constraints.pinnedCountry.toUpperCase();
  if (country.isNotEmpty) {
    final own = (fact?.countryCode.isNotEmpty ?? false)
        ? fact!.countryCode.toUpperCase()
        : r.country.toUpperCase();
    // Узел, страну которого источник не называет, под закреплением страны НЕ
    // проходит: «наверное он немецкий» — это ровно та догадка, из-за которой
    // на Home стояла «Германия» над канадским выходом.
    if (own != country) return false;
  }
  final family = constraints.protocolFamily.toLowerCase();
  if (family.isNotEmpty) {
    final own = (fact?.protocol ?? '').toLowerCase();
    if (own != family) return false;
  }
  return true;
}

/// Сколько узлов дало каждый вердикт.
Map<ProbeVerdict, int> tallyVerdicts(List<ProbeResult> results) {
  final out = <ProbeVerdict, int>{};
  for (final r in results) {
    out[r.verdict] = (out[r.verdict] ?? 0) + 1;
  }
  return out;
}

/// Класс отказа по большинству вердиктов.
///
/// «Большинство», а не «все»: на живом флоте один узел почти всегда отвечает
/// иначе прочих, и требовать единогласия значило бы всегда отвечать
/// «смешанная картина» — то есть не отвечать ничего.
AutoFailureKind _failureKindOf(List<ProbeResult> results) {
  final tally = tallyVerdicts(results);
  final judged = results.where((r) => r.verdict != ProbeVerdict.skipped).length;
  if (judged == 0) return AutoFailureKind.mixed;

  int count(ProbeVerdict v) => tally[v] ?? 0;
  final auth = count(ProbeVerdict.authRejected);
  final timeout = count(ProbeVerdict.timeout);
  final operator =
      count(ProbeVerdict.tlsUntrusted) + count(ProbeVerdict.portClosed);

  final half = judged / 2;
  if (auth > half) return AutoFailureKind.keyRejected;
  if (timeout > half) return AutoFailureKind.protocolsBlocked;
  if (operator > half) return AutoFailureKind.operatorSide;
  return AutoFailureKind.mixed;
}

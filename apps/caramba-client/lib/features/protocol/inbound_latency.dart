/// Задержка КАЖДОГО инбаунда, померенная устройством пользователя, и вердикт
/// этой проверки.
///
/// Владелец просил: «когда пользователь заходил в этот набор тоже показывался
/// пинг этих инбаундов». Слово «этих» здесь главное. Панель про инбаунд не
/// знает ничего: `nodes.latency_ms` — это RTT самой МАШИНЫ до её цели по
/// heartbeat, одно число на весь узел. Восемь входов немецкой машины получили
/// бы от панели одно и то же значение, и выбирать между ними по нему было бы
/// нельзя. Поэтому числа здесь только собственные: их приносит `probe` ядра,
/// которое соединяется с каждым прокси отдельно, со своей сети.
///
/// Мост между строкой списка и результатом замера — [ProtocolRow.proxyNames]:
/// имена прокси в теле конфига, те же, которыми ядро называет узлы в ответе
/// `probe`. Ровно этот мост уже используют узлы выхода
/// (`_withMeasurements` в exit_inventory_state.dart), и второй способ
/// сопоставления завёл бы второе расхождение.
///
/// ЧТО ЧИСЛО ЗНАЧИТ, А ЧТО НЕТ. Раньше не значило почти ничего: `probeOne` при
/// провале URL-теста молча возвращал время голого TCP-соединения, и вход с
/// отвергнутым ключом показывался самым быстрым в списке. Теперь ядро отдаёт
/// [ProbeVerdict], и `latencyMs` живёт только там, где сквозь вход прошёл
/// настоящий запрос ([ProbeVerdict.ok]) — или там, где ядра в сборке нет вовсе
/// и проверен один адрес ([ProbeVerdict.tcpOnly]). Разницу между этими двумя
/// случаями экран обязан произносить вслух: число у них одинаково выглядит, а
/// отвечает на разные вопросы. Всё остальное — не число, а названная причина.
library;

import 'package:caramba_vpn/caramba_vpn.dart' show ProbeVerdict;
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/probe_state.dart';

/// За сколько замер устаревает настолько, что экран меряет заново при
/// открытии. Десять минут — компромисс между «числа врут» и «каждый заход
/// открывает десяток соединений»: сеть за это время успевает смениться
/// незаметно для приложения, а держать свежесть точнее нечем — событий смены
/// сети приложение пока не получает.
const Duration kInboundProbeMaxAge = Duration(minutes: 10);

/// Что означает число в строке — один раз над списком, а не подписью под
/// каждым из семи.
const String kProbeMeaningNote =
    'Числа — ваш собственный замер с этого устройства, а не пинг панели: у '
    'панели одно число на всю машину, про отдельный вход она не знает ничего. '
    'Число здесь — сколько занял настоящий запрос СКВОЗЬ этот вход. Вход, '
    'который запрос не пропустил, числа не получает: на его месте стоит '
    'причина.';

/// Оговорка для сборки/ядра, которые протокол проверить не смогли. Показывается
/// только когда такие строки в списке есть: сказанная всегда, она обесценила бы
/// и честные числа.
const String kProbeTcpOnlyNote =
    'У части входов проверен только адрес, а не протокол. Такое число значит '
    '«адрес отвечает», а не «вход принял ваш ключ»: узел с отозванным ключом '
    'отвечает на TCP так же бодро, как рабочий.';

/// Итог замера для ОДНОЙ строки списка.
///
/// Строка — это тройка `протокол/транспорт/безопасность`, и на неё может
/// приходиться несколько прокси: у одной машины такой инбаунд один, а в
/// области «весь флот» их столько, сколько машин его предлагают. Поэтому
/// вместе с числом едет счёт — иначе «45 мс» на строке, которую предлагают
/// три машины, читалось бы как замер того самого входа, к которому
/// подключишься.
class InboundLatency {
  /// Что показать в строке. Число всегда [LatencySource.client]: операторского
  /// пинга у инбаунда не существует.
  final Latency latency;

  /// Чем кончилась проверка того прокси, чьё число показано; а если числа нет
  /// — самая содержательная из причин отказа.
  final ProbeVerdict verdict;

  /// RTT установки TCP с адресом лучшего из измеренных прокси; `-1` —
  /// неизвестно. Это число про АДРЕС, а не про вход, и живёт оно только в
  /// объяснении отказа: «адрес отвечает, а вход — нет» — самое полезное, что
  /// можно сказать про мёртвый узел.
  final int tcpMs;

  /// Сколько прокси этой строки замер вообще назвал (ответившие + отказы).
  final int measured;

  /// Сколько прокси у строки всего.
  final int named;

  /// Сколько из названных ответили числом.
  final int answered;

  const InboundLatency({
    required this.latency,
    required this.verdict,
    required this.tcpMs,
    required this.measured,
    required this.named,
    required this.answered,
  });

  /// Мерить нечего: источник не назвал ни одного имени прокси. Это не
  /// «медленно» и не «таймаут» — это отсутствие моста между строкой и ядром, и
  /// строка в таком состоянии не показывает даже прочерка.
  static const InboundLatency nothingToMeasure = InboundLatency(
    latency: Latency.none,
    verdict: ProbeVerdict.unknown,
    tcpMs: -1,
    measured: 0,
    named: 0,
    answered: 0,
  );

  bool get hasProxies => named > 0;

  /// Замерены не все прокси строки. Важно только там, где их больше одного.
  bool get isPartial => measured > 0 && measured < named;

  /// Число есть, но оно про адрес, а не про вход.
  bool get isUnconfirmed =>
      answered > 0 &&
      (verdict == ProbeVerdict.tcpOnly || verdict == ProbeVerdict.unknown);

  /// Чем кончилась проверка — словами, которые что-то меняют для человека.
  /// `null` — сквозь вход прошёл настоящий запрос, и число говорит само.
  String? get verdictNote {
    if (measured == 0) return null;
    final tcp = tcpMs >= 0 ? ' Адрес при этом отвечает за $tcpMs мс.' : '';
    return switch (verdict) {
      ProbeVerdict.ok => null,
      // Число есть — и оно отвечает на более слабый вопрос. Молчать об этом
      // нельзя: рядом в списке стоят числа, добытые настоящим запросом.
      ProbeVerdict.tcpOnly =>
        'Проверен только адрес: протокол этой сборке проверить нечем.',
      ProbeVerdict.unknown =>
        answered > 0
            ? 'Ядро не сказало, чем кончилась проверка: число значит лишь, '
                  'что ответ был.'
            : null,
      ProbeVerdict.authRejected =>
        'Не проходит: вход не принял ключ подписки.$tcp',
      ProbeVerdict.tlsUntrusted =>
        'Не проходит: сертификат входа не принят.$tcp',
      ProbeVerdict.portClosed => 'Не проходит: адрес не отвечает.',
      ProbeVerdict.timeout =>
        'Не проходит: запрос сквозь вход не уложился в срок.$tcp',
      ProbeVerdict.unsupported =>
        'Не проверен: ядро не собрало адаптер для этого входа.',
      ProbeVerdict.skipped => 'Проверку до этого входа не довели.',
    };
  }

  /// У строки несколько прокси, и число — лучшее из ответивших. Об этом надо
  /// сказать словами: иначе число читается как замер того входа, к которому
  /// подключишься, а подключишься не обязательно к нему.
  ///
  /// Счёт ведётся в ПРОКСИ, а не в узлах. Прокси на строку приходится больше
  /// одного по двум разным причинам, и «узлы» верны только для одной из них: в
  /// области «весь флот» вход предлагают несколько машин, а на одной машине
  /// панельный генератор выпускает тот же вход дважды — прямым набором и через
  /// вход («via 🇷🇺»). Сказать про немецкий узел «лучший из 2 узлов» значило бы
  /// назвать второй машиной второй путь до первой. Сколько за строкой машин,
  /// говорит соседняя фраза подписи — она считает по `ProtocolRow.exitKeys`.
  String? get spreadNote {
    if (named < 2) return null;
    if (answered == 0) {
      return measured == 0
          ? null
          : 'Ни один из $named прокси этого входа не пропустил запрос.';
    }
    return 'Это лучший из $answered ответивших прокси (всего их $named).';
  }
}

/// Замеры активного профиля, готовые к сопоставлению со строками списка.
///
/// Внутри — весь [ProbeSnapshot] целиком, а не выдержка из него: числа,
/// вердикты и TCP лежат в одной записи и обязаны читаться из одной. Разложив
/// их по трём провайдерам, мы бы завели три момента времени и получили строку,
/// у которой число из нового замера, а причина из старого.
class InboundLatencyLookup {
  /// Последний замер активного профиля; `null` — замера не было.
  final ProbeSnapshot? snapshot;

  /// Замер в полёте.
  final bool measuring;

  const InboundLatencyLookup({required this.snapshot, required this.measuring});

  static const InboundLatencyLookup empty = InboundLatencyLookup(
    snapshot: null,
    measuring: false,
  );

  Map<String, int> get byProxyName =>
      snapshot?.latencyMs ?? const <String, int>{};

  /// Когда замер выполнялся последний раз; `null` — не выполнялся ни разу.
  DateTime? get measuredAt => snapshot?.updatedAt;

  /// Задержка строки.
  ///
  /// Правило то же, что у узла выхода (`_withMeasurements`), и по той же
  /// причине: у строки несколько прокси, они мерятся по отдельности, и
  /// приписывать ей ХУДШИЙ — значит наказывать её за далёкую машину, к которой
  /// пользователь всё равно не подключится, раз есть ближняя. Поэтому берётся
  /// лучший ответивший.
  ///
  /// Если замер прокси НАЗВАЛ, но ни один не пропустил запрос, — это отказ
  /// строки (`-1`), а не «не мерили»: разницу видно и в цвете, и в сортировке,
  /// и склеивать их значило бы прятать мёртвый вход среди неизмеренных.
  InboundLatency of(ProtocolRow row) {
    final names = row.proxyNames;
    if (names.isEmpty) return InboundLatency.nothingToMeasure;
    final snap = snapshot;

    int? best;
    String? bestName;
    var measured = 0;
    var answered = 0;
    ProbeVerdict? failure;

    for (final name in names) {
      final ms = byProxyName[name];
      if (ms == null) continue;
      measured++;
      final v = snap?.verdictOf(name) ?? ProbeVerdict.unknown;
      if (ms < 0) {
        // Причина отказа выбирается по содержательности, а не по порядку имён:
        // «ключ не принят» человеку говорит больше, чем «адрес молчит», и
        // строка, у которой одна машина мертва, а другая отвергла ключ, должна
        // называть второе.
        if (_failureRank(v) < _failureRank(failure)) failure = v;
        continue;
      }
      answered++;
      if (best == null || ms < best) {
        best = ms;
        bestName = name;
      }
    }

    final Latency latency;
    ProbeVerdict verdict;
    if (measured == 0) {
      // Ни одного числа. «Меряю» ставится только пока замер идёт: строка,
      // застрявшая в «меряю» навсегда, хуже честного прочерка.
      latency = measuring ? Latency.measuring : Latency.none;
      verdict = ProbeVerdict.unknown;
    } else if (best == null) {
      latency = const Latency.fromClient(-1);
      verdict = failure ?? ProbeVerdict.unknown;
    } else {
      latency = Latency.fromClient(best);
      verdict = snap?.verdictOf(bestName!) ?? ProbeVerdict.unknown;
    }

    return InboundLatency(
      latency: latency,
      verdict: verdict,
      tcpMs: bestName != null
          ? (snap?.tcpMs[bestName] ?? -1)
          : _bestTcp(names, snap),
      measured: measured,
      named: names.length,
      answered: answered,
    );
  }

  /// Есть ли в списке строки, чьё число говорит только про адрес. От этого
  /// зависит, показывать ли [kProbeTcpOnlyNote]: сказанная там, где все числа
  /// добыты настоящим запросом, она обесценила бы их.
  bool anyUnconfirmed(List<ProtocolRow> rows) =>
      rows.any((r) => of(r).isUnconfirmed);

  int _bestTcp(List<String> names, ProbeSnapshot? snap) {
    if (snap == null) return -1;
    int? best;
    for (final n in names) {
      final ms = snap.tcpMs[n];
      if (ms == null || ms < 0) continue;
      if (best == null || ms < best) best = ms;
    }
    return best ?? -1;
  }
}

/// Ярус строки в списке: чем меньше, тем выше.
///
/// Порядок в этом списке до сих пор был порядком инбаундов у узла — то есть
/// решением оператора, а не подсказкой. С появлением чисел он обязан
/// подсказывать, ровно как в списке машин: сначала то, что проверено и быстро,
/// в конце то, что выбрать нельзя. Разница с машинами одна: у входа есть
/// состояние «число есть, а подтверждения нет», и оно стоит НИЖЕ
/// подтверждённых — иначе TCP-число обгоняло бы настоящий замер, что и было
/// исходной болезнью.
///
/// «Не мерили» стоит выше отказа намеренно: неизвестность — это не приговор, и
/// строка, которую замер не назвал, может оказаться лучшей.
int inboundTier({required bool selectable, required InboundLatency latency}) {
  if (!selectable) return 4;
  if (latency.answered == 0) return latency.measured == 0 ? 2 : 3;
  return latency.isUnconfirmed ? 1 : 0;
}

/// Сравнение двух строк списка. [index] — исходный порядок источника: он и
/// остаётся последним словом, чтобы одинаковые строки не прыгали местами между
/// перерисовками.
int compareInboundRows(
  (int index, bool selectable, InboundLatency latency) a,
  (int index, bool selectable, InboundLatency latency) b,
) {
  final ta = inboundTier(selectable: a.$2, latency: a.$3);
  final tb = inboundTier(selectable: b.$2, latency: b.$3);
  if (ta != tb) return ta.compareTo(tb);
  final ma = a.$3.latency.sortKey;
  final mb = b.$3.latency.sortKey;
  if (ma != mb) return ma.compareTo(mb);
  return a.$1.compareTo(b.$1);
}

/// Насколько причина отказа содержательна: меньше — важнее. `ok` и «не знаю»
/// причинами не являются и стоят в конце.
int _failureRank(ProbeVerdict? v) => switch (v) {
  ProbeVerdict.authRejected => 0,
  ProbeVerdict.tlsUntrusted => 1,
  ProbeVerdict.timeout => 2,
  ProbeVerdict.portClosed => 3,
  ProbeVerdict.unsupported => 4,
  ProbeVerdict.skipped => 5,
  _ => 99,
};

/// Замеры активного профиля для экрана «Тип подключения».
///
/// Числа берутся с профиля — того же места, откуда их берёт список серверов.
/// Своего хранилища у экрана нет намеренно: два источника одного факта
/// разъезжаются, и тогда одна и та же машина получает на двух экранах разные
/// числа.
final inboundLatencyProvider = Provider<InboundLatencyLookup>((ref) {
  return InboundLatencyLookup(
    snapshot: ref.watch(activeConnectionProfileProvider)?.lastProbe,
    measuring: ref.watch(probeRunProvider).measuring,
  );
});

/// Надо ли мерить при открытии экрана.
///
/// Три причины запустить замер и ни одной больше: замера не было вовсе, он
/// устарел, или чисел нет несмотря на отметку времени (профиль сменился).
/// Причина НЕ мерить сильнее любой из них ровно одна — замер уже идёт: два
/// параллельных прохода открыли бы вдвое больше соединений и переписали бы
/// результат друг друга.
bool shouldProbeInbounds({
  required bool measuring,
  required bool hasRowsToMeasure,
  required Map<String, int> measured,
  required DateTime? measuredAt,
  required DateTime now,
  Duration maxAge = kInboundProbeMaxAge,
}) {
  if (measuring) return false;
  if (!hasRowsToMeasure) return false;
  if (measured.isEmpty) return true;
  if (measuredAt == null) return true;
  return now.difference(measuredAt) > maxAge;
}

/// Когда мерили — словами. Точность нарочно грубая: секунды здесь ничего не
/// решают, а «17 сек назад» на строке, которую перерисовывают каждый кадр,
/// выглядит тикающим счётчиком, которым не является.
String measuredAgoText(DateTime measuredAt, DateTime now) {
  final d = now.difference(measuredAt);
  if (d.inSeconds < 60) return 'только что';
  if (d.inMinutes < 60) return '${d.inMinutes} мин назад';
  if (d.inHours < 24) return '${d.inHours} ч назад';
  return '${d.inDays} дн назад';
}

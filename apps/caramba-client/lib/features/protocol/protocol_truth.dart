/// Что сказать про тип подключения, когда закреплённый тип и тип на проводе —
/// разные вещи.
///
/// СНЯТО НА УСТРОЙСТВЕ. Закреплён TUIC (его собственный замер честно говорит
/// «Не проходит: адрес не отвечает»), переподключение, Главная: «Защищено»,
/// сервер «🇩🇪 Stream», строка «Тип подключения: TUIC». В логе ядра при этом
/// `match Match using CARAMBA[🇩🇪 Stream]` — vless. Экран назвал TUIC над
/// туннелем, которого по TUIC нет.
///
/// ПОЧЕМУ ЯДРО ТАК ДЕЛАЕТ. `applyProtocol` (libs/caramba-core/profile/
/// profile.go) собирает url-test группу `Caramba-Proto` из прокси, у которых
/// `m["type"]` равен `protocolClashType[Policy.Protocol]`, и ставит её первой в
/// селекторе CARAMBA. Группа собрана — но если ни один её узел не поднимается,
/// селектор уходит на следующий рабочий прокси. Это ПРАВИЛЬНОЕ поведение ядра:
/// связь по другому протоколу лучше, чем отсутствие связи. Менять его никто не
/// собирается — но раз оно молчаливое, назвать деградацию обязан экран.
///
/// ЧТО ПОКАЗЫВАТЬ. Выбран первый из трёх вариантов — ФАКТИЧЕСКИЙ тип с
/// пометкой о закреплённом:
///
///   * Значение строки — единственное, что читают наверняка. Оставить в нём
///     «TUIC» и увести правду в подпись значит повторить ровно ту ошибку,
///     из-за которой над мёртвым туннелем трижды стояло «Защищено»: громкое
///     утверждение и тихая оговорка рядом.
///   * Строка живёт в группе «Сервер / Relay (вход) / Режим», и все соседи
///     называют то, ЧТО ЕСТЬ, а не то, что было заказано. Строка «Сервер»
///     ровно так и устроена: заголовок берётся у живого выхода, а подмену
///     выбора объясняет баннер под группой.
///   * Закреплённый тип при этом не теряется: он назван и в самом значении
///     («VLESS вместо TUIC»), и целиком в баннере, и остаётся выделенным на
///     экране выбора, и продолжает стоять строкой в Настройках — там контекст
///     настройки, и показывать надо именно настройку.
///
/// Третий вариант — не показывать ничего до переподключения — отброшен: пустая
/// строка над живым туннелем это не отсутствие утверждения, а отказ отвечать на
/// вопрос, ради которого на строку смотрят.
///
/// ГРАНИЦА РАСХОЖДЕНИЯ — СЕМЕЙСТВО ЯДРА, а не строка пикера. Ядро сравнивает
/// только `type:`, поэтому закреплённый `VLESS · Reality` на инбаунде
/// `vless · ws · tls` — это НЕ подмена: ядро ничего другого и не обещало, а про
/// схлопывание форм экран выбора говорит сам. Считать расхождение по форме
/// значило бы поднимать тревогу на каждом пине Reality.
library;

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/domain/autopilot/auto_pick.dart' show FleetFact;
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/features/protocol/protocol_screen.dart'
    show coreFamilyForProtocol, protocolFamilyTitle, protocolGlyph;
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/vpn_state.dart';

/// Готовый ответ строки «Тип подключения» на Главной.
class ProtocolTruth {
  /// Текст значения строки.
  final String value;

  /// Иконка строки; `null` — брать иконку закреплённой опции.
  ///
  /// При расхождении она СВОЯ: молния TUIC рядом со словом «VLESS» — второе
  /// утверждение о протоколе, и оно осталось бы ложным.
  final String? glyph;

  /// Закреплённый тип и тип на проводе разошлись.
  final bool diverged;

  /// Полная фраза для баннера под группой; пусто — говорить нечего.
  ///
  /// Отдельно от [value] потому, что у `CRow` подписи нет вовсе, а в ширину
  /// значения объяснение не влезает — тем же приёмом под группой объясняются
  /// подмена страны и недоступная цепочка.
  final String note;

  const ProtocolTruth({
    required this.value,
    this.glyph,
    this.diverged = false,
    this.note = '',
  });
}

/// Сводит закреплённый тип с тем, по которому ядро работает прямо сейчас.
///
/// Чистая: всё, что нужно для решения, приходит аргументами — иначе это
/// проверялось бы только наблюдением за экраном, а расхождение типов ровно тем
/// и опасно, что на экране выглядит нормально.
///
/// [pinned] — индекс в [options] (`CoreConfig.protocol`). [activeProxy] — имя
/// прокси, на котором ядро стоит (ABI v2); пусто — туннеля нет. [facts] —
/// что предложение знает про имена прокси.
ProtocolTruth protocolTruthOf({
  required List<ProtocolOption> options,
  required int pinned,
  required String? activeProxy,
  required Map<String, FleetFact> facts,
  required AutoLabel auto,
}) {
  final option =
      (pinned >= 0 && pinned < options.length) ? options[pinned] : null;
  if (option == null) return const ProtocolTruth(value: '·');

  // «Авто» — отказ от выбора: расходиться нечему, и подпись автоподбора уже
  // называет то, что в туннеле.
  if (option.auto) return ProtocolTruth(value: auto.value);

  final pinnedTruth = ProtocolTruth(value: option.name);

  // Туннеля нет либо ядро не назвало узел: фактического типа НЕ СУЩЕСТВУЕТ, и
  // молчание источника не превращается в расхождение. Строка называет
  // настройку — это единственное, что в этот момент правда.
  final proxy = (activeProxy ?? '').trim();
  if (proxy.isEmpty) return pinnedTruth;
  final fact = facts[proxy];
  if (fact == null) return pinnedTruth;

  final wire = _wireFamilyOf(fact, options);
  // Ядро назвало узел, а чем он принимает соединение, предложение не знает.
  // Назвать это подменой нельзя: мы не знаем, что подменили.
  if (wire.isEmpty) return pinnedTruth;
  if (wire == option.coreFamily) return pinnedTruth;

  final actual = protocolFamilyTitle(wire);
  return ProtocolTruth(
    // Фактический тип стоит ПЕРВЫМ намеренно: значение обрезается
    // многоточием справа, и обрезанная строка обязана оставаться правдой.
    value: '$actual вместо ${option.name}',
    glyph: protocolGlyph(wire),
    diverged: true,
    note: 'Закреплён ${option.name}, но туннель поднят по $actual. Ядро '
        'собирает группу из входов закреплённого типа и, когда ни один из них '
        'не отвечает, берёт другой рабочий вход — иначе связи не было бы '
        'вовсе. Пока это так, трафик идёт по $actual.',
  );
}

/// Семейство, по которому ядро отбирает прокси, для инбаунда НА ПРОВОДЕ.
///
/// Идёт через [coreFamilyForProtocol], а не через собственную таблицу: имя
/// семейства должно приходить оттуда же, откуда его берёт пикер, иначе две
/// половины одного вопроса разъедутся. Ядру такой инбаунд попросить нечем
/// (`naive` и прочее) — берём слово источника как есть: выдумывать тут нечего,
/// а промолчать значит оставить закреплённый тип над чужим протоколом.
String _wireFamilyOf(FleetFact fact, List<ProtocolOption> options) {
  final key = ProtocolKey(
    protocol: fact.protocol,
    transport: fact.transport,
    security: fact.security,
  );
  final family = coreFamilyForProtocol(key, options);
  if (family != null && family.isNotEmpty) return family;
  return fact.protocol.trim().toLowerCase();
}

/// Строка «Тип подключения» на Главной — одним источником для обеих её веток.
final protocolTruthProvider = Provider<ProtocolTruth>((ref) {
  return protocolTruthOf(
    options: ref.watch(protocolsProvider),
    pinned: ref.watch(coreConfigProvider).protocol,
    activeProxy: ref.watch(activeProxyProvider),
    facts: ref.watch(fleetFactsProvider),
    auto: ref.watch(autoProtocolLabelProvider),
  );
});

/// Что сказать в момент закрепления типа.
///
/// НАДО ЛИ ВООБЩЕ ДАВАТЬ ЗАКРЕПИТЬ ТИП, ВСЕ ВХОДЫ КОТОРОГО МОЛЧАТ. Да, давать.
/// Замер — это снимок ОДНОЙ сети в ОДИН момент: за портал в кафе, за
/// перегруженный мобильный канал и за собственную оговорку экрана («проверен
/// только адрес») он не отвечает. Превратить такое измерение в запрет значит
/// сделать из мягкого сигнала жёсткое правило — та же неправда, только
/// наоборот. К тому же выбор применяется на СЛЕДУЮЩЕМ подъёме, а к тому
/// времени узел может ожить.
///
/// Но раз ядро деградирует молча, момент закрепления обязан назвать, чем это
/// кончится. Здесь текст короче обычного: у тоста 2,4 секунды, и подробность
/// съела бы главное. Подробность ждёт на Главной — там же, где человек увидит
/// последствие.
///
/// [exact] — закреплён конкретный прокси (сырой путь), а не только семейство.
/// [noneAnswered] — замер прошёл, и ни один вход этого типа запрос не
/// пропустил.
String protocolPinToast({
  required String label,
  required bool exact,
  required bool noneAnswered,
}) {
  // Про точность пина в этом случае молчим намеренно: «инбаунд закреплён»
  // рядом с «ни один вход не отвечает» читается как обещание, что закреплённое
  // и поднимется.
  if (noneAnswered) {
    return 'Тип подключения: $label — сейчас ни один вход этого типа не '
        'отвечает, ядро поднимет туннель другим типом';
  }
  return exact
      ? 'Тип подключения: $label — инбаунд закреплён'
      : 'Тип подключения: $label — ядро закрепит семейство';
}

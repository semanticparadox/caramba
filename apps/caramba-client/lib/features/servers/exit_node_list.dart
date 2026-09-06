import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/subscription.dart' show AccessState;
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/servers/access_card.dart';
import 'package:caramba_client/features/servers/fleet_alignment.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Плоский список выходов: МАШИНЫ всех стран сразу.
///
/// Раньше выбор шёл в два шага — страна, потом узел внутри неё, — и это была
/// лишняя дверь: узлов у оператора десяток, а не сотня, и владелец сервиса
/// прямо сказал, что хочет выбирать ноду сразу. Страна никуда не делась, она
/// переехала В СТРОКУ: флаг и код слева, имя машины заголовком. Так её видно
/// на том же экране, где делается выбор, а не уровнем выше.
///
/// Строка — МАШИНА, а не прокси. В теле подписки узла как сущности нет, есть
/// только инбаунды, и восемь входов одной немецкой машины когда-то читались
/// как восемь серверов. Поэтому строка берётся из [Offering], где машина уже
/// восстановлена (по одинаковому `server:` в импорте, по `nodes.id` у панели),
/// а её инбаунды посчитаны и лежат бейджем. Тип подключения выбирается
/// отдельно, на главном экране, и дублировать его строками списка незачем.
///
/// Выбор уходит через [ExitSelectionController], которому нужен [ExitNode]: у
/// панели это `node_id`, у импорта — имя прокси, которое читает `connectRaw`.
/// Машина разрешается в узел здесь, а не в контроллере, потому что связь
/// «машина → её прокси» знает только предложение.
///
/// Виджет ничего не выбирает сам: он зовёт [onSelect], а решение — что делать с
/// выбором и как показать результат синхронизации с панелью — принимает экран,
/// который его встроил. Иначе политика выбора жила бы в двух местах.
class ExitNodeList extends ConsumerWidget {
  /// Выбран узел; `null` — строка «Авто» (пин снят, узел выбирает ядро).
  final void Function(ExitNode? node) onSelect;

  const ExitNodeList({required this.onSelect, super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final inventory = ref.watch(exitInventoryProvider);
    final offering = ref.watch(offeringProvider);
    final selectedKey = inventory.selectedNodeKey;
    // Отказ подписки накрывает ВСЕ строки разом: они описывают один и тот же
    // флот, и оставить их нажимаемыми значило бы обещать подключение, которого
    // не будет.
    final blocked = inventory.blockedBy;

    // Предложение описывает ЭТОТ же источник — только тогда его машинам можно
    // верить. Пока каталог CSM ведёт инвентарь, а предложение ещё нет, обе
    // половины разошлись бы, и экран показал бы чужие машины; в этом случае
    // строкой остаётся узел инвентаря, как и раньше.
    final exits = fleetSourcesAgree(inventory.source, offering.source)
        ? offering.exits
        : const <ExitOffer>[];
    final auto = ref.watch(autoServerLabelProvider);

    if (inventory.nodes.isEmpty) {
      return const InlineEmpty(message: 'Узлов нет');
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ListItemCard(
          leading: const IBox(Lucide.gauge),
          // «Авто» называет свой выбор. Пустое «Авто» — контрол, который не
          // сообщает, что он сделал; ровно на это владелец и пожаловался.
          title: auto.value,
          subtitle: _autoSubtitle(inventory, auto),
          selected: selectedKey == null,
          // Слово берётся у подписи, а не пишется константой: при расхождении с
          // держателем замер свежий, и «устарело» отправляло бы перезамерять
          // там, где надо снять закрепление. Оба слова — в [AutoLabel.badge].
          titleBadges: [if (auto.badge.isNotEmpty) Tag(auto.badge)],
          // Замер строки НЕ блокирует: список показан сразу, числа доезжают
          // отдельно, и выбирать во время замера можно.
          onTap: () => onSelect(null),
        ),
        if (exits.isEmpty)
          for (final n in _sortedNodes(inventory.nodes))
            _NodeRow(
              node: n,
              selected: n.isAvailable && selectedKey == n.key,
              onTap: (n.isAvailable && blocked == null)
                  ? () => onSelect(n)
                  : null,
            )
        else
          ..._exitRows(
            _sortedExits(exits, inventory.nodes),
            inventory.nodes,
            selectedKey,
            blocked,
          ),
      ],
    );
  }

  /// Строки машин с уже разведёнными заголовками.
  ///
  /// Номер приписывается ЗДЕСЬ, а не в [machineTitleOf]: одинаковость видна
  /// только всему списку сразу, отдельная машина о своих тёзках не знает.
  List<Widget> _exitRows(
    List<ExitOffer> exits,
    List<ExitNode> nodes,
    String? selectedKey,
    AccessState? blocked,
  ) {
    final titles = disambiguateTitles(
      exits.map(machineTitleOf).toList(growable: false),
    );
    return <Widget>[
      for (var i = 0; i < exits.length; i++)
        _ExitRow(
          exit: exits[i],
          title: titles[i],
          selected: exitHoldsKey(exits[i], selectedKey),
          node: nodeForExit(exits[i], nodes),
          onSelect: onSelect,
          blockedBy: blocked,
        ),
    ];
  }

  /// Подпись «Авто». Закреплённая страна называется здесь, потому что своей
  /// строки у неё больше нет: иначе человек с прежним закреплением увидел бы
  /// список, где не выбрано ничего, и не понял бы, куда подключается.
  String _autoSubtitle(ExitInventory inventory, AutoLabel auto) {
    final cc = inventory.selectedCountry;
    final pinned = (cc == null || cc.isEmpty)
        ? null
        : (inventory.locationOf(cc)?.displayName ?? cc);

    // Что автоподбор УЖЕ выбрал — важнее объяснения, как он работает: первое
    // это факт, второе — инструкция. Узел, который держит ядро сейчас, здесь
    // намеренно НЕ называется: он и так виден галочкой на своей строке, а в
    // подписи контрола автоподбора он выдавал бы чужой выбор за его собственный.
    final chosen = auto.hasChoice ? auto.subtitle : null;

    if (pinned == null) {
      return chosen ?? 'Выберется при подключении';
    }
    final tail =
        'В пределах страны: $pinned. Нажмите, чтобы снять '
        'закрепление.';
    return chosen == null ? tail : _joinSentences(chosen, tail);
  }
}

/// Соединяет два предложения РОВНО одной точкой на стыке.
///
/// `chosen` — это [AutoLabel.subtitle], а он иногда УЖЕ законченное
/// предложение с точкой (устаревший выбор называется через
/// `AutoLabel._staleText`, который сам кончается на «…переподключении.»).
/// Слепое `'$chosen. $tail'` в этом случае удваивало точку; здесь точка
/// добавляется, только если её ещё нет.
String _joinSentences(String head, String tail) {
  final h = head.trimRight();
  return h.endsWith('.') ? '$h $tail' : '$h. $tail';
}

/// Порядок строк: доступные раньше недоступных, внутри — по задержке, затем по
/// имени.
///
/// В плоском списке порядок несёт больше, чем в списке по странам: он и есть
/// единственная подсказка, что выбирать. Узел без известной задержки уходит
/// ВНИЗ, а не наверх: «не мерили» это не «быстрее всех».
List<ExitNode> _sortedNodes(List<ExitNode> nodes) {
  final out = [...nodes];
  out.sort((a, b) {
    if (a.isAvailable != b.isAvailable) return a.isAvailable ? -1 : 1;
    final c = _rank(a.latency.ms).compareTo(_rank(b.latency.ms));
    return c != 0 ? c : a.name.compareTo(b.name);
  });
  return out;
}

List<ExitOffer> _sortedExits(List<ExitOffer> exits, List<ExitNode> nodes) {
  final out = [...exits];
  out.sort((a, b) {
    if (a.isAvailable != b.isAvailable) return a.isAvailable ? -1 : 1;
    final la = nodeForExit(a, nodes)?.latency.ms ?? a.pingMs;
    final lb = nodeForExit(b, nodes)?.latency.ms ?? b.pingMs;
    final c = _rank(la).compareTo(_rank(lb));
    return c != 0 ? c : machineTitleOf(a).compareTo(machineTitleOf(b));
  });
  return out;
}

/// Неизвестная и отрицательная (таймаут) задержка — в конец.
int _rank(int? ms) => (ms == null || ms < 0) ? 1 << 30 : ms;

/// Строка МАШИНЫ. Недоступная рисуется тем же приёмом, что и выключенный
/// вариант в [showPickerSheet]: приглушённая, с ПРИЧИНОЙ вместо подписи и без
/// цели для нажатия.
class _ExitRow extends StatelessWidget {
  final ExitOffer exit;

  /// Заголовок, уже разведённый с тёзками по списку.
  final String title;

  final bool selected;

  /// Узел, которым этот выход закрепляется; `null` — машина в списке выбора не
  /// представлена, и нажать не на что.
  final ExitNode? node;

  final void Function(ExitNode? node) onSelect;

  /// Отказ подписки: строка остаётся видимой, но не нажимается и несёт причину.
  final AccessState? blockedBy;

  const _ExitRow({
    required this.exit,
    required this.title,
    required this.selected,
    required this.node,
    required this.onSelect,
    this.blockedBy,
  });

  @override
  Widget build(BuildContext context) {
    final n = node;
    final off = !exit.isAvailable || n == null || blockedBy != null;
    // Задержку берём у УЗЛА, если машина в списке выбора представлена: там
    // лежит собственный замер вместе с именем автора. У предложения автора нет
    // — его `pingMs` пришёл с панели, и назвать его можно только операторским.
    final latency = n != null
        ? n.latency
        : (exit.pingMs == null
              ? Latency.none
              : Latency.fromOperator(exit.pingMs!));
    final live = exit.liveInbounds.length;
    final inboundsKnown = exit.inboundsKnown.isAvailable;

    return Opacity(
      opacity: off ? 0.45 : 1,
      child: ListItemCard(
        // Страна стоит НА СТРОКЕ, а не только в заголовке уровня: та же машина
        // упоминается в подписи под дайлом и в тостах, и без кода страны её
        // легко спутать с одноимённой в другой стране.
        leading: FlagChip(
          // Флаг решает УЗЕЛ: на импортированном пути страна — догадка по имени
          // прокси, и твёрдость этой догадки известна только там. Машины без
          // узла в списке выбора остаются с нейтральным глифом.
          flag: n?.flag ?? kNeutralFlag,
          code: exit.countryCode,
        ),
        title: title,
        subtitle: _subtitle(),
        selected: selected,
        titleBadges: [
          // Число инбаундов — то, чего экрану не хватало: без него восемь
          // прокси одной машины читались как восемь серверов.
          if (inboundsKnown)
            Tag('инбаундов: $live')
          else
            const Tag('инбаунды: ?'),
        ],
        onTap: off ? null : () => onSelect(n),
        trailing: LatencyReadout(latency),
      ),
    );
  }

  String? _subtitle() {
    // Причина подписки идёт ПЕРВОЙ: пока она в силе, всё остальное про эту
    // машину — правда, которая ничего не меняет.
    final blocked = blockedBy;
    if (blocked != null) return '${blocked.shortReason} · ${blocked.badge}';
    if (!exit.isAvailable) return exit.availability.message;
    if (node == null) {
      return 'Эта машина есть в предложении, но в списке выбора её нет: '
          'закрепить её нечем.';
    }
    final parts = <String>[];
    if (!exit.inboundsKnown.isAvailable) {
      parts.add(exit.inboundsKnown.message);
    } else {
      final families = <String>[];
      for (final i in exit.liveInbounds) {
        if (!families.contains(i.key.protocol)) families.add(i.key.protocol);
      }
      if (families.isNotEmpty) parts.add(families.join(', '));
      final dead = exit.inbounds.length - exit.liveInbounds.length;
      if (dead > 0) parts.add('ещё $dead не доезжает до конфига');
    }
    final load = exit.loadPct;
    if (load != null && load > 0) parts.add('загрузка ${load.round()}%');
    return parts.isEmpty ? null : parts.join(' · ');
  }
}

/// Строка узла инвентаря — путь для источника, который предложение ещё не
/// ведёт. Недоступный узел рисуется тем же приёмом: приглушённый, с ПРИЧИНОЙ
/// вместо подписи и без цели для нажатия.
class _NodeRow extends StatelessWidget {
  final ExitNode node;
  final bool selected;
  final VoidCallback? onTap;

  const _NodeRow({required this.node, required this.selected, this.onTap});

  @override
  Widget build(BuildContext context) {
    final off = !node.isAvailable;

    return Opacity(
      opacity: off ? 0.45 : 1,
      child: ListItemCard(
        leading: FlagChip(flag: node.flag, code: node.countryCode),
        title: node.name.isEmpty ? node.key : node.name,
        subtitle: off ? exitUnavailableText(node.availability) : _subtitle(),
        selected: selected,
        // Тип outbound'а показывается там, где источник его знает: у
        // импортированной подписки и у каталога он есть, у панельного
        // `GET /servers` его нет вовсе, и выдумывать его нечем.
        titleBadges: [if (node.protocol.isNotEmpty) Tag(node.protocol)],
        onTap: onTap,
        trailing: LatencyReadout(node.latency),
      ),
    );
  }

  String? _subtitle() {
    if (node.load > 0) return 'Загрузка ${node.load.round()}%';
    return null;
  }
}

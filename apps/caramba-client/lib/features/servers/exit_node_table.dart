import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/subscription.dart' show AccessState;
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/servers/access_card.dart';
import 'package:caramba_client/features/servers/exit_node_list.dart';
import 'package:caramba_client/features/servers/fleet_alignment.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Тот же флот, что в [ExitNodeList], но ТАБЛИЦЕЙ — для десктопа.
///
/// Почему не тот же виджет с другой плотностью. На телефоне строка — карточка:
/// у неё один столбец текста, и всё, что не влезло, уходит в подпись через
/// «·». На экране 1120 такая карточка тратит две трети ширины на воздух, а
/// сравнивать машины между собой (а этим на экране серверов и занимаются)
/// приходится по-прежнему построчно. Таблица ставит одинаковые величины в одну
/// колонку: задержки сравниваются взглядом сверху вниз, а не чтением.
///
/// Что общее с мобильным списком и почему это важно. ИСТОЧНИКИ те же
/// ([exitInventoryProvider], [offeringProvider], [fleetSourcesAgree],
/// [autoServerLabelProvider]) и ПОРЯДОК тот же ([sortedExits] / [sortedNodes]).
/// Порядок строк здесь — единственная подсказка, что выбирать; разойдись он с
/// мобильным, один и тот же флот читался бы на телефоне и на Маке по-разному, и
/// спорить было бы не с чем.
///
/// Что добавилось: ДВОЙНОЙ клик. На десктопе одиночный клик по строке — это
/// «выделил», а не «сделал»; поведение «выбрать и сразу подключиться» человек
/// ждёт от двойного, и отдельной кнопки под это заводить не нужно.
class ExitNodeTable extends ConsumerWidget {
  /// Выбран узел; `null` — строка «Авто» (пин снят, узел выбирает ядро).
  final void Function(ExitNode? node) onSelect;

  /// Двойной клик по строке: выбрать И применить (подключиться).
  final void Function(ExitNode node) onActivate;

  const ExitNodeTable({
    required this.onSelect,
    required this.onActivate,
    super.key,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final inventory = ref.watch(exitInventoryProvider);
    final offering = ref.watch(offeringProvider);
    final selectedKey = inventory.selectedNodeKey;
    // Отказ подписки накрывает ВСЕ строки разом: они описывают один и тот же
    // флот, и оставить их нажимаемыми значило бы обещать подключение, которого
    // не будет.
    final blocked = inventory.blockedBy;

    // Половины флота обязаны описывать ОДИН источник — иначе таблица показала
    // бы машины предложения со списком выбора инвентаря, и клик уходил бы не
    // туда. При расхождении строкой остаётся узел инвентаря, как и в списке.
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
        // «Авто» остаётся КАРТОЧКОЙ, а не первой строкой таблицы: это не
        // машина, у неё нет ни задержки, ни загрузки, и колонки таблицы про неё
        // врали бы прочерками. Подпись — та же, что на телефоне.
        ListItemCard(
          leading: const IBox(Lucide.gauge),
          title: auto.value,
          subtitle: _autoSubtitle(inventory, auto),
          selected: selectedKey == null,
          titleBadges: [if (auto.badge.isNotEmpty) Tag(auto.badge)],
          onTap: () => onSelect(null),
        ),
        const SizedBox(height: AppSpace.s4),
        const _TableHead(),
        if (exits.isEmpty)
          for (final n in sortedNodes(inventory.nodes))
            _NodeTableRow(
              node: n,
              selected: n.isAvailable && selectedKey == n.key,
              enabled: n.isAvailable && blocked == null,
              blockedBy: blocked,
              onSelect: onSelect,
              onActivate: onActivate,
            )
        else
          ..._exitRows(
            sortedExits(exits, inventory.nodes),
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
        _ExitTableRow(
          exit: exits[i],
          title: titles[i],
          selected: exitHoldsKey(exits[i], selectedKey),
          node: nodeForExit(exits[i], nodes),
          blockedBy: blocked,
          onSelect: onSelect,
          onActivate: onActivate,
        ),
    ];
  }
}

/// Подпись «Авто» — копия [ExitNodeList]. Копия, а не общая функция, потому что
/// в списке она приватный метод виджета, а вскрывать его наружу ради одной
/// строки значило бы расширить публичную поверхность мобильного экрана в тот
/// момент, когда его правки запрещены.
String _autoSubtitle(ExitInventory inventory, AutoLabel auto) {
  final cc = inventory.selectedCountry;
  final pinned = (cc == null || cc.isEmpty)
      ? null
      : (inventory.locationOf(cc)?.displayName ?? cc);
  final chosen = auto.hasChoice ? auto.subtitle : null;

  if (pinned == null) {
    return chosen ?? 'Выберется при подключении';
  }
  final tail = 'В пределах страны: $pinned. Нажмите, чтобы снять '
      'закрепление.';
  if (chosen == null) return tail;
  final head = chosen.trimRight();
  // Одна точка на стыке: `AutoLabel.subtitle` для устаревшего выбора уже
  // законченное предложение, и слепая склейка удваивала точку.
  return head.endsWith('.') ? '$head $tail' : '$head. $tail';
}

// ─── геометрия таблицы ─────────────────────────────────────────────────────
//
// Ширины лежат ЗДЕСЬ, одним набором на шапку и на строки: держи их в двух
// местах — и колонки однажды разъедутся, а таблица без выровненных колонок
// хуже списка, потому что обещает сравнение, которого не даёт.

/// Колонка страны. В карте окна стояло 56 под голый [CodeChip]; здесь стоит
/// [FlagChip] — тот же виджет, что в строке мобильного списка, — и флаг рядом с
/// кодом в 56 не помещается. Терять флаг ради числа нельзя: его показывать
/// просил владелец, и строка без него теряет то, по чему страну узнают быстрее
/// всего.
const double _wCode = 64;
const double _wType = 112;
const double _wLatency = 88;
const double _wLoad = 80;
const double _wMark = 32;
const double _gap = AppSpace.s3;
const double _padH = AppSpace.s3;

/// Полоса выделения слева. У невыделенной строки на её месте стоит пустота той
/// же ширины — иначе выделение сдвигало бы весь текст строки на 3 px.
const double _stripe = 3;

/// Ячейки строки: собираются один раз и раскладываются одинаково у шапки и у
/// строк.
class _Cells extends StatelessWidget {
  /// Полоса выделения (или пустота той же ширины).
  final Widget stripe;
  final Widget code;
  final Widget machine;
  final Widget type;
  final Widget latency;
  final Widget load;
  final Widget mark;

  const _Cells({
    required this.stripe,
    required this.code,
    required this.machine,
    required this.type,
    required this.latency,
    required this.load,
    required this.mark,
  });

  @override
  Widget build(BuildContext context) => Row(
        children: [
          stripe,
          const SizedBox(width: _padH),
          SizedBox(width: _wCode, child: code),
          const SizedBox(width: _gap),
          Expanded(child: machine),
          const SizedBox(width: _gap),
          SizedBox(width: _wType, child: type),
          const SizedBox(width: _gap),
          SizedBox(width: _wLatency, child: latency),
          const SizedBox(width: _gap),
          SizedBox(width: _wLoad, child: load),
          const SizedBox(width: _gap),
          SizedBox(width: _wMark, child: mark),
          const SizedBox(width: _padH),
        ],
      );
}

/// Шапка колонок. Она не сортирует: порядок строк здесь смысловой (доступные
/// раньше недоступных, внутри — по задержке), и отдать его щелчку по «ЗАГРУЗКЕ»
/// значило бы позволить человеку спрятать недоступные машины в конец списка,
/// решив, что их нет.
class _TableHead extends StatelessWidget {
  const _TableHead();

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final style = AppType.caption.copyWith(color: c.textLow);
    Widget label(String t, {TextAlign align = TextAlign.left}) =>
        Text(t.toUpperCase(), style: style, textAlign: align);

    return Container(
      padding: const EdgeInsets.only(bottom: AppSpace.s2),
      decoration: BoxDecoration(
        border: Border(
          bottom: BorderSide(color: c.borderSubtle, width: AppBorders.hairline),
        ),
      ),
      child: _Cells(
        stripe: const SizedBox(width: _stripe),
        code: label('Код'),
        machine: label('Машина'),
        type: label('Тип'),
        latency: label('Задержка', align: TextAlign.right),
        load: label('Загрузка', align: TextAlign.right),
        mark: const SizedBox.shrink(),
      ),
    );
  }
}

/// Общий каркас строки: наведение, выделение, приглушение, подсказка и оба
/// клика. Различаются только ЗНАЧЕНИЯ ячеек, и держать эту механику в двух
/// местах (машины предложения и узлы инвентаря) незачем.
class _TableRow extends StatefulWidget {
  final String flag;
  final String code;
  final String title;

  /// Бейдж у имени машины (число инбаундов, тип узла).
  final String? badge;

  /// Причина недоступности: вторая строка в колонке «Машина» И подсказка.
  /// Строка, у которой отняли нажатие и не сказали почему, читается как
  /// поломка приложения.
  final String? reason;

  /// Колонка «Тип»: семейства протоколов машины.
  final String type;

  final Latency latency;

  /// Готовая строка загрузки («12%» или прочерк).
  final String load;

  final bool selected;

  /// Строка приглушена и не нажимается.
  final bool off;

  final VoidCallback? onTap;
  final VoidCallback? onActivate;

  const _TableRow({
    required this.flag,
    required this.code,
    required this.title,
    required this.type,
    required this.latency,
    required this.load,
    required this.selected,
    required this.off,
    this.badge,
    this.reason,
    this.onTap,
    this.onActivate,
  });

  @override
  State<_TableRow> createState() => _TableRowState();
}

class _TableRowState extends State<_TableRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final tappable = widget.onTap != null;
    // Наведение и выделение красятся одной поверхностью: выделение отличает
    // полоса слева и галочка справа, а не второй оттенок — оттенков на этой
    // теме ровно столько, сколько состояний, и пятый выдумывать нечем.
    final bg = (widget.selected || (_hover && tappable)) ? c.surface2 : null;
    final reason = widget.reason;

    Widget row = Container(
      constraints: const BoxConstraints(
        minHeight: DesktopTokens.tableRowHeight,
      ),
      decoration: BoxDecoration(
        color: bg,
        border: Border(
          bottom: BorderSide(color: c.borderSubtle, width: AppBorders.hairline),
        ),
      ),
      child: _Cells(
        stripe: widget.selected
            ? Container(
                width: _stripe,
                height: DesktopTokens.tableRowHeight - 16,
                decoration: BoxDecoration(
                  color: c.textHi,
                  borderRadius: BorderRadius.circular(_stripe),
                ),
              )
            : const SizedBox(width: _stripe),
        code: FlagChip(flag: widget.flag, code: widget.code),
        machine: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Tooltip(
              message: widget.title,
              child: Text(
                widget.title,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: AppType.bodyMd.copyWith(color: c.textHi),
              ),
            ),
            if (widget.badge != null) ...[
              const SizedBox(height: 2),
              Text(
                widget.badge!,
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),
            ],
            if (reason != null) ...[
              const SizedBox(height: 2),
              Text(
                reason,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),
            ],
          ],
        ),
        type: Text(
          widget.type,
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          style: AppType.bodySm.copyWith(color: c.textMed),
        ),
        // Автор числа уже внутри [LatencyReadout] («ваш пинг» / «от
        // оператора»): второй подписью он бы задвоился, а разными словами про
        // одно и то же число — ещё и запутал.
        latency: Align(
          alignment: Alignment.centerRight,
          child: LatencyReadout(widget.latency),
        ),
        load: Text(
          widget.load,
          textAlign: TextAlign.right,
          style: AppType.monoMd.copyWith(color: c.textMed),
        ),
        mark: widget.selected
            ? LucideIcon(Lucide.check, color: c.textHi, size: 18)
            : const SizedBox.shrink(),
      ),
    );

    if (widget.off) {
      row = Opacity(opacity: 0.45, child: row);
      if (reason != null) {
        row = Tooltip(message: reason, child: row);
      }
    }

    row = MouseRegion(
      cursor: tappable ? SystemMouseCursors.click : MouseCursor.defer,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: row,
    );

    if (!tappable) return row;
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: widget.onTap,
      onDoubleTap: widget.onActivate,
      child: row,
    );
  }
}

/// Строка МАШИНЫ предложения.
class _ExitTableRow extends StatelessWidget {
  final ExitOffer exit;

  /// Заголовок, уже разведённый с тёзками по списку.
  final String title;

  final bool selected;

  /// Узел, которым этот выход закрепляется; `null` — машина в списке выбора не
  /// представлена, и нажать не на что.
  final ExitNode? node;

  /// Отказ подписки: строка остаётся видимой, но не нажимается и несёт причину.
  final AccessState? blockedBy;

  final void Function(ExitNode? node) onSelect;
  final void Function(ExitNode node) onActivate;

  const _ExitTableRow({
    required this.exit,
    required this.title,
    required this.selected,
    required this.node,
    required this.onSelect,
    required this.onActivate,
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
    final load = exit.loadPct;

    return _TableRow(
      // Флаг решает УЗЕЛ: на импортированном пути страна — догадка по имени
      // прокси, и твёрдость этой догадки известна только там.
      flag: n?.flag ?? kNeutralFlag,
      code: exit.countryCode,
      title: title,
      // Число инбаундов — то, чего экрану не хватало: без него восемь прокси
      // одной машины читались как восемь серверов.
      badge: inboundsKnown ? 'инбаундов: $live' : 'инбаунды: ?',
      reason: _reason(),
      type: _type(),
      latency: latency,
      load: (load != null && load > 0) ? '${load.round()}%' : '-',
      selected: selected,
      off: off,
      onTap: off ? null : () => onSelect(n),
      onActivate: off ? null : () => onActivate(n),
    );
  }

  /// Почему строка не нажимается. Причина подписки идёт ПЕРВОЙ: пока она в
  /// силе, всё остальное про эту машину — правда, которая ничего не меняет.
  String? _reason() {
    final blocked = blockedBy;
    if (blocked != null) return '${blocked.shortReason} · ${blocked.badge}';
    if (!exit.isAvailable) return exit.availability.message;
    if (node == null) {
      return 'Есть в предложении, но в списке выбора её нет: закрепить нечем.';
    }
    return null;
  }

  /// Колонка «Тип»: чем машина умеет выходить.
  String _type() {
    if (!exit.inboundsKnown.isAvailable) return exit.inboundsKnown.message;
    final families = <String>[];
    for (final i in exit.liveInbounds) {
      if (!families.contains(i.key.protocol)) families.add(i.key.protocol);
    }
    final parts = <String>[if (families.isNotEmpty) families.join(', ')];
    // Мёртвые инбаунды называются числом, а не молчанием: разница между «у
    // машины два протокола» и «у машины два из восьми доезжают» — это разница
    // между исправным флотом и наполовину сломанным.
    final dead = exit.inbounds.length - exit.liveInbounds.length;
    if (dead > 0) parts.add('ещё $dead не доезжает до конфига');
    return parts.join(' · ');
  }
}

/// Строка узла инвентаря — путь для источника, который предложение ещё не
/// ведёт.
class _NodeTableRow extends StatelessWidget {
  final ExitNode node;
  final bool selected;

  /// Узел доступен И подписка не закрыта.
  final bool enabled;

  final AccessState? blockedBy;
  final void Function(ExitNode? node) onSelect;
  final void Function(ExitNode node) onActivate;

  const _NodeTableRow({
    required this.node,
    required this.selected,
    required this.enabled,
    required this.onSelect,
    required this.onActivate,
    this.blockedBy,
  });

  @override
  Widget build(BuildContext context) {
    final blocked = blockedBy;
    final off = !enabled;
    return _TableRow(
      flag: node.flag,
      code: node.countryCode,
      title: node.name.isEmpty ? node.key : node.name,
      reason: blocked != null
          ? '${blocked.shortReason} · ${blocked.badge}'
          : (node.isAvailable ? null : exitUnavailableText(node.availability)),
      // Тип узла едет в СВОЮ колонку, а не бейджем к имени, как на телефоне:
      // колонка для того и заведена, и в ней он сравнивается по всему списку
      // сверху вниз.
      type: node.protocol,
      latency: node.latency,
      load: node.load > 0 ? '${node.load.round()}%' : '-',
      selected: selected,
      off: off,
      onTap: off ? null : () => onSelect(node),
      onActivate: off ? null : () => onActivate(node),
    );
  }
}

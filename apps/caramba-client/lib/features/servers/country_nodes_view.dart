import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/servers/fleet_alignment.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Второй уровень выбора выхода: МАШИНЫ одной страны.
///
/// Здесь и была та самая жалоба: «восемь серверов» на экране оказывались
/// восемью инбаундами одной машины. В теле подписки узла не существует как
/// сущности — есть только прокси, — и экран честно перечислял прокси, называя
/// их серверами. Поэтому строка теперь берётся из [Offering], где машина уже
/// восстановлена (по одинаковому `server:` в импорте, по `nodes.id` у панели),
/// а её инбаунды лежат внутри неё и пересчитаны.
///
/// Выбор при этом по-прежнему уходит через [ExitSelectionController], которому
/// нужен [ExitNode]: у панели это `node_id`, у импорта — имя прокси, которое
/// читает `connectRaw`. Машина разрешается в узел здесь, а не в контроллере,
/// потому что связь «машина → её прокси» знает только предложение.
///
/// Виджет ничего не выбирает сам: он зовёт [onSelect], а решение — что делать с
/// выбором и как показать результат синхронизации с панелью — принимает экран,
/// который его встроил. Иначе политика выбора жила бы в двух местах.
class CountryNodesView extends ConsumerWidget {
  final ExitLocation location;

  /// Вернуться к списку стран.
  final VoidCallback onBack;

  /// Выбран узел; `null` — строка «Авто» (пин снят, узел выбирает ядро).
  final void Function(ExitNode? node) onSelect;

  /// Идёт замер задержек: строки на это время не нажимаются.
  final bool probing;

  const CountryNodesView({
    required this.location,
    required this.onBack,
    required this.onSelect,
    this.probing = false,
    super.key,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final inventory = ref.watch(exitInventoryProvider);
    final nodes = ref.watch(exitNodesInCountryProvider(location.countryCode));
    final offering = ref.watch(offeringProvider);
    final selectedKey = inventory.selectedNodeKey;

    // Предложение описывает ЭТОТ же источник — только тогда его машинам можно
    // верить. Пока каталог CSM ведёт инвентарь, а предложение ещё нет, обе
    // половины разошлись бы, и экран показал бы чужие машины; в этом случае
    // строкой остаётся узел инвентаря, как и раньше.
    final exits = fleetSourcesAgree(inventory.source, offering.source)
        ? offering.exitsIn(location.countryCode)
        : const <ExitOffer>[];

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Align(
          alignment: Alignment.centerLeft,
          child: GhostButton(
            label: 'Все страны',
            icon: Lucide.arrowLeft,
            onPressed: onBack,
          ),
        ),
        const SizedBox(height: AppSpace.s4),
        SectionTitle(
          location.displayName,
          padding: const EdgeInsets.only(bottom: AppSpace.s3),
          trailing: Text(
            'узлов: ${exits.isNotEmpty ? exits.length : location.nodeCount}',
            style: AppType.bodySm.copyWith(color: c.textLow),
          ),
        ),
        if (!location.isAvailable) ...[
          InlineBanner(
            tone: BannerTone.warning,
            glyph: Lucide.alert,
            text: location.availability.message,
          ),
          const SizedBox(height: AppSpace.s4),
        ],
        if (nodes.isEmpty)
          const InlineEmpty(message: 'В этой стране узлов нет')
        else ...[
          ListItemCard(
            leading: const IBox(Lucide.gauge),
            title: 'Авто',
            subtitle: 'Ядро выберет узел в этой стране само',
            selected: selectedKey == null,
            onTap: probing ? null : () => onSelect(null),
          ),
          if (exits.isEmpty)
            for (final n in nodes)
              _NodeRow(
                node: n,
                selected: n.isAvailable && selectedKey == n.key,
                onTap: (probing || !n.isAvailable) ? null : () => onSelect(n),
              )
          else
            for (final e in exits)
              _ExitRow(
                exit: e,
                selected: exitHoldsKey(e, selectedKey),
                node: nodeForExit(e, nodes),
                probing: probing,
                onSelect: onSelect,
              ),
        ],
      ],
    );
  }
}

/// Строка МАШИНЫ. Недоступная рисуется тем же приёмом, что и выключенный
/// вариант в [showPickerSheet]: приглушённая, с ПРИЧИНОЙ вместо подписи и без
/// цели для нажатия.
class _ExitRow extends StatelessWidget {
  final ExitOffer exit;
  final bool selected;

  /// Узел, которым этот выход закрепляется; `null` — машина в списке выбора не
  /// представлена, и нажать не на что.
  final ExitNode? node;

  final bool probing;
  final void Function(ExitNode? node) onSelect;

  const _ExitRow({
    required this.exit,
    required this.selected,
    required this.node,
    required this.probing,
    required this.onSelect,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final n = node;
    final off = !exit.isAvailable || n == null;
    final ping = exit.pingMs;
    final bucket = ping == null ? null : pingBucketOf(ping);
    final color = switch (bucket) {
      PingBucket.good => c.success,
      PingBucket.fair => c.warning,
      PingBucket.poor => c.danger,
      PingBucket.timeout => c.danger,
      null => c.textLow,
    };
    final live = exit.liveInbounds.length;
    final inboundsKnown = exit.inboundsKnown.isAvailable;

    return Opacity(
      opacity: off ? 0.45 : 1,
      child: ListItemCard(
        // Страна стоит НА СТРОКЕ, а не только в заголовке уровня: та же машина
        // упоминается в подписи под дайлом и в тостах, и без кода страны её
        // легко спутать с одноимённой в другой стране.
        leading: exit.countryCode.isEmpty
            ? const IBox(Lucide.globe)
            : CodeChip(exit.countryCode),
        title: machineTitleOf(exit),
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
        onTap: (probing || off) ? null : () => onSelect(n),
        trailing: bucket == PingBucket.timeout
            ? Container(
                width: 8,
                height: 8,
                decoration: BoxDecoration(color: color, shape: BoxShape.circle),
              )
            : Text(
                ping == null ? '-' : '$ping мс',
                style: AppType.monoSm.copyWith(color: color),
              ),
      ),
    );
  }

  String? _subtitle() {
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
    final c = context.c;
    final off = !node.isAvailable;
    // `null` в pingMs это «не мерили», и оно не то же самое, что таймаут:
    // прочерк честнее нуля и честнее красной точки.
    final bucket = node.pingMs == null ? null : node.pingBucket;
    final color = switch (bucket) {
      PingBucket.good => c.success,
      PingBucket.fair => c.warning,
      PingBucket.poor => c.danger,
      PingBucket.timeout => c.danger,
      null => c.textLow,
    };

    return Opacity(
      opacity: off ? 0.45 : 1,
      child: ListItemCard(
        leading: const IBox(Lucide.globe),
        title: node.name.isEmpty ? node.key : node.name,
        subtitle: off ? node.availability.message : _subtitle(),
        selected: selected,
        // Тип outbound'а показывается там, где источник его знает: у
        // импортированной подписки и у каталога он есть, у панельного
        // `GET /servers` его нет вовсе, и выдумывать его нечем.
        titleBadges: [if (node.protocol.isNotEmpty) Tag(node.protocol)],
        onTap: onTap,
        trailing: bucket == PingBucket.timeout
            // Таймаут — точка цвета danger: числа тут нет, а «-1 мс» врал бы.
            ? Container(
                width: 8,
                height: 8,
                decoration: BoxDecoration(color: color, shape: BoxShape.circle),
              )
            : Text(
                node.pingMs == null ? '-' : '${node.pingMs} мс',
                style: AppType.monoSm.copyWith(color: color),
              ),
      ),
    );
  }

  String? _subtitle() {
    if (node.load > 0) return 'Загрузка ${node.load.round()}%';
    return null;
  }
}

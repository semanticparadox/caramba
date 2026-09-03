import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Второй уровень выбора выхода: узлы ОДНОЙ страны.
///
/// Вынесен из `servers_screen.dart` не ради размера файла. Экран серверов уже
/// ветвился по режиму на две почти одинаковые страницы; страна сверху делает
/// таких веток три, и третья неминуемо разошлась бы с двумя первыми. Здесь
/// узлы приходят из [exitNodesInCountryProvider] в общем виде, поэтому вид
/// один и для панели, и для импортированной подписки.
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
    final nodes = ref.watch(exitNodesInCountryProvider(location.countryCode));
    final selectedKey = ref.watch(exitInventoryProvider).selectedNodeKey;

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
            'узлов: ${location.nodeCount}',
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
          for (final n in nodes)
            _NodeRow(
              node: n,
              selected: n.isAvailable && selectedKey == n.key,
              onTap: (probing || !n.isAvailable) ? null : () => onSelect(n),
            ),
        ],
      ],
    );
  }
}

/// Строка узла. Недоступный узел рисуется тем же приёмом, что и выключенный
/// вариант в [showPickerSheet]: приглушённый, с ПРИЧИНОЙ вместо подписи и без
/// цели для нажатия.
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

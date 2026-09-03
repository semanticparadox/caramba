import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/protocol_inventory_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Протокол (демо §PROTOCOL): список протоколов с тегами «рекоменд.»/«умный»,
/// включая «Авто». Выбор пишет в [coreConfigProvider] -> Policy.Protocol.
///
/// Список сверяется с живым инвентарём ([protocolInventoryProvider]): протокол,
/// которого текущий источник не раздаёт, остаётся строкой — видимой, выключенной
/// и с причиной. Пока экран строился прямо по [ProtocolOption.defaults], выбор
/// такого протокола применялся «успешно», ядро не находило подходящего
/// outbound'а и молча оставалось на прежнем.
class ProtocolScreen extends ConsumerWidget {
  const ProtocolScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final inventory = ref.watch(protocolInventoryProvider);
    final cfg = ref.watch(coreConfigProvider);
    // Происхождение значения по CSM: оператор мог поставить протокол сам, и
    // пользователь вправе видеть это до того, как перевыберет (02-SPEC.md 7.6).
    final entry = ref.watch(csmSettingsProvider)[CsmSettingKey.protocol];
    // Текущий выбор мог перестать раздаваться после смены подписки или плана.
    // Молчать об этом нельзя: строка перестаёт быть выбранной, и без баннера
    // экран выглядел бы так, будто протокол не выбирали вовсе.
    final current = inventory.byIndex(cfg.protocol);
    final currentGone = current != null && !current.isAvailable;

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: ListView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s12,
          ),
          children: [
            ScreenHead(
              'Протокол',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            Text(
              'Способ маскировки трафика. «Авто» переключает протокол сам, если текущий перестаёт проходить.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            if (entry != null) ...[
              const SizedBox(height: AppSpace.s3),
              InlineBanner(
                tone: entry.userSet
                    ? BannerTone.info
                    : (entry.src == CsmProvenance.operator
                          ? BannerTone.warning
                          : BannerTone.info),
                glyph: Lucide.shield,
                text: entry.userSet
                    ? 'Протокол выбрали вы. Оператор не перезапишет его молча: '
                          'на попытку поднимется карточка с вопросом.'
                    : 'Текущее значение поставил '
                          '${csmProvenanceTitle(entry.src)}. Выбрав своё, вы '
                          'закрепите его за собой.',
              ),
            ],
            if (currentGone) ...[
              const SizedBox(height: AppSpace.s3),
              InlineBanner(
                tone: BannerTone.warning,
                glyph: Lucide.alert,
                text:
                    'Выбранный протокол «${current.name}» сейчас недоступен. '
                    '${current.availability.message} Ядро подключится по '
                    'другому, пока вы не выберете доступный.',
              ),
            ],
            if (inventory.unverified != null) ...[
              const SizedBox(height: AppSpace.s3),
              InlineBanner(
                glyph: Lucide.globe,
                text:
                    '${inventory.unverified!.message} Список показан целиком, '
                    'но какие протоколы раздаются на самом деле, отсюда не '
                    'видно.',
              ),
            ],
            const SizedBox(height: AppSpace.s4),
            for (final choice in inventory.choices)
              _ProtocolRow(
                choice: choice,
                selected: choice.isAvailable && choice.index == cfg.protocol,
                onTap: choice.isAvailable && choice.isRequestable
                    ? () => _apply(context, ref, choice)
                    : null,
              ),
          ],
        ),
      ),
    );
  }

  void _apply(BuildContext context, WidgetRef ref, ProtocolChoice choice) {
    // Правка уходит и ядру (следующий `Up`), и оператору (очередь записи).
    // Туннель не рвётся: поднимется баннер.
    CsmSettingsBridge.setProtocol(ref, choice.index);
    showCarambaToast(context, 'Протокол: ${choice.name}');
    Future.delayed(const Duration(milliseconds: 300), () {
      if (context.mounted) _close(context);
    });
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go('/home');
    }
  }
}

/// Строка протокола. Недоступная рисуется тем же приёмом, что и выключенный
/// вариант в [showPickerSheet]: приглушённая, с ПРИЧИНОЙ вместо описания и без
/// цели для нажатия. Спрятать её нельзя — пропавший протокол пользователь ищет
/// в обновлении приложения, которого ему не нужно.
class _ProtocolRow extends StatelessWidget {
  final ProtocolChoice choice;
  final bool selected;
  final VoidCallback? onTap;

  const _ProtocolRow({
    required this.choice,
    required this.selected,
    this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final off = !choice.isAvailable;
    return Opacity(
      opacity: off ? 0.45 : 1,
      child: ListItemCard(
        leading: IBox(choice.icon),
        title: choice.name,
        subtitle: off ? choice.availability.message : _subtitle(),
        selected: selected,
        titleBadges: [
          if (choice.recommended) const Tag('рекоменд.'),
          if (choice.auto) const Tag('умный', ok: true),
        ],
        onTap: onTap,
      ),
    );
  }

  /// Под доступной строкой стоит её описание плюс то, что о ней знает источник:
  /// в каких формах он её раздаёт и на скольких узлах. Формы идут ТЕКСТОМ, а не
  /// плашками: их у vless бывает пять сразу, и пять плашек вытеснили бы из
  /// строки её собственное имя.
  String _subtitle() {
    final parts = <String>[];
    if (choice.desc.isNotEmpty) parts.add(choice.desc);
    if (choice.shapes.isNotEmpty) {
      parts.add('Формы: ${choice.shapes.join(', ')}.');
    }
    if (choice.nodeCount > 0) parts.add('Узлов: ${choice.nodeCount}.');
    return parts.join(' ');
  }
}

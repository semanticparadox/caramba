import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Relay (вход): «Выкл» / «Авто» / страна входа в цепочку.
///
/// Раньше это был нижний лист с Home, и в generic-режиме его там просто не
/// было: цепочку на сыром конфиге собрать нечем, поэтому строку прятали. Прятать
/// нельзя. Спрятанный переключатель выглядит одинаково при «оператор не выдал
/// бит», «панель не настроена» и «эта подписка так не умеет» — и все три случая
/// пользователь читает как поломку приложения.
///
/// Поэтому здесь ВСЕ строки видны всегда, а недоступность приходит из
/// [relayAvailabilityProvider] вместе с машинной причиной. Своей формулировки у
/// экрана нет намеренно: правило живёт в ядре (`Capabilities`, запись
/// `relay_chaining`), и вторая его копия в Dart рано или поздно начнёт врать.
class RelayScreen extends ConsumerWidget {
  const RelayScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final relays = ref.watch(relaysProvider);
    final cfg = ref.watch(coreConfigProvider);
    final availability = ref.watch(relayAvailabilityProvider);
    // Происхождение значения по CSM: вход мог поставить оператор (02-SPEC.md
    // 7.6), и пользователь вправе видеть это до того, как перевыберет.
    final entry = ref.watch(csmSettingsProvider)[CsmSettingKey.relay];
    // Индекс мог быть выбран на дефолтном списке, а панельный список короче.
    final selected = relays.isEmpty ? 0 : cfg.relay.clamp(0, relays.length - 1);

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
              'Relay (вход)',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            Text(
              'Через какую страну идёт вход в цепочку. Удобно в России: вход через устойчивую страну, выход где нужно.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            if (!availability.isAvailable) ...[
              const SizedBox(height: AppSpace.s3),
              InlineBanner(
                tone: BannerTone.warning,
                glyph: Lucide.waypoints,
                text: availability.message,
              ),
            ],
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
                    ? 'Вход выбрали вы. Оператор не перезапишет его молча: '
                          'на попытку поднимется карточка с вопросом.'
                    : 'Текущее значение поставил '
                          '${csmProvenanceTitle(entry.src)}. Выбрав своё, вы '
                          'закрепите его за собой.',
              ),
            ],
            const SizedBox(height: AppSpace.s4),
            if (relays.isEmpty)
              const InlineEmpty(message: 'Оператор не отдал ни одного входа')
            else
              for (var i = 0; i < relays.length; i++)
                _RelayRow(
                  relay: relays[i],
                  selected: availability.isAvailable && i == selected,
                  onTap: availability.isAvailable
                      ? () => _apply(context, ref, i, relays)
                      : null,
                  reason: availability.isAvailable
                      ? null
                      : availability.message,
                ),
          ],
        ),
      ),
    );
  }

  void _apply(
    BuildContext context,
    WidgetRef ref,
    int index,
    List<Relay> relays,
  ) {
    // Вход уходит ядру через `CorePolicy.relay` (`?relay_country=` в запросе
    // конфига у панели) и оператору через очередь записи CSM. Панельного
    // закрепления на подписке здесь нет намеренно: пока `PUT
    // /subscriptions/{id}/selection` не задеплоен, вызов дал бы отказ на каждое
    // нажатие, а действующий путь уже работает.
    CsmSettingsBridge.setRelay(ref, index, relays);
    showCarambaToast(context, 'Relay: ${relays[index].name}');
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

/// Строка входа. Недоступная рисуется тем же приёмом, что и выключенный вариант
/// в [showPickerSheet]: приглушённая, с ПРИЧИНОЙ вместо описания и без цели для
/// нажатия.
class _RelayRow extends StatelessWidget {
  final Relay relay;
  final bool selected;
  final VoidCallback? onTap;

  /// Причина недоступности; `null` — строка доступна.
  final String? reason;

  const _RelayRow({
    required this.relay,
    required this.selected,
    this.onTap,
    this.reason,
  });

  @override
  Widget build(BuildContext context) {
    final code = relay.country;
    return Opacity(
      opacity: reason == null ? 1 : 0.45,
      child: ListItemCard(
        leading: code != null && code.isNotEmpty
            ? CodeChip(code)
            : IBox(relay.isOff ? Lucide.route : Lucide.gauge),
        title: relay.name,
        subtitle: reason ?? relay.desc,
        selected: selected,
        titleBadges: [if (relay.isAuto) const Tag('умный', ok: true)],
        onTap: onTap,
      ),
    );
  }
}

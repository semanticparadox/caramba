import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Протокол (демо §PROTOCOL): список протоколов с тегами «рекоменд.»/«умный»,
/// включая «Авто». Выбор пишет в [coreConfigProvider] -> Policy.Protocol.
class ProtocolScreen extends ConsumerWidget {
  const ProtocolScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final protocols = ref.watch(protocolsProvider);
    final cfg = ref.watch(coreConfigProvider);
    // Происхождение значения по CSM: оператор мог поставить протокол сам, и
    // пользователь вправе видеть это до того, как перевыберет (02-SPEC.md 7.6).
    final entry = ref.watch(csmSettingsProvider)[CsmSettingKey.protocol];

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
            const SizedBox(height: AppSpace.s4),
            for (var i = 0; i < protocols.length; i++)
              ListItemCard(
                leading: IBox(protocols[i].icon),
                title: protocols[i].name,
                subtitle: protocols[i].desc,
                selected: i == cfg.protocol,
                titleBadges: [
                  if (protocols[i].recommended) const Tag('рекоменд.'),
                  if (protocols[i].auto) const Tag('умный', ok: true),
                ],
                onTap: () {
                  // Правка уходит и ядру (следующий `Up`), и оператору
                  // (очередь записи). Туннель не рвётся: поднимется баннер.
                  CsmSettingsBridge.setProtocol(ref, i);
                  showCarambaToast(context, 'Протокол: ${protocols[i].name}');
                  Future.delayed(const Duration(milliseconds: 300), () {
                    if (context.mounted) _close(context);
                  });
                },
              ),
          ],
        ),
      ),
    );
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go('/home');
    }
  }
}

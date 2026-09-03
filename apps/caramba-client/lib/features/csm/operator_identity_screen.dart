/// Личность оператора, INV-18.
///
/// На экране обязаны быть: отображаемое имя, отпечаток корневого ключа
/// группами по четыре, дата энроллмента, как был установлен пин (вне полосы
/// или в приложении) и менялся ли он когда-нибудь, вместе с историей.
///
/// INV-10 разделяет поверхности: текст оператора живёт в своей карточке и
/// подписан как непроверенный, а хром проверки (отпечаток, происхождение пина,
/// состояние профиля) в другой. Смешивать их нельзя: тогда строка, которую
/// написал оператор, наследует авторитет строки, которую посчитало
/// приложение.
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

class OperatorIdentityScreen extends ConsumerWidget {
  const OperatorIdentityScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final ready = ref.watch(connectionProfilesReadyProvider);
    final identity = ref.watch(csmOperatorIdentityProvider);
    final csm = ref.watch(csmProfileStateProvider);

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
              'Оператор',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            if (!ready)
              const _IdentityLoading()
            else if (identity == null)
              ScreenEmpty(
                glyph: Lucide.key,
                title: 'Корневой ключ не закреплён',
                message:
                    'Этот профиль не проходил энроллмент CSM, поэтому у него '
                    'нет ни отпечатка, ни личности оператора для проверки.',
                actionLabel: 'Подключить оператора',
                onAction: () => context.go(AppRoute.enroll),
              )
            else
              ..._identity(context, ref, identity, csm),
          ],
        ),
      ),
    );
  }

  List<Widget> _identity(
    BuildContext context,
    WidgetRef ref,
    CsmOperatorIdentity identity,
    CsmProfileState? csm,
  ) {
    final c = context.c;
    final history = csm?.pinHistory ?? const <CsmPinHistoryEntry>[];
    final name = csmInertText(identity.displayName);

    return <Widget>[
      // Поверхность текста оператора. Отдельная карточка, отдельная подпись,
      // никакого хрома проверки внутри (INV-10).
      const SectionTitle(
        'Как оператор себя называет',
        padding: EdgeInsets.only(bottom: AppSpace.s3),
      ),
      Container(
        width: double.infinity,
        padding: const EdgeInsets.all(AppSpace.s4),
        decoration: BoxDecoration(
          color: c.surface1,
          borderRadius: AppRadius.r16,
          border: Border.all(color: c.borderSubtle),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              name.isEmpty ? 'Имя не указано' : name,
              style: AppType.titleMd.copyWith(color: c.textHi),
            ),
            const SizedBox(height: AppSpace.s2),
            Text(
              'Это текст оператора. Он показан как есть, ничего не '
              'подтверждает и приложение не открывает по нему ссылки.',
              style: AppType.bodySm.copyWith(color: c.textLow),
            ),
          ],
        ),
      ),

      // Хром проверки.
      const SectionTitle('Отпечаток корневого ключа'),
      Container(
        width: double.infinity,
        padding: const EdgeInsets.all(AppSpace.s4),
        decoration: BoxDecoration(
          color: c.surface1,
          borderRadius: AppRadius.r16,
          border: Border.all(color: c.borderSubtle),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SelectableText(
              identity.fingerprint,
              style: AppType.monoMd.copyWith(
                color: c.textHi,
                letterSpacing: 1.2,
                fontSize: 15,
              ),
            ),
            const SizedBox(height: AppSpace.s2),
            Text(
              'Сверьте эти группы с тем, что оператор продиктовал вам отдельно '
              'от приложения. Совпали значит документы подписаны тем самым '
              'ключом.',
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s3),
            GhostButton(
              label: 'Скопировать отпечаток',
              icon: Lucide.copy,
              minHeight: 44,
              onPressed: () async {
                await Clipboard.setData(
                  ClipboardData(text: identity.fingerprint),
                );
                if (context.mounted) {
                  showCarambaToast(context, 'Отпечаток скопирован');
                }
              },
            ),
          ],
        ),
      ),

      const SectionTitle('Проверка'),
      RowsGroup(
        children: [
          CRow(
            icon: Lucide.shield,
            label: 'Идентификатор оператора',
            value: identity.pid,
            mono: true,
          ),
          CRow(
            icon: Lucide.lock,
            label: 'Как установлен отпечаток',
            value: csmPinOriginTitle(identity.pinOrigin),
            valueColor: identity.pinOrigin == CsmPinOrigin.outOfBand
                ? c.success
                : c.warning,
          ),
          CRow(
            icon: Lucide.clock,
            label: 'Дата энроллмента',
            value: csmDateTime(identity.enrolledAtMs),
            mono: true,
          ),
          CRow(
            icon: Lucide.key,
            label: 'Отпечаток менялся',
            value: identity.pinEverChanged ? 'да' : 'нет, ни разу',
            valueColor: identity.pinEverChanged ? c.warning : c.textMed,
          ),
          CRow(
            icon: Lucide.phone,
            label: 'Ключ этого устройства',
            value: csmHardwareTierTitle(identity.hardwareTier),
          ),
          CRow(
            icon: Lucide.activity,
            label: 'Состояние профиля',
            value: csmStageTitle(identity.stage),
            valueColor: identity.stage.refusesToConnect ? c.danger : c.textMed,
          ),
        ],
      ),
      const SizedBox(height: AppSpace.s3),
      Text(
        csmPinOriginDesc(identity.pinOrigin),
        style: AppType.bodySm.copyWith(color: c.textMed),
      ),

      if (history.isNotEmpty) ...[
        const SectionTitle('История отпечатка'),
        RowsGroup(
          children: [
            for (final e in history)
              CRow(
                icon: Lucide.clock,
                label: e.pin.fingerprint,
                mono: true,
                value: csmDateTime(e.retiredMs),
              ),
          ],
        ),
        const SizedBox(height: AppSpace.s3),
        Text(
          'Законный способ сменить закреплённый корень в CSM один: удалить '
          'профиль и пройти энроллмент заново. Непустая история значит, что '
          'это уже происходило.',
          style: AppType.bodySm.copyWith(color: c.textMed),
        ),
      ] else ...[
        const SizedBox(height: AppSpace.s3),
        Text(
          'Отпечаток закреплён один раз и с тех пор неизменен. Сменить его '
          'внутри профиля нельзя по устройству протокола.',
          style: AppType.bodySm.copyWith(color: c.textMed),
        ),
      ],
    ];
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.settings);
    }
  }
}

/// Пока профили читаются из secure storage, показываем скелет: «ещё читаем» и
/// «ничего не закреплено» это разные ответы, и путать их нельзя.
class _IdentityLoading extends StatelessWidget {
  const _IdentityLoading();

  @override
  Widget build(BuildContext context) {
    return const Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SectionTitle(
          'Как оператор себя называет',
          padding: EdgeInsets.only(bottom: AppSpace.s3),
        ),
        SkeletonRows(rows: 2),
        SectionTitle('Отпечаток корневого ключа'),
        SkeletonRows(rows: 2),
        SectionTitle('Проверка'),
        SkeletonRows(rows: 5),
      ],
    );
  }
}

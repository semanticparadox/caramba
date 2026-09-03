/// Состояние проверки документов в работе, INV-19.
///
/// По каждому документу, на котором приложение сейчас работает: версия, когда
/// выпущен, когда истекает, отпечаток подписавшего, результат проверки и
/// разобранные поля. Форма рассчитана на подозрительного пользователя: цифры
/// показаны как есть, вердикт назван кодом из реестра 03-WIRE.md 6.6 и классом
/// строки из 02-SPEC.md 8.8.1, и ни одна строка оператора сюда не попадает
/// (INV-10).
///
/// Просроченный документ ОСТАЁТСЯ рабочим для подключения: просрочка означает
/// отказ принимать новые инструкции и новый статус, а не разрыв туннеля
/// (INV-16). Экран говорит это прямо, потому что иначе красная плашка рядом со
/// словом «просрочен» читается как «вы не защищены».
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_capability.dart';
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

class CsmDocumentsScreen extends ConsumerWidget {
  const CsmDocumentsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final ready = ref.watch(connectionProfilesReadyProvider);
    final docs = ref.watch(csmDocumentStateProvider);
    final caps = ref.watch(csmCapabilitiesProvider);
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
              'Документы',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            if (!ready)
              const Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  SkeletonRows(rows: 6),
                  SizedBox(height: AppSpace.s4),
                  SizedBox(height: AppSpace.s4),
                  SkeletonRows(rows: 6),
                ],
              )
            // Терминальное состояние: доверие к оператору отозвано. Списка
            // документов здесь нет и быть не может, поэтому весь экран это
            // состояние ошибки, а не таблица с красной строкой
            // (02-SPEC.md 2.1 правило 5).
            else if (csm?.stage == CsmProfileStage.compromised)
              ScreenError(
                message:
                    'Доверие к этому оператору отозвано. Ни один документ '
                    'больше не принимается, и обратной дороги у профиля нет: '
                    'его нужно удалить и пройти энроллмент заново.',
                details: _compromisedDetails(csm!, docs),
              )
            else if (!docs.hasAnything)
              ScreenEmpty(
                glyph: Lucide.layers,
                title: 'Проверенных документов нет',
                message: csm == null
                    ? 'Профиль не проходил энроллмент CSM, поэтому проверять '
                          'нечего.'
                    : 'Ни один документ ещё не проверился на этом устройстве. '
                          'Подключитесь к сети или принесите кадр вне полосы.',
                // Кнопки здесь нет намеренно. Экран энроллмента панели корень
                // не закрепляет: закрепление происходит только по ссылке,
                // несущей k. Отправить туда пользователя с обещанием
                // "подключить оператора" значит провести его через успешный
                // энроллмент и вернуть на этот же текст, из которого другого
                // выхода нет.
              )
            else
              ..._documents(
                context,
                docs,
                caps,
                csm,
                ref.watch(csmUnresolvableSelectionsProvider),
              ),
          ],
        ),
      ),
    );
  }

  List<Widget> _documents(
    BuildContext context,
    CsmDocumentState docs,
    CsmCapabilitySet caps,
    CsmProfileState? csm,
    List<CsmUnresolvableSelection> unresolvable,
  ) {
    final c = context.c;
    final revoked = _revokedRecord(docs);

    return <Widget>[
      // Три условия, которые модель угроз требует называть (02-SPEC.md 8.8.2).
      if (!docs.fleetRootAnchored) ...[
        const InlineBanner(
          tone: BannerTone.warning,
          glyph: Lucide.alert,
          text:
              'Список серверов оператора не покрыт его офлайновым ключом '
              '(fleet not root-anchored). Подмена онлайнового ключа здесь не '
              'будет поймана.',
        ),
        const SizedBox(height: AppSpace.s3),
      ],
      if (docs.capabilitiesDisagree) ...[
        const InlineBanner(
          tone: BannerTone.warning,
          glyph: Lucide.layers,
          text:
              'Возможности каталога и директивы разошлись: устройство работает '
              'на более старом каталоге.',
        ),
        const SizedBox(height: AppSpace.s3),
      ],
      if (docs.anchorPastExpiry) ...[
        InlineBanner(
          tone: BannerTone.info,
          glyph: Lucide.clock,
          text:
              'Ключевой документ просрочен и всё ещё остаётся якорем '
              'авторизации. Его возраст: '
              '${csmAgeText(docs.nowSec - (docs.keyDocument?.expiresSec ?? docs.nowSec))} '
              'после срока.',
        ),
        const SizedBox(height: AppSpace.s3),
      ],
      // 02-SPEC.md 7.4: выбор, который в связанном каталоге не находится.
      // Это ИНФОРМАЦИОННОЕ уведомление, а не карточка, и молчать о нём нельзя:
      // клиент откатился к умолчанию оператора, а пользователь закреплял
      // конкретный узел. Значение показывается инертным текстом (INV-10).
      for (final u in unresolvable) ...[
        InlineBanner(
          tone: BannerTone.info,
          glyph: Lucide.alert,
          text:
              'Выбор ${u.field} не находится в проверенном каталоге: '
              '${csmInertText(u.value, cap: 32)}. Действует умолчание '
              'оператора. Другой сервер молча не подставлен.',
        ),
        const SizedBox(height: AppSpace.s3),
      ],
      if (revoked != null) ...[
        const InlineBanner(
          tone: BannerTone.danger,
          glyph: Lucide.key,
          text:
              'Ваш провайдер заменил ключ подписи, а это устройство ещё не '
              'получило новую конфигурацию. Быстрее всего это чинится кадром '
              'вне полосы (ступень R6).',
        ),
        const SizedBox(height: AppSpace.s3),
      ],

      for (final record in <CsmDocumentRecord?>[
        docs.keyDocument,
        docs.catalog,
        docs.directive,
      ])
        if (record != null) ...[
          const SizedBox(height: AppSpace.s2),
          _DocumentCard(record: record, nowSec: docs.nowSec),
        ],

      // Разобранные поля, которые живут не на одном документе, а на профиле.
      const SectionTitle('Разобранные поля профиля'),
      RowsGroup(
        children: [
          CRow(
            icon: Lucide.shield,
            label: 'Локатор',
            value: csm?.locator ?? 'нет',
            mono: true,
          ),
          CRow(
            icon: Lucide.phone,
            label: 'Отпечаток устройства',
            value: csm?.deviceThumbprint ?? 'нет',
            mono: true,
          ),
          CRow(
            icon: Lucide.clock,
            label: 'Временной пол',
            value: (csm?.timeFloorSec ?? 0) == 0
                ? 'не установлен'
                : csmDateTimeSec(csm!.timeFloorSec),
            mono: true,
          ),
          CRow(
            icon: Lucide.layers,
            label: 'Поле возможностей',
            value: '0x${caps.toHex()}',
            mono: true,
          ),
        ],
      ),
      const SizedBox(height: AppSpace.s3),
      Text(
        'Временной пол монотонен и никогда не уменьшается: документ, '
        'выпущенный раньше него, отвергается даже с верной подписью.',
        style: AppType.bodySm.copyWith(color: c.textMed),
      ),

      const SectionTitle('Что оператор объявил доступным'),
      if (caps.bits.isEmpty)
        const InlineEmpty(message: 'Ни один бит возможностей не поднят.')
      else
        RowsGroup(
          children: [
            for (final bit in caps.bits)
              CRow(
                icon: Lucide.check,
                label: csmCapabilityTitle(bit),
                value: 'бит ${bit.bit}',
                mono: true,
              ),
          ],
        ),
      const SizedBox(height: AppSpace.s3),
      Text(
        'Это уже пересечение того, что объявил оператор, с тем, что умеет эта '
        'сборка. Бит, которого приложение не умеет, здесь не появится ни при '
        'каком подписанном документе.',
        style: AppType.bodySm.copyWith(color: c.textMed),
      ),
    ];
  }

  /// Сырые подробности для раскрытия под ошибкой: то, что нужно поддержке, и
  /// ничего, что нужно было бы объяснять словами.
  static String _compromisedDetails(
    CsmProfileState csm,
    CsmDocumentState docs,
  ) {
    final sb = StringBuffer()
      ..writeln('pid ${csm.pin.pid}')
      ..writeln('stage ${csm.stage.wire}')
      ..writeln('fingerprint ${csm.pin.fingerprint}');
    for (final r in <CsmDocumentRecord?>[
      docs.keyDocument,
      docs.catalog,
      docs.directive,
    ]) {
      if (r == null) continue;
      sb.writeln('${csmDocShortName(r.docType)} v${r.version} ${r.verdict}');
    }
    return sb.toString().trimRight();
  }

  /// Документ, отвергнутый из-за отозванного ключа подписи. Он и есть повод
  /// для строки 02-SPEC.md 10.4.
  static CsmDocumentRecord? _revokedRecord(CsmDocumentState docs) {
    for (final r in <CsmDocumentRecord?>[
      docs.keyDocument,
      docs.catalog,
      docs.directive,
    ]) {
      if (r != null && r.verdict == 'E_VERIFY_REVOKED') {
        return r;
      }
    }
    return null;
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.settings);
    }
  }
}

/// Один документ: заголовок с вердиктом и таблица разобранных полей.
class _DocumentCard extends StatelessWidget {
  final CsmDocumentRecord record;
  final int nowSec;

  const _DocumentCard({required this.record, required this.nowSec});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final expired = record.isExpiredAt(nowSec);
    final rung = record.viaRung == null
        ? null
        : CsmRung.fromId(record.viaRung!);

    return Container(
      margin: const EdgeInsets.only(bottom: AppSpace.s4),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r16,
        border: Border.all(color: c.borderSubtle),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(
              AppSpace.s4,
              AppSpace.s4,
              AppSpace.s4,
              AppSpace.s3,
            ),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        csmDocTypeName(record.docType),
                        style: AppType.titleMd.copyWith(color: c.textHi),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        '${csmDocShortName(record.docType)} · подписан '
                        '${csmDocRoleName(record.docType)}',
                        style: AppType.bodySm.copyWith(color: c.textMed),
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: AppSpace.s2),
                _VerdictBadge(verdict: record.verdict),
              ],
            ),
          ),
          Divider(height: 1, thickness: 1, color: c.borderSubtle),
          CRow(label: 'Версия', value: '${record.version}', mono: true),
          Divider(height: 1, thickness: 1, color: c.borderSubtle),
          CRow(
            label: 'Выпущен',
            value: csmDateTimeSec(record.issuedSec),
            mono: true,
          ),
          Divider(height: 1, thickness: 1, color: c.borderSubtle),
          CRow(
            label: 'Истекает',
            value: csmDateTimeSec(record.expiresSec),
            mono: true,
            valueColor: expired ? c.warning : null,
          ),
          Divider(height: 1, thickness: 1, color: c.borderSubtle),
          CRow(
            label: 'Проверен',
            value: csmDateTime(record.verifiedAtMs),
            mono: true,
          ),
          Divider(height: 1, thickness: 1, color: c.borderSubtle),
          CRow(
            label: 'Принесла ступень',
            value: rung == null
                ? 'с диска'
                : '${csmRungId(rung)} ${csmRungTitle(rung)}',
          ),
          if (record.scope.isNotEmpty) ...[
            Divider(height: 1, thickness: 1, color: c.borderSubtle),
            CRow(label: 'Область версии', value: record.scope, mono: true),
          ],
          Divider(height: 1, thickness: 1, color: c.borderSubtle),
          Padding(
            padding: const EdgeInsets.fromLTRB(
              AppSpace.s4,
              AppSpace.s3,
              AppSpace.s4,
              AppSpace.s3,
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Отпечатки подписавших',
                  style: AppType.bodyMd.copyWith(color: c.textHi),
                ),
                const SizedBox(height: AppSpace.s2),
                if (record.signerFingerprints.isEmpty)
                  Text(
                    'нет: подпись не сошлась ни с одним авторизованным ключом',
                    style: AppType.bodySm.copyWith(color: c.textMed),
                  )
                else
                  for (final fp in record.signerFingerprints)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 2),
                      child: SelectableText(
                        fp,
                        style: AppType.monoSm.copyWith(color: c.textMed),
                      ),
                    ),
              ],
            ),
          ),
          if (record.frameDigest.isNotEmpty) ...[
            Divider(height: 1, thickness: 1, color: c.borderSubtle),
            CRow(
              label: 'sha256 кадра',
              value: record.frameDigest,
              mono: true,
              trailing: IconBtn(
                Lucide.copy,
                size: 32,
                onTap: () async {
                  await Clipboard.setData(
                    ClipboardData(text: record.frameDigest),
                  );
                  if (context.mounted) {
                    showCarambaToast(context, 'Хеш кадра скопирован');
                  }
                },
              ),
            ),
          ],
          if (expired)
            Padding(
              padding: const EdgeInsets.fromLTRB(
                AppSpace.s4,
                AppSpace.s1,
                AppSpace.s4,
                AppSpace.s4,
              ),
              child: Text(
                'Просрочен. Подключаться это не мешает: просроченный документ '
                'перестаёт принимать новые инструкции и новый статус, но '
                'туннель на нём поднимается.',
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),
            ),
        ],
      ),
    );
  }
}

/// Вердикт проверки: `ok` зелёным, код отказа с классом строки красным.
class _VerdictBadge extends StatelessWidget {
  final String verdict;
  const _VerdictBadge({required this.verdict});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final ok = verdict == 'ok';
    final cls = csmStringClassOf(verdict);
    final color = ok
        ? c.success
        : (cls == CsmStringClass.stale ? c.warning : c.danger);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      constraints: const BoxConstraints(maxWidth: 180),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: color),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          Text(
            ok ? 'ПРОВЕРЕНО' : verdict,
            textAlign: TextAlign.right,
            style: AppType.monoSm.copyWith(color: color, letterSpacing: 0.5),
          ),
          if (!ok)
            Text(
              '${csmStringClassLabel(cls)}: ${csmErrorText(verdict)}',
              textAlign: TextAlign.right,
              style: AppType.bodySm.copyWith(color: c.textMed, fontSize: 11),
            ),
        ],
      ),
    );
  }
}

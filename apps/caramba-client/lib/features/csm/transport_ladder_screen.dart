/// Транспортная лестница, INV-17.
///
/// Все семь вкомпилированных ступеней на одном экране, с переключателем, с
/// порядком и с живой историей попыток. Недоступная ступень рендерится ВИДИМОЙ
/// И ВЫКЛЮЧЕННОЙ с причиной и НИКОГДА не прячется.
///
/// Различие с контролом, завязанным на бит возможности, намеренное: такой
/// контрол наоборот прячется, когда бит снят (02-SPEC.md 6.2). Ступень это
/// обещание, которое приложение даёт о себе, и пользователь вправе проверить
/// весь список; контрол это функция, которой за снятым битом нет, и рисовать
/// её включённой и молча ничего не делающей значит продавать пустышку.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/features/csm/attempt_history.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/csm_ladder_sync.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Как часто экран переспрашивает ядро, пока он открыт.
///
/// Опроса в фоне нет намеренно: история попыток нужна ровно тому экрану,
/// который её показывает, и заводить ради неё постоянный таймер значило бы
/// будить ядро там, где на это никто не смотрит.
const Duration kCsmLadderPollInterval = Duration(seconds: 3);

class TransportLadderScreen extends ConsumerStatefulWidget {
  const TransportLadderScreen({super.key});

  @override
  ConsumerState<TransportLadderScreen> createState() =>
      _TransportLadderScreenState();
}

class _TransportLadderScreenState extends ConsumerState<TransportLadderScreen> {
  Timer? _poll;

  /// Ядро отвергло последний порядок или переключатель.
  ///
  /// Экран, показывающий порядок, по которому ядро не ходит, врёт, а лестницей
  /// ходит именно ядро. Поэтому отказ виден здесь, а не гасится.
  bool _coreRefused = false;

  @override
  void initState() {
    super.initState();
    // Первый подъём сразу: экран, открытый на секунду, обязан показать то, что
    // ядро уже помнит, а не пустой список.
    WidgetsBinding.instance.addPostFrameCallback((_) => _pump());
    _poll = Timer.periodic(kCsmLadderPollInterval, (_) => _pump());
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  void _pump() {
    if (!mounted) {
      return;
    }
    unawaited(ref.read(csmLadderSyncProvider).pump());
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final ready = ref.watch(connectionProfilesReadyProvider);
    final ladder = ref.watch(csmLadderProvider);
    final attempts = ref.watch(csmAttemptHistoryProvider);

    // Профиль без CSM не имеет действующей лестницы, но список ступеней всё
    // равно показывается целиком: скрыть ступень нельзя ни по какой причине.
    final rungs = ladder.rungs.isNotEmpty ? ladder.rungs : _inertRungs();
    final live = ladder.rungs.isNotEmpty;

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
              'Транспорт',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            Text(
              'Приложение берёт конфигурацию по этому списку сверху вниз и '
              'переходит к следующей ступени, когда предыдущая не отвечает. '
              'Порядок и включённость ваши: подписанные умолчания оператора '
              'больше их не переписывают.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s4),
            if (!ready)
              const SkeletonRows(rows: 7)
            else ...[
              if (!live) ...[
                const InlineBanner(
                  tone: BannerTone.info,
                  glyph: Lucide.alert,
                  text:
                      'Профиль не проходил энроллмент CSM, поэтому лестница '
                      'сейчас не действует. Список показан целиком, чтобы было '
                      'видно, что именно появится после подключения оператора.',
                ),
                const SizedBox(height: AppSpace.s4),
              ],
              for (final r in rungs)
                _RungCard(
                  entry: r,
                  interactive: live,
                  onToggle: (v) => _toggle(ref, rungs, r, enable: v),
                  onMove: (delta) => _move(ref, rungs, r, delta),
                  canMoveUp: r.position > 0,
                  canMoveDown: r.position < rungs.length - 1,
                ),
              if (_coreRefused) ...[
                const SizedBox(height: AppSpace.s3),
                const InlineBanner(
                  tone: BannerTone.warning,
                  glyph: Lucide.alert,
                  text:
                      'Ядро не приняло этот порядок, поэтому выборка идёт по '
                      'прежнему. Список выше показывает ваш выбор, а не то, по '
                      'чему ядро сейчас ходит.',
                ),
              ],
              if (ladder.userTouched) ...[
                const SizedBox(height: AppSpace.s2),
                Text(
                  'Вы меняли лестницу сами. С этого момента ваш выбор побеждает '
                  'умолчания каталога навсегда.',
                  style: AppType.bodySm.copyWith(color: c.textMed),
                ),
              ],
            ],

            const SectionTitle('История попыток'),
            if (attempts.isEmpty)
              const ScreenEmpty(
                glyph: Lucide.listTree,
                title: 'Попыток ещё не было',
                message:
                    'Сюда попадает каждая попытка каждой ступени: время, хост, '
                    'исход и код отказа. Запись остаётся на устройстве и '
                    'никогда никуда не отправляется.',
              )
            else ...[
              RowsGroup(
                children: [
                  for (final a in attempts.take(kCsmAttemptHistoryLimit))
                    _AttemptRow(attempt: a),
                ],
              ),
              const SizedBox(height: AppSpace.s3),
              Text(
                'Последние ${attempts.length} из $kCsmAttemptHistoryLimit '
                'записей. История локальная: какая ступень принесла запрос, '
                'оператору не сообщается никогда.',
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),
            ],
          ],
        ),
      ),
    );
  }

  /// Ступени для профиля без CSM: полный список в порядке по умолчанию,
  /// выключенный, с честной причиной.
  static List<CsmLadderRung> _inertRungs() {
    final out = <CsmLadderRung>[];
    for (var i = 0; i < kCsmDefaultLadderOrder.length; i++) {
      final rung = CsmRung.fromId(kCsmDefaultLadderOrder[i]);
      if (rung == null) continue;
      final enabled = kCsmDefaultLadderEnabled.contains(rung.id);
      out.add(
        CsmLadderRung(
          rung: rung,
          position: i,
          enabled: enabled,
          reason: enabled
              ? CsmUnavailableReason.notConfigured
              : CsmUnavailableReason.userDisabled,
        ),
      );
    }
    return out;
  }

  void _toggle(
    WidgetRef ref,
    List<CsmLadderRung> rungs,
    CsmLadderRung target, {
    required bool enable,
  }) {
    final enabled = <int>{
      for (final r in rungs)
        if (r.enabled) r.rung.id,
    };
    if (enable) {
      enabled.add(target.rung.id);
    } else {
      enabled.remove(target.rung.id);
    }
    unawaited(
      _apply(
        () => ref
            .read(csmNotifierProvider)
            .setLadder(enabled: enabled.toList(growable: false)..sort()),
      ),
    );
  }

  /// Применяет изменение и запоминает, принято ли оно ЯДРОМ.
  Future<void> _apply(Future<CsmLadderApplyOutcome> Function() op) async {
    final outcome = await op();
    if (!mounted) {
      return;
    }
    final refused = outcome == CsmLadderApplyOutcome.coreRefused;
    if (refused != _coreRefused) {
      setState(() => _coreRefused = refused);
    }
  }

  void _move(
    WidgetRef ref,
    List<CsmLadderRung> rungs,
    CsmLadderRung target,
    int delta,
  ) {
    final order = rungs.map((r) => r.rung.id).toList();
    final from = target.position;
    final to = from + delta;
    if (to < 0 || to >= order.length) return;
    final id = order.removeAt(from);
    order.insert(to, id);
    unawaited(
      _apply(() => ref.read(csmNotifierProvider).setLadder(order: order)),
    );
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.settings);
    }
  }
}

/// Одна ступень. Недоступная остаётся на экране, тускнеет и называет причину.
class _RungCard extends StatelessWidget {
  final CsmLadderRung entry;
  final bool interactive;
  final ValueChanged<bool> onToggle;
  final ValueChanged<int> onMove;
  final bool canMoveUp;
  final bool canMoveDown;

  const _RungCard({
    required this.entry,
    required this.interactive,
    required this.onToggle,
    required this.onMove,
    required this.canMoveUp,
    required this.canMoveDown,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final rung = entry.rung;
    final mandatory = rung.isMandatory;
    final dim = !entry.available;

    return Container(
      margin: const EdgeInsets.only(bottom: AppSpace.s2),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r14,
        border: Border.all(color: c.borderSubtle),
      ),
      padding: const EdgeInsets.all(AppSpace.s4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Container(
                width: 34,
                height: 26,
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  color: c.surfaceInset,
                  borderRadius: AppRadius.r8,
                  border: Border.all(color: c.borderSubtle),
                ),
                child: Text(
                  csmRungId(rung),
                  style: AppType.monoSm.copyWith(color: c.textHi),
                ),
              ),
              const SizedBox(width: AppSpace.s3),
              Expanded(
                child: Opacity(
                  opacity: dim ? 0.62 : 1,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        csmRungTitle(rung),
                        style: AppType.bodyMd.copyWith(color: c.textHi),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        csmRungDesc(rung),
                        style: AppType.bodySm.copyWith(color: c.textMed),
                      ),
                    ],
                  ),
                ),
              ),
              const SizedBox(width: AppSpace.s2),
              Semantics(
                label: '${csmRungId(rung)} ${csmRungTitle(rung)}',
                child: Switch(
                  value: entry.enabled,
                  // Обязательную ступень выключить нельзя, но переключатель
                  // остаётся на экране: скрыть его значило бы спрятать сам
                  // факт, что эта ступень есть и работает всегда.
                  onChanged: (mandatory || !interactive) ? null : onToggle,
                ),
              ),
            ],
          ),
          const SizedBox(height: AppSpace.s3),
          Row(
            children: [
              Text(
                'Порядок: ${entry.position + 1}',
                style: AppType.monoSm.copyWith(color: c.textLow),
              ),
              const SizedBox(width: AppSpace.s3),
              if (mandatory) const Tag('всегда'),
              if (!entry.available) ...[
                if (mandatory) const SizedBox(width: AppSpace.s2),
                Flexible(
                  child: Text(
                    csmUnavailableReasonText(entry.reason!),
                    overflow: TextOverflow.ellipsis,
                    style: AppType.bodySm.copyWith(color: c.warning),
                  ),
                ),
              ],
              const Spacer(),
              IconBtn(
                Lucide.chevronUp,
                size: 34,
                onTap: (interactive && canMoveUp) ? () => onMove(-1) : null,
                color: (interactive && canMoveUp) ? c.textMed : c.textLow,
              ),
              const SizedBox(width: AppSpace.s1),
              IconBtn(
                Lucide.chevronDown,
                size: 34,
                onTap: (interactive && canMoveDown) ? () => onMove(1) : null,
                color: (interactive && canMoveDown) ? c.textMed : c.textLow,
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Одна попытка в истории.
///
/// Шесть фактов обязательны и все шесть на экране: ступень, хост, момент
/// старта, исход, причина отказа и то, сколько байт и сколько времени попытка
/// стоила. Неудачная попытка отрисовывается СО СВОЕЙ ПРИЧИНОЙ: попытка,
/// проглоченная молча, делает INV-17 декорацией.
/// Предел длины метки попытки на экране.
const int kCsmAttemptLabelMax = 64;

/// Обрезает метку до [kCsmAttemptLabelMax] символов с многоточием.
String _capLabel(String raw) => raw.length <= kCsmAttemptLabelMax
    ? raw
    : '${raw.substring(0, kCsmAttemptLabelMax)}...';

class _AttemptRow extends StatelessWidget {
  final CsmAttempt attempt;
  const _AttemptRow({required this.attempt});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final code = attempt.errorCode;
    final (String text, Color color) = switch (attempt.outcome) {
      CsmAttemptOutcome.ok => ('успех', c.success),
      CsmAttemptOutcome.failed => (code ?? 'отказ', c.danger),
      CsmAttemptOutcome.skipped => ('пропущена', c.textLow),
    };
    // Метка ступени ограничена по длине. Сегодня ядро кладёт сюда только
    // "mirror-N", "doh-N", "tunnel", "proxy", "cache" или имя хоста, введённое
    // самим пользователем, то есть строки оператора сюда не попадают (INV-10).
    // Предел держит это верным и в тот день, когда в Host поедет что-то ещё.
    final host = attempt.host.isEmpty ? '' : ' · ${_capLabel(attempt.host)}';
    final facts = <String>[
      csmDateTime(attempt.startedMs),
      if (attempt.durationMs != null) '${attempt.durationMs} мс',
      // Ноль байт это факт, а не отсутствие факта: он отличает пустой ответ от
      // кадра, который приехал и не проверился.
      '${attempt.bytes} Б',
      if (attempt.status != 0) 'HTTP ${attempt.status}',
    ];

    return Container(
      constraints: const BoxConstraints(minHeight: 54),
      padding: const EdgeInsets.symmetric(
        horizontal: AppSpace.s4,
        vertical: AppSpace.s3,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: Text(
                  '${csmRungId(attempt.rung)}$host',
                  style: AppType.monoSm.copyWith(color: c.textHi),
                ),
              ),
              const SizedBox(width: AppSpace.s3),
              Flexible(
                child: Text(
                  text,
                  textAlign: TextAlign.right,
                  style: AppType.monoSm.copyWith(color: color),
                ),
              ),
            ],
          ),
          const SizedBox(height: 2),
          Text(
            facts.join(' · '),
            style: AppType.monoSm.copyWith(color: c.textLow),
          ),
        ],
      ),
    );
  }
}

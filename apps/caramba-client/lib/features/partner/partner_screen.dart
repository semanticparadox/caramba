import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/partner.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Партнёрский дашборд. Партнёр заводит коды под источники трафика (youtube,
/// tg-канал, блогер), а панель копит по каждому статистику: клики, регистрации,
/// конверсии (первая оплата) и начисленный баланс. Данные — авторитетный
/// `/api/v2/app/partner/codes`. Раздел гейтится партнёрской ролью на панели
/// (`is_partner`); пункт входа в профиле скрыт у обычных пользователей.
class PartnerScreen extends ConsumerWidget {
  const PartnerScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final async = ref.watch(partnerProvider);

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: RefreshIndicator(
          color: c.accent,
          backgroundColor: c.surface2,
          onRefresh: () async {
            ref.invalidate(partnerProvider);
            await ref
                .read(partnerProvider.future)
                .catchError((_) => const PartnerOverview());
          },
          child: ListView(
            padding: const EdgeInsets.fromLTRB(
              AppSpace.s5,
              AppSpace.s5,
              AppSpace.s5,
              AppSpace.s12,
            ),
            children: [
              ScreenHead(
                'Партнёрам',
                trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
              ),
              async.when(
                data: (o) => o.isPartner
                    ? _PartnerBody(overview: o)
                    : const InlineEmpty(
                        top: AppSpace.s16,
                        message:
                            'Партнёрская программа недоступна для вашего аккаунта.',
                      ),
                loading: () => const InlineLoading(top: AppSpace.s16),
                error: (_, __) => InlineError(
                  top: AppSpace.s16,
                  message: 'Не удалось загрузить партнёрскую сводку',
                  onRetry: () => ref.invalidate(partnerProvider),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.profile);
    }
  }
}

class _PartnerBody extends StatelessWidget {
  final PartnerOverview overview;
  const _PartnerBody({required this.overview});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final codes = overview.codes;

    // Суммарные показатели для шапки.
    var clicks = 0, signups = 0, conversions = 0, earned = 0;
    for (final code in codes) {
      clicks += code.clicks;
      signups += code.signups;
      conversions += code.conversions;
      earned += code.balanceEarnedCents;
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          'Заводите отдельный код под каждый источник трафика. По ссылке кода '
          'считаются переходы, регистрации и оплаты, а начисления идут на ваш '
          'баланс.',
          style: AppType.bodyMd.copyWith(color: c.textMed),
        ),

        // ---- Итоги (минорные единицы для начислено, mono-значения).
        const SectionTitle('Итого'),
        Row(
          children: [
            Expanded(
              child: _StatCard(
                icon: Lucide.activity,
                label: 'Переходы',
                value: '$clicks',
                accent: false,
              ),
            ),
            const SizedBox(width: AppSpace.s2),
            Expanded(
              child: _StatCard(
                icon: Lucide.userPlus,
                label: 'Регистрации',
                value: '$signups',
                accent: false,
              ),
            ),
          ],
        ),
        const SizedBox(height: AppSpace.s2),
        Row(
          children: [
            Expanded(
              child: _StatCard(
                icon: Lucide.creditCard,
                label: 'Конверсии',
                value: '$conversions',
                accent: false,
              ),
            ),
            const SizedBox(width: AppSpace.s2),
            Expanded(
              child: _StatCard(
                icon: Lucide.wallet,
                label: 'Начислено',
                value: ReferralInfo.formatMinor(earned),
                accent: earned > 0,
              ),
            ),
          ],
        ),

        // ---- Новый код.
        const SectionTitle('Новый код'),
        const _CreateCodeForm(),

        // ---- Список кодов.
        SectionTitle(
          'Коды',
          trailing: Text(
            '${codes.length}',
            style: AppType.monoSm.copyWith(color: c.textLow),
          ),
        ),
        if (codes.isEmpty)
          const InlineEmpty(
            top: AppSpace.s4,
            message: 'Кодов пока нет. Создайте первый под ваш источник.',
          )
        else
          Column(
            children: [for (final code in codes) _PartnerCodeCard(code: code)],
          ),
      ],
    );
  }
}

/// Карточка-метрика итога: mono-значение крупно, подпись под ним.
class _StatCard extends StatelessWidget {
  final String icon;
  final String label;
  final String value;
  final bool accent;
  const _StatCard({
    required this.icon,
    required this.label,
    required this.value,
    required this.accent,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      padding: const EdgeInsets.all(AppSpace.s4),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r16,
        border: Border.all(color: c.borderSubtle),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          LucideIcon(icon, color: c.textMed, size: 18),
          const SizedBox(height: AppSpace.s3),
          Text(
            value,
            style: AppType.monoMd.copyWith(
              fontSize: 26,
              height: 1.1,
              color: accent ? c.success : c.textHi,
            ),
          ),
          const SizedBox(height: 4),
          Text(label, style: AppType.bodySm.copyWith(color: c.textMed)),
        ],
      ),
    );
  }
}

/// Форма создания кода: метка источника + кнопка. На пустую метку панель сама
/// вернёт 400 — поэтому кнопка неактивна без текста.
class _CreateCodeForm extends ConsumerStatefulWidget {
  const _CreateCodeForm();

  @override
  ConsumerState<_CreateCodeForm> createState() => _CreateCodeFormState();
}

class _CreateCodeFormState extends ConsumerState<_CreateCodeForm> {
  final _controller = TextEditingController();
  bool _busy = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final label = _controller.text.trim();
    if (label.isEmpty || _busy) return;
    setState(() => _busy = true);
    try {
      await ref.read(partnerProvider.notifier).create(label);
      if (!mounted) return;
      _controller.clear();
      showCarambaToast(context, 'Код создан');
    } on ApiException catch (e) {
      if (mounted) showCarambaToast(context, e.message);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        TextField(
          controller: _controller,
          enabled: !_busy,
          textInputAction: TextInputAction.done,
          onSubmitted: (_) => _submit(),
          style: AppType.bodyMd.copyWith(color: c.textHi),
          decoration: InputDecoration(
            hintText: 'Метка источника: youtube, tg-канал, блогер',
            hintStyle: AppType.bodyMd.copyWith(color: c.textLow),
            filled: true,
            fillColor: c.surface1,
            contentPadding: const EdgeInsets.symmetric(
              horizontal: AppSpace.s4,
              vertical: AppSpace.s3 + 2,
            ),
            enabledBorder: OutlineInputBorder(
              borderRadius: AppRadius.r12,
              borderSide: BorderSide(color: c.borderSubtle),
            ),
            focusedBorder: OutlineInputBorder(
              borderRadius: AppRadius.r12,
              borderSide: BorderSide(color: c.borderStrong),
            ),
            disabledBorder: OutlineInputBorder(
              borderRadius: AppRadius.r12,
              borderSide: BorderSide(color: c.borderSubtle),
            ),
          ),
        ),
        const SizedBox(height: AppSpace.s3),
        GhostButton(
          label: _busy ? 'Создаём...' : 'Создать код',
          icon: Lucide.plus,
          onPressed: _busy ? null : _submit,
        ),
      ],
    );
  }
}

/// Карточка одного кода: метка-источник, mono-код, ссылка, постатейная
/// статистика и удаление.
class _PartnerCodeCard extends ConsumerWidget {
  final PartnerCode code;
  const _PartnerCodeCard({required this.code});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    return Container(
      margin: const EdgeInsets.only(bottom: AppSpace.s2),
      padding: const EdgeInsets.all(AppSpace.s4),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r16,
        border: Border.all(color: c.borderSubtle),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      code.sourceDisplay,
                      overflow: TextOverflow.ellipsis,
                      style: AppType.bodyMd.copyWith(color: c.textHi),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      code.code,
                      overflow: TextOverflow.ellipsis,
                      style: AppType.monoSm.copyWith(color: c.textMed),
                    ),
                  ],
                ),
              ),
              IconBtn(
                Lucide.trash,
                size: 36,
                color: c.danger,
                onTap: () => _confirmDelete(context, ref),
              ),
            ],
          ),
          const SizedBox(height: AppSpace.s3),

          // Постатейная статистика: подпись слева, mono-значение справа.
          _StatRow(label: 'Переходы', value: '${code.clicks}'),
          _StatRow(label: 'Регистрации', value: '${code.signups}'),
          _StatRow(label: 'Конверсии', value: '${code.conversions}'),
          _StatRow(
            label: 'Начислено',
            value: code.balanceEarnedLabel,
            valueColor: code.balanceEarnedCents > 0 ? c.success : null,
          ),

          const SizedBox(height: AppSpace.s3),
          GhostButton(
            label: 'Скопировать ссылку',
            icon: Lucide.copy,
            minHeight: 42,
            onPressed: () {
              Clipboard.setData(ClipboardData(text: code.referralLink));
              showCarambaToast(context, 'Ссылка скопирована');
            },
          ),
        ],
      ),
    );
  }

  Future<void> _confirmDelete(BuildContext context, WidgetRef ref) async {
    final c = context.c;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface2,
        title: Text(
          'Удалить код?',
          style: AppType.titleMd.copyWith(color: c.textHi),
        ),
        content: Text(
          'Код ${code.code} перестанет считать новые переходы и оплаты. '
          'Уже начисленный баланс сохранится.',
          style: AppType.bodyMd.copyWith(color: c.textMed),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: Text(
              'Отмена',
              style: AppType.label.copyWith(color: c.textMed),
            ),
          ),
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(true),
            child: Text(
              'Удалить',
              style: AppType.label.copyWith(color: c.danger),
            ),
          ),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await ref.read(partnerProvider.notifier).remove(code.code);
      if (context.mounted) showCarambaToast(context, 'Код удалён');
    } on ApiException catch (e) {
      if (context.mounted) showCarambaToast(context, e.message);
    }
  }
}

/// Строка статистики внутри карточки кода: подпись + mono-значение.
class _StatRow extends StatelessWidget {
  final String label;
  final String value;
  final Color? valueColor;
  const _StatRow({required this.label, required this.value, this.valueColor});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          Text(label, style: AppType.bodySm.copyWith(color: c.textMed)),
          Text(
            value,
            style: AppType.monoMd.copyWith(color: valueColor ?? c.textHi),
          ),
        ],
      ),
    );
  }
}

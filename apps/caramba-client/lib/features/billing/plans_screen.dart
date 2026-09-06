/// Экран тарифов: что оператор продаёт, на какой срок и по какой цене.
///
/// Открывается по «Купить или продлить» в профиле, по «Оплатить» в карточке
/// отказа и с экрана подключения по ссылке. До него единственным путём к оплате
/// была внешняя ссылка на бота — а у оператора, который её не опубликовал,
/// кнопка вела в пустую строку.
///
/// ЧЕГО ЭКРАН НЕ ДЕЛАЕТ.
///
/// Не придумывает цену. Тариф без строк в `plan_durations` показывается
/// карточкой без кнопки и подписью «сейчас не продаётся» — ровно так его рисует
/// витрина мини-аппа, и ровно это означает решение оператора не продавать его.
/// Соблазн «подставить 30 дней по plans.price» уже был реализован на панели и
/// уже удалён оттуда: неверная цена хуже отсутствующей.
///
/// Не прячет то, чего не продаёт. Тариф без сроков остаётся в списке: оператор
/// его показывает, и человек, который видел его в боте, должен понять, почему
/// здесь нельзя нажать, а не решить, что приложение показывает не всё.
///
/// Не выдумывает адрес оплаты. Если у оператора не настроен ни мини-апп, ни
/// бот, экран говорит это словами. Подставить чужого `@bot` нельзя: приложение
/// не принадлежит ни одному оператору.
///
/// Не выдаёт «оплата в приложении» за общее правило. Флаг `in_app_purchase`
/// приходит от панели (это лицензия оператора), и когда он `false`, экран так и
/// пишет: покупка оформляется в Telegram. Молча показать кнопку, которая
/// вернёт 403, было бы хуже, чем не показать её вовсе.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/plan_catalog.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/data/models/subscription.dart'
    show formatBytesRu;
import 'package:caramba_client/features/billing/payment_sheet.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart'
    show subscriptionAccessProvider;
import 'package:caramba_client/state/subscription_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

class PlansScreen extends ConsumerWidget {
  const PlansScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final catalogAsync = ref.watch(planCatalogProvider);
    final subs = ref.watch(subscriptionsProvider).valueOrNull;

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
              'Тарифы',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            if (subs != null && subs.isNotEmpty) ...[
              Text(
                _currentLine(subs),
                style: AppType.bodyMd.copyWith(color: c.textMed),
              ),
              const SizedBox(height: AppSpace.s4),
            ],
            catalogAsync.when(
              loading: () => const SkeletonRows(),
              error: (e, _) => _failure(context, ref, e),
              data: (catalog) => _catalog(context, ref, catalog, subs),
            ),
          ],
        ),
      ),
    );
  }

  // ---------------------------------------------------------------------------
  // Тело витрины.
  // ---------------------------------------------------------------------------

  Widget _catalog(
    BuildContext context,
    WidgetRef ref,
    PlanCatalog catalog,
    List<SubPlan>? subs,
  ) {
    final c = context.c;
    if (catalog.plans.isEmpty) {
      return ScreenEmpty(
        glyph: Lucide.creditCard,
        title: 'Тарифов нет',
        message:
            'Оператор не опубликовал ни одного тарифа. Если вы ожидали увидеть '
            'здесь подписку — спросите там, где её оформляли.',
        actionLabel: catalog.pay.isEmpty ? null : 'Открыть Telegram',
        onAction: catalog.pay.isEmpty
            ? null
            : () => unawaited(openTelegramPay(context, catalog.pay)),
      );
    }

    final currentName = _activeName(subs);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Лицензия оператора решает, можно ли платить внутри приложения. Это
        // не наша политика и не сбой — так это и сказано.
        if (!catalog.inAppPurchase) ...[
          InlineBanner(
            glyph: Lucide.send,
            text: catalog.pay.isEmpty
                ? 'Оплата внутри приложения у этого оператора не включена, и '
                      'адреса, где платят, он не опубликовал. Он есть там, где '
                      'вы оформляли подписку.'
                : 'Оплата внутри приложения у этого оператора не включена: '
                      'покупка откроется в Telegram.',
          ),
          const SizedBox(height: AppSpace.s4),
        ],
        if (!catalog.anyPurchasable) ...[
          const InlineBanner(
            tone: BannerTone.warning,
            glyph: Lucide.clock,
            text:
                'Ни у одного тарифа не заведено срока продажи. Купить сейчас '
                'нечего — это настройка на стороне оператора, а не сбой '
                'приложения.',
          ),
          const SizedBox(height: AppSpace.s4),
        ],
        for (final plan in catalog.sorted)
          PlanCard(
            plan: plan,
            catalog: catalog,
            isCurrent:
                currentName != null &&
                currentName.toLowerCase() == plan.name.toLowerCase(),
          ),
        const SizedBox(height: AppSpace.s2),
        Text(
          'Оплата проходит на стороне оператора. Приложение не хранит ни '
          'карт, ни платёжных данных.',
          style: AppType.bodySm.copyWith(color: c.textLow),
        ),
      ],
    );
  }

  /// Отказ загрузки витрины. 404 отделён от остального намеренно: это не сбой
  /// сети и не пустой каталог, а панель, которая старее этого маршрута, — и
  /// повтор её не вылечит, поэтому кнопки «Повторить» у этой ветки нет.
  Widget _failure(BuildContext context, WidgetRef ref, Object error) {
    final code = error is ApiException ? error.statusCode : null;
    // Каталог не загрузился, значит адресов оплаты в нём нет; единственный
    // оставшийся живой источник — тот, что панель прислала с состоянием
    // доступа подписки. Выдуманного адреса здесь нет и тут.
    final pay = ref.read(subscriptionAccessProvider)?.pay;

    if (error is ApiNotAvailableException) {
      return ScreenEmpty(
        glyph: Lucide.layers,
        title: 'Панель не подключена',
        message:
            'Тарифы публикует оператор, а приложение сейчас работает с вашей '
            'собственной подпиской. Подключите панель — и витрина появится.',
        actionLabel: 'Подключить',
        onAction: () => context.go(AppRoute.connect),
      );
    }
    if (code == 404) {
      return ScreenEmpty(
        glyph: Lucide.creditCard,
        title: 'Витрина недоступна',
        message:
            'Панель этого оператора ещё не умеет отдавать тарифы приложению. '
            'Покупка и продление работают там, где вы оформляли подписку.',
        actionLabel: (pay == null || pay.isEmpty) ? null : 'Открыть Telegram',
        onAction: (pay == null || pay.isEmpty)
            ? null
            : () => unawaited(openTelegramPay(context, pay)),
      );
    }
    if (code == 401) {
      return const InlineBanner(
        tone: BannerTone.warning,
        text: 'Сессия устарела. Войдите заново, и тарифы загрузятся.',
      );
    }
    return InlineError(
      message: 'Не удалось загрузить тарифы',
      onRetry: () => ref.invalidate(planCatalogProvider),
    );
  }

  /// «Сейчас: Free · 200 МБ в день · 1 устройство» — из ЖИВОЙ подписки, а не из
  /// каталога: у человека может быть тариф, который оператор уже снял с витрины.
  String _currentLine(List<SubPlan> subs) {
    final sub = subs.firstWhere(
      (s) => s.isActive,
      orElse: () => subs.first,
    );
    final parts = <String>[sub.name];
    final limit = sub.access.limitBytes;
    if (sub.kind == SubKind.free && limit > 0 && sub.access.period == 'day') {
      parts.add('${formatBytesRu(limit)} в день');
    } else {
      parts.add(sub.meta);
    }
    parts.add('${sub.devLimit} ${_devWord(sub.devLimit)}');
    final expires = sub.expiresLabel;
    if (expires != null) parts.add(expires);
    return 'Сейчас: ${parts.join(' · ')}';
  }

  String? _activeName(List<SubPlan>? subs) {
    if (subs == null || subs.isEmpty) return null;
    for (final s in subs) {
      if (s.isActive) return s.name;
    }
    return subs.first.name;
  }

  /// Закрытие: назад, если есть куда, иначе домой. Экран открывают и из шелла
  /// (профиль, серверы), и с экрана подключения, где стека под ним нет.
  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.home);
    }
  }
}

/// Карточка одного тарифа: чем он отличается, сколько стоит и на какой срок.
///
/// Публичный класс намеренно (как [SubscriptionCard] в профиле): тесты
/// монтируют её отдельно, без сетевого стека, и проверяют на РЕНДЕРЕ, что у
/// непродаваемого тарифа нет кнопки покупки, а у бесплатного — нет цены.
class PlanCard extends ConsumerStatefulWidget {
  final CatalogPlan plan;
  final PlanCatalog catalog;
  final bool isCurrent;

  const PlanCard({
    required this.plan,
    required this.catalog,
    this.isCurrent = false,
    super.key,
  });

  @override
  ConsumerState<PlanCard> createState() => _PlanCardState();
}

class _PlanCardState extends ConsumerState<PlanCard> {
  /// Выбранный срок. Предвыбран самый дешёвый: человек, открывший экран, видит
  /// цену входа, а не самую дорогую строку.
  int? _durationId;

  @override
  void initState() {
    super.initState();
    _durationId = widget.plan.cheapest?.id;
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final plan = widget.plan;
    final matches = plan.durations.where((d) => d.id == _durationId);
    final selected = matches.isEmpty ? null : matches.first;

    return Container(
      margin: const EdgeInsets.only(bottom: AppSpace.s3),
      padding: const EdgeInsets.all(AppSpace.s4),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r16,
        border: Border.all(
          color: widget.isCurrent ? c.borderStrong : c.borderSubtle,
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              IBox(_glyphOf(plan), size: 34),
              const SizedBox(width: AppSpace.s3),
              Expanded(
                child: Text(
                  plan.name,
                  overflow: TextOverflow.ellipsis,
                  style: AppType.titleMd.copyWith(color: c.textHi),
                ),
              ),
              if (widget.isCurrent) const Tag('ваш тариф', ok: true),
            ],
          ),
          const SizedBox(height: AppSpace.s3),
          Text(
            '${plan.trafficLabel} · ${plan.deviceLabel}',
            style: AppType.bodyMd.copyWith(color: c.textHi),
          ),
          if (plan.serverCount > 0) ...[
            const SizedBox(height: 2),
            Text(
              _fleetLine(plan),
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
          ],
          if (plan.description.trim().isNotEmpty) ...[
            const SizedBox(height: AppSpace.s2),
            Text(
              plan.description.trim(),
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
          ],
          const SizedBox(height: AppSpace.s4),
          if (plan.purchasable) ...[
            if (plan.durations.length > 1) ...[
              _durations(context, plan),
              const SizedBox(height: AppSpace.s3),
            ],
            FilledButton(
              onPressed: selected == null ? null : () => _open(selected),
              child: Text(
                selected == null
                    ? 'Выберите срок'
                    : 'Продолжить · '
                          '${formatMoneyMinor(selected.priceMinor, widget.catalog.currency)}',
              ),
            ),
          ] else
            Text(
              _whyNotSold(plan),
              style: AppType.bodySm.copyWith(color: c.textLow),
            ),
        ],
      ),
    );
  }

  /// Сегменты сроков. Не выпадающий список: сроков у тарифа два-три, и цена
  /// каждого должна быть видна без нажатия — иначе выбор делается вслепую.
  Widget _durations(BuildContext context, CatalogPlan plan) {
    final c = context.c;
    return Wrap(
      spacing: AppSpace.s2,
      runSpacing: AppSpace.s2,
      children: [
        for (final d in plan.durations)
          Material(
            color: d.id == _durationId ? c.surface2 : c.surfaceInset,
            borderRadius: AppRadius.r12,
            child: InkWell(
              borderRadius: AppRadius.r12,
              onTap: () => setState(() => _durationId = d.id),
              child: Container(
                padding: const EdgeInsets.symmetric(
                  horizontal: AppSpace.s4,
                  vertical: AppSpace.s3,
                ),
                decoration: BoxDecoration(
                  borderRadius: AppRadius.r12,
                  border: Border.all(
                    color: d.id == _durationId
                        ? c.borderStrong
                        : c.borderSubtle,
                  ),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      d.daysLabel,
                      style: AppType.bodySm.copyWith(color: c.textMed),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      formatMoneyMinor(d.priceMinor, widget.catalog.currency),
                      style: AppType.monoSm.copyWith(color: c.textHi),
                    ),
                  ],
                ),
              ),
            ),
          ),
      ],
    );
  }

  void _open(PlanDurationOffer duration) => unawaited(
    showPaymentSheet(
      context,
      plan: widget.plan,
      duration: duration,
      catalog: widget.catalog,
    ),
  );

  /// Почему кнопки нет. Три разные причины, и путать их нельзя: бесплатный
  /// тариф не продаётся никогда, пробный выдаёт оператор, а платный без срока
  /// — это незаконченная настройка витрины.
  String _whyNotSold(CatalogPlan plan) {
    if (plan.isFree) {
      return 'Бесплатный тариф — его не покупают, он выдаётся сам.';
    }
    if (plan.isTrial) {
      return 'Пробный тариф. Его выдаёт оператор, купить нельзя.';
    }
    return 'Сейчас не продаётся: оператор не назначил этому тарифу ни одного '
        'срока. Спросите его, если ждали здесь цену.';
  }

  String _fleetLine(CatalogPlan plan) {
    final countries = plan.countries.length;
    if (countries <= 0) return '${plan.serverCount} серверов';
    return '${plan.serverCount} серверов в $countries '
        '${_countryWord(countries)}';
  }

  String _glyphOf(CatalogPlan plan) {
    if (plan.isFree) return Lucide.gift;
    if (plan.trafficLimitGb <= 0) return Lucide.infinity;
    return Lucide.layers;
  }
}

String _devWord(int n) {
  final mod100 = n % 100;
  if (mod100 >= 11 && mod100 <= 14) return 'устройств';
  return switch (n % 10) {
    1 => 'устройство',
    2 || 3 || 4 => 'устройства',
    _ => 'устройств',
  };
}

String _countryWord(int n) {
  final mod100 = n % 100;
  if (mod100 >= 11 && mod100 <= 14) return 'странах';
  return n % 10 == 1 ? 'стране' : 'странах';
}

/// Лист «Способ оплаты»: последний шаг покупки и единственное место, где
/// приложение решает, КУДА уводить человека за деньгами.
///
/// Почему лист есть даже с одним пунктом. У этого оператора способ ровно один
/// — Telegram Stars, — и соблазн «сразу открыть Telegram» велик. Но покупка это
/// единственное действие в приложении, после которого человек уходит в другое
/// приложение и возвращается с потраченными деньгами. Молчаливый прыжок туда
/// неотличим от сбоя: экран мигнул, открылся мессенджер, что именно куплено —
/// неизвестно. Поэтому пункт показывается, даже когда он один, и написано, что
/// произойдёт по нажатию.
///
/// Второе решение файла: `tg://` НЕ идёт через общий [openExternal]. Его
/// allowlist (см. `data/safe_url.dart`) пропускает только https — и правильно
/// делает: через него открываются ссылки, пришедшие с сервера, а `launchUrl`
/// без проверки схемы запускает и `javascript:`, и любое стороннее приложение.
/// Расширять общий список ради одной кнопки значило бы ослабить проверку для
/// ВСЕХ ссылок приложения, поэтому нативная форма проверяется здесь и
/// принимается ровно в одном виде: `tg://resolve?...`. Не открылось (Telegram не
/// установлен) — тихо уходим на https-форму, которую откроет браузер.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/plan_catalog.dart';
import 'package:caramba_client/data/models/subscription.dart' show AccessPay;
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart'
    show subscriptionAccessProvider;
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/subscription_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Псевдо-провайдер «пусть решает панель». Наружу не уходит: в `POST /purchase`
/// поле `provider` при нём просто не отправляется.
const String _anyProvider = '__any__';

/// Открывает лист выбора способа оплаты для одного срока тарифа.
Future<void> showPaymentSheet(
  BuildContext context, {
  required CatalogPlan plan,
  required PlanDurationOffer duration,
  required PlanCatalog catalog,
}) => showModalBottomSheet<void>(
  context: context,
  backgroundColor: context.c.surface1,
  isScrollControlled: true,
  showDragHandle: true,
  builder: (ctx) =>
      PaymentSheet(plan: plan, duration: duration, catalog: catalog),
);

/// Содержимое листа. Публичный класс намеренно: тесты монтируют его напрямую,
/// без модального слоя, и проверяют РЕНДЕР состояний (403, отсутствие адреса,
/// ожидание оплаты) — то, что руками воспроизводится только на живой панели.
class PaymentSheet extends ConsumerStatefulWidget {
  final CatalogPlan plan;
  final PlanDurationOffer duration;
  final PlanCatalog catalog;

  const PaymentSheet({
    required this.plan,
    required this.duration,
    required this.catalog,
    super.key,
  });

  @override
  ConsumerState<PaymentSheet> createState() => _PaymentSheetState();
}

class _PaymentSheetState extends ConsumerState<PaymentSheet> {
  /// Идёт запрос `POST /purchase` по этому провайдеру: строка блокируется, а не
  /// весь лист, иначе непонятно, что именно нажато.
  String? _busyProvider;

  /// Оплата из приложения запрещена лицензией оператора (403). Отдельное поле,
  /// а не текст ошибки: это не сбой, а конфигурация, и говорить о ней надо
  /// иначе.
  bool _billingDisabled = false;

  /// Ошибка последней попытки, человеческим текстом.
  String? _error;

  /// Открытая сессия оплаты: сюда человек возвращается из браузера.
  PurchaseCheckout? _session;
  PurchaseStatus? _sessionStatus;
  bool _checkingStatus = false;

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final methodsAsync = ref.watch(paymentMethodsProvider(widget.duration.id));

    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(
          AppSpace.s5,
          AppSpace.s1,
          AppSpace.s5,
          AppSpace.s6,
        ),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                'Способ оплаты',
                style: AppType.titleLg.copyWith(color: c.textHi),
              ),
              const SizedBox(height: AppSpace.s1),
              Text(
                '${widget.plan.name} · ${widget.duration.daysLabel} · '
                '${formatMoneyMinor(widget.duration.priceMinor, widget.catalog.currency)}',
                style: AppType.bodyMd.copyWith(color: c.textMed),
              ),
              const SizedBox(height: AppSpace.s4),
              if (_session != null)
                _waiting(context)
              else if (_billingDisabled)
                _disabled(context)
              else ...[
                if (_error != null) ...[
                  InlineBanner(tone: BannerTone.warning, text: _error!),
                  const SizedBox(height: AppSpace.s3),
                ],
                methodsAsync.when(
                  data: (methods) => _methods(context, methods),
                  loading: () => const InlineLoading(),
                  // Старая панель без `/payment-methods` (404) — не повод
                  // прятать оплату: что известно из каталога, то и предлагаем.
                  error: (e, _) => _methods(context, _fallbackMethods(e)),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }

  // ---------------------------------------------------------------------------
  // Состояния листа.
  // ---------------------------------------------------------------------------

  Widget _methods(BuildContext context, List<PaymentMethod> methods) {
    if (methods.isEmpty) {
      final pay = _payTargets;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          InlineBanner(
            tone: BannerTone.warning,
            glyph: Lucide.creditCard,
            text: pay.isEmpty
                ? 'Оператор не подключил ни одного способа оплаты и не '
                      'опубликовал адрес, где платят. Он есть там, где вы '
                      'оформляли подписку.'
                : 'Оплата внутри приложения у этого оператора не включена. '
                      'Покупка оформляется в Telegram.',
          ),
          if (!pay.isEmpty) ...[
            const SizedBox(height: AppSpace.s4),
            GhostButton(
              label: 'Открыть Telegram',
              icon: Lucide.send,
              onPressed: () => _openTelegram(pay),
            ),
          ],
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final m in methods)
          ListItemCard(
            leading: IBox(_glyphOf(m), size: 34),
            title: m.label,
            subtitle: _subtitleOf(m),
            trailing: _busyProvider == m.id
                ? const SizedBox(
                    width: 18,
                    height: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : LucideIcon(
                    m.checkout == PayCheckout.telegram
                        ? Lucide.externalLink
                        : Lucide.chevronRight,
                    color: context.c.textMed,
                    size: 18,
                  ),
            onTap: _busyProvider != null ? null : () => _pick(m),
          ),
      ],
    );
  }

  Widget _disabled(BuildContext context) {
    final pay = _payTargets;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const InlineBanner(
          tone: BannerTone.warning,
          glyph: Lucide.creditCard,
          text:
              'Оператор не включил оплату внутри приложения. Это не сбой: '
              'покупка у него оформляется в Telegram, и там она работает.',
        ),
        if (!pay.isEmpty) ...[
          const SizedBox(height: AppSpace.s4),
          GhostButton(
            label: 'Открыть Telegram',
            icon: Lucide.send,
            onPressed: () => _openTelegram(pay),
          ),
        ] else ...[
          const SizedBox(height: AppSpace.s3),
          Text(
            'Адрес для оплаты оператор не опубликовал. Он есть там, где вы '
            'оформляли подписку.',
            style: AppType.bodySm.copyWith(color: context.c.textLow),
          ),
        ],
      ],
    );
  }

  /// Человек ушёл платить во внешний браузер. Приложение об оплате не узнаёт
  /// ниоткуда — вебхук провайдера приходит на панель, — поэтому единственный
  /// честный текст здесь «спросим, когда нажмёте».
  Widget _waiting(BuildContext context) {
    final c = context.c;
    final s = _sessionStatus;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        RowsGroup(
          children: [
            CRow(
              icon: Lucide.clock,
              label: 'Состояние оплаты',
              value: s?.label ?? 'Ожидает оплаты',
              valueColor: (s?.isPaid ?? false) ? c.success : c.textHi,
            ),
          ],
        ),
        const SizedBox(height: AppSpace.s3),
        Text(
          (s?.isPaid ?? false)
              ? 'Оплата получена. Подписка обновлена — закройте это окно.'
              : 'Счёт выставлен. Оплатите его на открывшейся странице и '
                    'вернитесь сюда: приложение не узнаёт об оплате само, '
                    'состояние нужно спросить.',
          style: AppType.bodySm.copyWith(color: c.textMed),
        ),
        const SizedBox(height: AppSpace.s4),
        if (!(s?.isPaid ?? false))
          GhostButton(
            label: _checkingStatus ? 'Проверяем…' : 'Обновить состояние',
            icon: Lucide.refresh,
            onPressed: _checkingStatus ? null : _refreshStatus,
          ),
        const SizedBox(height: AppSpace.s2),
        QuietButton(
          label: 'Закрыть',
          color: c.textMed,
          onPressed: () => Navigator.of(context).maybePop(),
        ),
      ],
    );
  }

  // ---------------------------------------------------------------------------
  // Действия.
  // ---------------------------------------------------------------------------

  Future<void> _pick(PaymentMethod m) async {
    if (m.checkout == PayCheckout.telegram) {
      // У способа может быть свой адрес (панель кладёт его в `url`); если нет —
      // общий адрес оплаты оператора.
      await _openTelegram(
        m.url.isEmpty ? _payTargets : AccessPay(url: m.url, botUrl: m.url),
      );
      return;
    }

    setState(() {
      _busyProvider = m.id;
      _error = null;
    });
    // Адрес, который надо открыть, вычисляется внутри try, а ОТКРЫВАЕТСЯ после
    // него. Иначе отказ системного открывалки («нет браузера») попал бы в тот
    // же catch, что и отказ панели, и человек прочитал бы «не удалось создать
    // счёт» о счёте, который уже создан.
    String? openAfter;
    try {
      final checkout = await ref
          .read(apiClientProvider)
          .purchase(
            durationId: widget.duration.id,
            // Пункт-заглушка старой панели имени провайдера не несёт: пусть
            // панель выберет сама, вместо того чтобы мы угадали и получили
            // «Unknown or disabled payment provider».
            provider: m.id == _anyProvider ? null : m.id,
          );

      // Оплата с баланса: панель уже продлила подписку, открывать нечего.
      if (checkout.fulfilled || checkout.kind == PayUrlKind.balanceSuccess) {
        ref.invalidate(subscriptionsProvider);
        ref.invalidate(subscriptionProvider);
        if (!mounted) return;
        // Спиннер снимаем ДО закрытия: лист закрывается не всегда (его могли
        // смонтировать без модального маршрута), и оставленная крутилка
        // выглядела бы как незавершённая оплата.
        setState(() => _busyProvider = null);
        Navigator.of(context).maybePop();
        showCarambaToast(context, 'Оплачено с баланса. Подписка продлена');
        return;
      }

      final url = checkout.absoluteUrl(ref.read(apiClientProvider).panelOrigin);
      if (url == null) {
        setState(() {
          _busyProvider = null;
          _error =
              'Панель не дала адрес оплаты для этого способа. '
              'Выберите другой или оформите покупку в Telegram.';
        });
        return;
      }
      if (!mounted) return;
      setState(() {
        _busyProvider = null;
        _session = checkout;
        _sessionStatus = null;
      });
      openAfter = url;
    } on ApiException catch (e) {
      if (!mounted) return;
      setState(() {
        _busyProvider = null;
        // 403 — это `end_user_billing` в лицензии оператора, а не поломка.
        _billingDisabled = e.statusCode == 403;
        _error = _billingDisabled ? null : _humanPurchaseError(e);
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _busyProvider = null;
        _error = 'Не удалось создать счёт. Проверьте связь и повторите.';
      });
    }
    if (openAfter != null && mounted) {
      await openExternal(context, openAfter);
    }
  }

  /// Отказ панели человеческими словами.
  ///
  /// Панель отвечает на ошибки ПЛЕЙН-ТЕКСТОМ ПО-АНГЛИЙСКИ («Invalid duration
  /// ID», «Unknown or disabled payment provider»). Показать это как есть —
  /// ровно та поломка, из-за которой в приложении когда-то появилась стена
  /// «transport: код состояния 403»: строка верна для того, кто писал сервер, и
  /// бесполезна тому, кто нажал кнопку. Переводим по коду, а не по тексту:
  /// текст меняется вместе с версией панели, код — нет.
  String _humanPurchaseError(ApiException e) => switch (e.statusCode) {
    400 =>
      'Панель не приняла этот тариф или способ оплаты. Обновите витрину и '
          'попробуйте снова.',
    401 => 'Сессия устарела. Войдите заново и повторите покупку.',
    404 => 'Этот способ оплаты у оператора больше не подключён.',
    _ => 'Оператор не смог выставить счёт. Попробуйте позже или оформите '
        'покупку в Telegram.',
  };

  Future<void> _refreshStatus() async {
    final id = _session?.sessionId ?? '';
    if (id.isEmpty) {
      setState(
        () => _error = 'Панель не назвала номер счёта — состояние не спросить.',
      );
      return;
    }
    setState(() => _checkingStatus = true);
    try {
      final status = await ref.read(apiClientProvider).getPurchaseStatus(id);
      if (!mounted) return;
      setState(() {
        _checkingStatus = false;
        _sessionStatus = status;
      });
      if (status.isPaid) {
        ref.invalidate(subscriptionsProvider);
        ref.invalidate(subscriptionProvider);
      }
    } catch (_) {
      if (!mounted) return;
      setState(() {
        _checkingStatus = false;
        _error = 'Не удалось узнать состояние оплаты. Повторите позже.';
      });
    }
  }

  /// Адреса оплаты оператора: сначала те, что назвал каталог, иначе те, что
  /// пришли с состоянием доступа подписки. Выдуманных здесь нет ни одного.
  AccessPay get _payTargets {
    if (!widget.catalog.pay.isEmpty) return widget.catalog.pay;
    final fromAccess = ref.read(subscriptionAccessProvider)?.pay;
    return fromAccess ?? const AccessPay();
  }

  Future<void> _openTelegram(AccessPay pay) async {
    if (!mounted) return;
    final opened = await openTelegramPay(context, pay);
    if (!opened && mounted) {
      setState(
        () => _error =
            'Оператор не опубликовал адрес для оплаты. Он есть там, где вы '
            'оформляли подписку.',
      );
    }
  }

  /// Чем заменить список способов, когда `/payment-methods` не ответил.
  ///
  /// 404 значит «панель старее этого маршрута». Тогда единственное, что мы про
  /// оплату знаем, — разрешена ли она лицензией, и предлагаем один пункт без
  /// имени провайдера: `POST /purchase` без поля `provider` сам возьмёт первый
  /// доступный (`app_billing.rs`), и это честнее, чем выдумать имя.
  ///
  /// Любая другая ошибка — это сеть или отказ, и выдавать её за «способов нет»
  /// нельзя: список пуст, а причину показывает баннер пустого состояния.
  List<PaymentMethod> _fallbackMethods(Object error) {
    final is404 = error is ApiException && error.statusCode == 404;
    if (!is404 || !widget.catalog.inAppPurchase) return const <PaymentMethod>[];
    return const <PaymentMethod>[
      PaymentMethod(id: _anyProvider, label: 'Оплатить'),
    ];
  }

  String _glyphOf(PaymentMethod m) => switch (m.checkout) {
    PayCheckout.telegram => Lucide.send,
    PayCheckout.inApp => m.isBalance ? Lucide.wallet : Lucide.creditCard,
  };

  String? _subtitleOf(PaymentMethod m) {
    final parts = <String>[
      if (m.amountMinor != null)
        formatMoneyMinor(
          m.amountMinor!,
          m.currency.isEmpty ? widget.catalog.currency : m.currency,
        ),
      if (m.checkout == PayCheckout.telegram)
        'откроется Telegram'
      else if (m.isBalance)
        'спишется с баланса сразу'
      else
        'откроется страница оплаты',
    ];
    return parts.isEmpty ? null : parts.join(' · ');
  }
}

/// Открывает оплату в Telegram: сначала нативной ссылкой, затем https.
///
/// Возвращает `false`, только если открывать было НЕЧЕГО (оператор не
/// опубликовал ни одного адреса) — вызывающий обязан сказать об этом словами,
/// а не подставить чужого бота.
Future<bool> openTelegramPay(BuildContext context, AccessPay pay) async {
  final native = _telegramNativeUri(pay.native);
  if (native != null) {
    try {
      if (await launchUrl(native, mode: LaunchMode.externalApplication)) {
        return true;
      }
    } catch (_) {
      // Telegram не установлен — это штатный случай, а не ошибка: уходим на
      // https-форму, её откроет браузер.
    }
  }
  final web = pay.link;
  if (web == null || web.isEmpty) return false;
  if (!context.mounted) return true;
  await openExternal(context, web);
  return true;
}

/// Единственная нативная схема, которую мы соглашаемся запустить: `tg://resolve`.
///
/// Проверка узкая намеренно. Строка приходит с сервера, а `launchUrl` без
/// разбора схемы отдаёт её любому зарегистрированному обработчику в системе.
/// Общий [csmSafeExternalUri] такую ссылку правильно не пропускает, и ослаблять
/// его ради одной кнопки нельзя — поэтому исключение живёт здесь и ровно одно.
Uri? _telegramNativeUri(String? raw) {
  if (raw == null || raw.isEmpty) return null;
  final uri = Uri.tryParse(raw.trim());
  if (uri == null) return null;
  if (uri.scheme.toLowerCase() != 'tg') return null;
  if (uri.host.toLowerCase() != 'resolve') return null;
  return uri;
}

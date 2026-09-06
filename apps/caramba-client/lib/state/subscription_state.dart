import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/plan_catalog.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/state/providers.dart';

/// Активная подписка пользователя из `GET /api/v2/app/subscription`.
///
/// Содержит [Subscription.subscriptionUuid] (UUID для config-URL) и
/// [Subscription.clashUrl] — mihomo/clash-конфиг, который тянет Go-ядро при
/// подключении. Перезапрашивается через `ref.invalidate(subscriptionProvider)`.
///
/// Кэш держится живым (`keepAlive`) после успешной загрузки: auth-слой
/// прогревает его сразу после логина, и значение должно дожить до момента,
/// когда home/connect его прочитают, без повторного запроса.
final subscriptionProvider = FutureProvider<Subscription>((ref) async {
  final api = ref.watch(apiClientProvider);
  final sub = await api.getSubscription();
  ref.keepAlive();
  return sub;
});

/// Витрина тарифов оператора (`GET /api/v2/app/plans`).
///
/// `autoDispose`, а не `keepAlive`: каталог смотрят редко, а между заходами
/// оператор может завести срок или снять его с продажи — показать вчерашнюю
/// цену хуже, чем секунду подождать. Ошибка НЕ подменяется пустым каталогом:
/// «панель старее этого маршрута» (404) и «у оператора нет тарифов» — разные
/// вещи, и экран обязан говорить о них по-разному.
final planCatalogProvider = FutureProvider.autoDispose<PlanCatalog>((
  ref,
) async {
  final api = ref.watch(apiClientProvider);
  return api.getPlans();
});

/// Способы оплаты для конкретного срока (`GET /api/v2/app/payment-methods`).
///
/// Ключ — `plan_durations.id`: у провайдера бывает своя цена на срок
/// (per-provider override в `catalog_service`), поэтому список нельзя загрузить
/// один раз на весь каталог.
final paymentMethodsProvider = FutureProvider.autoDispose
    .family<List<PaymentMethod>, int>((ref, durationId) async {
      final api = ref.watch(apiClientProvider);
      return api.getPaymentMethods(durationId: durationId);
    });

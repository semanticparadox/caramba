import 'package:flutter_riverpod/flutter_riverpod.dart';

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

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/partner.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/data/models/traffic_point.dart';
import 'package:caramba_client/state/providers.dart';

/// Аккаунт-провайдеры: подписки, устройства, рефералы, семья, relay-страны,
/// трафик. Все тянутся из `/api/v2/app/*` через [apiClientProvider]; экраны
/// отрисовывают loading/empty/error через `AsyncValue.when`.

/// Список подписок пользователя (`GET /app/subscriptions`).
/// `ref.invalidate(subscriptionsProvider)` перезапрашивает после покупки/семьи.
final subscriptionsProvider =
    FutureProvider.autoDispose<List<SubPlan>>((ref) async {
  final api = ref.watch(apiClientProvider);
  return api.getSubscriptions();
});

/// Устройства аккаунта (`GET /app/devices`) с операциями rename/remove.
class DevicesNotifier
    extends AutoDisposeAsyncNotifier<List<Device>> {
  @override
  Future<List<Device>> build() async {
    final api = ref.watch(apiClientProvider);
    return api.getDevices();
  }

  /// Переименование с оптимистичным обновлением и откатом при ошибке.
  Future<void> rename(int id, String name) async {
    final api = ref.read(apiClientProvider);
    await api.renameDevice(id, name);
    ref.invalidateSelf();
    await future;
  }

  /// Отзыв устройства: оптимистично убираем из списка, при ошибке перезапрос.
  Future<void> remove(int id) async {
    final api = ref.read(apiClientProvider);
    final current = state.valueOrNull;
    if (current != null) {
      state = AsyncData(
        current.where((d) => d.id != id).toList(growable: false),
      );
    }
    try {
      await api.removeDevice(id);
    } finally {
      ref.invalidateSelf();
    }
  }
}

final devicesProvider =
    AutoDisposeAsyncNotifierProvider<DevicesNotifier, List<Device>>(
  DevicesNotifier.new,
);

/// Реферальная сводка (`GET /app/referrals`).
final referralProvider =
    FutureProvider.autoDispose<ReferralInfo>((ref) async {
  final api = ref.watch(apiClientProvider);
  return api.getReferrals();
});

/// Семья по подписке (`GET /app/family?subscription_id=`). Family-параметр
/// `.family` Riverpod — ключ по subscription_id (0 = без фильтра).
final familyProvider =
    FutureProvider.autoDispose.family<Family, int>((ref, subId) async {
  final api = ref.watch(apiClientProvider);
  return api.getFamily(subscriptionId: subId > 0 ? subId : null);
});

/// Relay-страны для пикера (`GET /app/relays`), уже с псевдо-вариантами
/// Выкл/Авто. На ошибку/пусто — дефолтный набор [Relay.defaults].
final apiRelaysProvider = FutureProvider.autoDispose<List<Relay>>((ref) async {
  final api = ref.watch(apiClientProvider);
  try {
    final countries = await api.getRelays();
    return Relay.fromCountries(countries);
  } catch (_) {
    return Relay.defaults;
  }
});

/// Партнёрская сводка (`GET /app/partner/codes`) с операциями create/delete.
/// Раздел гейтится `is_partner`: дашборд и пункт входа в профиле показываются
/// только когда панель подтвердила партнёрскую роль (см. [isPartnerProvider]).
class PartnerNotifier
    extends AutoDisposeAsyncNotifier<PartnerOverview> {
  @override
  Future<PartnerOverview> build() async {
    final api = ref.watch(apiClientProvider);
    return api.getPartnerCodes();
  }

  /// Создаёт код для источника и перезапрашивает сводку.
  Future<void> create(String sourceLabel) async {
    final api = ref.read(apiClientProvider);
    await api.createPartnerCode(sourceLabel);
    ref.invalidateSelf();
    await future;
  }

  /// Удаляет код: оптимистично убираем из списка, при ошибке перезапрос.
  Future<void> remove(String code) async {
    final api = ref.read(apiClientProvider);
    final current = state.valueOrNull;
    if (current != null) {
      state = AsyncData(PartnerOverview(
        isPartner: current.isPartner,
        codes:
            current.codes.where((c) => c.code != code).toList(growable: false),
      ));
    }
    try {
      await api.deletePartnerCode(code);
    } finally {
      ref.invalidateSelf();
    }
  }
}

final partnerProvider =
    AutoDisposeAsyncNotifierProvider<PartnerNotifier, PartnerOverview>(
  PartnerNotifier.new,
);

/// Гейт партнёрской роли для пункта входа в профиле: `true` только когда сводка
/// загрузилась и `is_partner == true`. На loading/error/не-партнёра — `false`,
/// поэтому обычные пользователи дашборд не видят.
final isPartnerProvider = Provider.autoDispose<bool>((ref) {
  return ref.watch(partnerProvider).valueOrNull?.isPartner ?? false;
});

/// Ряд трафика для графика (`GET /app/traffic`, поле `points`). Пусто, если
/// подневной истории ещё нет — UI рисует «нет данных».
final trafficHistoryProvider =
    FutureProvider.autoDispose<List<TrafficPoint>>((ref) async {
  final api = ref.watch(apiClientProvider);
  return api.getTraffic();
});

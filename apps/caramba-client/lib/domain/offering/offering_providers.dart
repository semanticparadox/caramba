/// Точка входа слоя: один провайдер, который отвечает «что этот пользователь
/// может выбрать прямо сейчас» одинаково в панельном режиме и на
/// импортированной подписке.
///
/// Экраны читают ЕГО, а не источники: ветвление по режиму жило в каждом экране
/// отдельно и расходилось между ними — именно так «сервер» на одном экране
/// означал узел, а на другом инбаунд.
library;

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_builder.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/servers_state.dart';

/// Шов для подписанного каталога CSM. Возвращает `null` — каталог предложение
/// не ведёт, и профиль разрешается панелью или импортом.
///
/// Тот же шов, что [csmExitCatalogProvider] в exit_inventory_state.dart, и он
/// так же не заполняется здесь: реализация принадлежит слою CSM, который
/// переопределит провайдер, когда каталог научится отдавать инвентарь.
/// Подменять рабочее предложение пустым каталогом — потеря выбора, а не
/// строгость.
final csmOfferingProvider = Provider<Offering?>((ref) => null);

/// Предложение активного профиля.
final offeringProvider = Provider<Offering>((ref) {
  final profile = ref.watch(activeConnectionProfileProvider);
  if (profile == null) {
    return buildEmptyOffering();
  }

  final catalog = ref.watch(csmOfferingProvider);
  if (catalog != null) return catalog;

  if (profile.isRaw) {
    final probe = profile.lastProbe;
    return buildImportedOffering(
      servers: profile.servers,
      latencyByProxy: probe?.latencyMs ?? const <String, int>{},
      selectedProxyName: profile.selectedServerId,
    );
  }

  final async = ref.watch(serversProvider);
  final servers = async.valueOrNull ?? const [];
  // Страны входа приходят отдельным эндпоинтом и приезжают уже с
  // псевдо-вариантами «Выкл»/«Авто» — их сюда пускать нельзя: это элементы
  // управления, а не узлы флота.
  final relayRows = <RelayCountryRow>[
    for (final r in ref.watch(apiRelaysProvider).valueOrNull ?? const <Relay>[])
      if (!r.isOff && !r.isAuto && (r.country ?? r.id) != null)
        RelayCountryRow(
          countryCode: (r.country ?? r.id)!,
          countryName: r.name,
          nodeCount: 0,
        ),
  ];

  return buildPanelOffering(
    servers: servers,
    relayCountries: relayRows,
    selectedExitKey: profile.selectedExitNodeId?.toString(),
    selectedRelayCountry: null,
    loading: async.isLoading,
    error: async.hasError ? async.error : null,
  );
});

/// Список протоколов, отфильтрованный ВЫБРАННЫМ узлом — правило владельца.
///
/// Узел не закреплён — список приезжает с областью
/// [ProtocolScope.wholeFleet], и экран обязан сказать, что это «что бывает во
/// флоте», а не «что применится».
final protocolSlateProvider = Provider<ProtocolSlate>((ref) {
  final offering = ref.watch(offeringProvider);
  return protocolSlateOf(offering, exitKey: offering.selectedExitKey);
});

/// Список протоколов конкретного узла — для экрана, где узел выбирают руками,
/// до того как выбор закреплён.
final protocolSlateForExitProvider = Provider.family<ProtocolSlate, String?>(
  (ref, exitKey) =>
      protocolSlateOf(ref.watch(offeringProvider), exitKey: exitKey),
);

/// Узлы выхода активного предложения (недоступные остаются в списке).
final exitOffersProvider = Provider<List<ExitOffer>>(
  (ref) => ref.watch(offeringProvider).exits,
);

/// Входы активного предложения (недоступные остаются в списке).
final relayOffersProvider = Provider<List<RelayOffer>>(
  (ref) => ref.watch(offeringProvider).relays,
);

/// Девять пресетов ядра с выводом о применимости.
final routePresetOffersProvider = Provider<List<RoutePresetOffer>>(
  (ref) => ref.watch(offeringProvider).routePresets,
);

/// Пять бит возможностей.
final capabilitiesProvider = Provider<Capabilities>(
  (ref) => ref.watch(offeringProvider).capabilities,
);

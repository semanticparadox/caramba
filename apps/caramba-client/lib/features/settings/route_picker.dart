/// Пикер маршрута — один на все экраны.
///
/// Он открывался из двух мест (Home и Настройки) двумя почти одинаковыми
/// кусками кода, и разойтись им было достаточно одной правки: на Home список
/// шёл вообще без карты недоступного, то есть предлагал маршруты, которых
/// оператор не предлагает.
///
/// Здесь же чинится вторая, более важная путаница. Владелец открыл «Маршрут» в
/// поисках выбора входа и увидел «Россия» — потому что маршрут ЗВУЧИТ как
/// страна, хотя он про правила трафика, а не про страну, через которую идёт
/// вход. Поэтому подзаголовок листа прямо разводит эти две вещи, а имена
/// пресетов взяты из реестра ядра целиком («Россия (умный)», а не «Россия»).
///
/// Третье: пресету, которому нужны внешние списки правил, никто не может
/// пообещать, что зеркало их отдаст, — и он приходит из
/// [routePresetOffersProvider] со статусом «неизвестно». Такая строка остаётся
/// выбираемой (остальные правила пресета работают), но помечена причиной.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Открывает лист выбора маршрута и применяет выбор. Возвращает выбранный
/// индекс в `RoutingMode.defaults` (он же `CoreConfig.route`) или `null`.
Future<int?> showRoutePicker(BuildContext context, WidgetRef ref) async {
  final modes = ref.read(routingModesProvider);
  final cfg = ref.read(coreConfigProvider);
  final offers = ref.read(routePresetOffersProvider);
  // Оператор мог не предлагать часть маршрутов: его карта причин строится по
  // тому же списку и потому складывается с нашей по индексу.
  final disabled = Map<int, String>.from(
    ref.read(csmDisabledRoutePresetsProvider),
  );

  RoutePresetOffer? offerFor(int index) {
    for (final o in offers) {
      if (o.legacyIndex == index) return o;
    }
    return null;
  }

  final options = <({String name, String desc, String? icon})>[];
  for (var i = 0; i < modes.length; i++) {
    final m = modes[i];
    final offer = offerFor(i);
    // Пресета нет в зеркале реестра ядра — значит списки разъехались, и это
    // факт, а не повод молча показать строку как обычную.
    if (offer == null) {
      disabled.putIfAbsent(
        i,
        () => 'Этого маршрута нет в реестре ядра этой сборки.',
      );
    } else if (offer.availability.isUnavailable) {
      disabled.putIfAbsent(i, () => offer.availability.message);
    }
    final unverified = offer != null && offer.availability.isUnknown
        ? ' ${offer.availability.message}'
        : '';
    options.add((name: m.name, desc: '${m.desc}$unverified', icon: m.icon));
  }

  final picked = await showPickerSheet(
    context: context,
    title: 'Маршрутизация',
    subtitle:
        'Какие сайты и сервисы идут через VPN, а какие напрямую. Это правила '
        'трафика, а не страна входа — вход выбирается в «Relay (вход)».',
    options: options,
    selected: cfg.route,
    disabled: disabled,
  );
  if (picked == null || !context.mounted) return null;
  CsmSettingsBridge.setRoute(ref, picked);
  showCarambaToast(context, 'Маршрут: ${modes[picked].name}');
  return picked;
}

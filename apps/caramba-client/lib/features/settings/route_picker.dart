/// Лист «Режим» — один на все экраны.
///
/// Он открывался из двух мест (Home и Настройки) двумя почти одинаковыми
/// кусками кода, и разойтись им было достаточно одной правки: на Home список
/// шёл вообще без карты недоступного, то есть предлагал маршруты, которых
/// оператор не предлагает.
///
/// ИМЯ. Лист назывался «Режим для страны», и это имя владелец прочитал как
/// четвёртый выбор страны подряд — после «Сервер», «Relay (вход)» и «Тип
/// подключения». Страна тут вообще не выбирается: пресет отвечает на вопрос
/// «какие сайты идут через VPN, а какие напрямую», а страна в его имени — это
/// страна СПИСКА БЛОКИРОВОК, по которому он сортирует, а не страна узла.
/// Поэтому лист называется просто «Режим», а разведение двух вещей ушло в
/// подпись, где ему и место.
///
/// ПОРЯДОК. Первым стоит «Полный обход» — единственный режим, который не
/// требует от человека знать, где он живёт и что у него блокируют; за ним
/// страновые, за ними узкие (Telegram, стриминг). Порядок ПОКАЗА задан здесь
/// [kRouteDisplayOrder] и намеренно отвязан от порядка хранения:
/// `CoreConfig.route` — это сохранённый ИНДЕКС в [RoutingMode.defaults], и
/// перестановка самого списка молча увела бы живого пользователя на соседний
/// маршрут. Здесь переставляются строки, а не индексы.
///
/// ФЛАГ. У пресета, чей список блокировок принадлежит стране, слева стоит её
/// флаг ([FlagChip]), а не абстрактный щит: «Российский режим» и «Иранский
/// режим» с одинаковым щитом различались только чтением подписи. Флаг и код
/// берутся из зеркала реестра ядра ([kCoreRoutePresets]), а не набираются
/// заново рядом с именем.
///
/// СОБСТВЕННЫЙ ЛИСТ, а не [showPickerSheet]. У общего листа слева стоит ровно
/// [IBox] с Lucide-глифом — его `icon` это SVG-путь, эмодзи туда не кладётся.
/// Дать ему произвольный виджет значило бы править `widgets/ui.dart`, общий
/// для всего приложения, ради одного экрана. Поэтому лист собран здесь, и
/// повторяет геометрию общего один в один (та же [ListItemCard], та же
/// прозрачность 0.45 у выключенных строк).
///
/// ПРАВИЛА ПОКАЗА строк, каждое — из отчёта, а не из вкуса:
///   * пресет, которого нет в зеркале реестра ядра, — виден и выключен: списки
///     разъехались, и это факт, а не повод показать строку обычной;
///   * пресет, которого не предлагает оператор, — виден и выключен с причиной
///     (02-SPEC.md 7.2, 7.9);
///   * пресету, которому нужны внешние списки правил, никто не может
///     пообещать, что зеркало их отдаст, — он приходит из
///     [routePresetOffersProvider] со статусом «неизвестно», остаётся
///     выбираемым (остальные правила пресета работают) и помечен причиной;
///   * «Только блок рекламы» СКРЫТ, пока он не выбран. Он перестал быть
///     режимом: блок рекламы теперь отдельный переключатель в Настройках и
///     работает поверх ЛЮБОГО режима, а эта строка вдобавок уводила весь
///     трафик мимо туннеля — то есть выбор «резать рекламу» молча выключал
///     VPN. Показывать его выключенным среди режимов, как требует 7.9 от
///     недоступных значений, здесь неверно: 7.9 про значение, которого не
///     дают, а это значение вообще не отвечает на вопрос листа. Из
///     [RoutingMode.defaults] он при этом НЕ удалён (индекс), и если он у
///     человека уже выбран — строка показывается выключенной с причиной, где
///     искать переключатель: отнять у человека его текущий выбор нельзя.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_policy_mapping.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Заголовок листа. Экспортирован, чтобы строка, которая его открывает, звалась
/// его именем не по памяти автора: имя строки и имя того, что по ней
/// открывается, уже расходились дважды (см. `route_mode_naming_test.dart`).
const String kRouteModeSheetTitle = 'Режим';

/// Подпись листа: разводит две вещи, которые тут путают.
const String kRouteModeSheetSubtitle =
    'Какие сайты и сервисы идут через VPN, а какие напрямую. Это правила '
    'трафика, а не страна входа — вход выбирается в «Relay (вход)».';

/// Порядок ПОКАЗА строк, идентификаторами [RoutingMode].
///
/// Первый — «Полный обход»: он не требует знать, где человек живёт. Дальше
/// страновые от самого частого к редкому, дальше узкие. `adblock` замыкает
/// список, потому что режимом он быть перестал и виден только тому, у кого он
/// уже выбран.
///
/// Состав обязан совпадать с [RoutingMode.defaults] — иначе режим просто
/// исчезнет с экрана; это фиксирует тест.
const List<String> kRouteDisplayOrder = <String>[
  'global',
  'ru-smart',
  'full',
  'by-smart',
  'ir-smart',
  'cn-smart',
  'telegram-only',
  'streaming',
  kAdBlockRouteId,
];

/// Идентификатор режима, ставшего переключателем.
const String kAdBlockRouteId = 'adblock';

/// Причина, по которой «Только блок рекламы» показан выключенным. Видит её
/// только тот, у кого он уже выбран, — поэтому она говорит, куда идти дальше.
const String kAdBlockRouteReason =
    'Это отдельный переключатель: Настройки → «Блокировать рекламу и '
    'трекеры». Он работает поверх любого режима, а как режим эта строка ещё '
    'и уводит весь трафик мимо VPN.';

/// Одна строка листа: индекс хранения + то, чем её рисовать.
class _RouteRow {
  /// Индекс в [RoutingMode.defaults], он же сохранённый `CoreConfig.route`.
  final int index;
  final RoutingMode mode;
  final String desc;

  /// Причина недоступности; `null` — строка выбираема.
  final String? disabled;

  /// Пресет реестра ядра — источник флага. `null`, если списки разъехались.
  final CoreRoutePreset? preset;

  const _RouteRow({
    required this.index,
    required this.mode,
    required this.desc,
    required this.disabled,
    required this.preset,
  });

  /// Слева: флаг страны списка блокировок, иначе глиф режима.
  ///
  /// [FlagChip] обёрнут в [UnconstrainedBox]: `ListItemCard` кладёт `leading` в
  /// `Row` внутри `IntrinsicHeight`, и для длинного описания (у «Российского
  /// режима» оно на несколько строк) высота этой строки становится большой,
  /// но КОНЕЧНОЙ. `FlagChip` — это `Container` с `alignment: center` и без
  /// собственных width/height, а такой контейнер при loose-но-ограниченных
  /// сверху constraints разворачивается на всю доступную высоту вместо своей
  /// естественной — отсюда узкий «столбик» на всю строку вместо плашки у
  /// начала. `UnconstrainedBox` снимает верхнюю границу и возвращает плашке
  /// её обычный размер независимо от высоты соседней колонки с описанием.
  /// [IBox] тем же не болеет — у него есть свои `width`/`height`, — но плашка
  /// нужна ровно там, где сейчас стоит.
  Widget get leading {
    final p = preset;
    if (p != null && p.countryCode.isNotEmpty) {
      return UnconstrainedBox(
        alignment: Alignment.centerLeft,
        child: FlagChip(flag: p.emoji, code: p.countryCode),
      );
    }
    return IBox(mode.icon);
  }
}

/// Пресет реестра ядра для UI-идентификатора режима.
///
/// Отдельная функция, потому что один идентификатор не совпадает: в UI пресет
/// исторически зовётся `full`, в ядре — `ru-full` (см. [kRoutingPresetWire]).
CoreRoutePreset? routePresetForMode(String uiId) =>
    coreRoutePresetById(kRoutingPresetWire[uiId] ?? uiId);

/// Индексы хранения (`CoreConfig.route`) в порядке ПОКАЗА.
///
/// Публичная и без виджетов намеренно: порядок строк — это то, что владелец
/// просил и что легче всего сломать молча, и проверять его тест обязан без
/// поднятия листа. [selected] нужен потому, что одна строка (`adblock`) видна
/// только тому, у кого она уже выбрана.
List<int> routeDisplayIndexes({
  required List<RoutingMode> modes,
  required int selected,
}) {
  final out = <int>[];
  for (final id in kRouteDisplayOrder) {
    final index = modes.indexWhere((m) => m.id == id);
    // Идентификатора нет в списке хранения — показывать нечего и подставлять
    // соседа нельзя: это увело бы человека на чужой маршрут.
    if (index < 0) continue;
    if (id == kAdBlockRouteId && index != selected) continue;
    out.add(index);
  }
  return out;
}

/// Строки листа в порядке показа.
List<_RouteRow> _rows({
  required List<RoutingMode> modes,
  required int selected,
  required List<RoutePresetOffer> offers,
  required Map<int, String> csmDisabled,
}) {
  RoutePresetOffer? offerFor(int index) {
    for (final o in offers) {
      if (o.legacyIndex == index) return o;
    }
    return null;
  }

  final rows = <_RouteRow>[];
  for (final index in routeDisplayIndexes(modes: modes, selected: selected)) {
    final mode = modes[index];
    final id = mode.id;
    final offer = offerFor(index);
    final String? disabled;
    // Порядок причин — от самой внешней к самой внутренней: слово оператора
    // старше нашего (он мог не предлагать маршрут вовсе), и только потом
    // говорим своё.
    if (csmDisabled.containsKey(index)) {
      disabled = csmDisabled[index];
    } else if (id == kAdBlockRouteId) {
      disabled = kAdBlockRouteReason;
    } else if (offer == null) {
      disabled = 'Этого маршрута нет в реестре ядра этой сборки.';
    } else if (offer.availability.isUnavailable) {
      disabled = offer.availability.message;
    } else {
      disabled = null;
    }

    // «Неизвестно» это не отказ: пресет работает, но часть его правил зависит
    // от зеркала, и умолчать об этом значило бы обещать полный набор.
    final unverified = offer != null && offer.availability.isUnknown
        ? ' ${offer.availability.message}'
        : '';

    rows.add(
      _RouteRow(
        index: index,
        mode: mode,
        desc: '${mode.desc}$unverified',
        disabled: disabled,
        preset: routePresetForMode(id),
      ),
    );
  }
  return rows;
}

/// Открывает лист выбора режима и применяет выбор. Возвращает выбранный
/// индекс в `RoutingMode.defaults` (он же `CoreConfig.route`) или `null`.
Future<int?> showRoutePicker(BuildContext context, WidgetRef ref) async {
  final modes = ref.read(routingModesProvider);
  final cfg = ref.read(coreConfigProvider);
  final rows = _rows(
    modes: modes,
    selected: cfg.route,
    offers: ref.read(routePresetOffersProvider),
    // Оператор мог не предлагать часть маршрутов: его карта причин строится по
    // тому же списку хранения и потому складывается с нашей по индексу.
    csmDisabled: ref.read(csmDisabledRoutePresetsProvider),
  );

  final picked = await _showRouteSheet(
    context: context,
    rows: rows,
    selected: cfg.route,
  );
  if (picked == null || !context.mounted) return null;
  CsmSettingsBridge.setRoute(ref, picked);
  showCarambaToast(context, 'Режим: ${modes[picked].name}');
  return picked;
}

/// Лист выбора. Геометрия повторяет [showPickerSheet] — отличие ровно одно:
/// слева произвольный виджет, чтобы страновой пресет мог показать флаг.
Future<int?> _showRouteSheet({
  required BuildContext context,
  required List<_RouteRow> rows,
  required int selected,
}) {
  final c = context.c;
  return showModalBottomSheet<int>(
    context: context,
    backgroundColor: c.surface1,
    isScrollControlled: true,
    showDragHandle: true,
    builder: (ctx) {
      return SafeArea(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s1,
            AppSpace.s5,
            AppSpace.s6,
          ),
          child: ConstrainedBox(
            constraints: BoxConstraints(
              maxHeight: MediaQuery.sizeOf(ctx).height * 0.78,
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  kRouteModeSheetTitle,
                  style: AppType.titleLg.copyWith(color: c.textHi),
                ),
                const SizedBox(height: AppSpace.s1),
                Text(
                  kRouteModeSheetSubtitle,
                  style: AppType.bodyMd.copyWith(color: c.textMed),
                ),
                const SizedBox(height: AppSpace.s3),
                Flexible(
                  child: ListView.builder(
                    shrinkWrap: true,
                    itemCount: rows.length,
                    itemBuilder: (_, i) {
                      final row = rows[i];
                      final off = row.disabled;
                      return Opacity(
                        opacity: off == null ? 1 : 0.45,
                        child: ListItemCard(
                          leading: row.leading,
                          title: row.mode.name,
                          subtitle: off ?? row.desc,
                          // Галочка стоит и на выключенной строке, если выбрана
                          // именно она: `adblock` показывается ровно в этом
                          // случае, и без галочки человек не нашёл бы, что у
                          // него сейчас включено.
                          selected: row.index == selected,
                          onTap: off == null
                              ? () => Navigator.of(ctx).pop(row.index)
                              : null,
                        ),
                      );
                    },
                  ),
                ),
              ],
            ),
          ),
        ),
      );
    },
  );
}

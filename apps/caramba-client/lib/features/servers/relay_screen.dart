import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Происхождение факта «эту строку можно выбрать всегда» для «Выкл» и «Авто».
///
/// Это не утверждение о флоте, а о кодировщике запроса: `_relay` в
/// state/core_policy_mapping.dart отдаёт для обеих строк пустую строку, и ядро
/// принимает её при любом источнике. Ссылка на конкретное место обязательна —
/// без неё через полгода никто не проверит, правда ли это ещё так.
const Provenance kRelayControlWire = Provenance(
  OfferingSource.coreRegistry,
  'state/core_policy_mapping.dart (CorePolicy.relay)',
);

/// «Выкл» и «Авто» доступны при любом флоте.
const Availability kRelayControlAlwaysTrue = Availability.available(
  kRelayControlWire,
);

/// Сохранённый `CoreConfig.relay`, приведённый к списку РОВНО так, как его
/// приводит кодировщик провода (`_relay` в state/core_policy_mapping.dart).
///
/// Расхождение здесь было по построению, а не по случайности. Экран клампил
/// индекс к `relays.length - 1` и называл ПОСЛЕДНЮЮ строку списка, а кодировщик
/// на том же значении отдаёт пустую строку — «входа не выбрано». У
/// пользователя, чей сохранённый индекс достался от удалённых выдуманных стран
/// (Турция/Казахстан/Финляндия занимали индексы 2..4), экран говорил про вход,
/// которого ядру никто не называл.
///
/// Согласовано в сторону ПРОВОДА: он и есть то, что происходит на самом деле.
/// Индекс вне списка означает «вход не выбран», и экран показывает ту строку,
/// чья кодировка совпадает с уходящей, — «Выкл».
///
/// Возвращает `-1`, если список пуст: называть тогда нечего.
int effectiveRelayIndex(int stored, List<Relay> relays) {
  if (relays.isEmpty) return -1;
  if (stored >= 0 && stored < relays.length) return stored;
  for (var i = 0; i < relays.length; i++) {
    if (relays[i].isOff) return i;
  }
  return 0;
}

/// Relay (вход): «Выкл» / «Авто» / ВХОДЫ, которые называет оператор.
///
/// Владелец: «маршрут нету выбора relay ... там только есть Россия хотя
/// функционально там могут быть любые ноды relay которые настроены в панели».
/// Обе половины этой фразы были правдой об экране. Страны здесь были ВПИСАНЫ в
/// приложение (Турция, Казахстан, Финляндия — их не существовало ни в панели,
/// ни в конфиге), а настоящие релэй-УЗЛЫ не показывались вовсе: `GET /relays`
/// агрегирует их по странам и узлов не называет.
///
/// Теперь список приходит из [relayOffersProvider]: узел там, где панель его
/// называет (`via_relay` у выхода), страна — там, где она сама остановилась на
/// стране, и с этой причиной прямо в строке. Ни одна строка не спрятана.
///
/// Своей формулировки недоступности у экрана нет намеренно: правило живёт в
/// возможности ([Capabilities.relayChaining]), которую считает слой
/// предложения по тому, строит ли генератор оператора цепочку. Вторая копия
/// этого правила в Dart рано или поздно начала бы врать — а сегодня она врала
/// бы уже сейчас: clash-тело `dialer-proxy` не выпускает.
///
/// Возможность цепочки при этом описывает ТОЛЬКО входы оператора. «Выкл» и
/// «Авто» цепочкой не являются: первый просит ядро не строить её вовсе, второй
/// оставляет решение оператору, и оба уходят на провод пустой строкой при любом
/// флоте. Раньше они гасились той же возможностью — и на живом флоте
/// (`chained_in_config: false`) экран приходил целиком мёртвым: «Выкл» стоял
/// приглушённым, с подписью, которая описывала работу самого «Выкл» как причину
/// его недоступности, а галочки не было ни на одной строке, потому что и
/// `selected` был завязан на ту же возможность. Пользователь не видел, что
/// сейчас в силе, и не мог снять вход, которого не выбирал.
///
/// Причина недоступности цепочки живёт теперь под заголовком «Входы
/// оператора» — над строками, к которым она относится, и только над ними.
///
/// Ключ к тому, ЧТО в силе, — [effectiveRelayIndex]: он приводит сохранённый
/// индекс к списку так же, как это делает кодировщик провода.
class RelayScreen extends ConsumerWidget {
  const RelayScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    // Список записи: именно его индекс уходит в `CoreConfig.relay`, поэтому
    // строка выбора обязана уметь показать на него.
    final relays = ref.watch(relaysProvider);
    final offers = ref.watch(relayOffersProvider);
    final chaining = ref.watch(capabilitiesProvider).relayChaining;
    final cfg = ref.watch(coreConfigProvider);
    // Происхождение значения по CSM: вход мог поставить оператор (02-SPEC.md
    // 7.6), и пользователь вправе видеть это до того, как перевыберет.
    final entry = ref.watch(csmSettingsProvider)[CsmSettingKey.relay];
    // Индекс мог быть выбран на дефолтном списке, а панельный список короче.
    // Приведение общее с проводом, а не кламп: см. [effectiveRelayIndex].
    final selected = effectiveRelayIndex(cfg.relay, relays);
    final can = chaining.availability;

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
              'Relay (вход)',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            Text(
              'Через какую машину идёт вход в цепочку. Выход при этом остаётся '
              'тем, который выбран на экране серверов.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            if (entry != null) ...[
              const SizedBox(height: AppSpace.s3),
              InlineBanner(
                tone: entry.userSet
                    ? BannerTone.info
                    : (entry.src == CsmProvenance.operator
                          ? BannerTone.warning
                          : BannerTone.info),
                glyph: Lucide.shield,
                text: entry.userSet
                    ? 'Вход выбрали вы. Оператор не перезапишет его молча: '
                          'на попытку поднимется карточка с вопросом.'
                    : 'Текущее значение поставил '
                          '${csmProvenanceTitle(entry.src)}. Выбрав своё, вы '
                          'закрепите его за собой.',
              ),
            ],
            const SizedBox(height: AppSpace.s4),

            // Две строки, истинные при любом флоте и не называющие ни одной
            // страны: прямое подключение и решение оператора. Возможность
            // цепочки их не касается — они и есть отказ от цепочки и передача
            // решения оператору, и на провод обе уходят пустой строкой всегда.
            for (var i = 0; i < relays.length; i++)
              if (relays[i].isOff || relays[i].isAuto)
                _RelayRow(
                  title: relays[i].name,
                  desc: relays[i].desc,
                  code: null,
                  auto: relays[i].isAuto,
                  availability: kRelayControlAlwaysTrue,
                  selected: i == selected,
                  onTap: () => _apply(context, ref, i, relays),
                ),

            const SectionTitle(
              'Входы оператора',
              padding: EdgeInsets.only(top: AppSpace.s4, bottom: AppSpace.s3),
            ),

            // Причина стоит здесь, а не над всем экраном: она описывает ровно
            // эти строки. Над «Выкл» она была неправдой о них.
            if (!can.isAvailable) ...[
              InlineBanner(
                tone: can.isUnavailable ? BannerTone.warning : BannerTone.info,
                glyph: Lucide.waypoints,
                text: can.message,
              ),
              const SizedBox(height: AppSpace.s3),
            ],

            if (offers.isEmpty)
              ..._fallbackRows(context, ref, relays, selected, can)
            else
              for (final o in offers)
                _offerRow(context, ref, o, relays, selected, can),
          ],
        ),
      ),
    );
  }

  /// Строка входа из предложения. Её недоступность приходит из самого входа
  /// (панель сообщает, строит ли генератор цепочку через него), а общий запрет
  /// пути — из возможности. Первая причина конкретнее, поэтому она и
  /// показывается, когда есть.
  Widget _offerRow(
    BuildContext context,
    WidgetRef ref,
    RelayOffer offer,
    List<Relay> relays,
    int selected,
    Availability can,
  ) {
    final index = _writeIndexOf(relays, offer.countryCode);
    final Availability availability;
    if (!offer.availability.isAvailable) {
      availability = offer.availability;
    } else if (!can.isAvailable) {
      availability = can;
    } else if (index == null) {
      // Панель называет вход у выхода, но в списке `GET /relays` его нет:
      // закрепить его нечем, и молчать об этом нельзя.
      availability = Availability.unavailable(
        OfferingReason.panelReportsRelaysByCountryOnly,
        offer.origin,
        detail: offer.countryName,
      );
    } else {
      availability = offer.availability;
    }

    final parts = <String>[];
    if (offer.panelNodeId != null) {
      parts.add('Узел оператора #${offer.panelNodeId}');
    }
    final reach = offer.reachableFromExitKeys.length;
    if (reach > 0) parts.add('через него выходят узлов: $reach');
    if (offer.panelNodeId == null && !offer.reachability.isAvailable) {
      parts.add(offer.reachability.message);
    }

    return _RelayRow(
      title: offer.label.isEmpty ? offer.countryName : offer.label,
      desc: parts.join(' · '),
      code: offer.countryCode.isEmpty ? null : offer.countryCode,
      auto: false,
      availability: availability,
      // Галочка отвечает на вопрос «что сейчас в силе», а не «что можно
      // выбрать». Недоступная строка, которая при этом записана в настройки, —
      // самый важный случай показать её: иначе стереть чужой вход нечем.
      selected: index != null && index == selected,
      onTap: (availability.isAvailable && index != null)
          ? () => _apply(context, ref, index, relays)
          : null,
    );
  }

  /// Предложение входов не ведёт (профиля нет, каталог их не отдал) — остаются
  /// страны из `GET /relays`. Пустого экрана здесь быть не может: спрятанный
  /// переключатель неотличим от «такой настройки не бывает».
  List<Widget> _fallbackRows(
    BuildContext context,
    WidgetRef ref,
    List<Relay> relays,
    int selected,
    Availability can,
  ) {
    final rows = <Widget>[];
    for (var i = 0; i < relays.length; i++) {
      final r = relays[i];
      if (r.isOff || r.isAuto) continue;
      rows.add(
        _RelayRow(
          title: r.name,
          desc: r.desc,
          code: r.country ?? r.id,
          auto: false,
          availability: can,
          selected: i == selected,
          onTap: can.isAvailable ? () => _apply(context, ref, i, relays) : null,
        ),
      );
    }
    if (rows.isEmpty) {
      rows.add(const InlineEmpty(message: 'Оператор не отдал ни одного входа'));
    }
    return rows;
  }

  int? _writeIndexOf(List<Relay> relays, String countryCode) {
    if (countryCode.isEmpty) return null;
    for (var i = 0; i < relays.length; i++) {
      final code = relays[i].country ?? relays[i].id;
      if (code != null && code.toUpperCase() == countryCode) return i;
    }
    return null;
  }

  void _apply(
    BuildContext context,
    WidgetRef ref,
    int index,
    List<Relay> relays,
  ) {
    // Вход уходит ядру через `CorePolicy.relay` (`?relay_country=` в запросе
    // конфига у панели) и оператору через очередь записи CSM. Панельного
    // закрепления на подписке здесь нет намеренно: пока `PUT
    // /subscriptions/{id}/selection` не задеплоен, вызов дал бы отказ на каждое
    // нажатие, а действующий путь уже работает.
    CsmSettingsBridge.setRelay(ref, index, relays);
    showCarambaToast(context, 'Relay: ${relays[index].name}');
    Future.delayed(const Duration(milliseconds: 300), () {
      if (context.mounted) _close(context);
    });
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go('/home');
    }
  }
}

/// Строка входа. Недоступная рисуется тем же приёмом, что и выключенный вариант
/// в [showPickerSheet]: приглушённая, с ПРИЧИНОЙ вместо описания и без цели для
/// нажатия. Неподтверждённая остаётся нажимаемой, но помеченной: молчание
/// источника это не запрет.
class _RelayRow extends StatelessWidget {
  final String title;
  final String desc;

  /// Код страны для плашки; `null` — строка страны не называет.
  final String? code;

  final bool auto;
  final Availability availability;
  final bool selected;
  final VoidCallback? onTap;

  const _RelayRow({
    required this.title,
    required this.desc,
    required this.code,
    required this.auto,
    required this.availability,
    required this.selected,
    this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final off = availability.isUnavailable;
    final c = code;
    return Opacity(
      opacity: off ? 0.45 : 1,
      child: ListItemCard(
        leading: c != null && c.isNotEmpty
            ? CodeChip(c)
            : IBox(auto ? Lucide.gauge : Lucide.route),
        title: title,
        subtitle: off
            ? availability.message
            : (availability.isUnknown
                  ? '${desc.isEmpty ? '' : '$desc. '}'
                        '${availability.message}'
                  : (desc.isEmpty ? null : desc)),
        selected: selected,
        // Плашка одна: строка узкая, и вторая уводит заголовок за край.
        titleBadges: [
          if (availability.isUnknown)
            const Tag('не проверено')
          else if (auto)
            const Tag('умный', ok: true),
        ],
        onTap: onTap,
      ),
    );
  }
}

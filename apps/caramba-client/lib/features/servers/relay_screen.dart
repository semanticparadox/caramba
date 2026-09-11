import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
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

/// Объяснение простыми словами: что такое вход и зачем он. Стоит над всем
/// экраном, потому что именно «логика непонятна» и была жалобой владельца.
const String kRelayExplanation =
    'Вход это сервер, через который ваш трафик заходит в сеть. Выход это '
    'страна, из которой вы выходите в интернет. Вход нужен там, где прямое '
    'подключение к серверам режут: обычно это ваша страна. Выход при этом '
    'остаётся тем, который выбран на экране серверов.';

/// Подпись к числам пинга у релеев: они не с устройства пользователя.
const String kRelayPingNote =
    'Пинг у релеев здесь по данным панели (время отклика самой машины), а не '
    'замер с вашего устройства: сквозь вход приложение пока мерить не умеет.';

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

/// Строка-релей внутри группы страны.
class RelayNodeRow {
  final int nodeId;
  final String name;
  final String? city;

  /// Пинг по данным панели (RTT машины), мс; `null` — панель не сообщила.
  final int? latencyMs;
  final double? loadPct;

  /// Индекс в списке записи (`CoreConfig.relay`); `null` — записать нечем
  /// (ни узла, ни его страны в `GET /relays` нет).
  final int? writeIndex;

  /// Что уходит панели через `PUT /selection` при выборе этой строки.
  final Relay pin;

  final Availability availability;

  /// Сколько выходов панель назвала идущими через этот вход (`via_relay`).
  final int reachableExits;

  const RelayNodeRow({
    required this.nodeId,
    required this.name,
    required this.city,
    required this.latencyMs,
    required this.loadPct,
    required this.writeIndex,
    required this.pin,
    required this.availability,
    required this.reachableExits,
  });
}

/// Группа «страна входа» с её релеями.
class RelayGroup {
  final String countryCode;
  final String countryName;

  /// Индекс строки-страны в списке записи; `null` — панель страну в
  /// `GET /relays` не назвала (узел известен только по `via_relay` у выхода).
  final int? writeIndex;

  /// Строка-страна для `PUT /selection` («вся страна»).
  final Relay pin;

  final Availability availability;
  final int nodeCount;
  final List<RelayNodeRow> nodes;

  const RelayGroup({
    required this.countryCode,
    required this.countryName,
    required this.writeIndex,
    required this.pin,
    required this.availability,
    required this.nodeCount,
    required this.nodes,
  });
}

/// Собирает группы «страна → релеи» из двух источников, не давая им
/// разойтись: списка записи [relays] (`GET /relays`: страны и их узлы) и
/// предложения [offers] (`via_relay` у выходов: узлы, о которых панель
/// сказала, строится ли через них цепочка).
///
/// Правила:
///   * порядок стран — порядок [relays] (панель сортирует по коду);
///   * узел из `GET /relays` пишется своим индексом; узел, известный только по
///     `via_relay`, пишется индексом СТРАНЫ, а закрепление узла уходит панели
///     отдельно (`node:<id>`);
///   * доступность узла — свидетельство панели по нему (`chained_in_config`),
///     а без свидетельства — общая возможность цепочки [chaining];
///   * страна доступна, если доступен хоть один её узел, иначе наследует
///     [chaining].
/// Чистая функция: проверяется тестом без экрана.
List<RelayGroup> buildRelayGroups(
  List<Relay> relays,
  List<RelayOffer> offers,
  Availability chaining,
) {
  final hopsById = <int, RelayOffer>{
    for (final o in offers)
      if (o.panelNodeId != null) o.panelNodeId!: o,
  };
  final groups = <String, _GroupAcc>{};
  final order = <String>[];

  _GroupAcc groupFor(String cc, String name) {
    final existing = groups[cc];
    if (existing != null) return existing;
    final acc = _GroupAcc(cc, name);
    groups[cc] = acc;
    order.add(cc);
    return acc;
  }

  // Страны и узлы из списка записи.
  for (var i = 0; i < relays.length; i++) {
    final r = relays[i];
    if (r.isOff || r.isAuto) continue;
    final cc = r.countryCode;
    if (cc.isEmpty) continue;
    if (r.isCountry) {
      final g = groupFor(cc, r.name.isNotEmpty ? r.name : countryNameOf(cc));
      g.countryIndex ??= i;
      g.countryRow ??= r;
      g.nodeCount = r.nodeCount > g.nodeCount ? r.nodeCount : g.nodeCount;
    } else if (r.isNode) {
      final g = groupFor(cc, countryNameOf(cc));
      final hop = hopsById[r.nodeId!];
      g.nodes[r.nodeId!] = RelayNodeRow(
        nodeId: r.nodeId!,
        name: r.name,
        city: r.city,
        latencyMs: r.latencyMs,
        loadPct: r.loadPct,
        writeIndex: i,
        pin: r,
        availability: hop?.availability ?? _fromChaining(chaining),
        reachableExits: hop?.reachableFromExitKeys.length ?? 0,
      );
    }
  }

  // Узлы, названные только у выходов (`via_relay`): панель старше их в
  // `GET /relays` не отдаёт. Пишутся индексом страны.
  for (final o in offers) {
    final id = o.panelNodeId;
    if (id == null) continue;
    final cc = normalizeCountryCode(o.countryCode);
    if (cc.isEmpty) continue;
    final g = groupFor(cc, o.countryName);
    if (g.nodes.containsKey(id)) continue;
    g.nodes[id] = RelayNodeRow(
      nodeId: id,
      name: o.label.isNotEmpty ? o.label : o.countryName,
      city: null,
      latencyMs: null,
      loadPct: null,
      writeIndex: g.countryIndex,
      pin: Relay(
        id: 'node:$id',
        name: o.label.isNotEmpty ? o.label : o.countryName,
        desc: '',
        country: cc,
        nodeId: id,
      ),
      availability: g.countryIndex == null
          // Панель называет вход у выхода, но в списке `GET /relays` его
          // нет: закрепить его нечем, и молчать об этом нельзя.
          ? Availability.unavailable(
              OfferingReason.panelReportsRelaysByCountryOnly,
              o.origin,
              detail: o.countryName,
            )
          : o.availability,
      reachableExits: o.reachableFromExitKeys.length,
    );
  }

  return <RelayGroup>[
    for (final cc in order)
      () {
        final g = groups[cc]!;
        final nodes = g.nodes.values.toList(growable: false);
        final Availability availability;
        if (nodes.any((n) => n.availability.isAvailable)) {
          availability = nodes
              .firstWhere((n) => n.availability.isAvailable)
              .availability;
        } else if (g.countryIndex == null) {
          availability = Availability.unavailable(
            OfferingReason.panelReportsRelaysByCountryOnly,
            kRelayControlWire,
            detail: g.name,
          );
        } else {
          availability = _fromChaining(chaining);
        }
        return RelayGroup(
          countryCode: cc,
          countryName: g.name,
          writeIndex: g.countryIndex,
          pin:
              g.countryRow ??
              Relay(id: cc, name: g.name, desc: '', country: cc),
          availability: availability,
          nodeCount: g.nodeCount > nodes.length ? g.nodeCount : nodes.length,
          nodes: nodes,
        );
      }(),
  ];
}

Availability _fromChaining(Availability chaining) =>
    chaining.isAvailable ? kRelayControlAlwaysTrue : chaining;

class _GroupAcc {
  final String cc;
  final String name;
  int? countryIndex;
  Relay? countryRow;
  int nodeCount = 0;
  final Map<int, RelayNodeRow> nodes = <int, RelayNodeRow>{};
  _GroupAcc(this.cc, this.name);
}

/// Экран «Вход»: объяснение, «Авто» / «Выкл» и группы «страна → релеи».
///
/// Владелец: «логика настройки Relay непонятна, нельзя выбрать вход через
/// Россию вручную; будет много релеев по регионам». Отсюда три вещи на
/// экране: объяснение словами наверху; страны с их релеями (имя, город, пинг
/// панели) и выбор как всей страны, так и конкретного релея; единый источник
/// правды о выборе — панель (`PUT /subscriptions/{id}/selection`), а ядро и
/// очередь CSM получают страну и вторичны.
///
/// Своей формулировки недоступности у экрана нет намеренно: правило живёт в
/// возможности ([Capabilities.relayChaining]) и в свидетельстве панели по
/// каждому узлу (`via_relay.chained_in_config`). На clash-теле, которое читает
/// это приложение, цепочка через вход сегодня не строится, и экран говорит
/// это прямо над строками, к которым это относится, — а выбор при этом всё
/// равно сохраняется на панели, чтобы не пропасть к моменту, когда генератор
/// цепочку построит.
///
/// «Выкл» и «Авто» цепочкой не являются: первый просит ядро не строить её
/// вовсе, второй оставляет решение панели, и оба уходят на провод пустой
/// строкой при любом флоте. Ключ к тому, ЧТО в силе, — [effectiveRelayIndex]:
/// он приводит сохранённый индекс к списку так же, как кодировщик провода.
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
    final selected = effectiveRelayIndex(cfg.relay, relays);
    final can = chaining.availability;
    // Вход, который назвала панель по активной подписке (`relay_country`).
    //
    // Спрашиваем ТОЛЬКО при живой сессии панели: `/app/subscriptions` — её
    // эндпоинт, и в generic-режиме (своя подписка, панели нет) запрос ушёл бы
    // в 401. Ровно так же ветвится Home, и по той же причине.
    final hasPanel = ref.watch(authProvider).stage == AuthStage.authenticated;
    final operatorRelay = hasPanel
        ? _operatorRelayCountry(ref.watch(subscriptionsProvider).valueOrNull)
        : null;

    final groups = buildRelayGroups(relays, offers, can);
    final anyPing = groups.any((g) => g.nodes.any((n) => n.latencyMs != null));

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
              'Вход',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            Text(
              kRelayExplanation,
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
            // страны. Возможность цепочки их не касается.
            for (var i = 0; i < relays.length; i++)
              if (relays[i].isAuto)
                _RelayRow(
                  // «Авто» обязано называть, ЧТО оно выбрало. Единственный
                  // источник этого — сама панель: вход выбирает генератор её
                  // конфига, а не приложение.
                  title: operatorRelay != null
                      ? 'Авто · через $operatorRelay'
                      : 'Авто',
                  desc: _autoDesc(operatorRelay),
                  leading: const IBox(Lucide.gauge),
                  availability: kRelayControlAlwaysTrue,
                  selected: i == selected,
                  onTap: () => _apply(context, ref, i, relays[i], relays),
                )
              else if (relays[i].isOff)
                _RelayRow(
                  title: relays[i].name,
                  desc: 'Без входа: напрямую к выбранному серверу.',
                  leading: const IBox(Lucide.route),
                  availability: kRelayControlAlwaysTrue,
                  selected: i == selected,
                  onTap: () => _apply(context, ref, i, relays[i], relays),
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
            if (anyPing) ...[
              Text(
                kRelayPingNote,
                style: AppType.bodySm.copyWith(color: c.textLow),
              ),
              const SizedBox(height: AppSpace.s3),
            ],

            if (groups.isEmpty)
              const InlineEmpty(message: 'Оператор не отдал ни одного входа')
            else
              for (final g in groups)
                ..._groupRows(context, ref, g, relays, selected),
          ],
        ),
      ),
    );
  }

  /// Строка страны («любой релей страны») и под ней её релеи.
  List<Widget> _groupRows(
    BuildContext context,
    WidgetRef ref,
    RelayGroup g,
    List<Relay> relays,
    int selected,
  ) {
    final countryIndex = g.writeIndex;
    final countrySelected =
        countryIndex != null &&
        countryIndex == selected &&
        relays[selected].isCountry;
    final rows = <Widget>[
      _RelayRow(
        title: g.countryName,
        desc: g.nodes.length > 1
            ? 'Любой релей страны, узлов: ${g.nodeCount}. Ниже можно выбрать '
                  'конкретный.'
            : (g.nodeCount > 0
                  ? 'Вход через ${g.countryCode}, узлов: ${g.nodeCount}'
                  : 'Вход через ${g.countryCode}'),
        leading: FlagChip(
          flag: flagOf(g.countryCode),
          code: normalizeCountryCode(g.countryCode),
        ),
        availability: g.availability,
        // Галочка отвечает на вопрос «что сейчас в силе», а не «что можно
        // выбрать». Недоступная строка, которая при этом записана в
        // настройки, — самый важный случай показать её.
        selected: countrySelected,
        // Нажимается всё, что источник не запретил прямо: неподтверждённая
        // строка помечена, но выбор по ней сохраняется на панели и уходит
        // ядру. Запрещённая (`unavailable`) цели для нажатия не имеет.
        onTap: (!g.availability.isUnavailable && countryIndex != null)
            ? () => _apply(context, ref, countryIndex, g.pin, relays)
            : null,
      ),
    ];
    for (final n in g.nodes) {
      final idx = n.writeIndex;
      final nodeSelected =
          idx != null &&
          idx == selected &&
          relays[selected].isNode &&
          relays[selected].nodeId == n.nodeId;
      final parts = <String>[
        if (n.city != null) n.city!,
        if (n.latencyMs != null) 'пинг панели: ${n.latencyMs} мс',
        if (n.loadPct != null) 'нагрузка ${n.loadPct!.round()}%',
        if (n.reachableExits > 0)
          'через него выходят узлов: ${n.reachableExits}',
      ];
      rows.add(
        Padding(
          padding: const EdgeInsets.only(left: AppSpace.s5),
          child: _RelayRow(
            title: n.name,
            desc: parts.join(' · '),
            leading: const IBox(Lucide.waypoints),
            availability: n.availability,
            selected: nodeSelected,
            onTap: (!n.availability.isUnavailable && idx != null)
                ? () => _apply(context, ref, idx, n.pin, relays)
                : null,
          ),
        ),
      );
    }
    return rows;
  }

  /// Подпись строки «Авто». Молчание панели это НЕ «идём напрямую»:
  /// приложение о цепочке не знает ничего, и обещать её отсутствие — та же
  /// выдумка, что и обещать её наличие.
  String _autoDesc(String? operatorRelay) {
    if (operatorRelay == null) {
      return 'Панель подберёт вход по вашей стране. По этой подписке она его '
          'пока не назвала.';
    }
    return 'Панель подбирает вход по вашей стране. По этой подписке она '
        'назвала $operatorRelay.';
  }

  /// Страна входа активной подписки; `null` — панель её не называет.
  static String? _operatorRelayCountry(List<SubPlan>? subs) {
    if (subs == null || subs.isEmpty) return null;
    final active = subs.where((s) => s.isActive);
    final plan = active.isNotEmpty ? active.first : subs.first;
    final cc = (plan.relayCountry ?? '').trim().toUpperCase();
    return cc.isEmpty ? null : cc;
  }

  /// Выбор уходит в три места, и порядок важен:
  ///   1. `CoreConfig.relay` (индекс) + очередь CSM — через мост, немедленно;
  ///      ядро и CSM получают только страну;
  ///   2. панель — `PUT /subscriptions/{id}/selection` с точной формой выбора
  ///      (`none` / страна / `node:<id>`): это и есть источник правды, по
  ///      нему панель фильтрует релеи при следующем запросе конфига.
  /// Отказ панели показывается тостом с её текстом, а не глотается: иначе
  /// пользователь видит галочку на входе, которого панель не приняла.
  Future<void> _apply(
    BuildContext context,
    WidgetRef ref,
    int index,
    Relay pin,
    List<Relay> relays,
  ) async {
    CsmSettingsBridge.setRelay(ref, index, relays);
    showCarambaToast(context, 'Вход: ${pin.name}');
    final outcome = await ref
        .read(exitSelectionControllerProvider)
        .selectRelay(pin);
    if (!context.mounted) return;
    if (!outcome.applied &&
        outcome.sync.reason == ExitUnavailableReason.panelRejected) {
      showCarambaToast(
        context,
        'Панель не приняла вход: ${outcome.sync.message}',
      );
      return;
    }
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
  final Widget leading;
  final Availability availability;
  final bool selected;
  final VoidCallback? onTap;

  const _RelayRow({
    required this.title,
    required this.desc,
    required this.leading,
    required this.availability,
    required this.selected,
    this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final off = availability.isUnavailable;
    return Opacity(
      opacity: off ? 0.45 : 1,
      child: ListItemCard(
        leading: leading,
        title: title,
        subtitle: off
            ? availability.message
            : (availability.isUnknown
                  ? '${desc.isEmpty ? '' : '$desc. '}'
                        '${availability.message}'
                  : (desc.isEmpty ? null : desc)),
        selected: selected,
        titleBadges: [if (availability.isUnknown) const Tag('не проверено')],
        onTap: onTap,
      ),
    );
  }
}

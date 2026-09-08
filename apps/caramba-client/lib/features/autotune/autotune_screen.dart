import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/domain/autopilot/auto_pick.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/features/servers/access_card.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Автопилот: подбор лучшей РАБОЧЕЙ комбинации и объяснение, почему выбрана
/// именно она.
///
/// ЗДЕСЬ БЫЛИ ТРИ ШАГА-ИМИТАЦИИ («Проверяю, какие протоколы проходят»,
/// «Выбираю лучший маршрут») с таймерами по 700-950 мс. Ни один из них ничего
/// не проверял; в мок-сборке они ещё и заканчивались `applyAutotune(protocol:
/// 1, stack: 2)` — выдуманным выбором, выданным за результат замера. Полоска
/// прогресса, за которой ничего не происходит, хуже отсутствия полоски: она
/// обещает работу.
///
/// Теперь путь один в любой сборке: настоящий замер всех узлов конфига →
/// ранжирование (задержка × форма подключения × загрузка) → закрепление
/// победителя → карточка «Выбрано и почему» со списком всех кандидатов и
/// вердиктом у каждого.
///
/// ЧЕГО ЗДЕСЬ НЕТ И ПОЧЕМУ. Строк «проверено 4 из 13», доезжающих по одной,
/// нет: `Probe` у ядра — ОДИН блокирующий вызов, отдающий весь список разом, и
/// потока результатов в ABI не существует. Рисовать нарастающий счётчик поверх
/// одного ожидания значило бы вернуть ту же имитацию, только красивее. Пока
/// идёт проход, экран честно показывает, сколько узлов в работе и сколько
/// секунд это уже длится.
class AutotuneScreen extends ConsumerStatefulWidget {
  /// Если экран вызван повторно из настроек, по «Продолжить» возвращаемся назад,
  /// а не на Home (роутер уже на home-ветке).
  final bool fromSettings;
  const AutotuneScreen({this.fromSettings = false, super.key});

  @override
  ConsumerState<AutotuneScreen> createState() => _AutotuneScreenState();
}

class _AutotuneScreenState extends ConsumerState<AutotuneScreen> {
  /// Тикер только для секунд ожидания. Он не двигает никакой логики: замер
  /// живёт в [autopilotProvider] и переживает уход с экрана.
  Timer? _ticker;
  final ValueNotifier<int> _tick = ValueNotifier<int>(0);

  @override
  void initState() {
    super.initState();
    // Запуск после первого кадра: провайдеры в initState трогать рано.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _run();
    });
  }

  @override
  void dispose() {
    _ticker?.cancel();
    _tick.dispose();
    super.dispose();
  }

  void _run() {
    _ticker ??= Timer.periodic(
      const Duration(seconds: 1),
      (_) => _tick.value++,
    );
    unawaited(
      ref.read(autopilotProvider.notifier).run().whenComplete(() {
        _ticker?.cancel();
        _ticker = null;
      }),
    );
  }

  void _continue() {
    ref.read(firstRunProvider.notifier).done();
    if (widget.fromSettings && context.canPop()) {
      context.pop();
      return;
    }
    context.go(AppRoute.home);
  }

  /// «Оставить прошлый выбор»: новый проход не нашёл ничего рабочего, а
  /// прошлый выбор когда-то работал. Стирать его молча значило бы наказать
  /// человека за то, что он запустил подбор в плохой сети.
  Future<void> _keepPrevious(AutoPickRecord previous) async {
    final profile = ref.read(activeConnectionProfileProvider);
    if (profile == null) return;
    final profiles = ref.read(connectionProfilesProvider.notifier);
    if (profile.isRaw) {
      await profiles.setSelectedServer(profile.id, previous.proxyName);
    }
    await profiles.setAutoPick(profile.id, previous);
    if (!mounted) return;
    showCarambaToast(
      context,
      'Оставили прошлый выбор: ${_pickLabel(previous)}',
    );
    _continue();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final run = ref.watch(autopilotProvider);
    final blocked = ref.watch(subscriptionAccessProvider)?.isBlocked ?? false;

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s6,
          ),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 460),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const SizedBox(height: AppSpace.s4),
                Center(child: IBox(Lucide.gauge, size: 56, color: c.textHi)),
                const SizedBox(height: AppSpace.s4),
                Text(
                  _title(run),
                  textAlign: TextAlign.center,
                  style: AppType.headline.copyWith(color: c.textHi),
                ),
                const SizedBox(height: AppSpace.s2),
                Text(
                  _subtitle(run),
                  textAlign: TextAlign.center,
                  style: AppType.bodyMd.copyWith(color: c.textMed),
                ),
                const SizedBox(height: AppSpace.s6),
                if (run.running)
                  _progress(run)
                else ...[
                  if (blocked) ...[
                    const AccessCard(),
                    const SizedBox(height: AppSpace.s5),
                  ] else if (run.error != null) ...[
                    FailureNotice(
                      message: run.error!,
                      technical: run.errorDetail,
                      payable: run.errorPayable,
                      onRetry: run.errorPayable ? null : _run,
                    ),
                    const SizedBox(height: AppSpace.s5),
                  ],
                  ..._result(run),
                  const SizedBox(height: AppSpace.s5),
                  FilledButton(
                    onPressed: _continue,
                    child: const Text('Продолжить'),
                  ),
                  const SizedBox(height: AppSpace.s1),
                  QuietButton(
                    label: 'Подобрать заново',
                    color: c.textMed,
                    onPressed: _run,
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }

  String _title(AutopilotRun run) {
    if (run.running) return 'Подбираю узел';
    if (run.error != null) return 'Не смогли проверить';
    final outcome = run.outcome;
    if (outcome == null) return 'Готово';
    return outcome.hasPick ? 'Выбрано' : 'Рабочего узла не нашлось';
  }

  String _subtitle(AutopilotRun run) {
    if (run.running) {
      return 'Проверяю каждый узел настоящим запросом. Туннель для этого не '
          'поднимается.';
    }
    if (run.error != null) return 'Замер не состоялся, и подбирать не из чего.';
    final outcome = run.outcome;
    if (outcome?.hasPick ?? false) {
      return 'Выбор закреплён. Поменять можно в любой момент на экране '
          'серверов.';
    }
    return 'Ниже — что ответил каждый узел.';
  }

  /// Прогресс без вранья: сколько узлов в работе и сколько секунд это длится.
  Widget _progress(AutopilotRun run) {
    final c = context.c;
    return Container(
      padding: const EdgeInsets.all(AppSpace.s4),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r14,
        border: Border.all(color: c.borderSubtle),
      ),
      child: Row(
        children: [
          SizedBox(
            width: 18,
            height: 18,
            child: CircularProgressIndicator(
              strokeWidth: 2,
              valueColor: AlwaysStoppedAnimation(c.textHi),
              backgroundColor: c.borderStrong,
            ),
          ),
          const SizedBox(width: AppSpace.s3),
          Expanded(
            child: ValueListenableBuilder<int>(
              valueListenable: _tick,
              builder: (context, _, __) {
                final started = run.startedAt;
                final secs = started == null
                    ? 0
                    : DateTime.now().difference(started).inSeconds;
                final nodes = run.nodeCount;
                return Text(
                  nodes > 0
                      ? 'Проверяю узлов: $nodes · $secs с'
                      : 'Проверяю узлы · $secs с',
                  style: AppType.bodyMd.copyWith(color: c.textHi),
                );
              },
            ),
          ),
        ],
      ),
    );
  }

  List<Widget> _result(AutopilotRun run) {
    final outcome = run.outcome;
    if (outcome == null) return const <Widget>[];
    final pick = outcome.pick;
    // Обещание входа проверяется по живым фактам: в записи выбора поля про
    // цепочку нет, и без фактов единственным свидетелем осталось бы само имя —
    // то есть тот самый ярлык, из-за которого всё и затевалось. Выбора нет —
    // разбирать нечего, и пустое имя честно даёт «обещаний нет».
    final naming = namingOfProxy(
      pick?.proxyName ?? '',
      ref.watch(fleetFactsProvider),
    );

    return <Widget>[
      if (pick != null) ...[
        const SectionTitle(
          'Выбрано и почему',
          padding: EdgeInsets.only(bottom: AppSpace.s3),
        ),
        RowsGroup(
          children: [
            CRow(
              icon: Lucide.globe,
              label: 'Узел',
              value: pick.countryCode.isEmpty
                  ? _pickTitle(pick, naming)
                  : '${pick.countryCode} · ${_pickTitle(pick, naming)}',
            ),
            // Имя оператора не теряется: под ним узел лежит в конфиге и под ним
            // же его знает поддержка. Строка появляется только там, где имя
            // расходится с проводом, — иначе она дублировала бы строку выше.
            if (naming.overPromises)
              CRow(
                icon: Lucide.eye,
                label: 'Имя у оператора',
                value: naming.operatorName,
              ),
            if (pick.protocolLabel.isNotEmpty)
              CRow(
                icon: Lucide.layers,
                label: 'Тип подключения',
                value: pick.protocolLabel,
              ),
            CRow(
              icon: Lucide.gauge,
              label: 'Задержка',
              value: '${pick.latencyMs} мс',
              mono: true,
            ),
            // Два числа — две строки. Одной строкой «29 из 29 · работает 12»
            // не помещалось: `CRow` отдаёт подписи всё место, а значение режет
            // многоточием, и человек читал «работае…».
            CRow(
              icon: Lucide.check,
              label: 'Проверено',
              value: '${pick.checked} из ${pick.total}',
              mono: true,
            ),
            CRow(
              icon: Lucide.activity,
              label: 'Работает',
              value: '${pick.working}',
              mono: true,
            ),
          ],
        ),
        const SizedBox(height: AppSpace.s3),
        InlineBanner(glyph: Lucide.route, text: _whyText(pick)),
        if (naming.overPromises) ...[
          const SizedBox(height: AppSpace.s3),
          InlineBanner(
            tone: BannerTone.warning,
            glyph: Lucide.waypoints,
            text: naming.note,
          ),
        ],
        const SizedBox(height: AppSpace.s3),
        InlineBanner(glyph: Lucide.waypoints, text: outcome.relayNote),
      ] else ...[
        InlineBanner(
          tone: BannerTone.warning,
          glyph: Lucide.alert,
          text: outcome.failure?.message ?? 'Подобрать не удалось.',
        ),
        if (outcome.previous != null) ...[
          const SizedBox(height: AppSpace.s3),
          GhostButton(
            label:
                'Оставить прошлый выбор: '
                '${_pickLabel(outcome.previous!)}',
            icon: Lucide.clock,
            onPressed: () => _keepPrevious(outcome.previous!),
          ),
          const SizedBox(height: AppSpace.s1),
          Text(
            'Он работал ${autoAgeText(outcome.previous!.updatedAt)}.',
            style: AppType.bodySm.copyWith(color: context.c.textLow),
          ),
        ],
      ],
      if (outcome.ranked.isNotEmpty || outcome.rejected.isNotEmpty) ...[
        const SectionTitle(
          'Что ответили узлы',
          padding: EdgeInsets.only(top: AppSpace.s5, bottom: AppSpace.s3),
        ),
        for (final cand in outcome.ranked.take(6))
          _CandidateRow(
            candidate: cand,
            selected: pick != null && pick.proxyName == cand.name,
          ),
        // Непрошедшие узлы ОСТАЮТСЯ на экране с причиной. Спрятать их значило
        // бы показать флот из четырёх машин там, где их тринадцать, и оставить
        // человека гадать, куда делись остальные.
        for (final cand in outcome.rejected.take(8))
          _CandidateRow(candidate: cand, selected: false),
      ],
    ];
  }

  /// Чем назвать выбор там, где место есть только на одну строку.
  ///
  /// `shortLabel` записи сюда не годится: когда имени машины нет, он отдаёт
  /// имя прокси КАК ЕСТЬ — то есть вместе с обещанием входа, которого конфиг
  /// не строит.
  String _pickLabel(AutoPickRecord pick) => pick.machineTitle.isNotEmpty
      ? pick.machineTitle
      : namingOfProxy(pick.proxyName, ref.read(fleetFactsProvider)).title;

  /// Чем подписать выбранный узел.
  ///
  /// Заголовок машины (там, где источник его знает) впереди имени прокси: он
  /// не меняется от инбаунда к инбаунду. Там, где его нет, подписывает сам
  /// узел — и подписывает тем, что НА ПРОВОДЕ.
  String _pickTitle(AutoPickRecord pick, NodeNaming naming) =>
      pick.machineTitle.isNotEmpty ? pick.machineTitle : naming.title;

  /// Почему выбран именно он. «Быстрее всех» — не вся правда: счёт учитывает
  /// форму подключения и загрузку машины, и раз учитывает — обязан это сказать.
  String _whyText(AutoPickRecord pick) {
    if (pick.reasonCode == 'kept_previous') {
      return 'Прошлый выбор оставлен: новый лидер выиграл слишком мало, чтобы '
          'ради него переподключаться.';
    }
    if (pick.reasonCode == 'plain_twin') {
      return 'Лидер по замеру оказался вторым именем этого же подключения: тот '
          'же узел и порт, но в имени обещан вход через другую страну, '
          'которого конфиг не строит. Разница их замеров — шум одного канала, '
          'и выбран тот, чьё имя говорит правду.';
    }
    if (pick.reasonCode == 'plain_over_labelled') {
      return 'Лидер по счёту обещал именем вход через другую страну, которого '
          'конфиг не строит. Разница с этим узлом — в пределах шума замера, и '
          'при равном качестве выбран тот, чьё имя не обещает лишнего.';
    }
    if (!pick.confirmed) {
      return 'Лучший по замеру. Учтите: сквозь узел проверочный запрос не '
          'проходил — в этой сборке проверить можно только адрес.';
    }
    return 'Лучший счёт среди узлов, сквозь которые прошёл проверочный '
        'запрос: задержка с поправкой на тип подключения и загрузку машины.';
  }
}

/// Строка кандидата: имя узла, машина, форма, вердикт и число.
///
/// Заголовком стоит ИМЯ УЗЛА, а не заголовок машины. Заголовок машины на
/// импортированном пути — это страна (имени машины в теле конфига нет), и
/// двенадцать инбаундов одной канадской машины давали двенадцать строк
/// «Канада · vless», по которым выбрать было нечего. Имя узла у оператора
/// («🇨🇦 Secure», «🇨🇦 Stealth») различает их и говорит человеку то же самое,
/// что скажет поддержка.
class _CandidateRow extends StatelessWidget {
  final AutoCandidate candidate;
  final bool selected;

  const _CandidateRow({required this.candidate, required this.selected});

  @override
  Widget build(BuildContext context) {
    final fact = candidate.fact;
    final naming = candidate.naming;
    final parts = <String>[
      if (fact.machineTitle.isNotEmpty) fact.machineTitle,
      if (fact.protocolLabel.isNotEmpty) fact.protocolLabel,
      probeVerdictShort(candidate.probe.verdict),
    ];
    return ListItemCard(
      leading: FlagChip(
        flag: kNeutralFlag,
        code: fact.countryCode.isEmpty
            ? candidate.probe.country
            : fact.countryCode,
      ),
      // Обещание входа снято с заголовка, но не выброшено: имя оператора
      // целиком стоит второй строкой подписи вместе с тем, чем оно не
      // является. Значком тут не обойтись — значок сообщил бы расхождение и
      // потерял имя, под которым узел лежит в конфиге.
      title: naming.title,
      // Приписку даёт кандидат, а не одно только имя: там, где у обещания
      // нашёлся живой близнец, она называет его — иначе строка с хорошим
      // числом, которую подбор молча пропустил, выглядела бы как ошибка.
      subtitle: candidate.listNote.isEmpty
          ? parts.join(' · ')
          : '${parts.join(' · ')}\n${candidate.listNote}',
      selected: selected,
      titleBadges: [
        if (candidate.confirmed)
          const Tag('работает', ok: true)
        else
          const Tag('не подтверждено'),
      ],
      trailing: LatencyReadout(
        candidate.probe.latencyMs < 0
            ? Latency.none
            : Latency.fromClient(candidate.probe.latencyMs),
      ),
    );
  }
}

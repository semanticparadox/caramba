import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/features/protocol/inbound_latency.dart';
import 'package:caramba_client/features/protocol/protocol_truth.dart'
    show protocolPinToast, protocolTruthProvider;
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/probe_state.dart';
import 'package:caramba_client/state/protocol_inventory_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// «Тип подключения» — это ИНБАУНДЫ ВЫБРАННОГО УЗЛА, а не список того, что ядро
/// умеет попросить.
///
/// Имя экрана сменилось со «Протокола» намеренно. «Протокол» врал дважды:
/// строк в списке больше, чем протоколов (`vless/tcp/reality` и `vless/ws/tls`
/// — разные строки одного протокола), и само слово звучит как выбор
/// технологии, а не входа на конкретную машину. Владелец сформулировал это
/// сам, назвав вещь «типом конфига». В ряду соседей имена теперь не спорят:
/// «Сервер» — где, «Тип подключения» — как, «Relay (вход)» — через что.
///
/// Владелец сформулировал и правило состава дословно: «протокол это и есть
/// выбор inbounds доступные в конфиге, а сервер должен быть выбор из нод». До
/// этого экран печатал семь строк из [ProtocolOption.defaults] — список
/// ЗАПРОСОВ ядра — и лишь помечал их доступность. На узле с одним инбаундом
/// пользователь всё равно видел семь протоколов, и выбор шести из них ничего
/// не менял.
///
/// Теперь строки приходят из [protocolSlateProvider]: тройка
/// `протокол/транспорт/безопасность` каждого инбаунда именно того узла, который
/// закреплён. Узел не закреплён — область названа честно («что бывает во
/// флоте»), а не выдана за «что применится».
///
/// Второе, что экран обязан не скрывать: `Policy.Protocol` умеет закрепить
/// только СЕМЕЙСТВО (`applyProtocol` в profile.go группирует прокси по
/// `type:`), поэтому `vless/tcp/reality` и `vless/ws/reality` для ядра
/// неразличимы. Там, где это так, строка говорит об этом сама. Там, где пин
/// может быть точным (сырой путь: `connectRaw` знает ИМЯ ПРОКСИ), он и делается
/// точным.
///
/// Третье — задержка. Она у КАЖДОЙ строки и она СВОЯ: панель про инбаунд не
/// знает ничего, у неё одно число на машину. Как именно она считается и чего
/// она не доказывает — см. [inbound_latency.dart].
class ProtocolScreen extends ConsumerStatefulWidget {
  const ProtocolScreen({super.key});

  @override
  ConsumerState<ProtocolScreen> createState() => _ProtocolScreenState();
}

class _ProtocolScreenState extends ConsumerState<ProtocolScreen> {
  /// Замер за открытие экрана запускается не больше одного раза.
  ///
  /// Без этого флага пустой ответ ядра (панельный профиль без шва — тот самый
  /// случай, из-за которого автоподбор говорил «Ядро не вернуло ни одного
  /// узла») стал бы вечным циклом: результат пуст, значит «чисел нет», значит
  /// мерим снова. Кнопка «Замерить» остаётся, и повтор — это решение человека.
  bool _autoProbed = false;

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final ref = this.ref;
    final offering = ref.watch(offeringProvider);
    final slate = ref.watch(protocolSlateProvider);
    final options = ref.watch(protocolsProvider);
    final cfg = ref.watch(coreConfigProvider);
    final probe = ref.watch(inboundLatencyProvider);
    final run = ref.watch(probeRunProvider);
    final hasProfile = ref.watch(activeConnectionProfileProvider) != null;
    // Происхождение значения по CSM: оператор мог поставить тип подключения
    // сам, и пользователь вправе видеть это до того, как перевыберет
    // (02-SPEC.md 7.6).
    final entry = ref.watch(csmSettingsProvider)[CsmSettingKey.protocol];

    final exit = offering.selectedExit;
    // Сколько строк схлопывается в одно семейство: об этом нельзя молчать,
    // иначе выбор «reality на tcp» тихо применится как «любой vless».
    //
    // Считаем по семейству ЯДРА (`ProtocolOption.coreFamily`), а не по индексу
    // опции. Индекс — это то, как список делит протоколы у нас; ядро делит их
    // по `m["type"]`, и `protocolClashType` сводит `VLESS-Reality` и `VLESS` в
    // один `vless`. Счёт по индексу давал Reality соседей «0» — строка
    // выглядела точным выбором, хотя `applyProtocol` собирает url-test по всем
    // vless-прокси узла разом.
    //
    // В счёт идут только строки, которые доезжают до тела конфига: недоступный
    // инбаунд в `proxies` не появляется, и ядро его в группу не возьмёт —
    // приписать его в соседи значило бы завысить схлопывание.
    final byFamily = <String, int>{};
    for (final r in slate.rows) {
      if (r.availability.isUnavailable) continue;
      final f = coreFamilyForProtocol(r.key, options);
      if (f != null) byFamily[f] = (byFamily[f] ?? 0) + 1;
    }

    final measurable = slate.rows.any((r) => r.proxyNames.isNotEmpty);
    _maybeAutoProbe(measurable: measurable && hasProfile, probe: probe);

    // Что выбрал «Авто», решает не этот экран: подпись собирает
    // [autoProtocolLabelProvider] из живого туннеля и прошлого замера — тех же
    // источников, из которых её берут строки «Сервер» и Home. Собери экран
    // подпись сам, два контрола с одним смыслом разошлись бы в первый же день.
    final auto = ref.watch(autoProtocolLabelProvider);
    // Тот же провайдер, что и у строки Главной: расхождение типов обязано
    // называться на обоих экранах одними словами.
    final truth = ref.watch(protocolTruthProvider);

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
              'Тип подключения',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            Text(
              _scopeText(slate, exit),
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            // Расхождение стоит ПЕРВЫМ и тем же текстом, что на Главной.
            // Человек приходит сюда по строке, которая только что сказала
            // «VLESS вместо TUIC», — и видит выделенным TUIC. Без этой фразы
            // выделение читалось бы как опровержение Главной, а два экрана,
            // спорящих о туннеле, хуже одного молчащего.
            if (truth.diverged) ...[
              const SizedBox(height: AppSpace.s3),
              InlineBanner(
                tone: BannerTone.warning,
                glyph: Lucide.layers,
                text: truth.note,
              ),
            ],
            if (slate.scope == ProtocolScope.wholeFleet) ...[
              const SizedBox(height: AppSpace.s3),
              // Не предупреждение об ошибке, а название области: список верен,
              // но отвечает на другой вопрос.
              const InlineBanner(
                glyph: Lucide.globe,
                text:
                    'Сервер не закреплён, поэтому здесь собраны инбаунды всех '
                    'доступных узлов. Выберите сервер на экране «Серверы» — и '
                    'список станет инбаундами именно его.',
              ),
            ],
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
                    ? 'Тип подключения выбрали вы. Оператор не перезапишет его '
                        'молча: на попытку поднимется карточка с вопросом.'
                    : 'Текущее значение поставил '
                        '${csmProvenanceTitle(entry.src)}. Выбрав своё, вы '
                        'закрепите его за собой.',
              ),
            ],
            // Источник не назвал форм — про это говорим ДО списка: иначе
            // «vless» в строке читается как полная тройка, которой источник не
            // сообщал.
            if (!slate.known.isAvailable && slate.rows.isNotEmpty) ...[
              const SizedBox(height: AppSpace.s3),
              InlineBanner(
                tone: BannerTone.warning,
                glyph: Lucide.alert,
                text: slate.known.message,
              ),
            ],
            if (measurable) ...[
              const SizedBox(height: AppSpace.s4),
              ..._probeHeader(
                run: run,
                probe: probe,
                rows: slate.rows,
                enabled: hasProfile,
              ),
            ],
            const SizedBox(height: AppSpace.s4),

            // «Авто» — отказ от выбора. Он верен при любом флоте и потому
            // стоит первым и всегда.
            _AutoRow(
              option: options.first,
              label: auto,
              // Заголовок строки — тот же язык, что и в списке: «VLESS · tcp ·
              // reality», а не `vless · tcp · reality`. Провайдер отдаёт форму
              // ключом источника, и без этого перевода одна и та же тройка
              // выглядела бы на экране двумя разными.
              choice: _prettyChoice(auto.choice, slate.rows),
              selected: cfg.protocol == 0,
              onTap: () => _applyFamily(context, ref, 0, options.first.name),
            ),

            if (slate.rows.isNotEmpty)
              for (final row in _ordered(slate.rows, options, probe))
                _InboundRow(
                  row: row,
                  optionIndex: optionIndexForProtocol(row.key, options),
                  siblings:
                      byFamily[coreFamilyForProtocol(row.key, options)] ?? 1,
                  familyName: _familyNameOf(row, options),
                  latency: probe.of(row),
                  selected: _isSelected(row, options, cfg.protocol),
                  onTap: (int index, String label) => _apply(
                    context,
                    ref,
                    row,
                    index,
                    label,
                    probe.of(row),
                  ),
                )
            else
              ..._silentRows(context, ref, slate, options, cfg.protocol),
          ],
        ),
      ),
    );
  }

  /// Порядок строк: сначала проверенные и быстрые, в конце — те, что выбрать
  /// нельзя.
  ///
  /// Пока чисел не было, порядок источника был единственным возможным. Теперь
  /// он был бы вредным: строка, про которую замер сказал «ключ не принят»,
  /// стояла бы первой только потому, что оператор завёл этот инбаунд раньше
  /// прочих. Правило то же, что в списке машин (exit_node_list), и отличается
  /// оно ровно на один ярус — «число есть, подтверждения нет».
  List<ProtocolRow> _ordered(
    List<ProtocolRow> rows,
    List<ProtocolOption> options,
    InboundLatencyLookup probe,
  ) {
    final ranked = <(int, bool, InboundLatency)>[
      for (var i = 0; i < rows.length; i++)
        (
          i,
          !rows[i].availability.isUnavailable &&
              optionIndexForProtocol(rows[i].key, options) != null,
          probe.of(rows[i]),
        ),
    ]..sort(compareInboundRows);
    return <ProtocolRow>[for (final r in ranked) rows[r.$1]];
  }

  /// Шапка замера: кнопка, время последнего прохода и — главное — оговорка о
  /// том, что именно измерено.
  ///
  /// Оговорка стоит ЗДЕСЬ, над числами, а не подписью под каждым из них:
  /// повторённая семь раз, она превратилась бы в шум, который перестают читать,
  /// а сказанная один раз перед списком — это условие, на котором стоит верить
  /// всем числам сразу.
  List<Widget> _probeHeader({
    required ProbeRun run,
    required InboundLatencyLookup probe,
    required List<ProtocolRow> rows,
    required bool enabled,
  }) {
    final c = context.c;
    final at = probe.measuredAt;
    return <Widget>[
      GhostButton(
        label: probe.measuring ? 'Меряю задержки' : 'Замерить свой пинг',
        icon: Lucide.gauge,
        onPressed: (probe.measuring || !enabled) ? null : _probe,
      ),
      const SizedBox(height: AppSpace.s2),
      Text(
        // Три разных ответа, и склеивать их нельзя. «Не мерили» — числа
        // взяться неоткуда, и пустая колонка читается как «быстро». «Мерили, а
        // чисел нет» — это НЕ «не мерили»: замер прошёл и вернул пустоту (так
        // выглядел панельный путь без шва), и отметка времени без единого
        // числа обещала бы, что числа где-то есть. Третий случай — числа есть.
        at == null
            ? 'Пока не мерили: задержки входов появятся после замера.'
            : (probe.byProxyName.isEmpty
                ? 'Замер прошёл (${measuredAgoText(at, DateTime.now())}), '
                    'но ядро не назвало ни одного входа.'
                : 'Ваш замер: ${measuredAgoText(at, DateTime.now())}'),
        style: AppType.bodySm.copyWith(color: c.textLow),
      ),
      const SizedBox(height: AppSpace.s3),
      const InlineBanner(glyph: Lucide.gauge, text: kProbeMeaningNote),
      // Оговорка про «проверен только адрес» появляется ровно тогда, когда
      // такие строки в списке есть. Сказанная всегда, она обесценила бы числа,
      // добытые настоящим запросом; не сказанная никогда — выдала бы TCP за
      // рабочий вход.
      if (probe.anyUnconfirmed(rows)) ...[
        const SizedBox(height: AppSpace.s3),
        const InlineBanner(
          tone: BannerTone.warning,
          glyph: Lucide.alert,
          text: kProbeTcpOnlyNote,
        ),
      ],
      if (run.error != null) ...[
        const SizedBox(height: AppSpace.s3),
        InlineBanner(
          tone: BannerTone.warning,
          glyph: Lucide.alert,
          text: run.error!,
        ),
      ],
    ];
  }

  /// Запускает замер один раз на открытие экрана, когда мерить есть что, а
  /// числа отсутствуют или устарели.
  ///
  /// Именно «после того как строки показаны», а не «вместо того»: список
  /// рисуется немедленно, а числа доезжают по мере ответов узлов, и выбирать
  /// во время замера можно.
  void _maybeAutoProbe({
    required bool measurable,
    required InboundLatencyLookup probe,
  }) {
    if (_autoProbed) return;
    if (!shouldProbeInbounds(
      measuring: probe.measuring,
      hasRowsToMeasure: measurable,
      measured: probe.byProxyName,
      measuredAt: probe.measuredAt,
      now: DateTime.now(),
    )) {
      return;
    }
    _autoProbed = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) ref.read(probeRunProvider.notifier).measure();
    });
  }

  Future<void> _probe() => ref.read(probeRunProvider.notifier).measure();

  /// Что за список сейчас на экране — одной фразой, без иносказаний.
  ///
  /// «Тип конфига» — слово владельца — сказано здесь ровно один раз: в
  /// заголовках и строках имя одно, «Тип подключения», иначе два синонима на
  /// одном экране читались бы как две разные настройки.
  String _scopeText(ProtocolSlate slate, ExitOffer? exit) {
    switch (slate.scope) {
      case ProtocolScope.singleExit:
        final name = exit == null || exit.label.isEmpty
            ? (slate.exitKey ?? 'узла')
            : exit.label;
        final country = exit == null || exit.countryCode.isEmpty
            ? ''
            : ' (${countryNameOf(exit.countryCode)})';
        return 'Инбаунды узла «$name»$country — то, чем этот узел умеет '
            'поднять туннель. Его же называют типом конфига.';
      case ProtocolScope.wholeFleet:
        return 'Чем узел принимает соединение (его же называют типом '
            'конфига). Инбаунд принадлежит конкретному узлу, поэтому список '
            'зависит от выбранного сервера.';
      case ProtocolScope.none:
        return slate.known.message;
    }
  }

  /// Строки, когда инбаундов в предложении нет вовсе.
  ///
  /// Пустой экран здесь был бы худшим исходом: настройка существует, она
  /// применится на следующем подъёме, а показать её нечем. Поэтому список ядра
  /// остаётся — но КАЖДАЯ строка помечена непроверенной и названа причиной.
  /// Нажать нельзя ровно в одном случае: профиля подключения нет, и настройке
  /// не к чему прикрепиться.
  ///
  /// Задержки у этих строк нет и быть не может: имени прокси, которым ядро
  /// называет узел в ответе `probe`, источник не сообщал. Прочерк в колонке
  /// был бы обещанием, что число когда-нибудь появится.
  List<Widget> _silentRows(
    BuildContext context,
    WidgetRef ref,
    ProtocolSlate slate,
    List<ProtocolOption> options,
    int selected,
  ) {
    final blocked = slate.known.reason == OfferingReason.noProfile;
    // Схлопывание здесь надо назвать ТЕМ ЖЕ ГОЛОСОМ, что и в списке инбаундов.
    // Молчание источника ничего не меняет в ядре: `protocolClashType` сводит
    // `VLESS-Reality` и `VLESS` в один `vless` независимо от того, что панель
    // успела рассказать про узел. Две соседние строки, применяющиеся
    // одинаково, — это тот же несуществующий выбор, только на другом пути
    // отрисовки.
    final census = _familyCensus(options);
    return <Widget>[
      for (var i = 1; i < options.length; i++)
        _AskRow(
          option: options[i],
          collapse: _collapseNote(options[i], census),
          reason: slate.known.message,
          blocked: blocked,
          selected: !blocked && i == selected,
          onTap: () => _applyFamily(context, ref, i, options[i].name),
        ),
    ];
  }

  bool _isSelected(
    ProtocolRow row,
    List<ProtocolOption> options,
    int selected,
  ) {
    final i = optionIndexForProtocol(row.key, options);
    // Ноль это «Авто», и подсвечивать им конкретный инбаунд нельзя.
    return i != null && i != 0 && i == selected;
  }

  /// Сколько СТРОК СПИСКА ЯДРА приходится на каждое его семейство. Считается по
  /// самому списку, а не по флоту: это факт о `protocolClashType`, и он верен
  /// даже когда источник не сказал про узлы ничего.
  Map<String, int> _familyCensus(List<ProtocolOption> options) {
    final out = <String, int>{};
    for (final o in options) {
      if (o.coreFamily.isEmpty) continue;
      out[o.coreFamily] = (out[o.coreFamily] ?? 0) + 1;
    }
    return out;
  }

  /// Признание строки списка ядра в том, что она не одна в своём семействе;
  /// `null` — семейство её собственное, и признаваться не в чем.
  String? _collapseNote(ProtocolOption option, Map<String, int> census) {
    final n = census[option.coreFamily] ?? 0;
    if (n < 2) return null;
    return 'Ядро закрепляет семейство '
        '«${protocolFamilyTitle(option.coreFamily)}»: для него эта строка и '
        'ещё ${n - 1} в списке неразличимы.';
  }

  /// Имя семейства ТАК, КАК ЕГО ЗНАЕТ ЯДРО. Для строки `vless · tcp · reality`
  /// это «VLESS», а не «VLESS · Reality»: подписать схлопывание именем опции
  /// значило бы сказать «ядро закрепляет семейство VLESS · Reality» — семейства,
  /// которого в `protocolClashType` нет.
  String? _familyNameOf(ProtocolRow row, List<ProtocolOption> options) {
    final f = coreFamilyForProtocol(row.key, options);
    return f == null ? null : protocolFamilyTitle(f);
  }

  /// Применяет выбор инбаунда.
  ///
  /// Половин у него две, и они разной точности. Семейство уходит ядру и
  /// оператору всегда — это `Policy.Protocol`. Точный инбаунд закрепляется
  /// только там, где для него есть ключ: на сыром пути это имя прокси, которое
  /// читает `connectRaw`. Тост называет, что произошло на самом деле, а не то,
  /// что хотелось бы.
  ///
  /// [latency] нужен ровно за одним: строку, чей замер не пропустил ни одного
  /// запроса, закрепить МОЖНО (почему — в [protocolPinToast]), но молчать об
  /// этом нельзя. Ядро деградирует на такой пин молча, и единственный момент,
  /// когда деградацию ещё можно назвать заранее, — этот.
  Future<void> _apply(
    BuildContext context,
    WidgetRef ref,
    ProtocolRow row,
    int optionIndex,
    String label,
    InboundLatency latency,
  ) async {
    CsmSettingsBridge.setProtocol(ref, optionIndex);
    final pinned = await _pinExactInbound(ref, row);
    if (!context.mounted) return;
    showCarambaToast(
      context,
      protocolPinToast(
        label: label,
        exact: pinned,
        // «Мерили и ничего не прошло» — не то же самое, что «не мерили»:
        // молчание замера обещанием провала не является.
        noneAnswered: latency.measured > 0 && latency.answered == 0,
      ),
    );
    Future.delayed(const Duration(milliseconds: 300), () {
      if (context.mounted) _close(context);
    });
  }

  void _applyFamily(
    BuildContext context,
    WidgetRef ref,
    int index,
    String name,
  ) {
    // Правка уходит и ядру (следующий `Up`), и оператору (очередь записи).
    // Туннель не рвётся: поднимется баннер.
    CsmSettingsBridge.setProtocol(ref, index);
    showCarambaToast(context, 'Тип подключения: $name');
    Future.delayed(const Duration(milliseconds: 300), () {
      if (context.mounted) _close(context);
    });
  }

  /// Закрепляет КОНКРЕТНЫЙ прокси, когда строка указывает ровно на один и он
  /// есть в списке выбора. Возвращает `false`, когда точного ключа нет — тогда
  /// закреплено только семейство, и тост обязан сказать именно это.
  Future<bool> _pinExactInbound(WidgetRef ref, ProtocolRow row) async {
    if (row.proxyNames.length != 1) return false;
    final name = row.proxyNames.single;
    final inventory = ref.read(exitInventoryProvider);
    ExitNode? node;
    for (final n in inventory.nodes) {
      if (n.key == name) node = n;
    }
    if (node == null || !node.isAvailable) return false;
    final outcome =
        await ref.read(exitSelectionControllerProvider).selectNode(node);
    return outcome.applied;
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go('/home');
    }
  }
}

/// Заголовок выбора «Авто» на языке этого списка.
///
/// [autoProtocolLabelProvider] называет форму ключом источника
/// (`vless · tcp · reality`), а список — отображаемым именем семейства
/// (`VLESS · tcp · reality`). Строки в одном экране обязаны совпадать
/// побуквенно, иначе человек читает их как два разных входа. Не нашлось —
/// возвращаем как есть: подпись провайдера верна, просто её строки в текущем
/// предложении нет.
String _prettyChoice(String choice, List<ProtocolRow> rows) {
  if (choice.isEmpty) return choice;
  for (final r in rows) {
    if (r.key.label == choice) return protocolRowTitle(r.key);
  }
  return choice;
}

/// Строка «Авто».
class _AutoRow extends StatelessWidget {
  final ProtocolOption option;

  /// Подпись автоподбора: что выбрано, откуда известно и не устарело ли.
  final AutoLabel label;

  /// [AutoLabel.choice], переведённый на язык этого списка.
  final String choice;

  final bool selected;
  final VoidCallback onTap;

  const _AutoRow({
    required this.option,
    required this.label,
    required this.choice,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) => ListItemCard(
        leading: IBox(option.icon),
        // Выбор стоит В ЗАГОЛОВКЕ, а не в подписи: заголовок — единственное, что
        // читают в списке наверняка.
        title: label.hasChoice ? '${option.name} · $choice' : option.name,
        // Бейджа «умный» тут больше нет. Он ничего не сообщал: «умный» — это
        // похвала выбору, а не сам выбор, и на строке, которая до сих пор не
        // называла результат, он был единственным словом про качество. Его место
        // заняла пометка «устарело» — единственная, которая что-то меняет.
        titleBadges: [if (label.isStale) const Tag('устарело')],
        subtitle: '${option.desc} ${label.subtitle}.',
        selected: selected,
        onTap: onTap,
      );
}

/// Строка запроса к ядру — путь для молчащего источника. Нажимается, но
/// помечена непроверенной: источник про этот протокол ничего не сказал.
class _AskRow extends StatelessWidget {
  final ProtocolOption option;
  final String reason;

  /// Признание в том, что ядро не отличит эту строку от соседней по списку;
  /// `null` — семейство у неё своё. Говорится и на выключенной строке: это
  /// свойство `protocolClashType`, а не флота, и от молчания источника оно не
  /// зависит.
  final String? collapse;

  /// Выбирать нельзя вовсе (профиля нет).
  final bool blocked;

  final bool selected;
  final VoidCallback onTap;

  const _AskRow({
    required this.option,
    required this.reason,
    required this.collapse,
    required this.blocked,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) => Opacity(
        opacity: blocked ? 0.45 : 1,
        child: ListItemCard(
          leading: IBox(option.icon),
          title: option.name,
          subtitle: <String>[
            if (!blocked) option.desc,
            reason,
            if (collapse != null) collapse!,
          ].where((p) => p.isNotEmpty).join(' '),
          selected: selected,
          // Плашка ровно одна: строка узкая, и две уводят заголовок за край. У
          // непроверенной строки побеждает именно эта пометка — «рекоменд.» на
          // протоколе, существование которого источник не подтвердил, обещает
          // больше, чем мы знаем.
          titleBadges: [
            if (!blocked)
              const Tag('не проверено')
            else if (option.recommended)
              const Tag('рекоменд.'),
          ],
          onTap: blocked ? null : onTap,
        ),
      );
}

/// Строка инбаунда. Недоступный рисуется тем же приёмом, что и выключенный
/// вариант в [showPickerSheet]: приглушённый, с ПРИЧИНОЙ вместо описания и без
/// цели для нажатия. Спрятать его нельзя — оператор его включил, и пропавшую
/// строку пользователь ищет в обновлении приложения, которого ему не нужно.
class _InboundRow extends StatelessWidget {
  final ProtocolRow row;

  /// Индекс в [ProtocolOption.defaults]; `null` — попросить этот инбаунд ядру
  /// нечем.
  final int? optionIndex;

  /// Сколько строк ЯДРО складывает в то же семейство (включая эту). Считается
  /// по `ProtocolOption.coreFamily` — типу, по которому `applyProtocol`
  /// отбирает прокси, — а не по индексу опции в списке.
  final int siblings;

  /// Имя семейства в терминах ядра: «VLESS» и для Reality, и для TLS.
  final String? familyName;

  /// Собственный замер устройства до этого входа.
  final InboundLatency latency;

  final bool selected;
  final void Function(int optionIndex, String label) onTap;

  const _InboundRow({
    required this.row,
    required this.optionIndex,
    required this.siblings,
    required this.familyName,
    required this.latency,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final i = optionIndex;
    final title = protocolRowTitle(row.key);
    // Выключают строку ровно две вещи: источник сказал, что инбаунда нет, — и
    // у ядра нет строки, которой его попросить. Обе называются, ни одна не
    // прячется.
    final String? blocked = row.availability.isUnavailable
        ? row.availability.message
        : (i == null
            ? const ProtocolAvailability.unavailable(
                ProtocolUnavailableReason.notRequestable,
              ).message
            : null);

    return Opacity(
      opacity: blocked == null ? 1 : 0.45,
      child: ListItemCard(
        leading: IBox(protocolGlyph(row.key.protocol)),
        title: title,
        subtitle: blocked ?? _subtitle(),
        selected: blocked == null && selected,
        // Плашка ровно одна: строка узкая, и вторая уводит заголовок за край.
        // Непроверенность важнее подписи генератора, поэтому она и побеждает.
        // Сама подпись (`Stealth`, `Speed`) не показывается, когда просто
        // повторяет тип прокси: у импортированного тела `label` это и есть
        // `type`, и плашка «VLESS» рядом с заголовком «VLESS» ничего не
        // добавляет.
        titleBadges: [
          if (row.availability.isUnknown)
            const Tag('не проверено')
          else if (row.label.isNotEmpty &&
              row.label.toLowerCase() != row.key.protocol)
            Tag(row.label),
        ],
        onTap: (blocked == null && i != null) ? () => onTap(i, title) : null,
        // Числа нет там, где его неоткуда взять: имени прокси источник не
        // назвал, и мерить нечего. Пустая колонка честнее прочерка, который
        // обещал бы, что число появится.
        trailing: latency.hasProxies ? LatencyReadout(latency.latency) : null,
      ),
    );
  }

  String? _subtitle() {
    final parts = <String>[];
    if (row.availability.isUnknown) parts.add(row.availability.message);
    if (row.exitKeys.length > 1) {
      parts.add('Узлов с таким инбаундом: ${row.exitKeys.length}.');
    }
    // Число на строке, которую предлагают несколько машин, — лучшее из
    // ответивших, а не замер того входа, к которому подключишься. Молчать об
    // этом нельзя ровно по той же причине, по которой нельзя молчать о
    // схлопывании семейства.
    // Причина отказа стоит РАНЬШЕ всего прочего: пока вход не пропускает
    // запрос, остальное про него — правда, которая ничего не меняет.
    final verdict = latency.verdictNote;
    if (verdict != null) parts.add(verdict);
    final spread = latency.spreadNote;
    if (spread != null) parts.add(spread);
    // Про схлопывание молчать нельзя: две разные строки, применяющиеся
    // одинаково, выглядят как выбор, которого нет. Границу семейства проводит
    // ядро, поэтому и счёт, и имя здесь — его: `vless · tcp · reality` попадает
    // в «VLESS» вместе со всеми TLS-инбаундами узла.
    if (siblings > 1 && familyName != null) {
      parts.add(
        'Ядро закрепляет семейство «$familyName»: этот инбаунд и ещё '
        '${siblings - 1} в нём неразличимы.',
      );
    }
    return parts.isEmpty ? null : parts.join(' ');
  }
}

/// Заголовок строки инбаунда: тройка источника без дописанных частей.
String protocolRowTitle(ProtocolKey key) {
  final proto = _protoDisplay[key.protocol] ?? key.protocol.toUpperCase();
  return <String>[
    proto,
    key.transport,
    key.security,
  ].where((p) => p.isNotEmpty).join(' · ');
}

/// Иконка семейства; незнакомому — общая.
String protocolGlyph(String protocol) => switch (protocol) {
      'vless' || 'vmess' || 'trojan' => Lucide.shield,
      'hysteria2' || 'hysteria' || 'tuic' => Lucide.zap,
      'wireguard' || 'amneziawg' => Lucide.lock,
      _ => Lucide.globe,
    };

/// Индекс опции ядра, которой просят этот инбаунд; `null` — такой строки у
/// ядра нет.
///
/// `Policy.Protocol` сопоставляется с прокси ПО ТИПУ (`applyProtocol` в
/// profile.go), поэтому уточнение формы проверяется только там, где опция его
/// заявляет: `VLESS-Reality` берёт лишь `security = reality`, а голый `VLESS`
/// подходит любому vless, которому Reality не нашлось.
int? optionIndexForProtocol(ProtocolKey key, List<ProtocolOption> options) {
  final proto = _canonicalProtocol(key.protocol);
  if (proto.isEmpty) return null;
  int? family;
  for (var i = 0; i < options.length; i++) {
    final o = options[i];
    // «Авто» ничему не сопоставляется: это отказ от выбора.
    if (o.outboundTypes.isEmpty) continue;
    if (!o.outboundTypes.any((t) => t.toLowerCase() == proto)) continue;
    final shape = o.shape;
    if (shape != null) {
      if (key.security == shape) return i;
      continue;
    }
    family ??= i;
  }
  return family;
}

/// Семейство ЯДРА, в которое попадёт этот инбаунд, если его выбрать; `null` —
/// попросить его ядру нечем.
///
/// Это и есть та граница, по которой `applyProtocol` (profile.go) отбирает
/// прокси: он берёт `protocolClashType[Policy.Protocol]` и сравнивает с
/// `m["type"]` каждого элемента `proxies`. Уточнения формы (`reality`, `ws`) в
/// сравнении не участвуют вовсе, поэтому две строки пикера с одним семейством
/// применяются ОДИНАКОВО, как бы по-разному они ни назывались.
///
/// Функция намеренно идёт через [optionIndexForProtocol], а не подбирает
/// семейство сама: строка сначала обязана быть запрашиваемой (иначе выбирать
/// нечего), и оба вывода должны опираться на одно и то же сопоставление —
/// разъехавшись, они дали бы ровно ту ложь, которую этот счёт и закрывает.
String? coreFamilyForProtocol(ProtocolKey key, List<ProtocolOption> options) {
  final i = optionIndexForProtocol(key, options);
  if (i == null) return null;
  final family = options[i].coreFamily;
  return family.isEmpty ? null : family;
}

/// Как назвать семейство ядра пользователю: `vless` → «VLESS», `ss` →
/// «Shadowsocks». Незнакомое — своим же именем в верхнем регистре, а не
/// «неизвестно»: имя семейства это факт ядра, и выдумывать тут нечего.
String protocolFamilyTitle(String family) =>
    _protoDisplay[family] ?? family.toUpperCase();

/// AmneziaWG приезжает от панели своим именем, а в теле конфига становится
/// `wireguard` — так его и просит ядро (`protocolClashType`).
String _canonicalProtocol(String raw) {
  final p = raw.trim().toLowerCase();
  return switch (p) {
    'amneziawg' || 'awg' => 'wireguard',
    'shadowsocks' => 'ss',
    _ => p,
  };
}

const Map<String, String> _protoDisplay = <String, String>{
  'vless': 'VLESS',
  'vmess': 'VMess',
  'trojan': 'Trojan',
  'hysteria2': 'Hysteria2',
  'hysteria': 'Hysteria',
  'tuic': 'TUIC',
  'ss': 'Shadowsocks',
  'shadowsocks': 'Shadowsocks',
  'wireguard': 'WireGuard',
  'amneziawg': 'AmneziaWG',
  'naive': 'NaiveProxy',
};

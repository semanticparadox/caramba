import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/subscription.dart' show AccessState;
import 'package:caramba_client/features/servers/access_card.dart';
import 'package:caramba_client/features/servers/exit_node_table.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_error.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/probe_state.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// «Серверы» на десктопе: та же логика, другая раскладка.
///
/// Экран живёт в накладной панели 720 справа от шелла, поэтому шапка с
/// крестиком остаётся (её рисует сам экран, а не тулбар окна), а вот
/// «потяните, чтобы обновить» уходит: жеста протяжки на мыши нет, и его место
/// занимают кнопки замера — они и раньше были главным способом обновить числа.
///
/// Почему это ОТДЕЛЬНЫЙ файл, а не ветка в [ServersScreen]. Мобильный экран
/// закрыт для правок в этой волне (975 тестов держат его поведение), а разница
/// здесь не косметическая: список карточек становится таблицей, кнопки встают
/// в строку вместо колонки, появляется двойной клик. Копия состояния — цена,
/// которую платим осознанно; общие части (порядок строк, сшивка предложения и
/// инвентаря, контроллер выбора) не копируются, а импортируются.
class ServersDesktopScreen extends ConsumerStatefulWidget {
  const ServersDesktopScreen({super.key});

  @override
  ConsumerState<ServersDesktopScreen> createState() =>
      _ServersDesktopScreenState();
}

class _ServersDesktopScreenState extends ConsumerState<ServersDesktopScreen> {
  /// Последняя НЕсостоявшаяся синхронизация выбора с панелью. Локально выбор
  /// применён всегда, поэтому это не ошибка действия, а состояние режима, и
  /// живёт оно баннером, а не тостом с красным словом.
  ExitAvailability? _syncNote;

  /// Автозамер уже запускался на этом открытии экрана.
  bool _autoProbed = false;

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final inventory = ref.watch(exitInventoryProvider);
    _maybeAutoProbe(inventory);

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: Padding(
        padding: const EdgeInsets.all(AppSpace.s6),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            ScreenHead('Серверы', trailing: IconBtn(Lucide.x, onTap: _close)),
            ..._toolbar(inventory),
            ..._banners(inventory),
            // Прокручивается только СПИСОК: шапка, кнопки и баннеры остаются на
            // месте. На телефоне они уезжали вместе со списком, потому что
            // экрана не хватало ни на что; здесь хватает, и увозить наверх
            // кнопку замера ровно тогда, когда человек смотрит на числа, было
            // бы вредительством.
            Expanded(
              child: ListView(
                padding: const EdgeInsets.only(bottom: AppSpace.s6),
                children: [_body(inventory)],
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _body(ExitInventory inventory) {
    if (inventory.loading && inventory.isEmpty) return const _Loading();
    if (inventory.error != null && inventory.isEmpty) {
      return _Error(
        error: inventory.error!,
        onRetry: () => _refresh(inventory.source),
      );
    }
    if (inventory.isEmpty) {
      return _Empty(source: inventory.source, access: inventory.blockedBy);
    }
    return ExitNodeTable(
      onSelect: (node) => node == null ? _pickCountry(null) : _pickNode(node),
      onActivate: _activate,
    );
  }

  /// Строка действий: замер, автоподбор и одна фраза о том, чей замер показан.
  ///
  /// Кнопки здесь по СОДЕРЖИМОМУ, а не во всю ширину: растянутая на 720 px
  /// «Замерить свой пинг» читается как главное действие экрана, а главное
  /// действие экрана — выбрать машину.
  List<Widget> _toolbar(ExitInventory inventory) {
    final c = context.c;
    final profile = ref.watch(activeConnectionProfileProvider);
    // Без профиля мерить нечего и подбирать не из чего: строка действий
    // исчезает целиком, а не стоит с двумя выключенными кнопками.
    if (profile == null) return const <Widget>[];
    final run = ref.watch(probeRunProvider);
    final measuredAt = ref.watch(clientLatencyAtProvider);
    final nothingToMeasure = inventory.nodes.isEmpty;

    return <Widget>[
      Wrap(
        spacing: AppSpace.s3,
        runSpacing: AppSpace.s2,
        children: [
          OutlinedButton.icon(
            style: OutlinedButton.styleFrom(
              minimumSize: const Size(200, _actionHeight),
            ),
            onPressed: (run.measuring || nothingToMeasure) ? null : _probe,
            icon: LucideIcon(Lucide.gauge, color: c.textHi, size: 18),
            label:
                Text(run.measuring ? 'Меряю задержки' : 'Замерить свой пинг'),
          ),
          OutlinedButton.icon(
            style: OutlinedButton.styleFrom(
              minimumSize: const Size(200, _actionHeight),
            ),
            onPressed: nothingToMeasure
                ? null
                : () => context.go('${AppRoute.settings}/autotune'),
            icon: LucideIcon(Lucide.route, color: c.textHi, size: 18),
            label: const Text('Подобрать лучший узел'),
          ),
        ],
      ),
      const SizedBox(height: AppSpace.s2),
      Text(
        _probeSummary(measuredAt, profile),
        style: AppType.bodySm.copyWith(color: c.textLow),
      ),
      const SizedBox(height: AppSpace.s5),
    ];
  }

  /// Всё, что экран обязан сказать ДО списка: почему строки не нажимаются,
  /// почему выбор не доехал до панели и почему замер не прошёл.
  List<Widget> _banners(ExitInventory inventory) {
    final run = ref.watch(probeRunProvider);
    final note = _syncNote;
    return <Widget>[
      // Тот же гейт, что на «Подключении» и в Настройках: смена узла
      // применяется сама, и баннер здесь — её отчёт.
      if (ref.watch(reconnectRequiredProvider)) ...[
        const ReconnectBanner(),
        const SizedBox(height: AppSpace.s4),
      ],
      if (note != null) ...[
        InlineBanner(
          tone: BannerTone.warning,
          glyph: Lucide.alert,
          text: 'Выбор применён на этом устройстве. ${note.message}',
        ),
        const SizedBox(height: AppSpace.s4),
      ],
      // Отказ подписки стоит ВЫШЕ таблицы: он объясняет и то, почему строки не
      // нажимаются, и почему замер не прошёл. Таблица при этом остаётся на
      // месте — исчерпанный трафик не отменяет существования флота оператора.
      if (inventory.blockedBy != null) ...[
        AccessCard(access: inventory.blockedBy),
        const SizedBox(height: AppSpace.s4),
        if (inventory.remembered) ...[
          const InlineBanner(
            tone: BannerTone.info,
            glyph: Lucide.clock,
            text: 'Оператор сейчас не отдаёт список узлов по этой подписке. '
                'Показан последний, который он присылал.',
          ),
          const SizedBox(height: AppSpace.s4),
        ],
      ],
      // Ошибка замера — уже переведённая строка; сырой текст ядра достаётся по
      // «Подробности». Повтор предлагаем только там, где отказ не объяснён
      // подпиской: под исчерпанным лимитом он вернёт то же самое.
      if (run.error != null) ...[
        FailureNotice.fromText(
          run.error!,
          onRetry: inventory.blockedBy == null ? _probe : null,
          payable: inventory.blockedBy != null,
        ),
        const SizedBox(height: AppSpace.s4),
      ],
    ];
  }

  /// Одна строка о том, чей замер показан и что он показал.
  ///
  /// «Работает N из M» — не украшение: на боевом флоте часть прокси мертва для
  /// клиента при живом ping, и число работающих — единственное место, где эта
  /// разница видна человеку сразу.
  String _probeSummary(DateTime? measuredAt, ConnectionProfile profile) {
    if (measuredAt == null) {
      return 'Пока не мерили: показаны задержки, которые сообщил оператор.';
    }
    final probe = profile.lastProbe;
    final judged = probe?.verdicts.length ?? 0;
    if (probe == null || judged == 0) {
      return 'Ваш замер: ${_timeText(measuredAt)}';
    }
    return 'Ваш замер: ${_timeText(measuredAt)} · работает '
        '${probe.workingCount} из $judged';
  }

  /// Запускает замер один раз на открытие экрана, когда узлы уже есть, а своих
  /// чисел ещё нет. Именно «после того как узлы показаны», а не «вместо того»:
  /// таблица рисуется из инвентаря немедленно.
  void _maybeAutoProbe(ExitInventory inventory) {
    if (_autoProbed || inventory.nodes.isEmpty) return;
    if (ref.read(probeRunProvider).measuring) return;
    if (ref.read(clientLatencyProvider).isNotEmpty) return;
    _autoProbed = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) ref.read(probeRunProvider.notifier).measure();
    });
  }

  Future<void> _refresh(ExitInventorySource source) {
    if (source == ExitInventorySource.importedSub) return _probe();
    if (source == ExitInventorySource.panelRest) {
      return ref.refresh(serversProvider.future);
    }
    return Future<void>.value();
  }

  /// Закрепляет страну (или снимает пин на `null`). Узел внутри страны выбирает
  /// контроллер: страна не доезжает до подключения ни в одном режиме.
  Future<void> _pickCountry(String? code) async {
    final outcome =
        await ref.read(exitSelectionControllerProvider).selectCountry(code);
    if (!mounted) return;
    _noteSync(outcome);
    if (!outcome.applied) {
      showCarambaToast(context, outcome.sync.message);
      return;
    }
    showCarambaToast(
      context,
      code == null
          ? 'Страна выхода: авто'
          : '${countryNameOf(code)}: автоподбор узла',
    );
  }

  /// Одиночный клик: только выбор. Возвращает, применился ли он, — двойному
  /// клику этого достаточно, чтобы решить, есть ли что подключать.
  Future<bool> _pickNode(ExitNode node) async {
    final outcome =
        await ref.read(exitSelectionControllerProvider).selectNode(node);
    if (!mounted) return false;
    _noteSync(outcome);
    if (!outcome.applied) {
      // Выбор не применился вовсе — причина уже названа на самом узле.
      showCarambaToast(context, outcome.sync.message);
      return false;
    }
    showCarambaToast(
      context,
      '${node.name.isEmpty ? node.key : node.name} выбран',
    );
    return true;
  }

  /// Двойной клик: выбрать И подключиться.
  ///
  /// Подключение идёт ТОЛЬКО после применённого выбора: подключаться по
  /// двойному клику на строке, которую не удалось закрепить, значит уводить
  /// туннель на чужой узел и показывать галочку на том, который человек
  /// выбирал.
  Future<void> _activate(ExitNode node) async {
    if (!await _pickNode(node)) return;
    if (!mounted) return;
    // Профиля нет — подключать нечем; выбор при этом уже сохранён и потерян не
    // будет.
    if (ref.read(activeConnectionProfileProvider) == null) return;
    await ref.read(vpnProvider.notifier).connect();
  }

  /// Баннер синхронизации поднимается только там, где панель ЕСТЬ и она не
  /// приняла выбор. Отсутствие панели в generic-режиме это не новость.
  void _noteSync(ExitSelectionOutcome outcome) {
    final reason = outcome.sync.reason;
    final worth = reason == ExitUnavailableReason.panelRejected ||
        reason == ExitUnavailableReason.panelUnavailable;
    setState(() => _syncNote = worth ? outcome.sync : null);
  }

  /// Замер идёт через ядро (`ProbeRunNotifier`): ход и результат живут в
  /// состоянии, а не в этом виджете, потому что числа нужны и строкам таблицы,
  /// и инвентарю — они не принадлежат экрану.
  Future<void> _probe() => ref.read(probeRunProvider.notifier).measure();

  /// Экран накладной: крестик снимает панель. Запасной выход — само
  /// «Подключение», если стека под экраном нет (диплинк, тест).
  void _close() {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.home);
    }
  }
}

/// Высота действия из карты окна; ширину кнопки определяет полная подпись.
const double _actionHeight = 44;

String _timeText(DateTime at) {
  String two(int v) => v.toString().padLeft(2, '0');
  return '${two(at.hour)}:${two(at.minute)}';
}

/// Пусто по-разному в разных режимах, и разница здесь важна: в импорте чинится
/// обновлением подписки, на панели — повтором запроса, а без профиля выбирать
/// не из чего вообще.
class _Empty extends StatelessWidget {
  final ExitInventorySource source;

  /// Доступ закрыт — тогда пустой список объясняется подпиской, а не молчанием
  /// оператора.
  final AccessState? access;

  const _Empty({required this.source, this.access});

  @override
  Widget build(BuildContext context) {
    final blocked = access;
    if (blocked != null) {
      return ScreenEmpty(
        glyph: Lucide.globe,
        title: 'Узлы сейчас недоступны',
        message: 'Оператор не отдаёт список узлов по этой подписке, пока '
            '${blocked.shortReason.toLowerCase()}. Список вернётся вместе с '
            'доступом.',
      );
    }
    return switch (source) {
      ExitInventorySource.importedSub => const ScreenEmpty(
          glyph: Lucide.globe,
          title: 'Узлов в подписке нет',
          message: 'Обновите подписку в разделе «Подключения».',
        ),
      ExitInventorySource.none => const ScreenEmpty(
          glyph: Lucide.globe,
          title: 'Профиль подключения не выбран',
          message: 'Добавьте подключение или подключите аккаунт панели.',
        ),
      // Протяжки на десктопе нет, и совет «потяните список» был бы указанием
      // на жест, которого у мыши не существует: повтор здесь — кнопка «Замерить
      // свой пинг» и обновление панели.
      _ => const ScreenEmpty(
          glyph: Lucide.globe,
          title: 'Серверы недоступны',
          message: 'Оператор не отдал ни одного узла.',
        ),
    };
  }
}

class _Loading extends StatelessWidget {
  const _Loading();

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.only(top: AppSpace.s12),
      child: Center(
        child: CircularProgressIndicator(strokeWidth: 2, color: c.textHi),
      ),
    );
  }
}

/// Список не загрузился. Экран называет причину словами и держит исходный текст
/// под «Подробности»: «Не удалось загрузить серверы» без причины — это ровно та
/// строка, из-за которой отказ по трафику диагностировали часами.
class _Error extends StatelessWidget {
  final Object error;
  final VoidCallback onRetry;

  const _Error({required this.error, required this.onRetry});

  @override
  Widget build(BuildContext context) {
    final failure = describeFailure(error);
    return Padding(
      padding: const EdgeInsets.only(top: AppSpace.s8),
      child: FailureNotice(
        message: failure?.text ?? 'Не удалось загрузить список серверов.',
        technical: failure?.technical,
        onRetry: (failure?.retryable ?? true) ? onRetry : null,
        payable: failure?.payable ?? false,
      ),
    );
  }
}

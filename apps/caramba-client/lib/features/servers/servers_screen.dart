import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/subscription.dart' show AccessState;
import 'package:caramba_client/features/servers/access_card.dart';
import 'package:caramba_client/features/servers/exit_node_list.dart';
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

/// Серверы. ОДИН список машин, страна — на строке.
///
/// Экран пережил две перестройки. Сначала он ветвился по режиму на две почти
/// одинаковые страницы (узлы импортированной подписки и узлы панели); режимы
/// свели в [exitInventoryProvider]. Потом выбор шёл через страну: список стран,
/// а внутри — узлы. Второй шаг убран по прямой просьбе владельца сервиса, и он
/// прав: узлов у оператора десяток, лишняя дверь перед ними ничего не даёт.
/// Страна не исчезла — она стоит В СТРОКЕ флагом и кодом, рядом с именем
/// машины, то есть там же, где делается выбор.
///
/// Недоступная машина остаётся в списке приглушённой и с причиной: строка,
/// пропавшая из списка, неотличима от машины, которой у оператора никогда не
/// было, и пользователь ищет её в обновлении приложения.
class ServersScreen extends ConsumerStatefulWidget {
  const ServersScreen({super.key});

  @override
  ConsumerState<ServersScreen> createState() => _ServersScreenState();
}

class _ServersScreenState extends ConsumerState<ServersScreen> {
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
      body: SafeArea(
        bottom: false,
        child: RefreshIndicator(
          onRefresh: () => _refresh(inventory.source),
          color: c.accent,
          backgroundColor: c.surface2,
          child: ListView(
            padding: const EdgeInsets.fromLTRB(
              AppSpace.s5,
              AppSpace.s5,
              AppSpace.s5,
              AppSpace.s20 + AppSpace.s6,
            ),
            children: [
              ScreenHead(
                'Серверы',
                trailing: IconBtn(
                  Lucide.x,
                  onTap: () => context.go(AppRoute.home),
                ),
              ),
              // Тот же гейт, что на Главной и в Настройках. Смена страны или
              // узла теперь применяется сама (окно тишины в
              // `auto_reconnect.dart`), и баннер здесь — её отчёт; он же
              // остаётся ручным для правок, которые ждут человека.
              if (ref.watch(reconnectRequiredProvider)) ...[
                const ReconnectBanner(),
                const SizedBox(height: AppSpace.s4),
              ],
              if (_syncNote != null) ...[
                InlineBanner(
                  tone: BannerTone.warning,
                  glyph: Lucide.alert,
                  text:
                      'Выбор применён на этом устройстве. '
                      '${_syncNote!.message}',
                ),
                const SizedBox(height: AppSpace.s4),
              ],
              // Отказ подписки стоит ВЫШЕ замера и списка: он объясняет и то,
              // почему строки не нажимаются, и почему замер не прошёл. Список
              // при этом остаётся на месте — исчерпанный трафик не отменяет
              // существования флота оператора.
              if (inventory.blockedBy != null) ...[
                AccessCard(access: inventory.blockedBy),
                const SizedBox(height: AppSpace.s4),
                if (inventory.remembered) ...[
                  const InlineBanner(
                    tone: BannerTone.info,
                    glyph: Lucide.clock,
                    text:
                        'Оператор сейчас не отдаёт список узлов по этой '
                        'подписке. Показан последний, который он присылал.',
                  ),
                  const SizedBox(height: AppSpace.s4),
                ],
              ],
              ..._probeHeader(inventory),
              if (inventory.loading && inventory.isEmpty)
                const _Loading()
              else if (inventory.error != null && inventory.isEmpty)
                _Error(
                  error: inventory.error!,
                  onRetry: () => _refresh(inventory.source),
                )
              else if (inventory.isEmpty)
                _Empty(source: inventory.source, access: inventory.blockedBy)
              else
                ExitNodeList(
                  onSelect: (node) =>
                      node == null ? _pickCountry(null) : _pickNode(node),
                ),
            ],
          ),
        ),
      ),
    );
  }

  /// Шапка замера — теперь в ОБОИХ режимах.
  ///
  /// Раньше она стояла только под импортированной подпиской, с доводом «у
  /// панели задержки приходят с сервером, мерить нечего». Довод был неверен:
  /// панель присылает RTT самого узла до его цели по heartbeat, то есть
  /// расстояние узла, а не пользователя. Пользователь просил свой пинг, и
  /// мерить его есть чем — тем же `probe`, что и в импорте.
  List<Widget> _probeHeader(ExitInventory inventory) {
    final c = context.c;
    final profile = ref.watch(activeConnectionProfileProvider);
    if (profile == null) return const <Widget>[];
    final run = ref.watch(probeRunProvider);
    final measuredAt = ref.watch(clientLatencyAtProvider);
    final nothingToMeasure = inventory.nodes.isEmpty;

    return <Widget>[
      if (inventory.source == ExitInventorySource.importedSub) ...[
        Text(
          profile.displayName.isEmpty
              ? 'Узлы импортированной подписки'
              : 'Узлы подписки «${profile.displayName}»',
          style: AppType.bodyMd.copyWith(color: c.textMed),
        ),
        const SizedBox(height: AppSpace.s4),
      ],
      GhostButton(
        label: run.measuring ? 'Меряю задержки' : 'Замерить свой пинг',
        icon: Lucide.gauge,
        onPressed: (run.measuring || nothingToMeasure) ? null : _probe,
      ),
      const SizedBox(height: AppSpace.s2),
      // Вход в автоподбор стоит РЯДОМ с замером, а не только в настройках:
      // человек, который смотрит на список узлов и не знает, какой выбрать, —
      // это ровно тот, кому нужен подбор.
      GhostButton(
        label: 'Подобрать лучший узел',
        icon: Lucide.route,
        onPressed: nothingToMeasure
            ? null
            : () => context.go('${AppRoute.settings}/autotune'),
      ),
      const SizedBox(height: AppSpace.s2),
      Text(
        _probeSummary(measuredAt, profile),
        style: AppType.bodySm.copyWith(color: c.textLow),
      ),
      const SizedBox(height: AppSpace.s5),
      // Ошибка замера — уже переведённая строка; сырой текст ядра лежит рядом
      // и достаётся по «Подробности». Повтор предлагаем только там, где отказ
      // не объяснён подпиской: под исчерпанным лимитом он вернёт то же самое.
      if (run.error != null) ...[
        FailureNotice.fromText(
          run.error!,
          onRetry: inventory.blockedBy == null ? _probe : null,
          payable: inventory.blockedBy != null,
        ),
        const SizedBox(height: AppSpace.s5),
      ],
    ];
  }

  /// Одна строка о том, чей замер показан и что он показал.
  ///
  /// «Работает N из M» — не украшение. На боевом флоте девять прокси из
  /// тринадцати мертвы для клиента (самоподписанный сертификат), и до
  /// вердиктов ядра список показывал их живыми: у них был ping. Число
  /// работающих — единственное место, где эта разница видна человеку сразу.
  String _probeSummary(DateTime? measuredAt, ConnectionProfile profile) {
    if (measuredAt == null) {
      // Пока своего замера не было, числа в списке принадлежат оператору, и
      // сказать это надо один раз словами, а не только подписью у цифр.
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
  /// чисел ещё нет.
  ///
  /// Именно «после того как узлы показаны», а не «вместо того»: список рисуется
  /// из инвентаря немедленно, а замер добавляет к нему числа по мере готовности.
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
  /// контроллер: страна не доезжает до подключения ни в одном режиме, поэтому
  /// превращать её в узел здесь, вторым вызовом с экрана, значило бы держать эту
  /// логику в двух местах — и разойтись с ней в третьем.
  Future<void> _pickCountry(String? code) async {
    final outcome = await ref
        .read(exitSelectionControllerProvider)
        .selectCountry(code);
    if (!mounted) return;
    _noteSync(outcome);
    if (!outcome.applied) {
      // Страну не удалось разрешить в узел. Молчаливая галочка здесь была бы
      // хуже всего: выбор не применён, и пользователь обязан это увидеть.
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

  Future<void> _pickNode(ExitNode node) async {
    final outcome = await ref
        .read(exitSelectionControllerProvider)
        .selectNode(node);
    if (!mounted) return;
    _noteSync(outcome);
    if (!outcome.applied) {
      // Выбор не применился вовсе — причина уже названа на самом узле.
      showCarambaToast(context, outcome.sync.message);
      return;
    }
    showCarambaToast(
      context,
      '${node.name.isEmpty ? node.key : node.name} выбран',
    );
  }

  /// Баннер синхронизации поднимается только там, где панель ЕСТЬ и она не
  /// приняла выбор. Отсутствие панели в generic-режиме это не новость и не
  /// повод для предупреждения на каждое нажатие.
  void _noteSync(ExitSelectionOutcome outcome) {
    final reason = outcome.sync.reason;
    final worth =
        reason == ExitUnavailableReason.panelRejected ||
        reason == ExitUnavailableReason.panelUnavailable;
    setState(() => _syncNote = worth ? outcome.sync : null);
  }

  /// Замер идёт через ядро (`ProbeRunNotifier`): подписка при необходимости
  /// сначала отдаётся ему на разбор, затем `probe`. Ход замера и результат
  /// живут в состоянии, а не в этом виджете, потому что числа нужны и строкам
  /// списка, и инвентарю — они не принадлежат экрану.
  Future<void> _probe() => ref.read(probeRunProvider.notifier).measure();
}

/// Первый уровень: страны выхода. «Авто» сверху, затем страны в порядке
/// инвентаря (доступные раньше недоступных, внутри — по лучшему пингу).
/// Пусто по-разному в разных режимах, и разница здесь важна: в импорте чинится
/// обновлением подписки, на панели — повтором запроса, а без профиля выбирать
/// не из чего вообще.
class _Empty extends StatelessWidget {
  final ExitInventorySource source;

  /// Доступ закрыт — тогда пустой список объясняется подпиской, а не молчанием
  /// оператора, и совет «потяните, чтобы повторить» был бы враньём.
  final AccessState? access;

  const _Empty({required this.source, this.access});

  @override
  Widget build(BuildContext context) {
    final blocked = access;
    if (blocked != null) {
      return ScreenEmpty(
        glyph: Lucide.globe,
        title: 'Узлы сейчас недоступны',
        message:
            'Оператор не отдаёт список узлов по этой подписке, пока '
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
        message: 'Импортируйте подписку или войдите в аккаунт панели.',
      ),
      _ => const ScreenEmpty(
        glyph: Lucide.globe,
        title: 'Серверы недоступны',
        message:
            'Оператор не отдал ни одного узла. Потяните список, чтобы '
            'повторить запрос.',
      ),
    };
  }
}

String _timeText(DateTime at) {
  String two(int v) => v.toString().padLeft(2, '0');
  return '${two(at.hour)}:${two(at.minute)}';
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

/// Список не загрузился. Экран называет причину словами и держит исходный
/// текст под «Подробности»: «Не удалось загрузить серверы» без причины — это
/// ровно та строка, из-за которой отказ по трафику диагностировали часами.
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

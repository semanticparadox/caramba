import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_error.dart';
import 'package:caramba_client/state/probe_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/vpn/vpn_models.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Автоподбор настроек (демо §AUTOTUNE).
///
/// Два режима, по [isNativeVpnProvider]:
///   * нативное ядро — НАСТОЯЩИЙ прогон: подписка отдаётся ядру на разбор,
///     затем `probe` с таймаутом 6 с, узел с наименьшей задержкой закрепляется
///     на профиле (`selectedServerId`). Никаких таймеров-имитаций;
///   * мок — прежняя трёхшаговая имитация, чтобы UI работал без ядра.
class AutotuneScreen extends ConsumerStatefulWidget {
  /// Если экран вызван повторно из настроек, по «Продолжить» возвращаемся назад,
  /// а не на Home (роутер уже на home-ветке).
  final bool fromSettings;
  const AutotuneScreen({this.fromSettings = false, super.key});

  @override
  ConsumerState<AutotuneScreen> createState() => _AutotuneScreenState();
}

class _AutotuneScreenState extends ConsumerState<AutotuneScreen> {
  static const _mockSteps = [
    'Измеряю задержку до серверов',
    'Проверяю, какие протоколы проходят',
    'Выбираю лучший маршрут',
  ];

  static const _nativeSteps = [
    'Разбираю подписку',
    'Меряю задержки узлов',
    'Выбираю самый быстрый узел',
  ];

  int _current = 0; // индекс активного шага
  bool _done = false;
  String? _error;

  /// Победитель реального замера (нативный путь). `null` в моке и до финиша.
  ProbeResult? _best;

  /// Сколько узлов ответило из скольких (нативный путь).
  int _reachable = 0;
  int _total = 0;

  final List<Timer> _timers = [];

  List<String> get _steps =>
      ref.read(isNativeVpnProvider) ? _nativeSteps : _mockSteps;

  @override
  void initState() {
    super.initState();
    // Запуск после первого кадра: нативный путь читает провайдеры и делает
    // await, а в initState провайдеры трогать рано.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _run();
    });
  }

  void _run() {
    setState(() {
      _current = 0;
      _done = false;
      _error = null;
      _best = null;
    });
    if (ref.read(isNativeVpnProvider)) {
      unawaited(_runNative());
    } else {
      _runMock();
    }
  }

  /// Имитация для dev-сборок без ядра: три шага по таймеру и эвристический
  /// результат. Никогда не выполняется, когда бэкендом стоит настоящее ядро.
  void _runMock() {
    for (var i = 0; i < _mockSteps.length; i++) {
      _timers.add(
        Timer(Duration(milliseconds: 700 + 950 * (i + 1)), () {
          if (!mounted) return;
          setState(() => _current = i + 1);
          if (i == _mockSteps.length - 1) {
            // Выбор: рекомендованный протокол + gVisor-стек (как в autotune ядра).
            ref
                .read(coreConfigProvider.notifier)
                .applyAutotune(protocol: 1, stack: 2);
            setState(() => _done = true);
          }
        }),
      );
    }
  }

  /// Настоящий прогон через ядро.
  Future<void> _runNative() async {
    final profile = ref.read(activeConnectionProfileProvider);
    if (profile == null) {
      setState(() {
        _error =
            'Нет активного профиля. Импортируйте подписку или войдите в панель.';
        _done = true;
      });
      return;
    }
    try {
      // Шаг 1 закрывается, когда конфиг ушёл в ядро; probeProfile делает это
      // внутри, поэтому шаг переключаем до вызова.
      setState(() => _current = 1);
      final results = await probeProfile(
        ref.read(vpnConnectionProvider),
        profile,
      );
      if (!mounted) return;
      setState(() => _current = 2);
      final best = bestOf(results);
      if (best != null) {
        await ref
            .read(connectionProfilesProvider.notifier)
            .setSelectedServer(profile.id, best.id);
      }
      await ref
          .read(connectionProfilesProvider.notifier)
          .setProbe(profile.id, ProbeSnapshot.fromResults(results));
      if (!mounted) return;
      setState(() {
        _current = _nativeSteps.length;
        _best = best;
        _total = results.length;
        _reachable = results.where((r) => !r.timedOut).length;
        _error = best == null && results.isNotEmpty
            ? 'Ни один узел не ответил. Проверьте сеть или обновите подписку.'
            : (results.isEmpty ? 'Ядро не вернуло ни одного узла.' : null);
        _done = true;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = coreErrorText(e) ?? 'Не удалось замерить узлы.';
        _done = true;
      });
    }
  }

  void _continue() {
    ref.read(firstRunProvider.notifier).done();
    if (widget.fromSettings) {
      if (context.canPop()) {
        context.pop();
      } else {
        context.go(AppRoute.home);
      }
    } else {
      context.go(AppRoute.home);
    }
  }

  @override
  void dispose() {
    for (final t in _timers) {
      t.cancel();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final steps = _steps;

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
                  _done ? 'Готово' : 'Подбираем настройки',
                  textAlign: TextAlign.center,
                  style: AppType.headline.copyWith(color: c.textHi),
                ),
                const SizedBox(height: AppSpace.s2),
                Text(
                  _done
                      ? 'Можно подключаться. Поменять можно в настройках в любой момент.'
                      : 'Один раз проверим сеть и подберём лучший протокол и сервер.',
                  textAlign: TextAlign.center,
                  style: AppType.bodyMd.copyWith(color: c.textMed),
                ),
                const SizedBox(height: AppSpace.s6),
                if (!_done)
                  ...List.generate(steps.length, (i) => _stepRow(steps, i))
                else ...[
                  if (_error != null) ...[
                    _Notice(text: _error!),
                    const SizedBox(height: AppSpace.s5),
                  ],
                  _result(),
                  const SizedBox(height: AppSpace.s5),
                  FilledButton(
                    onPressed: _continue,
                    child: const Text('Продолжить'),
                  ),
                  const SizedBox(height: AppSpace.s1),
                  QuietButton(
                    label: 'Настрою вручную',
                    color: c.textMed,
                    onPressed: _continue,
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// Итог. В нативном режиме это результат настоящего замера, в моке — прежняя
  /// сводка выбранных настроек.
  Widget _result() {
    final protocols = ref.watch(protocolsProvider);
    final cfg = ref.watch(coreConfigProvider);

    if (ref.read(isNativeVpnProvider)) {
      final best = _best;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const SectionTitle(
            'Результат замера',
            padding: EdgeInsets.only(bottom: AppSpace.s3),
          ),
          RowsGroup(
            children: [
              CRow(
                icon: Lucide.globe,
                label: 'Узел',
                value: best == null
                    ? 'Авто'
                    : (best.name.isEmpty ? best.id : best.name),
              ),
              CRow(
                icon: Lucide.gauge,
                label: 'Задержка',
                value: best == null ? '-' : '${best.latencyMs} мс',
                mono: true,
              ),
              CRow(
                icon: Lucide.layers,
                label: 'Ответили',
                value: _total == 0 ? '-' : '$_reachable из $_total',
                mono: true,
              ),
            ],
          ),
        ],
      );
    }

    final servers = ref.watch(serversProvider).valueOrNull ?? const [];
    final best = servers.isEmpty ? null : servers.first;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SectionTitle(
          'Выбрано',
          padding: EdgeInsets.only(bottom: AppSpace.s3),
        ),
        RowsGroup(
          children: [
            CRow(
              icon: protocols[cfg.protocol].icon,
              label: 'Протокол',
              value: protocols[cfg.protocol].name,
            ),
            CRow(
              icon: Lucide.globe,
              label: 'Сервер',
              value: best == null
                  ? 'Авто'
                  : '${best.name}${best.pingMs != null ? ' · ${best.pingMs} мс' : ''}',
            ),
            CRow(
              icon: Lucide.layers,
              label: 'Сетевой стек',
              value: CoreOption.stacks[cfg.stack].name,
            ),
          ],
        ),
      ],
    );
  }

  Widget _stepRow(List<String> steps, int i) {
    final c = context.c;
    final done = i < _current;
    return Container(
      margin: const EdgeInsets.only(bottom: AppSpace.s2),
      padding: const EdgeInsets.symmetric(
        horizontal: AppSpace.s4,
        vertical: AppSpace.s4,
      ),
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
            child: done
                ? LucideIcon(Lucide.check, color: c.success, size: 18)
                : CircularProgressIndicator(
                    strokeWidth: 2,
                    valueColor: AlwaysStoppedAnimation(c.textHi),
                    backgroundColor: c.borderStrong,
                  ),
          ),
          const SizedBox(width: AppSpace.s3),
          Expanded(
            child: Text(
              steps[i],
              style: AppType.bodyMd.copyWith(
                color: done ? c.textMed : c.textHi,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// Плоское предупреждение под итогом: замер прошёл, но результат не тот,
/// которого ждали. Это не отказ экрана, дальше пройти можно.
class _Notice extends StatelessWidget {
  final String text;
  const _Notice({required this.text});

  @override
  Widget build(BuildContext context) {
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
          LucideIcon(Lucide.alert, color: c.warning, size: 18),
          const SizedBox(width: AppSpace.s3),
          Expanded(
            child: Text(text, style: AppType.bodySm.copyWith(color: c.textMed)),
          ),
        ],
      ),
    );
  }
}

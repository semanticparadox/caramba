import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Автоподбор настроек (демо §AUTOTUNE).
///
/// Прогоняет 3 шага (имитация измерения сети, проверки протоколов, выбора
/// маршрута), затем показывает выбранную конфигурацию и применяет её в
/// [coreConfigProvider]. На проде шаги питает caramba-core `AutoTune(Prober)`.
class AutotuneScreen extends ConsumerStatefulWidget {
  /// Если экран вызван повторно из настроек, по «Продолжить» возвращаемся назад,
  /// а не на Home (роутер уже на home-ветке).
  final bool fromSettings;
  const AutotuneScreen({this.fromSettings = false, super.key});

  @override
  ConsumerState<AutotuneScreen> createState() => _AutotuneScreenState();
}

class _AutotuneScreenState extends ConsumerState<AutotuneScreen> {
  static const _steps = [
    'Измеряю задержку до серверов',
    'Проверяю, какие протоколы проходят',
    'Выбираю лучший маршрут',
  ];

  int _current = 0; // индекс активного шага
  bool _done = false;
  final List<Timer> _timers = [];

  @override
  void initState() {
    super.initState();
    _run();
  }

  void _run() {
    setState(() {
      _current = 0;
      _done = false;
    });
    for (var i = 0; i < _steps.length; i++) {
      _timers.add(Timer(Duration(milliseconds: 700 + 950 * (i + 1)), () {
        if (!mounted) return;
        setState(() => _current = i + 1);
        if (i == _steps.length - 1) _finish();
      }));
    }
  }

  void _finish() {
    // Выбор: рекомендованный протокол + gVisor-стек (как в caramba-core autotune).
    ref.read(coreConfigProvider.notifier).applyAutotune(protocol: 1, stack: 2);
    setState(() => _done = true);
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
    final protocols = ref.watch(protocolsProvider);
    final cfg = ref.watch(coreConfigProvider);
    final servers = ref.watch(serversProvider).valueOrNull ?? const [];
    final best = servers.isEmpty ? null : servers.first;

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
                  ...List.generate(_steps.length, (i) => _stepRow(i))
                else ...[
                  SectionTitle('Выбрано',
                      padding: const EdgeInsets.only(bottom: AppSpace.s3)),
                  RowsGroup(children: [
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
                  ]),
                  const SizedBox(height: AppSpace.s5),
                  FilledButton(
                      onPressed: _continue, child: const Text('Продолжить')),
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

  Widget _stepRow(int i) {
    final c = context.c;
    final done = i < _current;
    return Container(
      margin: const EdgeInsets.only(bottom: AppSpace.s2),
      padding: const EdgeInsets.symmetric(
          horizontal: AppSpace.s4, vertical: AppSpace.s4),
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
              _steps[i],
              style: AppType.bodyMd
                  .copyWith(color: done ? c.textMed : c.textHi),
            ),
          ),
        ],
      ),
    );
  }
}

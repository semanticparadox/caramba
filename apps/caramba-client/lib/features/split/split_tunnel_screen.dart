import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Раздельное туннелирование (per-app split). Режим (off / только выбранные /
/// кроме выбранных) + поиск и список приложений с пер-аппными тумблерами.
/// Пишет в [coreConfigProvider] -> caramba-core Policy.Split.
class SplitTunnelScreen extends ConsumerStatefulWidget {
  const SplitTunnelScreen({super.key});

  @override
  ConsumerState<SplitTunnelScreen> createState() => _SplitTunnelScreenState();
}

class _SplitTunnelScreenState extends ConsumerState<SplitTunnelScreen> {
  final _searchCtrl = TextEditingController();
  String _query = '';

  @override
  void dispose() {
    _searchCtrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final cfg = ref.watch(coreConfigProvider);
    final cfgN = ref.read(coreConfigProvider.notifier);
    final apps = ref.watch(installedAppsProvider);
    final enabled = cfg.splitMode != SplitMode.off;

    final filtered = _query.isEmpty
        ? apps
        : apps
              .where((a) => a.name.toLowerCase().contains(_query.toLowerCase()))
              .toList();

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                AppSpace.s5,
                AppSpace.s5,
                AppSpace.s5,
                0,
              ),
              child: ScreenHead(
                'Раздельное туннелирование',
                trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
              ),
            ),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.fromLTRB(
                  AppSpace.s5,
                  0,
                  AppSpace.s5,
                  AppSpace.s12,
                ),
                children: [
                  Text(
                    'Выберите, какие приложения идут через VPN, а какие напрямую.',
                    style: AppType.bodyMd.copyWith(color: c.textMed),
                  ),
                  const SizedBox(height: AppSpace.s4),
                  // Режим
                  for (final m in SplitMode.values)
                    ListItemCard(
                      leading: IBox(switch (m) {
                        SplitMode.off => Lucide.shield,
                        SplitMode.onlySelected => Lucide.appWindow,
                        SplitMode.bypassSelected => Lucide.route,
                      }),
                      title: m.title,
                      subtitle: m.desc,
                      selected: cfg.splitMode == m,
                      onTap: () => cfgN.setSplitMode(m),
                    ),

                  if (enabled) ...[
                    SectionTitle('Приложения'),
                    TextField(
                      controller: _searchCtrl,
                      onChanged: (v) => setState(() => _query = v),
                      style: AppType.bodyMd.copyWith(color: c.textHi),
                      decoration: InputDecoration(
                        hintText: 'Поиск приложений',
                        prefixIcon: Padding(
                          padding: const EdgeInsets.all(AppSpace.s3),
                          child: LucideIcon(
                            Lucide.search,
                            color: c.textMed,
                            size: 18,
                          ),
                        ),
                        prefixIconConstraints: const BoxConstraints(
                          minWidth: 44,
                          minHeight: 44,
                        ),
                      ),
                    ),
                    const SizedBox(height: AppSpace.s3),
                    if (filtered.isEmpty)
                      Padding(
                        padding: const EdgeInsets.only(top: AppSpace.s6),
                        child: Center(
                          child: Text(
                            'Ничего не найдено',
                            style: AppType.bodyMd.copyWith(color: c.textMed),
                          ),
                        ),
                      )
                    else
                      RowsGroup(
                        children: [
                          for (final a in filtered)
                            _AppRow(
                              app: a,
                              on: cfg.splitApps.contains(a.id),
                              onChanged: (_) => cfgN.toggleSplitApp(a.id),
                            ),
                        ],
                      ),
                  ],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go('/settings');
    }
  }
}

class _AppRow extends StatelessWidget {
  final SplitApp app;
  final bool on;
  final ValueChanged<bool> onChanged;
  const _AppRow({required this.app, required this.on, required this.onChanged});

  @override
  Widget build(BuildContext context) {
    return CRow(
      icon: app.icon,
      label: app.name,
      trailing: Switch(value: on, onChanged: onChanged),
    );
  }
}

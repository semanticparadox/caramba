import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Раздельное туннелирование (per-app split). Режим (off / только выбранные /
/// кроме выбранных), домены мимо туннеля и список приложений с пер-аппными
/// тумблерами. Пишет в [coreConfigProvider] -> caramba-core Policy.Split.
///
/// Список приложений на desktop пока демонстрационный (платформенного канала
/// перечисления приложений нет), поэтому домены — рабочий инструмент здесь и
/// сейчас: они уходят в `split.bypassDomains` и работают на всех платформах.
class SplitTunnelScreen extends ConsumerStatefulWidget {
  const SplitTunnelScreen({super.key});

  @override
  ConsumerState<SplitTunnelScreen> createState() => _SplitTunnelScreenState();
}

class _SplitTunnelScreenState extends ConsumerState<SplitTunnelScreen> {
  final _searchCtrl = TextEditingController();
  final _domainsCtrl = TextEditingController();
  String _query = '';

  @override
  void initState() {
    super.initState();
    _domainsCtrl.text = ref.read(coreConfigProvider).bypassDomains;
  }

  @override
  void dispose() {
    _searchCtrl.dispose();
    _domainsCtrl.dispose();
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
                      leading: IBox(_modeGlyph(m)),
                      title: m.title,
                      subtitle: m.desc,
                      selected: cfg.splitMode == m,
                      onTap: () => cfgN.setSplitMode(m),
                    ),

                  if (ref.watch(reconnectRequiredProvider)) ...[
                    const SizedBox(height: AppSpace.s3),
                    const ReconnectBanner(),
                  ],

                  if (enabled) ...[
                    const SectionTitle('Домены мимо туннеля'),
                    TextField(
                      controller: _domainsCtrl,
                      minLines: 2,
                      maxLines: 5,
                      style: AppType.monoMd.copyWith(color: c.textHi),
                      onChanged: cfgN.setBypassDomains,
                      decoration: const InputDecoration(
                        hintText: 'example.com, bank.ru\nmail.local',
                      ),
                    ),
                    const SizedBox(height: AppSpace.s2),
                    Text(
                      'Через запятую или с новой строки. Эти домены всегда идут '
                      'напрямую, мимо туннеля.',
                      style: AppType.bodySm.copyWith(color: c.textLow),
                    ),
                    const SizedBox(height: AppSpace.s5),
                    const SectionTitle('Приложения'),
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

  /// Глиф режима: вынесен из билда, чтобы switch не жил внутри аргумента.
  static String _modeGlyph(SplitMode m) => switch (m) {
    SplitMode.off => Lucide.shield,
    SplitMode.onlySelected => Lucide.appWindow,
    SplitMode.bypassSelected => Lucide.route,
  };

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

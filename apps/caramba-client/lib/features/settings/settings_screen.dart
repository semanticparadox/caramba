import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Настройки (демо §SETTINGS): подключение, сеть и ядро, автонастройка, вид,
/// аккаунт. Все переключатели/пикеры пишут в [coreConfigProvider]/[settings].
class SettingsScreen extends ConsumerWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final cfg = ref.watch(coreConfigProvider);
    final cfgN = ref.read(coreConfigProvider.notifier);
    final settings = ref.watch(settingsProvider);
    final settingsN = ref.read(settingsProvider.notifier);

    final protocols = ref.watch(protocolsProvider);
    final modes = ref.watch(routingModesProvider);
    final isLight = settings.themeMode == ThemeMode.light;

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: ListView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s20 + AppSpace.s6,
          ),
          children: [
            const ScreenHead('Настройки'),

            const SectionTitle(
              'Подключение',
              padding: EdgeInsets.only(bottom: AppSpace.s3),
            ),
            RowsGroup(
              children: [
                CRow(
                  icon: Lucide.shield,
                  label: 'Протокол',
                  value: protocols[cfg.protocol].name,
                  chevron: true,
                  onTap: () => context.go(AppRoute.protocol),
                ),
                CRow(
                  icon: Lucide.route,
                  label: 'Маршрутизация',
                  value: modes[cfg.route].name,
                  chevron: true,
                  onTap: () async {
                    final i = await showPickerSheet(
                      context: context,
                      title: 'Маршрутизация',
                      subtitle: 'Что идёт через VPN, а что напрямую.',
                      options: modes
                          .map(
                            (m) => (
                              name: m.name,
                              desc: m.desc,
                              icon: m.icon as String?,
                            ),
                          )
                          .toList(),
                      selected: cfg.route,
                    );
                    if (i != null) cfgN.setRoute(i);
                  },
                ),
                CRow(
                  label: 'Kill-switch',
                  trailing: Switch(
                    value: cfg.killSwitch,
                    onChanged: cfgN.setKillSwitch,
                  ),
                ),
                CRow(
                  label: 'Автоподключение',
                  trailing: Switch(
                    value: cfg.autoConnect,
                    onChanged: cfgN.setAutoConnect,
                  ),
                ),
              ],
            ),

            const SectionTitle('Сеть и ядро'),
            RowsGroup(
              children: [
                CRow(
                  icon: Lucide.layers,
                  label: 'Сетевой стек (TUN)',
                  value: CoreOption.stacks[cfg.stack].name,
                  chevron: true,
                  onTap: () async {
                    final i = await _pickCore(
                      context,
                      'Сетевой стек (TUN)',
                      'Как приложение поднимает туннель в системе.',
                      CoreOption.stacks,
                      cfg.stack,
                    );
                    if (i != null) cfgN.setStack(i);
                  },
                ),
                CRow(
                  icon: Lucide.net,
                  label: 'DNS-резолвер',
                  value: CoreOption.dns[cfg.dns].name,
                  chevron: true,
                  onTap: () async {
                    final i = await _pickCore(
                      context,
                      'DNS-резолвер',
                      'Кто резолвит домены внутри туннеля.',
                      CoreOption.dns,
                      cfg.dns,
                    );
                    if (i != null) cfgN.setDns(i);
                  },
                ),
                CRow(
                  label: 'MTU',
                  value: CoreOption.mtu[cfg.mtu].name,
                  chevron: true,
                  onTap: () async {
                    final i = await _pickCore(
                      context,
                      'MTU',
                      'Размер пакета. Меньше значение стабильнее, больше быстрее.',
                      CoreOption.mtu,
                      cfg.mtu,
                    );
                    if (i != null) cfgN.setMtu(i);
                  },
                ),
                CRow(
                  label: 'Fake-IP',
                  trailing: Switch(
                    value: cfg.fakeIp,
                    onChanged: cfgN.setFakeIp,
                  ),
                ),
                CRow(
                  label: 'IPv6',
                  trailing: Switch(value: cfg.ipv6, onChanged: cfgN.setIpv6),
                ),
                CRow(
                  label: 'Раздельное туннелирование',
                  value: cfg.splitMode == SplitMode.off
                      ? 'Выкл'
                      : '${cfg.splitCount} прил.',
                  chevron: true,
                  onTap: () => context.go(AppRoute.splitTunnel),
                ),
              ],
            ),

            const SectionTitle('Автонастройка'),
            RowsGroup(
              children: [
                CRow(
                  icon: Lucide.gauge,
                  label: 'Подобрать настройки заново',
                  chevron: true,
                  onTap: () => context.go('${AppRoute.settings}/autotune'),
                ),
              ],
            ),

            const SectionTitle('Поддержка'),
            RowsGroup(
              children: [
                CRow(
                  icon: Lucide.lifeBuoy,
                  label: 'Запросы в поддержку',
                  chevron: true,
                  onTap: () => context.go(AppRoute.tickets),
                ),
              ],
            ),

            const SectionTitle('Вид'),
            RowsGroup(
              children: [
                CRow(
                  icon: Lucide.sun,
                  label: 'Светлая тема',
                  trailing: Switch(
                    value: isLight,
                    onChanged: (v) => settingsN.setThemeMode(
                      v ? ThemeMode.light : ThemeMode.dark,
                    ),
                  ),
                ),
              ],
            ),

            const SizedBox(height: AppSpace.s5),
            QuietButton(
              label: 'Выйти из аккаунта',
              onPressed: () async {
                if (ref.read(vpnProvider).isConnected) {
                  await ref.read(vpnProvider.notifier).disconnect();
                }
                ref.read(firstRunProvider.notifier).reset();
                await ref.read(authProvider.notifier).logout();
              },
            ),
          ],
        ),
      ),
    );
  }

  Future<int?> _pickCore(
    BuildContext context,
    String title,
    String sub,
    List<CoreOption> opts,
    int selected,
  ) {
    return showPickerSheet(
      context: context,
      title: title,
      subtitle: sub,
      options: opts
          .map((o) => (name: o.name, desc: o.desc, icon: null as String?))
          .toList(),
      selected: selected,
    );
  }
}

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/features/csm/config_age_card.dart';
import 'package:caramba_client/features/csm/keep_or_revert_card.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/features/settings/csm_write_status_note.dart';
import 'package:caramba_client/features/settings/enhancements_summary.dart';
import 'package:caramba_client/features/settings/route_report.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/vpn/core_policy.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
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
    final isLight = settings.themeMode == ThemeMode.light;
    final tunnelMode = ref.watch(tunnelModeProvider);
    final authed = ref.watch(authProvider).stage == AuthStage.authenticated;

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

            // Политика ядра применяется при следующем поднятии туннеля, а не
            // на лету: пока правки не применены, показываем это явно.
            if (ref.watch(reconnectRequiredProvider)) ...[
              const ReconnectBanner(),
              const SizedBox(height: AppSpace.s4),
            ],

            // Липкая ошибка и возраст конфигурации (INV-13, INV-21), а следом
            // карточки «Оставить или Вернуть» (INV-22). Они висят, пока
            // пользователь не ответит, и навигацией не закрываются.
            const CsmConfigAgeCard(),
            const CsmPendingChangesSection(),

            const SectionTitle(
              'Подключение',
              padding: EdgeInsets.only(bottom: AppSpace.s3),
            ),
            RowsGroup(
              children: [
                CRow(
                  icon: Lucide.shield,
                  label: 'Тип подключения',
                  value: protocols[cfg.protocol].name,
                  chevron: true,
                  trailing: const CsmProvenanceTag(
                    settingKey: CsmSettingKey.protocol,
                  ),
                  onTap: () => context.go(AppRoute.protocol),
                ),
                // Одна строка вместо двух.
                //
                // «Маршрутизация» и «Раздельное туннелирование» жили в разных
                // разделах, хотя решают один вопрос — что и как идёт через
                // туннель, — и ни одно из двух имён не говорило человеку, что
                // за ним. Обе ведут на «Улучшения», где блок рекламы, список
                // сайтов и режим страны лежат вместе и подписаны тем, что
                // ядро реально применило.
                //
                // Значение — не [CRow], а [_EnhancementsSummaryRow]: сводка
                // складывается из режима, счётчика сайтов и статуса рекламы
                // (см. [enhancementsSummary]) и легко перерастает половину
                // строки, которую [CRow] оставляет колонке значения. На узком
                // экране это резало «только 1 сайт · без рекламы» до «...
                // без ре…» посреди слова — [CRow] общий для всего приложения
                // и здесь не трогается (тот же выбор уже сделан в
                // [AppliedRouteCard] для собственных длинных строк).
                _EnhancementsSummaryRow(
                  value: enhancementsSummary(
                    cfg,
                    applied: ref.watch(appliedRouteProvider).valueOrNull,
                  ),
                  trailing: const CsmProvenanceTag(
                    settingKey: CsmSettingKey.preset,
                  ),
                  onTap: () => context.go(AppRoute.splitTunnel),
                ),
                CRow(
                  label: 'Kill-switch',
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const CsmProvenanceTag(
                        settingKey: CsmSettingKey.killSwitch,
                      ),
                      Switch(
                        value: cfg.killSwitch,
                        onChanged: (v) =>
                            CsmSettingsBridge.setKillSwitch(ref, v),
                      ),
                    ],
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
                  trailing: const CsmProvenanceTag(
                    settingKey: CsmSettingKey.stack,
                  ),
                  onTap: () async {
                    final i = await _pickCore(
                      context,
                      'Сетевой стек (TUN)',
                      'Как приложение поднимает туннель в системе.',
                      CoreOption.stacks,
                      cfg.stack,
                    );
                    if (i != null) CsmSettingsBridge.setStack(ref, i);
                  },
                ),
                CRow(
                  icon: Lucide.net,
                  label: 'DNS-резолвер',
                  value: CoreOption.dns[cfg.dns].name,
                  chevron: true,
                  trailing: const CsmProvenanceTag(
                    settingKey: CsmSettingKey.dnsNameservers,
                  ),
                  onTap: () async {
                    final i = await _pickCore(
                      context,
                      'DNS-резолвер',
                      'Кто резолвит домены внутри туннеля.',
                      CoreOption.dns,
                      cfg.dns,
                    );
                    if (i != null) CsmSettingsBridge.setDns(ref, i);
                  },
                ),
                CRow(
                  label: 'MTU',
                  value: CoreOption.mtu[cfg.mtu].name,
                  chevron: true,
                  trailing: const CsmProvenanceTag(
                    settingKey: CsmSettingKey.mtu,
                  ),
                  onTap: () async {
                    final i = await _pickCore(
                      context,
                      'MTU',
                      'Размер пакета. Меньше значение стабильнее, больше быстрее.',
                      CoreOption.mtu,
                      cfg.mtu,
                    );
                    if (i != null) CsmSettingsBridge.setMtu(ref, i);
                  },
                ),
                CRow(
                  label: 'Fake-IP',
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const CsmProvenanceTag(settingKey: CsmSettingKey.fakeIp),
                      Switch(
                        value: cfg.fakeIp,
                        onChanged: (v) => CsmSettingsBridge.setFakeIp(ref, v),
                      ),
                    ],
                  ),
                ),
                CRow(
                  label: 'IPv6',
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const CsmProvenanceTag(settingKey: CsmSettingKey.ipv6),
                      Switch(
                        value: cfg.ipv6,
                        onChanged: (v) => CsmSettingsBridge.setIpv6(ref, v),
                      ),
                    ],
                  ),
                ),
                CRow(
                  icon: Lucide.route,
                  label: 'Захват трафика',
                  value: tunnelMode == TunnelMode.tun
                      ? 'Системный TUN'
                      : 'Локальный прокси',
                  chevron: true,
                  onTap: () async {
                    final i = await showPickerSheet(
                      context: context,
                      title: 'Захват трафика',
                      subtitle:
                          'TUN заворачивает весь трафик системы и требует прав. '
                          'Прокси поднимает 127.0.0.1:$kMixedPort без прав.',
                      options: const [
                        (
                          name: 'Системный TUN',
                          desc:
                              'Весь трафик устройства. Нужны права '
                              'администратора или системное расширение.',
                          icon: Lucide.shield as String?,
                        ),
                        (
                          name: 'Локальный прокси',
                          desc:
                              'SOCKS5 и HTTP на 127.0.0.1:7890. Без прав, '
                              'трафик направляют приложения или система.',
                          icon: Lucide.net as String?,
                        ),
                      ],
                      selected: tunnelMode == TunnelMode.tun ? 0 : 1,
                    );
                    if (i != null) {
                      ref
                          .read(tunnelModeProvider.notifier)
                          .set(i == 0 ? TunnelMode.tun : TunnelMode.proxy);
                    }
                  },
                ),
              ],
            ),

            // Судьба записи настроек. Без неё пользователь менял настройку,
            // локальное значение применялось, запись молча не уходила, и экран
            // об этом не говорил ничего.
            const CsmWriteStatusNote(),

            // Раздел показывается только профилю, который закрепил корневой
            // ключ. Четыре строки, каждая из которых открывает пустое
            // состояние, это не прозрачность, а четыре тупика; они появляются
            // ровно тогда, когда за ними есть что показать.
            if (ref.watch(csmProfileStateProvider) != null) ...[
              const SectionTitle('Проверка и прозрачность'),
              RowsGroup(
                children: [
                  CRow(
                    icon: Lucide.key,
                    label: 'Оператор',
                    value: 'отпечаток и энроллмент',
                    chevron: true,
                    onTap: () => context.go(AppRoute.csmOperator),
                  ),
                  CRow(
                    icon: Lucide.fileCheck,
                    label: 'Документы',
                    value: 'что проверено',
                    chevron: true,
                    onTap: () => context.go(AppRoute.csmDocuments),
                  ),
                  CRow(
                    icon: Lucide.listTree,
                    label: 'Транспорт',
                    value: 'ступени и попытки',
                    chevron: true,
                    onTap: () => context.go(AppRoute.csmTransport),
                  ),
                  CRow(
                    icon: Lucide.eye,
                    label: 'Что мы отправляем',
                    chevron: true,
                    onTap: () => context.go(AppRoute.csmDisclosure),
                  ),
                ],
              ),
            ],

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

            // Поддержка живёт в панели: без аккаунта раздел пустой, поэтому в
            // generic-режиме показываем вход вместо тикетов.
            if (authed) ...[
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
            ] else ...[
              const SectionTitle('Аккаунт панели'),
              RowsGroup(
                children: [
                  CRow(
                    icon: Lucide.userPlus,
                    label: 'Войти или подключить панель',
                    chevron: true,
                    onTap: () => context.go(AppRoute.login),
                  ),
                ],
              ),
            ],

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
            if (authed)
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

/// Строка «Улучшения»: подпись и сводка на СВОИХ строках, а не в одной.
///
/// [CRow] кладёт подпись и значение в одну строку и режет значение
/// [TextOverflow.ellipsis] — а обрезка у dart:ui включается уже тем, что
/// `overflow` задан, ДАЖЕ без `maxLines`: движок молча подставляет
/// `maxLines: 1`, когда предела нет явно. Тут это резало «только 1 сайт ·
/// без рекламы» до «... · бе…» ровно на границе слова. [CRow] общий для
/// всего приложения и здесь не трогается (тот же фикс уже стоит в
/// [AppliedRouteCard._WrapRow] для карточки «Что применилось») — сводке
/// нужна не более широкая колонка, а разрешение перенестись, поэтому она
/// вынесена под подпись, на всю ширину строки.
class _EnhancementsSummaryRow extends StatelessWidget {
  final String value;
  final Widget? trailing;
  final VoidCallback? onTap;

  const _EnhancementsSummaryRow({
    required this.value,
    this.trailing,
    this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return InkWell(
      onTap: onTap,
      child: Container(
        constraints: const BoxConstraints(minHeight: 54),
        padding: const EdgeInsets.symmetric(
          horizontal: AppSpace.s4,
          vertical: AppSpace.s3,
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                LucideIcon(Lucide.sliders, color: c.textMed, size: 20),
                const SizedBox(width: AppSpace.s3 + 2),
                Expanded(
                  child: Text(
                    'Улучшения',
                    style: AppType.bodyMd.copyWith(color: c.textHi),
                  ),
                ),
                if (trailing != null) trailing!,
                const SizedBox(width: AppSpace.s2),
                LucideIcon(Lucide.chevronRight, color: c.textLow, size: 18),
              ],
            ),
            const SizedBox(height: 2),
            Text(
              value,
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
          ],
        ),
      ),
    );
  }
}

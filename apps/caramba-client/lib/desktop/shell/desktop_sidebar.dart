/// Левый сайдбар десктопной оболочки: бренд, состояние, навигация, прокси.
///
/// ПОЧЕМУ НЕ [NavigationRail]. Рейл умеет ровно одно — список пунктов, — а
/// сайдбару здесь положено держать три вещи разом: состояние туннеля (оно
/// видно со всех веток), четыре пункта, из которых один вообще не ветка
/// («Серверы» — накладной маршрут), и подвал с адресом локального прокси,
/// который на десктопе прописывают руками в браузер. Рейл на такую композицию
/// не гнётся, а подделка его вида поверх `Column` честнее.
///
/// ЧЕТЫРЕ ПУНКТА ПРИ ТРЁХ ВЕТКАХ. «Серверы» открываются накладным маршрутом
/// поверх шелла (`AppRoute.servers`), а не пятой вкладкой: вкладкой они быть
/// перестали ещё на мобильном, и таблица маршрутов этого решения не меняет.
/// Поэтому активность у этого пункта считается по ВЕРХНЕЙ СТРАНИЦЕ стека, а у
/// остальных трёх — по индексу ветки.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:window_manager/window_manager.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/desktop_strings.dart';
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/desktop/shell/desktop_shortcuts.dart';
import 'package:caramba_client/desktop/shell/sidebar_status.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Один пункт навигации сайдбара.
///
/// [branch] — индекс ветки шелла, `null` у пункта-маршрута («Серверы»).
@immutable
class SidebarDestination {
  final String glyph;
  final String label;
  final int? branch;
  final String? route;

  const SidebarDestination({
    required this.glyph,
    required this.label,
    this.branch,
    this.route,
  });
}

/// Порядок пунктов сайдбара. «Серверы» стоят вторыми, сразу под
/// «Подключением»: это продолжение того же разговора, а не отдельный раздел.
const List<SidebarDestination> kSidebarDestinations = <SidebarDestination>[
  SidebarDestination(
    glyph: Lucide.power,
    label: DesktopStrings.navConnection,
    branch: 0,
  ),
  SidebarDestination(
    glyph: Lucide.globe,
    label: DesktopStrings.navServers,
    route: AppRoute.servers,
  ),
  SidebarDestination(
    glyph: Lucide.user,
    label: DesktopStrings.navProfile,
    branch: 1,
  ),
  SidebarDestination(
    glyph: Lucide.sliders,
    label: DesktopStrings.navSettings,
    branch: 2,
  ),
];

class DesktopSidebar extends ConsumerWidget {
  /// Индекс активной ветки шелла.
  final int currentBranch;

  /// Переключение ветки. Приходит от шелла: ветками владеет он.
  final void Function(int index) onSelectBranch;

  const DesktopSidebar({
    required this.currentBranch,
    required this.onSelectBranch,
    super.key,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final name = ref.watch(activeBrandingProvider).displayName(kBrandName);

    return Container(
      width: DesktopTokens.sidebarWidth,
      decoration: BoxDecoration(
        color: c.surface1,
        border: Border(
          right: BorderSide(color: c.borderSubtle, width: AppBorders.hairline),
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _Wordmark(name: name),
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: DesktopTokens.sidebarPad),
            child: SidebarStatus(),
          ),
          const SizedBox(height: AppSpace.s4),
          // Список короткий и фиксированный, но окно бывает низким: 640 по
          // высоте это минимум, а не обещание, и на нём навигация обязана
          // прокручиваться, а не обрезаться.
          Expanded(
            // Подсветка «Серверов» зависит от ВЕРХНЕЙ страницы стека, а её
            // меняет навигация, а не состояние шелла: без подписки на делегата
            // пункт загорался бы только вместе со следующей перестройкой
            // сайдбара по какой-нибудь посторонней причине.
            child: AnimatedBuilder(
              animation: GoRouter.of(context).routerDelegate,
              builder: (context, _) => ListView(
                padding: const EdgeInsets.symmetric(
                  horizontal: DesktopTokens.sidebarPad,
                ),
                children: <Widget>[
                  for (final d in kSidebarDestinations)
                    _NavRow(
                      destination: d,
                      active: _isActive(context, d),
                      onTap: () => _open(context, d),
                    ),
                ],
              ),
            ),
          ),
          const _SidebarFooter(),
        ],
      ),
    );
  }

  bool _isActive(BuildContext context, SidebarDestination d) {
    final route = d.route;
    // Накладной маршрут активен, пока лежит ВЕРХНИМ в стеке: ветка под ним
    // своей подсветки при этом не теряет — она и правда всё ещё выбрана.
    if (route != null) return topRouteLocation(context) == route;
    return d.branch == currentBranch;
  }

  void _open(BuildContext context, SidebarDestination d) {
    final route = d.route;
    if (route != null) {
      context.go(route);
      return;
    }
    final branch = d.branch;
    if (branch != null) onSelectBranch(branch);
  }
}

/// Верхняя полоса сайдбара: имя оператора.
///
/// Высота та же, что у тулбара, — вордмарк и заголовок раздела обязаны стоять
/// на одной линии. На macOS слева освобождено место под системные трафик-лайты:
/// их рисует система, и залезть под них нельзя.
class _Wordmark extends StatelessWidget {
  final String name;

  const _Wordmark({required this.name});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final row = SizedBox(
      height: DesktopTokens.toolbarHeight,
      child: Padding(
        padding: EdgeInsets.only(
          left: isMacOSPlatform
              ? DesktopTokens.macTrafficLightInset
              : AppSpace.s4,
          right: AppSpace.s3,
        ),
        child: Align(
          alignment: Alignment.centerLeft,
          child: Text(
            name,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: AppType.titleMd.copyWith(color: c.textHi),
          ),
        ),
      ),
    );
    // Linux рисует системный заголовок сам: перетаскивание за нашу полосу там
    // не нужно и мешало бы менеджеру окон.
    return isLinuxPlatform ? row : DragToMoveArea(child: row);
  }
}

/// Строка навигации: 40 px, активная с полосой слева.
class _NavRow extends StatelessWidget {
  final SidebarDestination destination;
  final bool active;
  final VoidCallback onTap;

  const _NavRow({
    required this.destination,
    required this.active,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final color = active ? c.textHi : c.textMed;

    return Semantics(
      button: true,
      selected: active,
      label: destination.label,
      child: Padding(
        padding: const EdgeInsets.only(bottom: AppSpace.s1),
        child: Material(
          color: active ? c.surface2 : Colors.transparent,
          borderRadius: AppRadius.r12,
          child: InkWell(
            onTap: onTap,
            borderRadius: AppRadius.r12,
            hoverColor: c.accentSubtle,
            child: SizedBox(
              height: DesktopTokens.navRowHeight,
              child: Row(
                children: [
                  // Полоса активного пункта. Место под неё занято всегда,
                  // иначе строка дёргалась бы вбок при выборе.
                  Container(
                    width: 3,
                    height: 18,
                    decoration: BoxDecoration(
                      color: active ? c.textHi : Colors.transparent,
                      borderRadius: const BorderRadius.horizontal(
                        right: Radius.circular(2),
                      ),
                    ),
                  ),
                  const SizedBox(width: AppSpace.s3),
                  LucideIcon(destination.glyph, color: color, size: 18),
                  const SizedBox(width: AppSpace.s3),
                  Expanded(
                    child: Text(
                      destination.label,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: AppType.bodyMd.copyWith(color: color),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Подвал: адрес локального прокси и версия.
///
/// Адрес показывается ТОЛЬКО в proxy-режиме и только при поднятом туннеле —
/// ровно тогда, когда его есть куда прописать. В tun-режиме его нет вовсе, и
/// строка-заглушка означала бы адрес, на котором никто не слушает.
class _SidebarFooter extends ConsumerWidget {
  const _SidebarFooter();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final endpoint = ref.watch(proxyEndpointProvider);

    return Padding(
      padding: const EdgeInsets.fromLTRB(
        DesktopTokens.sidebarPad,
        AppSpace.s2,
        DesktopTokens.sidebarPad,
        DesktopTokens.sidebarPad,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (endpoint != null) ...[
            Semantics(
              button: true,
              label: DesktopStrings.copyProxyHint,
              child: Tooltip(
                message: DesktopStrings.copyProxyHint,
                child: Material(
                  color: c.surface2,
                  borderRadius: AppRadius.r8,
                  child: InkWell(
                    borderRadius: AppRadius.r8,
                    hoverColor: c.accentSubtle,
                    onTap: () => _copy(context, endpoint),
                    child: Padding(
                      padding: const EdgeInsets.symmetric(
                        horizontal: AppSpace.s2,
                        vertical: AppSpace.s1 + 2,
                      ),
                      child: Row(
                        children: [
                          Expanded(
                            child: Text(
                              endpoint,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: AppType.monoSm.copyWith(color: c.textMed),
                            ),
                          ),
                          const SizedBox(width: AppSpace.s1),
                          LucideIcon(Lucide.copy, color: c.textLow, size: 14),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            ),
            const SizedBox(height: AppSpace.s2),
          ],
          Text(
            DesktopStrings.versionLabel(DesktopTokens.kAppVersion),
            style: AppType.caption.copyWith(color: c.textLow),
          ),
        ],
      ),
    );
  }

  void _copy(BuildContext context, String endpoint) {
    unawaited(Clipboard.setData(ClipboardData(text: endpoint)));
    showCarambaToast(context, DesktopStrings.proxyCopied);
  }
}

import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';

/// Пункты нижней навигации: Главная / Серверы / Профиль / Настройки.
/// Lucide-глифы, порядок совпадает с ветками [StatefulShellRoute].
const List<({String glyph, String label})> _destinations = [
  (glyph: Lucide.power, label: 'Главная'),
  (glyph: Lucide.globe, label: 'Серверы'),
  (glyph: Lucide.user, label: 'Профиль'),
  (glyph: Lucide.sliders, label: 'Настройки'),
];

/// Шелл с нижней навигацией (mobile) или левым рейлом (desktop). Активный
/// пункт = только высококонтрастный текст/иконка, без цветной пилюли (демо).
class AppShell extends StatelessWidget {
  final StatefulNavigationShell navigationShell;

  const AppShell({required this.navigationShell, super.key});

  int get _currentIndex => navigationShell.currentIndex;

  void _go(int index) =>
      navigationShell.goBranch(index, initialLocation: index == _currentIndex);

  @override
  Widget build(BuildContext context) {
    final isDesktop =
        MediaQuery.sizeOf(context).width >= AppBreakpoints.desktop;
    final c = context.c;

    if (isDesktop) {
      return Scaffold(
        backgroundColor: c.bgCanvas,
        body: Row(
          children: [
            NavigationRail(
              selectedIndex: _currentIndex,
              onDestinationSelected: _go,
              labelType: NavigationRailLabelType.all,
              backgroundColor: c.surface1,
              destinations: [
                for (final d in _destinations)
                  NavigationRailDestination(
                    icon: LucideIcon(d.glyph, color: c.textLow, size: 22),
                    selectedIcon: LucideIcon(
                      d.glyph,
                      color: c.textHi,
                      size: 22,
                    ),
                    label: Text(d.label),
                  ),
              ],
            ),
            VerticalDivider(width: 1, color: c.borderSubtle),
            Expanded(
              child: Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(
                    maxWidth: AppBreakpoints.contentMaxWidth,
                  ),
                  child: navigationShell,
                ),
              ),
            ),
          ],
        ),
      );
    }

    return Scaffold(
      backgroundColor: c.bgCanvas,
      extendBody: true,
      body: navigationShell,
      bottomNavigationBar: _BottomNav(currentIndex: _currentIndex, onTap: _go),
    );
  }
}

class _BottomNav extends StatelessWidget {
  final int currentIndex;
  final ValueChanged<int> onTap;
  const _BottomNav({required this.currentIndex, required this.onTap});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final bottomInset = MediaQuery.viewPaddingOf(context).bottom;

    return ClipRect(
      child: BackdropFilter(
        filter: ImageFilter.blur(sigmaX: 12, sigmaY: 12),
        child: Container(
          decoration: BoxDecoration(
            color: c.bgCanvas.withValues(alpha: 0.92),
            border: Border(
              top: BorderSide(
                color: c.borderSubtle,
                width: AppBorders.hairline,
              ),
            ),
          ),
          padding: EdgeInsets.only(bottom: bottomInset),
          child: SizedBox(
            height: 64,
            child: Row(
              children: [
                for (var i = 0; i < _destinations.length; i++)
                  Expanded(
                    child: _NavCell(
                      glyph: _destinations[i].glyph,
                      label: _destinations[i].label,
                      selected: i == currentIndex,
                      onTap: () => onTap(i),
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _NavCell extends StatelessWidget {
  final String glyph;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  const _NavCell({
    required this.glyph,
    required this.label,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final color = selected ? c.textHi : c.textLow;
    return InkResponse(
      onTap: onTap,
      radius: 44,
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          LucideIcon(glyph, color: color, size: 22),
          const SizedBox(height: 5),
          Text(label, style: AppType.caption.copyWith(color: color)),
        ],
      ),
    );
  }
}

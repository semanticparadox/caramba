import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Список профилей подключения с активным маркером и свитчером.
///
/// Профиль может быть импортированной подпиской ([ProfileType.rawSub]) или
/// аккаунтом панели ([ProfileType.panelAccount]). Тап по карточке делает
/// профиль активным. Кнопка «Импорт» ведёт на [AppRoute.connectionImport].
class ConnectionsScreen extends ConsumerWidget {
  const ConnectionsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final state = ref.watch(connectionProfilesProvider);

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: ListView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s12,
          ),
          children: [
            ScreenHead(
              'Подключения',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            Text(
              'Профили подключения. Активный ведёт туннель. Импортируйте '
              'подписку или используйте аккаунт панели.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s4),
            FilledButton.icon(
              onPressed: () => context.go(AppRoute.connectionImport),
              icon: LucideIcon(Lucide.plus, color: c.textOnAccent, size: 18),
              label: const Text('Импорт подписки'),
            ),
            const SizedBox(height: AppSpace.s5),
            if (state.loading)
              const InlineLoading()
            else if (state.isEmpty)
              const _EmptyConnections()
            else
              Column(
                children: [
                  for (final p in state.profiles)
                    _ProfileCard(
                      profile: p,
                      active: p.id == state.active?.id,
                    ),
                ],
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
      context.go(AppRoute.home);
    }
  }
}

/// Карточка одного профиля. Активный помечен полоской слева и галочкой
/// (через [ListItemCard.selected]). Тап активирует профиль; трейлинг —
/// меню переименования/удаления.
class _ProfileCard extends ConsumerWidget {
  final ConnectionProfile profile;
  final bool active;
  const _ProfileCard({required this.profile, required this.active});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final glyph = profile.isRaw ? Lucide.globe : Lucide.appWindow;
    final typeLabel = profile.isRaw ? 'Подписка' : 'Аккаунт панели';

    return ListItemCard(
      leading: IBox(glyph),
      title: profile.displayName.isEmpty ? 'Без имени' : profile.displayName,
      subtitle: profile.source.isEmpty ? typeLabel : profile.source,
      selected: active,
      // Тип профиля — нейтральный бейдж. Цвет резервируем за статусом
      // подключения (зелёный/янтарный/красный); активность профиля несёт
      // ListItemCard.selected (полоска + галочка), не цвет.
      titleBadges: [Tag(typeLabel)],
      trailing: IconButton(
        icon: LucideIcon(Lucide.sliders, color: c.textMed, size: 18),
        onPressed: () => _openMenu(context, ref),
      ),
      onTap: () =>
          ref.read(connectionProfilesProvider.notifier).activate(profile.id),
    );
  }

  void _openMenu(BuildContext context, WidgetRef ref) {
    final c = context.c;
    showModalBottomSheet<void>(
      context: context,
      backgroundColor: c.surface1,
      showDragHandle: true,
      builder: (ctx) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              leading: LucideIcon(Lucide.check, color: c.textHi, size: 20),
              title: Text('Сделать активным',
                  style: AppType.bodyMd.copyWith(color: c.textHi)),
              enabled: !active,
              onTap: () {
                Navigator.of(ctx).pop();
                ref
                    .read(connectionProfilesProvider.notifier)
                    .activate(profile.id);
              },
            ),
            ListTile(
              leading: LucideIcon(Lucide.copy, color: c.textHi, size: 20),
              title: Text('Переименовать',
                  style: AppType.bodyMd.copyWith(color: c.textHi)),
              onTap: () {
                Navigator.of(ctx).pop();
                _rename(context, ref);
              },
            ),
            ListTile(
              leading: LucideIcon(Lucide.trash, color: c.danger, size: 20),
              title: Text('Удалить',
                  style: AppType.bodyMd.copyWith(color: c.danger)),
              onTap: () {
                Navigator.of(ctx).pop();
                ref
                    .read(connectionProfilesProvider.notifier)
                    .remove(profile.id);
              },
            ),
            const SizedBox(height: AppSpace.s2),
          ],
        ),
      ),
    );
  }

  void _rename(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final controller = TextEditingController(text: profile.displayName);
    showDialog<void>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface1,
        title: Text('Переименовать',
            style: AppType.titleMd.copyWith(color: c.textHi)),
        content: TextField(
          controller: controller,
          autofocus: true,
          style: AppType.bodyMd.copyWith(color: c.textHi),
          decoration: const InputDecoration(hintText: 'Имя профиля'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(),
            child: const Text('Отмена'),
          ),
          FilledButton(
            onPressed: () {
              ref
                  .read(connectionProfilesProvider.notifier)
                  .rename(profile.id, controller.text);
              Navigator.of(ctx).pop();
            },
            child: const Text('Сохранить'),
          ),
        ],
      ),
    );
  }
}

class _EmptyConnections extends StatelessWidget {
  const _EmptyConnections();

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.only(top: AppSpace.s12),
      child: Column(
        children: [
          LucideIcon(Lucide.layers, color: c.textLow, size: 32),
          const SizedBox(height: AppSpace.s3),
          Text('Профилей пока нет',
              style: AppType.bodyMd.copyWith(color: c.textMed)),
          const SizedBox(height: AppSpace.s1),
          Text(
            'Импортируйте подписку, чтобы подключиться.',
            textAlign: TextAlign.center,
            style: AppType.bodySm.copyWith(color: c.textLow),
          ),
        ],
      ),
    );
  }
}

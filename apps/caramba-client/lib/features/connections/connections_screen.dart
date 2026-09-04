import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/subscription_fetch.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_error.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Список профилей подключения с активным маркером и свитчером.
///
/// Профиль может быть импортированной подпиской ([ProfileType.rawSub]) или
/// аккаунтом панели ([ProfileType.panelAccount]). Тап по карточке делает
/// профиль активным и открывает лист действий: главное — «Подключить»
/// (профиль сразу ведёт туннель), рядом остальные операции над профилем.
/// Кнопка «Импорт» ведёт на [AppRoute.connectionImport].
class ConnectionsScreen extends ConsumerWidget {
  const ConnectionsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final state = ref.watch(connectionProfilesProvider);
    // Один профиль, и он активный — типичная картина generic-режима сразу после
    // импорта. Тогда главное действие экрана не «завести ещё один профиль», а
    // «подключиться»: импорт уезжает в тихую кнопку.
    final only = state.profiles.length == 1 ? state.active : null;

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
            if (only != null) ...[
              FilledButton.icon(
                onPressed: () => connectProfile(context, ref, only),
                icon: LucideIcon(Lucide.zap, color: c.textOnAccent, size: 18),
                label: const Text('Подключить'),
              ),
              const SizedBox(height: AppSpace.s2),
              GhostButton(
                label: 'Импорт подписки',
                icon: Lucide.plus,
                onPressed: () => context.go(AppRoute.connectionImport),
              ),
            ] else
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
                    _ProfileCard(profile: p, active: p.id == state.active?.id),
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
/// (через [ListItemCard.selected]). Тап активирует профиль и открывает лист
/// действий; трейлинг открывает тот же лист, не меняя активный профиль.
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
      subtitle: _subtitle(),
      selected: active,
      // Тип профиля — нейтральный бейдж. Цвет резервируем за статусом
      // подключения (зелёный/янтарный/красный); активность профиля несёт
      // ListItemCard.selected (полоска + галочка), не цвет.
      titleBadges: [Tag(typeLabel)],
      trailing: IconButton(
        icon: LucideIcon(Lucide.sliders, color: c.textMed, size: 18),
        onPressed: () => _openMenu(context, ref),
      ),
      onTap: () async {
        await ref
            .read(connectionProfilesProvider.notifier)
            .activate(profile.id);
        if (!context.mounted) return;
        _openMenu(context, ref);
      },
    );
  }

  /// Вторая строка карточки: для подписки это число ПРОКСИ и когда список
  /// обновляли, для аккаунта панели — её URL.
  ///
  /// Именно прокси, а не узлов. `profile.serverCount` — длина списка прокси в
  /// теле подписки, то есть по строке на каждый инбаунд каждой машины: у живой
  /// подписки это 13 на две машины. Считать их не запрещено — запрещено
  /// называть их узлами, потому что «узлов» на экране серверов и на Home
  /// означает машины, и одно слово с двумя значениями и породило жалобу
  /// владельца про «восемь серверов». Машину отсюда не посчитать: группировка
  /// живёт в слое предложения и строится для АКТИВНОГО профиля, а карточка
  /// рисуется для каждого.
  String _subtitle() {
    if (!profile.isRaw) {
      return profile.source.isEmpty ? 'Аккаунт панели' : profile.source;
    }
    final count = profile.serverCount;
    final proxies = count == 0 ? 'прокси не загружены' : 'прокси: $count';
    final updated = _agoText(profile.serversUpdatedMs);
    return updated == null ? proxies : '$proxies · обновлено $updated';
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
            // Главное действие: профиль становится активным и сразу ведёт
            // туннель. Всё остальное ниже — операции над записью профиля.
            Padding(
              padding: const EdgeInsets.fromLTRB(
                AppSpace.s5,
                0,
                AppSpace.s5,
                AppSpace.s3,
              ),
              child: FilledButton.icon(
                onPressed: () {
                  Navigator.of(ctx).pop();
                  connectProfile(context, ref, profile);
                },
                icon: LucideIcon(Lucide.zap, color: c.textOnAccent, size: 18),
                label: const Text('Подключить'),
              ),
            ),
            ListTile(
              leading: LucideIcon(Lucide.check, color: c.textHi, size: 20),
              title: Text(
                active ? 'Уже активный' : 'Сделать активным',
                style: AppType.bodyMd.copyWith(color: c.textHi),
              ),
              enabled: !active,
              onTap: () {
                Navigator.of(ctx).pop();
                ref
                    .read(connectionProfilesProvider.notifier)
                    .activate(profile.id);
              },
            ),
            if (profile.isRaw)
              ListTile(
                leading: LucideIcon(Lucide.refresh, color: c.textHi, size: 20),
                title: Text(
                  'Обновить подписку',
                  style: AppType.bodyMd.copyWith(color: c.textHi),
                ),
                subtitle: Text(
                  'Скачать по ссылке заново и обновить список узлов',
                  style: AppType.bodySm.copyWith(color: c.textLow),
                ),
                enabled: _canRefresh,
                onTap: () {
                  Navigator.of(ctx).pop();
                  _refresh(context, ref);
                },
              ),
            ListTile(
              leading: LucideIcon(Lucide.copy, color: c.textHi, size: 20),
              title: Text(
                'Переименовать',
                style: AppType.bodyMd.copyWith(color: c.textHi),
              ),
              onTap: () {
                Navigator.of(ctx).pop();
                _rename(context, ref);
              },
            ),
            ListTile(
              leading: LucideIcon(Lucide.trash, color: c.danger, size: 20),
              title: Text(
                'Удалить',
                style: AppType.bodyMd.copyWith(color: c.danger),
              ),
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

  /// Обновить можно только подписку, заведённую ссылкой: сырой текст перекачать
  /// неоткуда.
  bool get _canRefresh =>
      profile.isRaw &&
      (profile.source.startsWith('http://') ||
          profile.source.startsWith('https://'));

  /// Перекачивает подписку по её ссылке и переразбирает ядром, обновляя кэш
  /// узлов на профиле. Туннель при этом не поднимается.
  Future<void> _refresh(BuildContext context, WidgetRef ref) async {
    showCarambaToast(context, 'Обновляю подписку');
    try {
      final raw = await fetchSubscriptionBody(profile.source);
      final result = await ref
          .read(vpnConnectionProvider)
          .importSubscription(raw: raw, format: profile.format);
      await ref
          .read(connectionProfilesProvider.notifier)
          .setImported(
            profile.id,
            rawConfig: raw,
            format: profile.format,
            servers: result.servers,
          );
      if (!context.mounted) return;
      showCarambaToast(context, 'Обновлено. Узлов: ${result.servers.length}');
    } on SubscriptionFetchException catch (e) {
      if (!context.mounted) return;
      showCarambaToast(context, 'Не удалось скачать подписку: ${e.message}');
    } catch (e) {
      if (!context.mounted) return;
      showCarambaToast(
        context,
        coreErrorText(e) ?? 'Не удалось разобрать подписку.',
      );
    }
  }

  void _rename(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final controller = TextEditingController(text: profile.displayName);
    showDialog<void>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface1,
        title: Text(
          'Переименовать',
          style: AppType.titleMd.copyWith(color: c.textHi),
        ),
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

/// Делает профиль активным, поднимает по нему туннель и уводит на Home.
/// Общий путь для главной кнопки экрана и для листа действий карточки.
Future<void> connectProfile(
  BuildContext context,
  WidgetRef ref,
  ConnectionProfile profile,
) async {
  await ref.read(connectionProfilesProvider.notifier).activate(profile.id);
  final ok = await ref.read(vpnProvider.notifier).connect();
  if (!context.mounted) return;
  if (!ok) {
    showCarambaToast(context, 'Подключаться не к чему. Проверьте профиль.');
    return;
  }
  context.go(AppRoute.home);
}

/// Грубая давность в плоских словах. `null`, если отметки времени нет.
String? _agoText(int ms) {
  if (ms <= 0) return null;
  final diff = DateTime.now().difference(
    DateTime.fromMillisecondsSinceEpoch(ms),
  );
  if (diff.inMinutes < 1) return 'только что';
  if (diff.inMinutes < 60) return '${diff.inMinutes} мин назад';
  if (diff.inHours < 24) return '${diff.inHours} ч назад';
  return '${diff.inDays} дн назад';
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
          Text(
            'Профилей пока нет',
            style: AppType.bodyMd.copyWith(color: c.textMed),
          ),
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

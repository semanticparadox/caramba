import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Серверы (демо §SERVERS): общий пул + раздел частного пула (Private).
///
/// Деление на пулы по `Server.status`/имени: пока панель не выдаёт явный
/// признак пула, считаем приватными узлы со статусом `private` или именем,
/// начинающимся с `Private`. Выбор пишет в [selectedServerProvider].
class ServersScreen extends ConsumerWidget {
  const ServersScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final async = ref.watch(serversProvider);
    final selected = ref.watch(selectedServerProvider);

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: RefreshIndicator(
          onRefresh: () => ref.refresh(serversProvider.future),
          color: c.accent,
          backgroundColor: c.surface2,
          child: ListView(
            padding: const EdgeInsets.fromLTRB(
              AppSpace.s5,
              AppSpace.s5,
              AppSpace.s5,
              AppSpace.s20 + AppSpace.s6,
            ),
            children: [
              ScreenHead(
                'Серверы',
                trailing: IconBtn(
                  Lucide.x,
                  onTap: () => context.go(AppRoute.home),
                ),
              ),
              async.when(
                data: (servers) => _list(context, ref, servers, selected),
                loading: () => const _Loading(),
                error: (_, __) =>
                    _Error(onRetry: () => ref.refresh(serversProvider)),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _list(
    BuildContext context,
    WidgetRef ref,
    List<Server> servers,
    Server? selected,
  ) {
    final c = context.c;
    bool isPrivate(Server s) =>
        s.status == 'private' || s.name.toLowerCase().startsWith('private');
    final common = servers.where((s) => !isPrivate(s)).toList();
    final priv = servers.where(isPrivate).toList();

    if (servers.isEmpty) {
      return Padding(
        padding: const EdgeInsets.only(top: AppSpace.s12),
        child: Center(
          child: Text(
            'Серверы недоступны',
            style: AppType.bodyMd.copyWith(color: c.textMed),
          ),
        ),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SectionTitle(
          'Общий пул',
          padding: EdgeInsets.only(bottom: AppSpace.s3),
        ),
        for (final s in common) _tile(context, ref, s, selected),
        if (priv.isNotEmpty) ...[
          SectionTitle(
            'Частный пул · Private',
            trailing: LucideIcon(Lucide.key, color: c.textLow, size: 14),
          ),
          for (final s in priv) _tile(context, ref, s, selected),
        ],
      ],
    );
  }

  Widget _tile(
    BuildContext context,
    WidgetRef ref,
    Server s,
    Server? selected,
  ) {
    final c = context.c;
    final isSel = selected?.id == s.id;
    final pingColor = switch (s.pingBucket) {
      PingBucket.good => c.success,
      PingBucket.fair => c.warning,
      PingBucket.poor => c.danger,
      PingBucket.timeout => c.textLow,
    };
    final level = switch (s.loadBucket) {
      LoadBucket.low => 3,
      LoadBucket.med => 2,
      LoadBucket.high => 1,
    };
    return ListItemCard(
      leading: CodeChip(s.countryCode ?? '··'),
      title: s.name,
      subtitle: _city(s),
      selected: isSel,
      onTap: () {
        ref.read(selectedServerProvider.notifier).select(s);
        showCarambaToast(context, '${s.name} выбран');
        Future.delayed(const Duration(milliseconds: 300), () {
          if (context.mounted) context.go(AppRoute.home);
        });
      },
      trailing: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          Text(
            s.pingMs == null ? '·' : '${s.pingMs} мс',
            style: AppType.monoSm.copyWith(color: pingColor),
          ),
          const SizedBox(height: 5),
          SignalBars(level: level),
        ],
      ),
    );
  }

  String _city(Server s) {
    if (s.status == 'private' || s.name.toLowerCase().startsWith('private')) {
      return 'Только для Private';
    }
    return s.countryCode == null ? 'Сервер' : 'Узел ${s.countryCode}';
  }
}

class _Loading extends StatelessWidget {
  const _Loading();
  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.only(top: AppSpace.s12),
      child: Center(
        child: CircularProgressIndicator(strokeWidth: 2, color: c.textHi),
      ),
    );
  }
}

class _Error extends StatelessWidget {
  final VoidCallback onRetry;
  const _Error({required this.onRetry});
  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.only(top: AppSpace.s12),
      child: Column(
        children: [
          LucideIcon(Lucide.alert, color: c.textMed, size: 28),
          const SizedBox(height: AppSpace.s3),
          Text(
            'Не удалось загрузить серверы',
            style: AppType.bodyMd.copyWith(color: c.textMed),
          ),
          const SizedBox(height: AppSpace.s4),
          GhostButton(label: 'Повторить', onPressed: onRetry),
        ],
      ),
    );
  }
}

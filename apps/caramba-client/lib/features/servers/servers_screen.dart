import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_error.dart';
import 'package:caramba_client/state/probe_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/vpn/vpn_models.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Серверы. Экран показывает узлы АКТИВНОГО профиля подключения, поэтому у него
/// две ветки:
///   * профиль-подписка ([ProfileType.rawSub]) — узлы из кэша импорта, с
///     задержками из последнего замера и пином выбранного узла;
///   * аккаунт панели (или профиля нет) — общий/частный пул из `GET /servers`.
class ServersScreen extends ConsumerWidget {
  const ServersScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profile = ref.watch(activeConnectionProfileProvider);
    if (profile != null && profile.isRaw) {
      return _ImportedServersView(profile: profile);
    }
    return const _PanelServersView();
  }
}

// --- ветка импортированной подписки -----------------------------------------

/// Узлы импортированной подписки: страна моно-кодом, имя, тип outbound'а и
/// задержка из последнего замера. Тап по строке прикрепляет селектор CARAMBA к
/// узлу, строка «Авто» снимает пин и возвращает выбор ядру.
class _ImportedServersView extends ConsumerStatefulWidget {
  final ConnectionProfile profile;
  const _ImportedServersView({required this.profile});

  @override
  ConsumerState<_ImportedServersView> createState() =>
      _ImportedServersViewState();
}

class _ImportedServersViewState extends ConsumerState<_ImportedServersView> {
  bool _probing = false;
  String? _error;

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final profile = widget.profile;
    final servers = profile.servers;

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
            ScreenHead(
              'Серверы',
              trailing: IconBtn(
                Lucide.x,
                onTap: () => context.go(AppRoute.home),
              ),
            ),
            Text(
              profile.displayName.isEmpty
                  ? 'Узлы импортированной подписки'
                  : 'Узлы подписки «${profile.displayName}»',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s4),
            GhostButton(
              label: _probing ? 'Меряю задержки' : 'Замерить задержки',
              icon: Lucide.gauge,
              onPressed: (_probing || servers.isEmpty) ? null : _probe,
            ),
            if (profile.lastProbe?.updatedAt != null) ...[
              const SizedBox(height: AppSpace.s2),
              Text(
                'Последний замер: ${_timeText(profile.lastProbe!.updatedAt!)}',
                style: AppType.bodySm.copyWith(color: c.textLow),
              ),
            ],
            const SizedBox(height: AppSpace.s5),
            if (_error != null) ...[
              InlineError(message: _error!, onRetry: _probe),
              const SizedBox(height: AppSpace.s5),
            ],
            if (servers.isEmpty)
              const _EmptyImported()
            else ...[
              _AutoRow(
                selected: profile.selectedServerId == null,
                onTap: () => ref
                    .read(connectionProfilesProvider.notifier)
                    .setSelectedServer(profile.id, null),
              ),
              for (final s in servers)
                _ImportedRow(
                  server: s,
                  latencyMs: profile.latencyOf(s.id),
                  selected: profile.selectedServerId == s.id,
                  probing: _probing,
                  onTap: () => _select(s),
                ),
            ],
          ],
        ),
      ),
    );
  }

  Future<void> _select(ImportedServer server) async {
    await ref
        .read(connectionProfilesProvider.notifier)
        .setSelectedServer(widget.profile.id, server.id);
    if (!mounted) return;
    showCarambaToast(
      context,
      '${server.name.isEmpty ? server.id : server.name} выбран',
    );
  }

  /// Замер идёт через ядро: подписка сначала отдаётся ему на разбор, затем
  /// `probe`. Результат ложится на профиль и переживает перезапуск.
  Future<void> _probe() async {
    setState(() {
      _probing = true;
      _error = null;
    });
    try {
      final results = await probeProfile(
        ref.read(vpnConnectionProvider),
        widget.profile,
      );
      await ref
          .read(connectionProfilesProvider.notifier)
          .setProbe(widget.profile.id, ProbeSnapshot.fromResults(results));
      if (!mounted) return;
      setState(() => _probing = false);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _probing = false;
        _error = coreErrorText(e) ?? 'Не удалось замерить задержки.';
      });
    }
  }
}

/// Строка «Авто»: селектор снят, узел выбирает ядро.
class _AutoRow extends StatelessWidget {
  final bool selected;
  final VoidCallback onTap;
  const _AutoRow({required this.selected, required this.onTap});

  @override
  Widget build(BuildContext context) {
    return ListItemCard(
      leading: const IBox(Lucide.gauge),
      title: 'Авто',
      subtitle: 'Ядро выберет узел само',
      selected: selected,
      onTap: onTap,
    );
  }
}

/// Строка одного узла подписки.
class _ImportedRow extends StatelessWidget {
  final ImportedServer server;

  /// Задержка из последнего замера: `null` — не мерили, -1 — таймаут.
  final int? latencyMs;
  final bool selected;
  final bool probing;
  final VoidCallback onTap;

  const _ImportedRow({
    required this.server,
    required this.latencyMs,
    required this.selected,
    required this.probing,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final bucket = latencyMs == null ? null : pingBucketOf(latencyMs);
    final color = switch (bucket) {
      PingBucket.good => c.success,
      PingBucket.fair => c.warning,
      PingBucket.poor => c.danger,
      PingBucket.timeout => c.danger,
      null => c.textLow,
    };

    return ListItemCard(
      leading: CodeChip(server.country.isEmpty ? '··' : server.country),
      title: server.name.isEmpty ? server.id : server.name,
      subtitle: '${server.server}:${server.port}',
      selected: selected,
      titleBadges: [if (server.type.isNotEmpty) Tag(server.type)],
      onTap: probing ? null : onTap,
      trailing: bucket == PingBucket.timeout
          // Таймаут — точка цвета danger: числа тут нет, а «-1 мс» врал бы.
          ? Container(
              width: 8,
              height: 8,
              decoration: BoxDecoration(color: color, shape: BoxShape.circle),
            )
          : Text(
              latencyMs == null ? '-' : '$latencyMs мс',
              style: AppType.monoSm.copyWith(color: color),
            ),
    );
  }
}

class _EmptyImported extends StatelessWidget {
  const _EmptyImported();

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.only(top: AppSpace.s12),
      child: Column(
        children: [
          LucideIcon(Lucide.globe, color: c.textLow, size: 32),
          const SizedBox(height: AppSpace.s3),
          Text(
            'Узлов в подписке нет',
            style: AppType.bodyMd.copyWith(color: c.textMed),
          ),
          const SizedBox(height: AppSpace.s1),
          Text(
            'Обновите подписку в разделе «Подключения».',
            textAlign: TextAlign.center,
            style: AppType.bodySm.copyWith(color: c.textLow),
          ),
        ],
      ),
    );
  }
}

String _timeText(DateTime at) {
  String two(int v) => v.toString().padLeft(2, '0');
  return '${two(at.hour)}:${two(at.minute)}';
}

// --- ветка панели ------------------------------------------------------------

/// Серверы панели (демо §SERVERS): общий пул + раздел частного пула (Private).
///
/// Деление на пулы по `Server.status`/имени: пока панель не выдаёт явный
/// признак пула, считаем приватными узлы со статусом `private` или именем,
/// начинающимся с `Private`. Выбор пишет в [selectedServerProvider].
class _PanelServersView extends ConsumerWidget {
  const _PanelServersView();

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

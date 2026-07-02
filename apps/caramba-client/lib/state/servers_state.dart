import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/state/providers.dart';

/// Асинхронный список серверов из `GET /api/v2/app/servers`.
/// Перезапрашивается через `ref.refresh(serversProvider)` (pull-to-refresh).
final serversProvider = FutureProvider.autoDispose<List<Server>>((ref) async {
  final api = ref.watch(apiClientProvider);
  final servers = await api.getServers();
  // Сортировка: сначала подключаемые, затем по пингу (timeout — в конец).
  servers.sort((a, b) {
    if (a.isSelectable != b.isSelectable) return a.isSelectable ? -1 : 1;
    final pa = a.pingMs ?? 1 << 30;
    final pb = b.pingMs ?? 1 << 30;
    return pa.compareTo(pb);
  });
  return servers;
});

/// Текущий выбранный сервер (на Home показывается в пилюле, к нему коннектимся).
/// Заполняется выбором в списке серверов; не сбрасывается auto-dispose.
class SelectedServerNotifier extends StateNotifier<Server?> {
  SelectedServerNotifier() : super(null);

  void select(Server server) => state = server;
  void clear() => state = null;
}

final selectedServerProvider =
    StateNotifierProvider<SelectedServerNotifier, Server?>(
      (ref) => SelectedServerNotifier(),
    );

/// Рекомендуемый сервер: выбранный пользователем, иначе самый быстрый
/// подключаемый из загруженного списка.
final recommendedServerProvider = Provider<Server?>((ref) {
  final selected = ref.watch(selectedServerProvider);
  if (selected != null) return selected;
  final servers = ref.watch(serversProvider).valueOrNull;
  if (servers == null || servers.isEmpty) return null;
  return servers.firstWhere((s) => s.isSelectable, orElse: () => servers.first);
});

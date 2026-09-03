import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
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
///
/// Выбор ПИШЕТСЯ в активный профиль: до этого он жил только в памяти, и один и
/// тот же контрол вёл себя по-разному — пин импортированной подписки переживал
/// перезапуск, а панельный сбрасывался.
class SelectedServerNotifier extends StateNotifier<Server?> {
  /// Пишет `nodes.id` в активный профиль; `null` — снять пин.
  final void Function(int? nodeId)? _persist;

  SelectedServerNotifier({void Function(int? nodeId)? persist})
    : _persist = persist,
      super(null);

  void select(Server server) {
    state = server;
    _persist?.call(server.id);
  }

  void clear() {
    state = null;
    _persist?.call(null);
  }
}

final selectedServerProvider =
    StateNotifierProvider<SelectedServerNotifier, Server?>((ref) {
      return SelectedServerNotifier(
        persist: (nodeId) {
          // Профиль читается в момент записи, а не при создании нотифаера:
          // watch здесь означал бы пересборку нотифаера на собственную же
          // запись, то есть потерю только что сделанного выбора.
          final id = ref.read(activeConnectionProfileProvider)?.id;
          if (id == null) return;
          unawaited(
            ref
                .read(connectionProfilesProvider.notifier)
                .setSelectedExitNode(id, nodeId),
          );
        },
      );
    });

/// Выбранный сервер с учётом сохранённого пина.
///
/// Выбор этой сессии важнее; если его нет — поднимаем `nodes.id`, сохранённый
/// на профиле, как только список серверов приехал. Экран показывает галочку
/// именно по этому провайдеру, чтобы после перезапуска она стояла там же, где
/// её оставили.
final resolvedSelectedServerProvider = Provider<Server?>((ref) {
  final explicit = ref.watch(selectedServerProvider);
  if (explicit != null) return explicit;
  final pinned = ref.watch(activeConnectionProfileProvider)?.selectedExitNodeId;
  if (pinned == null) return null;
  final servers = ref.watch(serversProvider).valueOrNull;
  if (servers == null) return null;
  for (final s in servers) {
    if (s.id == pinned) return s;
  }
  // Закреплённый узел исчез из выдачи (сняли с плана, выключили): пин молча не
  // воскрешаем — дальше решает автоподбор по стране.
  return null;
});

/// Рекомендуемый сервер: то, к чему подключится connect.
///
/// Порядок разрешения: явный выбор пользователя (в т.ч. восстановленный пин) →
/// лучший подключаемый узел ЗАКРЕПЛЁННОЙ СТРАНЫ → лучший подключаемый вообще.
/// Страна попадает в туннель именно здесь: `VpnNotifier` читает этот провайдер,
/// и его контракт менять не пришлось.
final recommendedServerProvider = Provider<Server?>((ref) {
  final selected = ref.watch(resolvedSelectedServerProvider);
  if (selected != null) return selected;
  final servers = ref.watch(serversProvider).valueOrNull;
  if (servers == null || servers.isEmpty) return null;

  final country = ref
      .watch(activeConnectionProfileProvider)
      ?.selectedExitCountry;
  if (country != null && country.isNotEmpty) {
    for (final s in servers) {
      if (s.isSelectable &&
          (s.countryCode ?? '').toUpperCase() == country.toUpperCase()) {
        return s;
      }
    }
    // Страна закреплена, но живых узлов в ней сейчас нет. Оставить пользователя
    // без подключения хуже, чем подключить его в другую страну, — но тогда
    // заголовок обязан назвать ту страну, куда трафик ушёл НА САМОМ ДЕЛЕ, и
    // отдельно сказать, что закреплённая недоступна. Обе половины этого долга
    // отдаёт [exitHeadlineProvider] (`vpn_state.dart`), и Home читает его, а не
    // пин: пин, оставленный заголовком над узлом чужой страны, — это не
    // неточность, а неправда о том, где пользователь виден сети.
  }

  return servers.firstWhere((s) => s.isSelectable, orElse: () => servers.first);
});

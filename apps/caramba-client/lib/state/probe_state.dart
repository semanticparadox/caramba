/// Замер задержек узлов активного профиля (ABI v2 `probe`).
///
/// Ядро меряет узлы КОНФИГА, который у него сейчас загружен, поэтому для
/// импортированной подписки конфиг сначала нужно ему отдать — `importSubscription`
/// делает это без поднятия туннеля и идемпотентен. Для панельного профиля
/// конфиг ядро тянет само (после `configure`), отдельного шага не требуется.
library;

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/vpn/vpn_service.dart';

/// Таймаут одного замера. 6 с — верхняя граница, за которой узел всё равно
/// непригоден для интерактивного трафика.
const Duration kProbeTimeout = Duration(seconds: 6);

/// Меряет задержки узлов [profile]. Бросает ошибку ядра как есть — вызывающий
/// показывает её текст через `coreErrorText`.
Future<List<ProbeResult>> probeProfile(
  VpnConnection conn,
  ConnectionProfile profile, {
  Duration timeout = kProbeTimeout,
}) async {
  if (profile.isRaw) {
    final raw = profile.rawConfig ?? profile.source;
    if (raw.isNotEmpty) {
      await conn.importSubscription(raw: raw, format: profile.format);
    }
  }
  return conn.probe(timeout: timeout);
}

/// Узел с наименьшей задержкой среди ответивших. `null`, если не ответил никто.
ProbeResult? bestOf(List<ProbeResult> results) {
  ProbeResult? best;
  for (final r in results) {
    if (r.timedOut) continue;
    if (best == null || r.latencyMs < best.latencyMs) best = r;
  }
  return best;
}

/// Замер задержек узлов активного профиля (ABI v2 `probe`) — СОБСТВЕННЫЙ замер
/// устройства.
///
/// Это и есть ответ на «пинг должен показываться от пользователя, а не от
/// панели». Панельное `nodes.latency_ms` — RTT самого узла до его цели по
/// heartbeat раз в ~30 с: расстояние УЗЛА до цели. Здесь приложение соединяется
/// с узлом само, со своей сети и со своего устройства.
///
/// Туннель для этого поднимать НЕ нужно и нельзя: замер идёт напрямую до
/// `server:port` каждого прокси (в сборке с ядром — настоящий URL-тест сквозь
/// адаптер прокси, иначе TCP-достижимость). Мерить сквозь уже поднятый туннель
/// значило бы мерить один узел — тот, через который туннель и стоит.
///
/// Ядро меряет узлы КОНФИГА, который у него загружен. Для импортированной
/// подписки конфиг сначала отдаётся ему (`importSubscription`, идемпотентно, без
/// подъёма). Для панельного профиля конфиг ядро тянет само — по закреплённой
/// подписке; ровно этого раньше и не происходило, потому что Android-плагин
/// строил для замера ПУСТОЕ ядро без панельного шва, и `probe` честно возвращал
/// пустой список (он же «Ядро не вернуло ни одного узла» на автоподборе).
library;

import 'package:caramba_vpn/caramba_vpn.dart' show CarambaVpn;
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_error.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/vpn/vpn_service.dart';

/// Таймаут одного замера. 6 с — верхняя граница, за которой узел всё равно
/// непригоден для интерактивного трафика.
const Duration kProbeTimeout = Duration(seconds: 6);

/// Меряет задержки узлов [profile]. Бросает ошибку ядра как есть — вызывающий
/// показывает её текст через `coreErrorText`.
/// [seam] отдаёт ПОЛНЫЙ панельный шов (тот же, что уходит перед `connect`).
/// Без него замер собирается из полей профиля — снимка, сделанного при его
/// создании: токен там устаревает за час, а refresh не лежит вовсе.
Future<List<ProbeResult>> probeProfile(
  VpnConnection conn,
  ConnectionProfile profile, {
  Duration timeout = kProbeTimeout,
  VpnConfigResolver? seam,
}) async {
  if (profile.isRaw) {
    final raw = profile.rawConfig ?? profile.source;
    if (raw.isNotEmpty) {
      await conn.importSubscription(raw: raw, format: profile.format);
    }
  } else {
    await _sendPanelSeam(profile, seam);
  }
  return conn.probe(timeout: timeout);
}

/// Отдаёт ядру панельный шов перед замером на панельном пути.
///
/// Шов до сих пор уходил только из `connect`: ядро, которое приложение держит
/// под метаданные, узнавало о панели лишь после первого подключения — а
/// спрашивают про задержки как раз ДО него, выбирая, куда подключаться. Без шва
/// ядру нечего мерить и неоткуда взять конфиг.
///
/// Второе, что делает этот вызов: он сообщает нативной стороне, что в игре
/// панельный путь. Импортированная подписка шов не шлёт никогда, поэтому его
/// приход — единственный момент, когда плагин вправе сбросить ядро, в котором
/// лежит чужой импорт; иначе панельный замер вернул бы узлы той подписки.
///
/// Ошибка глотается намеренно: на платформах без канала (mock, тесты) метода
/// нет вовсе, и замер это не отменяет — он просто останется без панельного
/// конфига и честно вернёт пустой список с причиной.
/// Третье: шов обязан нести ВСЮ сессию, а не снимок токена с профиля. Снимок
/// делается при создании профиля и живёт вечно, тогда как access — 15 минут, и
/// refresh в нём не лежит совсем. Замер на отлежавшемся телефоне уходил в ядро
/// с мёртвой сессией и возвращал «api: загрузка узлов подписки для замера: …».
/// Поэтому [seam] (общий с `connect` резолвер) главнее полей профиля, а поля
/// остаются запасным путём для вызывающего без Ref.
Future<void> _sendPanelSeam(
  ConnectionProfile profile,
  VpnConfigResolver? seam,
) async {
  VpnConfig? cfg;
  try {
    cfg = seam == null ? null : await seam();
  } catch (_) {
    // Сессию не удалось прочитать (нет плагина secure storage, вход не
    // выполнен). Замер это не отменяет: ниже остаются поля профиля, и лучше
    // померить со снимком, чем не померить вовсе.
  }
  final panelUrl = cfg?.panelUrl ?? profile.panelUrl ?? '';
  if (panelUrl.isEmpty) return;
  try {
    await CarambaVpn.instance.configure(
      panelUrl: panelUrl,
      subscriptionId: cfg?.subscriptionUuid ?? profile.subscriptionUuid ?? '',
      accessToken: cfg?.accessToken ?? profile.accessToken ?? '',
      refreshToken: cfg?.refreshToken ?? '',
      accessExpiry: cfg?.accessExpiry,
    );
  } catch (_) {
    // Канала нет — см. выше.
  }
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

/// Состояние ХОДА замера — и только его.
///
/// Сами числа живут на профиле (`ConnectionProfile.lastProbe`) и переживают
/// перезапуск; держать их ещё и здесь значило бы завести второй источник того
/// же факта. Здесь — «идёт ли замер прямо сейчас» и «чем он кончился», то, чего
/// на профиле нет и быть не должно.
class ProbeRun {
  /// Замер в полёте. Список на это время НЕ блокируется: строки уже показаны,
  /// а «меряю» — состояние их значения задержки.
  final bool measuring;

  /// Текст последней неудачи; `null` — её не было.
  final String? error;

  /// Ключ профиля, который меряли последним. Смена профиля обнуляет ход: чужой
  /// «меряю» на новом профиле — враньё про узлы, к которым он не относится.
  final String? profileId;

  const ProbeRun({this.measuring = false, this.error, this.profileId});

  static const ProbeRun idle = ProbeRun();
}

/// Ход замера активного профиля.
class ProbeRunNotifier extends StateNotifier<ProbeRun> {
  final Ref _ref;

  ProbeRunNotifier(this._ref) : super(ProbeRun.idle);

  /// Запускает замер активного профиля и складывает результат на профиль.
  ///
  /// Возвращает `true`, если ядро вернуло хоть один узел. Повторный вызов, пока
  /// предыдущий не кончился, игнорируется: два параллельных замера открыли бы
  /// вдвое больше соединений и переписали бы результат друг друга.
  Future<bool> measure() async {
    if (state.measuring) return false;
    final profile = _ref.read(activeConnectionProfileProvider);
    if (profile == null) {
      state = const ProbeRun(error: 'Профиль подключения не выбран.');
      return false;
    }
    state = ProbeRun(measuring: true, profileId: profile.id);
    try {
      final results = await probeProfile(
        _ref.read(vpnConnectionProvider),
        profile,
        seam: _ref.read(probeSeamResolverProvider),
      );
      await _ref
          .read(connectionProfilesProvider.notifier)
          .setProbe(profile.id, ProbeSnapshot.fromResults(results));
      if (!mounted) return results.isNotEmpty;
      state = ProbeRun(
        profileId: profile.id,
        // Пустой ответ — это не успех с нулём узлов, а отказ, который надо
        // назвать: именно им и выглядела панельная ветка до починки ядра.
        error: results.isEmpty
            ? 'Ядро не вернуло ни одного узла для замера.'
            : null,
      );
      return results.isNotEmpty;
    } catch (e) {
      if (!mounted) return false;
      state = ProbeRun(
        profileId: profile.id,
        error: coreErrorText(e) ?? 'Не удалось замерить задержки.',
      );
      return false;
    }
  }

  /// Убирает сообщение об ошибке, не трогая числа.
  void clearError() {
    if (state.error == null) return;
    state = ProbeRun(measuring: state.measuring, profileId: state.profileId);
  }
}

final probeRunProvider = StateNotifierProvider<ProbeRunNotifier, ProbeRun>(
  ProbeRunNotifier.new,
);

/// Собственные замеры активного профиля: имя прокси -> мс (`-1` — таймаут).
///
/// Пусто — своего замера ещё не было. Ключ здесь ИМЯ ПРОКСИ, потому что мерит
/// ядро и называет узлы так, как они названы в теле конфига; в узел панели это
/// имя превращает инвентарь, у которого есть `inbounds[].proxy_name`.
final clientLatencyProvider = Provider<Map<String, int>>((ref) {
  final probe = ref.watch(activeConnectionProfileProvider)?.lastProbe;
  return probe?.latencyMs ?? const <String, int>{};
});

/// Когда собственный замер выполнялся последний раз; `null` — не выполнялся.
final clientLatencyAtProvider = Provider<DateTime?>(
  (ref) => ref.watch(activeConnectionProfileProvider)?.lastProbe?.updatedAt,
);

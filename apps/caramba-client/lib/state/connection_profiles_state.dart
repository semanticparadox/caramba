import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

/// Состояние мульти-профиля подключения (план §4.4).
///
/// КОЛЛИЗИЯ ИМЁН: это НЕ аккаунт-профиль (`profile_state.dart`). Здесь живут
/// [ConnectionProfile] — импортированные подписки и аккаунты панели. Ровно один
/// активен. Активный профиль ведёт подключение (Build C читает
/// [activeConnectionProfileProvider]).
///
/// Персист: первый нотифаер в приложении с реальным write-through в secure
/// storage (см. [ConnectionProfilesStore]). Грузим в конструкторе (async,
/// seed из стора), пишем после каждой мутации.

/// Платформенное secure storage профилей подключения. Параллель
/// `tokenStoreProvider` из `providers.dart`, но объявлена здесь, чтобы не
/// трогать чужой файл (Build C владеет providers.dart).
final connectionProfilesStoreProvider = Provider<ConnectionProfilesStore>(
  (ref) => ConnectionProfilesStore(),
);

/// Иммутабельный снимок мульти-профиля: список + id активного + флаг загрузки.
class ConnectionProfilesState {
  final List<ConnectionProfile> profiles;
  final String? activeId;

  /// `true`, пока идёт первичная подгрузка из стора (для состояния «загрузка»).
  final bool loading;

  const ConnectionProfilesState({
    this.profiles = const [],
    this.activeId,
    this.loading = true,
  });

  bool get isEmpty => profiles.isEmpty;

  /// Активный профиль или `null`. Если [activeId] не совпал ни с одним
  /// (удалили активный), отдаём первый из списка как безопасный дефолт.
  ConnectionProfile? get active {
    if (profiles.isEmpty) return null;
    for (final p in profiles) {
      if (p.id == activeId) return p;
    }
    return profiles.first;
  }

  ConnectionProfilesState copyWith({
    List<ConnectionProfile>? profiles,
    String? activeId,
    bool clearActive = false,
    bool? loading,
  }) => ConnectionProfilesState(
    profiles: profiles ?? this.profiles,
    activeId: clearActive ? null : (activeId ?? this.activeId),
    loading: loading ?? this.loading,
  );
}

class ConnectionProfilesNotifier
    extends StateNotifier<ConnectionProfilesState> {
  final ConnectionProfilesStore _store;

  ConnectionProfilesNotifier(this._store)
    : super(const ConnectionProfilesState()) {
    _load();
  }

  Future<void> _load() async {
    final stored = await _store.readProfiles();
    final storedActiveId = await _store.readActiveId();

    // Мутация могла прилететь раньше, чем ответило хранилище: энроллмент по
    // deeplink на холодном старте заводит профиль сразу, и загрузка не имеет
    // права его затереть. Поэтому сохранённые записи ДОБАВЛЯЮТСЯ к тем, что
    // уже есть в памяти, а не заменяют их.
    final pending = state.profiles;
    if (pending.isEmpty) {
      state = ConnectionProfilesState(
        profiles: stored,
        activeId: storedActiveId,
        loading: false,
      );
      return;
    }
    final known = pending.map((p) => p.id).toSet();
    state = ConnectionProfilesState(
      profiles: [...stored.where((p) => !known.contains(p.id)), ...pending],
      // Активный, выбранный уже в этой сессии, важнее сохранённого.
      activeId: state.activeId ?? storedActiveId,
      loading: false,
    );
    // Мутация успела записать в стор ТОЛЬКО свою часть списка: возвращаем туда
    // полный, иначе сохранённые профили потерялись бы.
    await _persist();
  }

  Future<void> _persist() async {
    await _store.writeProfiles(state.profiles);
    await _store.writeActiveId(state.activeId);
  }

  /// Добавляет профиль. Первый добавленный становится активным автоматически.
  /// Возвращает id добавленного профиля.
  Future<String> add(ConnectionProfile profile) async {
    final next = [...state.profiles, profile];
    final makeActive = state.profiles.isEmpty || state.activeId == null;
    state = state.copyWith(
      profiles: next,
      activeId: makeActive ? profile.id : state.activeId,
    );
    await _persist();
    return profile.id;
  }

  /// Удаляет профиль по id. Если удалили активный — активным становится первый
  /// оставшийся (или null, если список опустел).
  Future<void> remove(String id) async {
    final next = state.profiles
        .where((p) => p.id != id)
        .toList(growable: false);
    if (state.activeId == id) {
      state = ConnectionProfilesState(
        profiles: next,
        activeId: next.isEmpty ? null : next.first.id,
        loading: false,
      );
    } else {
      state = state.copyWith(profiles: next);
    }
    await _persist();
  }

  /// Делает профиль активным и отмечает время активации.
  Future<void> activate(String id) async {
    final exists = state.profiles.any((p) => p.id == id);
    if (!exists) return;
    final now = DateTime.now().millisecondsSinceEpoch;
    final next = state.profiles
        .map((p) => p.id == id ? p.copyWith(lastActiveMs: now) : p)
        .toList(growable: false);
    state = state.copyWith(profiles: next, activeId: id);
    await _persist();
  }

  /// P2-хелпер: заводит профиль аккаунта панели по URL панели из enroll-ссылки.
  ///
  /// subscriptionUuid/accessToken остаются `null` до входа (их проставит auth-
  /// слой/Build C после логина). [displayName] берётся из `panel_name`
  /// валидации энроллмента, иначе из [kBrandName]-фолбэка вызывающего. Дубликат
  /// по [panelUrl] не заводим повторно — возвращаем id существующего профиля и
  /// делаем его активным. Возвращает id профиля (нового или существующего).
  Future<String> addPanelAccount({
    required String panelUrl,
    required String displayName,
  }) async {
    final existing = state.profiles
        .where((p) => p.isPanel && p.panelUrl == panelUrl)
        .toList(growable: false);
    if (existing.isNotEmpty) {
      final id = existing.first.id;
      // Уточняем имя, если профиль ещё носит placeholder (URL панели) и пришло
      // настоящее имя из panel_name. Пользовательское переименование не трогаем.
      if (existing.first.displayName == panelUrl &&
          displayName.isNotEmpty &&
          displayName != panelUrl) {
        await rename(id, displayName);
      }
      await activate(id);
      return id;
    }
    final profile = ConnectionProfile(
      id: 'cp_${DateTime.now().millisecondsSinceEpoch}',
      type: ProfileType.panelAccount,
      displayName: displayName,
      source: panelUrl,
      panelUrl: panelUrl,
      lastActiveMs: 0,
    );
    return add(profile);
  }

  /// Кэширует брендинг на профиль (P3, contract E). [branding] — это
  /// `Branding.toJson()` (ключи совпадают с контрактом панели `brand_*`).
  /// Пишет только если значение изменилось, чтобы не дёргать стор зря.
  Future<void> setBranding(String id, Map<String, dynamic> branding) async {
    final idx = state.profiles.indexWhere((p) => p.id == id);
    if (idx < 0) return;
    final current = state.profiles[idx].brandingCache;
    if (_sameBranding(current, branding)) return;
    final next = [...state.profiles];
    next[idx] = next[idx].copyWith(brandingCache: branding);
    state = state.copyWith(profiles: next);
    await _persist();
  }

  static bool _sameBranding(Map<String, dynamic>? a, Map<String, dynamic> b) {
    if (a == null) return false;
    if (a.length != b.length) return false;
    for (final e in b.entries) {
      if (a[e.key] != e.value) return false;
    }
    return true;
  }

  /// Возвращает id профиля панели по её URL, если такой уже заведён.
  /// Энроллмент этим отличает «создали новый» от «нашли существующий», чтобы
  /// на невалидном коде убрать за собой только СВОЙ профиль-плейсхолдер.
  String? findPanelId(String panelUrl) {
    for (final p in state.profiles) {
      if (p.isPanel && p.panelUrl == panelUrl) return p.id;
    }
    return null;
  }

  /// Точечное обновление профиля по id. Мутатор получает текущую запись и
  /// возвращает новую; несуществующий id — no-op.
  Future<void> _update(
    String id,
    ConnectionProfile Function(ConnectionProfile) mutate,
  ) async {
    final idx = state.profiles.indexWhere((p) => p.id == id);
    if (idx < 0) return;
    final next = [...state.profiles];
    next[idx] = mutate(next[idx]);
    state = state.copyWith(profiles: next);
    await _persist();
  }

  /// Записывает результат `importSubscription`: тело подписки, формат и кэш
  /// узлов. Пин узла сбрасывается, если его больше нет в новом списке (после
  /// «Обновить подписку» состав узлов мог поменяться).
  ///
  /// Вместе с пином проверяется и СТРАНА. Пережить обновление она имеет право
  /// ровно до тех пор, пока в новом составе есть хоть один её узел: страна без
  /// узлов не разрешается ни `rawProxyNameForCountry`, ни автоподбором, и
  /// `connectRaw` уходит с пустым именем прокси — то есть в любой узел, какой
  /// выберет ядро. Оставленная в профиле, такая страна остаётся галочкой
  /// «Германия» над канадским выходом; это то же расхождение, что и уцелевший
  /// пин, только с другим триггером, поэтому и снимается здесь же.
  Future<void> setImported(
    String id, {
    required String rawConfig,
    required String format,
    required List<ImportedServer> servers,
    DateTime? at,
  }) => _update(id, (p) {
    final keepPin =
        p.selectedServerId != null &&
        servers.any((s) => s.id == p.selectedServerId);
    final country = normalizeCountryCode(p.selectedExitCountry);
    final keepCountry =
        country.isEmpty ||
        servers.any((s) => normalizeCountryCode(s.country) == country);
    return p.copyWith(
      rawConfig: rawConfig,
      format: format,
      servers: servers,
      serversUpdatedMs: (at ?? DateTime.now()).millisecondsSinceEpoch,
      clearSelectedServer: !keepPin,
      clearExitCountry: !keepCountry,
    );
  });

  /// Прикрепляет селектор к узлу подписки. `null` — авто-выбор ядром.
  Future<void> setSelectedServer(String id, String? serverId) => _update(
    id,
    (p) => p.copyWith(
      selectedServerId: serverId,
      clearSelectedServer: serverId == null,
    ),
  );

  /// Закрепляет страну выхода на профиле. `null` — «авто».
  ///
  /// Пины, противоречащие новой стране, снимаются здесь же: профиль, у которого
  /// страна DE, а закреплённый узел канадский, разрешался бы по-разному в
  /// зависимости от того, кто прочитал его первым. Пин узла подписки снимается
  /// по известной стране узла, панельный — безусловно: списка узлов панели у
  /// профиля нет, и устаревший `nodes.id` хуже, чем честный автоподбор.
  Future<void> setSelectedExitCountry(String id, String? countryCode) {
    final code = (countryCode ?? '').trim().toUpperCase();
    final country = code.length == 2 ? code : null;
    return _update(id, (p) {
      final pinned = p.selectedServerId;
      final keepRawPin =
          pinned != null &&
          (country == null ||
              p.servers.any(
                (s) => s.id == pinned && s.country.toUpperCase() == country,
              ));
      return p.copyWith(
        selectedExitCountry: country,
        clearExitCountry: country == null,
        clearExitNode: true,
        clearSelectedServer: !keepRawPin,
      );
    });
  }

  /// Закрепляет узел панели (`nodes.id`). `null` — автоподбор приложением.
  Future<void> setSelectedExitNode(String id, int? nodeId) => _update(
    id,
    (p) =>
        p.copyWith(selectedExitNodeId: nodeId, clearExitNode: nodeId == null),
  );

  /// Сохраняет результат замера задержек (`probe`).
  Future<void> setProbe(String id, ProbeSnapshot probe) =>
      _update(id, (p) => p.copyWith(lastProbe: probe));

  /// Проставляет профилю панели его собственные креды, чтобы `configure`
  /// уходило на ЕГО инстанс, а не на тенант-1 (`kApiBaseUrl`). Вызывается
  /// после успешного энроллмент-входа.
  Future<void> setPanelCredentials(
    String id, {
    String? panelUrl,
    String? subscriptionUuid,
    String? accessToken,
  }) => _update(
    id,
    (p) => p.copyWith(
      panelUrl: panelUrl,
      subscriptionUuid: subscriptionUuid,
      accessToken: accessToken,
    ),
  );

  /// Записывает состояние CSM/1 на профиль.
  ///
  /// Единственный путь мутации состояния CSM: закреплённый корень, отметки
  /// максимума версий, временной пол, настройки с происхождением и карточки
  /// живут ровно здесь, в одной записи secure storage на профиль. Второго дома
  /// у них нет, потому что два хранилища отметок это дыра для отката, а не
  /// защита в глубину (02-SPEC.md 5.1, 8.8.3).
  Future<void> setCsm(String id, CsmProfileState csm) =>
      _update(id, (p) => p.copyWith(csm: csm));

  /// Переименование профиля.
  Future<void> rename(String id, String displayName) async {
    final trimmed = displayName.trim();
    if (trimmed.isEmpty) return;
    final next = state.profiles
        .map((p) => p.id == id ? p.copyWith(displayName: trimmed) : p)
        .toList(growable: false);
    state = state.copyWith(profiles: next);
    await _persist();
  }
}

/// Мульти-профиль: список + активный id. Build C читает производный
/// [activeConnectionProfileProvider], чтобы выбрать путь подключения.
final connectionProfilesProvider =
    StateNotifierProvider<ConnectionProfilesNotifier, ConnectionProfilesState>(
      (ref) => ConnectionProfilesNotifier(
        ref.watch(connectionProfilesStoreProvider),
      ),
    );

/// Текущий активный [ConnectionProfile] или `null` (нет профилей / ещё грузим).
/// Build C ветвится по `profile.type`: panelAccount -> configure+connect;
/// rawSub -> connectRaw.
final activeConnectionProfileProvider = Provider<ConnectionProfile?>(
  (ref) => ref.watch(connectionProfilesProvider).active,
);

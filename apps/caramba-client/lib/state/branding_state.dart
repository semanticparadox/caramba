import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/branding.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';

/// Брендинг активного инстанса панели (P3, contract E).
///
/// Ведёт за активным `panelAccount`-[ConnectionProfile]: тянет
/// `GET /api/v2/app/branding` с его `panelUrl` (ПУБЛИЧНО, без JWT) и применяет
/// `brand_name/logo_url/accent_hex` в рантайме. Дефолт = вид Caramba Connect,
/// когда `enabled=false`.
///
/// КЭШ: branding кладётся в `ConnectionProfile.brandingCache` (per-panelUrl,
/// контракт E), чтобы при следующем старте сразу нарисовать прошлый бренд без
/// мигания дефолта, пока летит сетевой запрос.
///
/// АНТИ-СЛОП: акцент применяется ТОЛЬКО к нейтральным accent-токенам после
/// [Branding.brandAccentColor] (отклоняет purple/violet/indigo и статус-цвета).
/// Статус-цвета (connected/connecting/error) бренд НЕ трогает — это делает
/// `AppColors.withBrandAccent`, который правит лишь четыре accent-поля.

/// Текущее состояние брендинга: значение + флаг фоновой загрузки.
@immutable
class BrandingState {
  /// Применяемый прямо сейчас брендинг (из кэша или из сети). Никогда не null —
  /// дефолт = [Branding.fallback] (Caramba Connect look).
  final Branding branding;

  /// Идёт ли сейчас сетевой fetch (для возможного индикатора; UI может игнорить
  /// и просто рисовать [branding], т.к. оно всегда валидно).
  final bool loading;

  const BrandingState({
    this.branding = Branding.fallback,
    this.loading = false,
  });

  BrandingState copyWith({Branding? branding, bool? loading}) => BrandingState(
        branding: branding ?? this.branding,
        loading: loading ?? this.loading,
      );
}

/// Нотифаер брендинга. Реагирует на смену активного профиля (panelUrl) и
/// перезагружает бренд; для rawSub / отсутствия профиля — дефолт Caramba Connect.
class BrandingNotifier extends StateNotifier<BrandingState> {
  final Ref _ref;

  /// panelUrl, под который сейчас загружен/загружается бренд. Чтобы не
  /// перезапрашивать при неважных пересборках и игнорировать «протухший» ответ
  /// после быстрого переключения профиля.
  String? _activePanelUrl;

  BrandingNotifier(this._ref) : super(const BrandingState()) {
    // Реагируем на смену активного профиля подключения.
    _ref.listen<ConnectionProfile?>(
      activeConnectionProfileProvider,
      (_, next) => _onProfile(next),
      fireImmediately: true,
    );
  }

  void _onProfile(ConnectionProfile? profile) {
    // Бренд ведёт только аккаунт панели. Импорт-подписка / пустой профиль =>
    // дефолтный вид Caramba Connect.
    final panelUrl = profile?.panelUrl;
    if (profile == null || !profile.isPanel || panelUrl == null || panelUrl.isEmpty) {
      _activePanelUrl = null;
      state = const BrandingState();
      return;
    }

    // Тот же panel — ничего не делаем (бренд уже актуален).
    if (panelUrl == _activePanelUrl) return;
    _activePanelUrl = panelUrl;

    // Мгновенно красим из кэша профиля (если есть), пока летит сеть.
    final cached = profile.brandingCache;
    final seed = cached != null && cached.isNotEmpty
        ? Branding.fromJson(cached)
        : Branding.fallback;
    state = BrandingState(branding: seed, loading: true);

    // Фоновый рефреш по сети.
    _fetch(profile.id, panelUrl);
  }

  Future<void> _fetch(String profileId, String panelUrl) async {
    // Отдельный клиент, нацеленный на URL панели активного профиля; без JWT.
    final client = ApiClient(tokens: TokenStore(), baseUrl: panelUrl);
    final fresh = await client.getBranding();

    // Профиль успел переключиться, пока летел запрос — игнорируем ответ.
    if (_activePanelUrl != panelUrl) return;

    state = BrandingState(branding: fresh, loading: false);

    // Кэшируем на профиль для мгновенной отрисовки в следующий раз.
    await _ref
        .read(connectionProfilesProvider.notifier)
        .setBranding(profileId, fresh.toJson());
  }

  /// Ручной рефреш (pull-to-refresh / повтор после ошибки сети).
  Future<void> refresh() async {
    final profile = _ref.read(activeConnectionProfileProvider);
    final panelUrl = profile?.panelUrl;
    if (profile == null || !profile.isPanel || panelUrl == null || panelUrl.isEmpty) {
      return;
    }
    state = state.copyWith(loading: true);
    await _fetch(profile.id, panelUrl);
  }
}

/// Брендинг активного инстанса панели. UI/тема читают `.branding`; всегда
/// валиден (дефолт = Caramba Connect). Применяется в `main.dart`:
///   * `title` / wordmark         => `branding.displayName(kBrandName)`
///   * `AppTheme.dark/light`      => `brandAccent: branding.brandAccentColor`
///   * powered-by/upsell-блок     => виден при `branding.upstreamAds`
final brandingProvider =
    StateNotifierProvider<BrandingNotifier, BrandingState>(
  (ref) => BrandingNotifier(ref),
);

/// Удобный производный провайдер: только сам [Branding] (для виджетов, которым
/// не нужен флаг загрузки).
final activeBrandingProvider = Provider<Branding>(
  (ref) => ref.watch(brandingProvider).branding,
);

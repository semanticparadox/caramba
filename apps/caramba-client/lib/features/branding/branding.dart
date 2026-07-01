/// Бренд-фича клиента (P3, contract E): рантайм-брендинг + powered-by/upsell.
///
/// Экспортирует виджеты, которые экраны (login/home/settings) подключают:
///   * [BrandWordmark] — логотип/текстовый вордмарк активного инстанса;
///   * [PoweredBy]     сдержанный powered-by/upsell блок (виден на Free).
///
/// Состояние брендинга живёт в `state/branding_state.dart`
/// (`brandingProvider` / `activeBrandingProvider`), тема применяет акцент в
/// `main.dart` через `AppTheme.dark/light(brandAccent: ...)`.
library;

export 'package:caramba_client/features/branding/brand_wordmark.dart';
export 'package:caramba_client/features/branding/powered_by.dart';

/// Профиль-провайдеры переехали в `account_state.dart` (реальные вызовы панели:
/// подписки, устройства, рефералы, семья). Этот файл оставлен как точка
/// реэкспорта, чтобы не плодить импорты в существующих экранах.
export 'package:caramba_client/state/account_state.dart'
    show
        subscriptionsProvider,
        devicesProvider,
        DevicesNotifier,
        referralProvider,
        familyProvider;

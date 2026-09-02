/// Модели generic-режима: метаданные импортированной подписки и результаты
/// замера задержек узлов.
///
/// Сами модели живут в плагине `package:caramba_vpn` (их разбирают и канальный,
/// и FFI-путь), здесь — канонический путь импорта для приложения:
/// `import 'package:caramba_client/vpn/vpn_models.dart';`.
///
/// * `ImportResult` — `{name?, servers[]}`, результат `importSubscription` БЕЗ
///   подключения;
/// * `ImportedServer` — `id`, `name`, `type`, `server`, `port`, `country`
///   (`id` — имя прокси, оно же `serverId` для `connectRaw`);
/// * `ProbeResult` — `id`, `name`, `country`, `latencyMs` (-1 = таймаут).
library;

export 'package:caramba_vpn/caramba_vpn.dart'
    show ImportResult, ImportedServer, ProbeResult;

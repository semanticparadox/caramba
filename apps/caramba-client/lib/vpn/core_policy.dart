/// Политика ядра caramba-core (ABI v2 `CarambaSetPolicy` /
/// `Client.SetPolicyJSON`) и способ захвата трафика.
///
/// Сами модели живут в плагине `package:caramba_vpn` (их использует и
/// FFI-реализация), здесь — канонический путь импорта для приложения:
/// `import 'package:caramba_client/vpn/core_policy.dart';`.
///
/// `CorePolicy.toJson` отдаёт JSON ровно по контракту ABI v2: все поля
/// опциональны, null-поля не пишутся вовсе (ядро трактует отсутствие ключа как
/// «не менять»).
library;

export 'package:caramba_vpn/caramba_vpn.dart'
    show CorePolicy, CorePolicyDns, CorePolicySplit, TunnelMode;

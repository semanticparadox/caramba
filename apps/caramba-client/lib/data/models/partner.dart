import 'package:caramba_client/data/models/sub_plan.dart';

/// Один партнёрский код с накопленной статистикой источника. Соответствует
/// элементу `codes[]` из `GET /api/v2/app/partner/codes`
/// (`apps/caramba-panel/src/api/v2/app_partner.rs`):
/// ```json
/// { "code":"EXARO-YT1", "source_label":"youtube",
///   "created_at":"RFC3339", "clicks":1240, "signups":86,
///   "conversions":12, "balance_earned":3600 }
/// ```
/// Партнёрские коды используют тот же механизм атрибуции, что и реферальные:
/// код привязывает нового пользователя к владельцу. `conversion` =
/// атрибутированный пользователь совершил первую платную покупку;
/// `balance_earned` — реферальный баланс с пользователей этого кода, в минорных
/// единицах (копейки/центы), как в биллинге.
class PartnerCode {
  final String code;

  /// Метка источника (youtube, tg-канал, имя блогера) — задаётся при создании.
  final String sourceLabel;

  final DateTime? createdAt;

  /// Переходы по ссылке кода.
  final int clicks;

  /// Регистрации, атрибутированные коду.
  final int signups;

  /// Конверсии: атрибутированные пользователи с первой оплатой.
  final int conversions;

  /// Всего начислено с пользователей этого кода, минорные единицы.
  final int balanceEarnedCents;

  const PartnerCode({
    required this.code,
    this.sourceLabel = '',
    this.createdAt,
    this.clicks = 0,
    this.signups = 0,
    this.conversions = 0,
    this.balanceEarnedCents = 0,
  });

  /// Метка источника для UI или плейсхолдер, если не задана.
  String get sourceDisplay {
    final s = sourceLabel.trim();
    return s.isEmpty ? 'Без метки' : s;
  }

  /// Реферальная ссылка кода (короткий путь панели `/r/CODE`).
  String get referralLink => 'https://exarobot.top/r/$code';

  /// Всего начислено в денежных единицах строкой без хвостовых нулей.
  String get balanceEarnedLabel => ReferralInfo.formatMinor(balanceEarnedCents);

  factory PartnerCode.fromJson(Map<String, dynamic> json) => PartnerCode(
        code: (json['code'] as String?) ?? '',
        sourceLabel: (json['source_label'] as String?) ?? '',
        createdAt: SubPlan.parseDate(json['created_at']),
        clicks: (json['clicks'] as num?)?.toInt() ?? 0,
        signups: (json['signups'] as num?)?.toInt() ?? 0,
        conversions: (json['conversions'] as num?)?.toInt() ?? 0,
        balanceEarnedCents: (json['balance_earned'] as num?)?.toInt() ?? 0,
      );
}

/// Партнёрская сводка (`GET /api/v2/app/partner/codes`):
/// ```json
/// { "is_partner": true, "codes": [ { ... } ] }
/// ```
/// `is_partner` гейтит весь раздел: вход в дашборд показывается только когда
/// панель подтвердила партнёрскую роль пользователя.
class PartnerOverview {
  final bool isPartner;
  final List<PartnerCode> codes;

  const PartnerOverview({this.isPartner = false, this.codes = const []});

  factory PartnerOverview.fromJson(Map<String, dynamic> json) =>
      PartnerOverview(
        isPartner: (json['is_partner'] as bool?) ?? false,
        codes: ((json['codes'] as List?) ?? const [])
            .whereType<Map>()
            .map((e) => PartnerCode.fromJson(e.cast<String, dynamic>()))
            .toList(growable: false),
      );
}

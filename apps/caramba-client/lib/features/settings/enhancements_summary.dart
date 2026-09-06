/// Подписи раздела «Реклама и блокировки» в Настройках и — главное — правило,
/// по которому здесь вообще позволено писать «включено».
///
/// Имя файла осталось от вкладки «Улучшения», где эти подписи жили раньше;
/// вкладка растворена (реклама уехала в Настройки, списки сайтов — на
/// «Правила по сайтам», режим — на Главную), а правило переезда не пережило
/// бы переименование файла ради одного слова: его импортирует ещё и
/// [AppliedRouteCard].
///
/// Владелец просил галочки: блок рекламы, «через VPN только эти сайты»,
/// пресеты по странам. Галочка, которая ничего не включает, хуже её
/// отсутствия, поэтому каждое утверждение этого файла разделено надвое:
///   * ЧТО ПОПРОСИЛИ — [CoreConfig], выбор человека;
///   * ЧТО ПРИМЕНИЛОСЬ — [AppliedRoute], отчёт ядра о поднятом туннеле.
///
/// Первое без второго называется «включится при подключении», а не
/// «работает». Второе, пришедшее отказом, называет причину. Ни одна ветка
/// здесь не превращает «мы попросили» в «работает».
library;

import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/features/settings/route_report.dart';
import 'package:caramba_client/state/core_config_state.dart';

/// Состояние блока рекламы глазами ядра.
enum AdBlockState {
  /// Переключатель выключен.
  off,

  /// Включён, но подъёма ещё не было — подтверждать нечего.
  pending,

  /// Список правил доехал до сборки: правила в туннеле есть.
  working,

  /// Список выпущен ссылкой на зеркало оператора: доедет ли он, из сборки
  /// конфига не видно.
  mirrored,

  /// Список не доехал, и запасное правило по встроенной базе тоже мертво.
  broken,

  /// Включён, туннель поднят, а подтвердить нечем.
  unconfirmed,
}

/// Разбор блока рекламы: просьба пользователя + отчёт ядра.
class AdBlockStatus {
  final AdBlockState state;

  /// Что показать человеку под переключателем. Одна фраза, без английского
  /// `detail` ядра — он для журнала.
  final String message;

  const AdBlockStatus(this.state, this.message);

  /// Правила действительно есть в применённой сборке.
  bool get isWorking => state == AdBlockState.working;

  /// Показывать ли строку тревожной.
  bool get isBad => state == AdBlockState.broken;
}

/// Имя списка рекламы на зеркале оператора (`routing.AdBlockRuleSet`).
const String kAdsRuleSetName = 'ads';

/// Строка списка рекламы в отчёте ядра; `null` — ядро его не называло.
///
/// Ищется ПО ИМЕНИ, а не по признаку «этот пресет режет рекламу»: с появлением
/// переключателя блок рекламы приходит поверх любого пресета, и признак
/// реестра отвечал бы про соседнее утверждение — включает ли рекламу САМ
/// пресет. Два ответа на одном экране расходились бы на глазах у человека.
AppliedRuleSource? adsSourceOf(AppliedRoute? applied) {
  for (final s in applied?.preset?.sources ?? const <AppliedRuleSource>[]) {
    if (s.name == kAdsRuleSetName) return s;
  }
  return null;
}

/// Состояние блока рекламы. [applied] == null означает «отчёта нет».
AdBlockStatus adBlockStatus(CoreConfig cfg, AppliedRoute? applied) {
  if (!cfg.blockAds) {
    return const AdBlockStatus(
      AdBlockState.off,
      'Реклама и трекеры не блокируются.',
    );
  }
  if (applied == null || !applied.hasReport) {
    return const AdBlockStatus(
      AdBlockState.pending,
      'Включится при следующем подключении. Пока туннель не поднимался, '
      'проверить нечем.',
    );
  }

  final s = adsSourceOf(applied);
  if (s != null) {
    return switch (s.state) {
      RuleSourceState.file => AdBlockStatus(
        AdBlockState.working,
        'Работает: список из проверенного файла, правил ${s.keptRules}.',
      ),
      RuleSourceState.mirror => const AdBlockStatus(
        AdBlockState.mirrored,
        'Список выпущен ссылкой на зеркало оператора. Доехал ли он, сборка '
        'конфига не видит — это знает только движок во время работы.',
      ),
      RuleSourceState.dropped => AdBlockStatus(
        // Список выброшен, но запасное правило по встроенной базе GEOSITE
        // остаётся — мёртво оно только когда мертва сама база.
        //
        // На живом устройстве `s.reason` здесь стабильно "not_in_catalog", а
        // не "no_mirror" — то есть панель список ОТДАЁТ (148 872 домена по
        // /rulesets), но не подписывает его в доверенном каталоге. Цепочка,
        // почему это выброс, а не сеть: libs/caramba-core/routing/presets.go
        // (`available()`, ~строка 206) видит пустой `opt.Files["ads"]` при
        // `opt.Verified == true` → libs/caramba-core/api/bootstrap.go
        // (`guard.Entry(name)`, ~строка 142) не находит запись `ads` в
        // страже → libs/caramba-core/transport/resource.go:65
        // (`resourceSet`) строит этот страж ИСКЛЮЧИТЕЛЬНО из `cat.RS` —
        // списка ресурсов, подписанных панелью в CSM-каталоге, а не из
        // содержимого /rulesets (инвариант 12 запрещает подставлять
        // неподписанный файл, даже если он доступен по сети). Правка — на
        // стороне панели (подписать `ads` в каталоге), это вне Go-ядра и вне
        // этого приложения; здесь можно только не приукрашивать результат.
        applied.geositeDead ? AdBlockState.broken : AdBlockState.unconfirmed,
        applied.geositeDead
            ? 'Не работает: ${s.message}, и встроенная база GEOSITE в этой '
                  'сборке тоже недоступна.'
            : 'Список оператора не доехал (${s.message}). Осталось запасное '
                  'правило по встроенной базе GEOSITE — подтвердить его нечем.',
      ),
      RuleSourceState.unknown => const AdBlockStatus(
        AdBlockState.unconfirmed,
        'Состояние списка рекламы ядру неизвестно.',
      ),
    };
  }

  if (applied.geositeDead) {
    return const AdBlockStatus(
      AdBlockState.broken,
      'Не работает: правила держатся на базе GEOSITE, а она в этой сборке '
      'недоступна.',
    );
  }
  return const AdBlockStatus(
    AdBlockState.unconfirmed,
    'Ядро не назвало источник списка рекламы. Правила могли собраться по '
    'встроенной базе, но подтвердить это нечем.',
  );
}

/// Сводка строки «Правила по сайтам» в Настройках.
///
/// Режим (страна) здесь не называется НАМЕРЕННО, хотя раньше сводка
/// «Улучшений» начиналась именно с него: режим переехал на Главную и там же
/// подписан своим именем. Строка, которая повторяла бы его в Настройках,
/// снова создала бы два места для одной величины — ровно то, что владелец и
/// просил убрать.
///
/// Счёт идёт по тому, что РЕАЛЬНО уйдёт ядру ([CoreConfig.siteRuleCount]):
/// домены, набранные при выключенном режиме, не считаются, потому что ядру
/// они не уходят.
String siteRulesSummary(CoreConfig cfg) => switch (cfg.splitMode) {
  SplitMode.off => 'Выключено — списков нет',
  SplitMode.onlySelected =>
    'Только выбранные · ${_countSites(cfg.siteRuleCount)}',
  SplitMode.bypassSelected =>
    'Кроме выбранных · ${_countSites(cfg.siteRuleCount)} напрямую',
};

String _countSites(int n) {
  final mod100 = n % 100;
  final mod10 = n % 10;
  final word = (mod100 >= 11 && mod100 <= 14)
      ? 'сайтов'
      : switch (mod10) {
          1 => 'сайт',
          2 || 3 || 4 => 'сайта',
          _ => 'сайтов',
        };
  return '$n $word';
}

/// Человеческий список выбранных наборов сайтов, для подписи под списком.
String allowSitesLine(CoreConfig cfg) {
  final names = <String>[
    for (final t in kAllowSiteTags)
      if (cfg.allowSites.contains(t.tag)) t.name,
  ];
  if (names.isEmpty) return '';
  return names.join(', ');
}

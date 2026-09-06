import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/features/settings/applied_route_card.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/features/settings/route_report.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// «Правила по сайтам» — списки доменов, которые ведут трафик мимо общего
/// режима или, наоборот, только через туннель.
///
/// Экран собран из бывших «Улучшений» (`/split-tunnel`), и это не переезд ради
/// переезда. На «Улучшениях» лежали три независимые вещи: блок рекламы, списки
/// сайтов и выбор режима. Режим владелец забрал на Главную, блок рекламы — в
/// Настройки, и оставшийся экран из трёх разделов стал бы оболочкой с одним.
/// Хуже того, пока он существовал, блок рекламы жил в ДВУХ местах сразу, и на
/// вопрос «где это включается» приложение отвечало двумя разными адресами.
///
/// Что осталось здесь и почему именно здесь: списки сайтов правятся сериями
/// (набрал домены, посмотрел, дописал), у них есть свои поля ввода и свои
/// причины отказа, и строке настроек такой объём не по размеру. Поэтому это
/// отдельный экран, а не раздел на «Настройках».
///
/// Правило экрана прежнее и здесь не смягчено: ни одного переключателя без
/// последствия и без подтверждения.
///   * готовые наборы гаснут, когда база GEOSITE в сборке мертва: без неё
///     правило `GEOSITE,telegram` не совпадает никогда;
///   * заглушки «правила по приложениям» здесь нет вовсе — тумблер, который
///     не менял ни одного байта на проводе, удалён вместе с «Улучшениями», а
///     не перевезён сюда;
///   * внизу стоит [AppliedRouteCard] — отчёт ЯДРА о поднятом туннеле. Он
///     отвечает ровно про пресет, split и рекламу, то есть про то, что
///     настраивается здесь и в разделе «Реклама и блокировки».
class SiteRulesScreen extends ConsumerStatefulWidget {
  const SiteRulesScreen({super.key});

  @override
  ConsumerState<SiteRulesScreen> createState() => _SiteRulesScreenState();
}

class _SiteRulesScreenState extends ConsumerState<SiteRulesScreen> {
  final _bypassCtrl = TextEditingController();
  final _allowCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    final cfg = ref.read(coreConfigProvider);
    _bypassCtrl.text = cfg.bypassDomains;
    _allowCtrl.text = cfg.allowDomains;
  }

  @override
  void dispose() {
    _bypassCtrl.dispose();
    _allowCtrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final cfg = ref.watch(coreConfigProvider);
    final cfgN = ref.read(coreConfigProvider.notifier);
    final applied = ref.watch(appliedRouteProvider).valueOrNull;

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                AppSpace.s5,
                AppSpace.s5,
                AppSpace.s5,
                0,
              ),
              child: ScreenHead(
                'Правила по сайтам',
                trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
              ),
            ),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.fromLTRB(
                  AppSpace.s5,
                  0,
                  AppSpace.s5,
                  AppSpace.s12,
                ),
                children: [
                  Text(
                    'Списки доменов поверх общего режима: вести через VPN '
                    'только перечисленное — или, наоборот, пускать '
                    'перечисленное мимо туннеля.',
                    style: AppType.bodyMd.copyWith(color: c.textMed),
                  ),

                  if (ref.watch(reconnectRequiredProvider)) ...[
                    const SizedBox(height: AppSpace.s4),
                    const ReconnectBanner(),
                  ],

                  // ── Как применять списки ──────────────────────────────────
                  const SectionTitle('Как применять списки'),
                  for (final m in SplitMode.values)
                    ListItemCard(
                      leading: IBox(_modeGlyph(m)),
                      title: m.title,
                      subtitle: m.desc,
                      selected: cfg.splitMode == m,
                      onTap: () => CsmSettingsBridge.setSplitMode(ref, m),
                    ),

                  if (cfg.splitMode == SplitMode.onlySelected)
                    ..._allowSection(cfg, cfgN, applied),
                  if (cfg.splitMode == SplitMode.bypassSelected)
                    ..._bypassSection(cfgN),

                  // Список сайтов СИЛЬНЕЕ режима, и молчать об этом нельзя:
                  // человек выбрал режим на Главной, а трафик идёт по списку
                  // отсюда. Причина стоит там же, где включается список, а не
                  // там, где выбирается режим, — иначе её увидит не тот, кто
                  // её создал.
                  if (cfg.allowSitesActive) ...[
                    const SizedBox(height: AppSpace.s3),
                    const InlineBanner(
                      tone: BannerTone.warning,
                      glyph: Lucide.alert,
                      text:
                          'Режим, выбранный на Главной, сейчас НЕ '
                          'применяется: включён список «только выбранные '
                          'сайты», и через VPN идёт ровно он. Очистите '
                          'список, чтобы вернуть режим.',
                    ),
                  ],

                  const AppliedRouteCard(),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// Режим «через VPN только эти сайты»: свои домены + готовые наборы.
  List<Widget> _allowSection(
    CoreConfig cfg,
    CoreConfigNotifier cfgN,
    AppliedRoute? applied,
  ) {
    final c = context.c;
    // База GEOSITE мертва — значит правило `GEOSITE,telegram` не совпадёт
    // никогда. Такой набор показывается выключенным с причиной, а не
    // включаемым тумблером, за которым ничего нет.
    final geositeDead = applied?.geositeDead ?? false;

    return <Widget>[
      const SizedBox(height: AppSpace.s3),
      if (!cfg.allowSitesActive)
        const InlineBanner(
          tone: BannerTone.warning,
          glyph: Lucide.alert,
          text:
              'Список пуст, поэтому режим не применяется: пустой список «только '
              'эти сайты» увёл бы мимо туннеля вообще весь трафик. Добавьте '
              'домен или набор — тогда правило уйдёт ядру.',
        ),
      const SectionTitle('Свои сайты'),
      TextField(
        // Ключ обязателен: баннер «Список пуст» выше по дереву гаснет уже
        // после первого введённого символа (allowSitesActive взводится, как
        // только строка перестаёт быть пустой), и это меняет число виджетов
        // ПЕРЕД полем. Без ключа Flutter не находит старый элемент поля на
        // сдвинутой позиции и пересобирает его заново — вместе с ним гибнет
        // и внутренний FocusNode, так что клавиатура закрывается на первом же
        // символе. Ключ делает элемент узнаваемым независимо от позиции в
        // списке, и он переживает эту перестройку.
        key: const ValueKey('site-rules-allow-domains-field'),
        controller: _allowCtrl,
        minLines: 2,
        maxLines: 5,
        style: AppType.monoMd.copyWith(color: c.textHi),
        onChanged: cfgN.setAllowDomains,
        decoration: const InputDecoration(
          hintText: 'youtube.com, chatgpt.com\nexample.org',
        ),
      ),
      const SizedBox(height: AppSpace.s2),
      Text(
        'Через запятую или с новой строки. Правило доменное: «youtube.com» '
        'покрывает и поддомены. Соединение, открытое по голому IP-адресу, под '
        'него не попадает — приложения со своим DNS или с зашитыми адресами '
        'пойдут напрямую.',
        style: AppType.bodySm.copyWith(color: c.textLow),
      ),
      const SectionTitle('Готовые наборы'),
      RowsGroup(
        children: [
          for (final t in kAllowSiteTags)
            CRow(
              label: t.name,
              value: geositeDead ? 'база GEOSITE мертва' : null,
              trailing: Switch(
                value: cfg.allowSites.contains(t.tag),
                onChanged: geositeDead
                    ? null
                    : (_) => cfgN.toggleAllowSite(t.tag),
              ),
            ),
        ],
      ),
      if (geositeDead) ...[
        const SizedBox(height: AppSpace.s3),
        const InlineBanner(
          tone: BannerTone.danger,
          glyph: Lucide.alert,
          text:
              'Готовые наборы держатся на базе GEOSITE, а её в этой сборке '
              'нет — каждое такое правило заведомо мертво. Пока это так, '
              'перечисляйте сайты доменами выше.',
        ),
      ],
    ];
  }

  /// Режим «кроме выбранных сайтов»: домены всегда напрямую.
  List<Widget> _bypassSection(CoreConfigNotifier cfgN) {
    final c = context.c;
    return <Widget>[
      const SectionTitle('Сайты мимо туннеля'),
      TextField(
        // Тот же приём, что и у поля «Свои сайты»: ключ переживает сдвиг
        // позиции в списке, если раздел над этим полем когда-нибудь тоже
        // обзаведётся условным баннером.
        key: const ValueKey('site-rules-bypass-domains-field'),
        controller: _bypassCtrl,
        minLines: 2,
        maxLines: 5,
        style: AppType.monoMd.copyWith(color: c.textHi),
        onChanged: cfgN.setBypassDomains,
        decoration: const InputDecoration(
          hintText: 'bank.ru, gosuslugi.ru\nmail.local',
        ),
      ),
      const SizedBox(height: AppSpace.s2),
      Text(
        'Через запятую или с новой строки. Эти домены всегда идут напрямую, '
        'мимо туннеля — вместе с поддоменами. Всё остальное продолжает идти по '
        'режиму, выбранному на Главной.',
        style: AppType.bodySm.copyWith(color: c.textLow),
      ),
    ];
  }

  /// Глиф режима: вынесен из билда, чтобы switch не жил внутри аргумента.
  static String _modeGlyph(SplitMode m) => switch (m) {
    SplitMode.off => Lucide.shield,
    SplitMode.onlySelected => Lucide.globe,
    SplitMode.bypassSelected => Lucide.route,
  };

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.settings);
    }
  }
}

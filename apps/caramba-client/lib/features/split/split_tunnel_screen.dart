import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/features/settings/applied_route_card.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/features/settings/enhancements_summary.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/features/settings/route_picker.dart';
import 'package:caramba_client/features/settings/route_report.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// «Улучшения» — то, что добавляется к соединению поверх выбранного сервера.
///
/// Раньше вкладка называлась «Маршрут», и это имя не отвечало ни на один
/// вопрос, с которым сюда приходят: рядом уже есть «Сервер» (где выход) и
/// «Relay (вход)» (через что вход), и третье слово про то же самое читалось
/// как четвёртый вид выбора страны. На деле здесь лежат три независимые вещи,
/// каждая из которых МЕНЯЕТ трафик: что вырезать (реклама), что вести через
/// туннель (список сайтов) и по каким правилам вести остальное (режим страны).
/// Общее у них — «улучшения», а не «маршрут».
///
/// Путь `/split-tunnel` и имя класса сохранены намеренно: маршрутизатор общий
/// с другими экранами, и переименование файла стоило бы правки в чужом файле
/// ради подписи.
///
/// Правило экрана — ни одного переключателя без последствия и без
/// подтверждения:
///   * блок рекламы подписан состоянием списка правил из отчёта ядра, а не
///     словом «включено» (см. [adBlockStatus]);
///   * готовые наборы сайтов гаснут, когда база GEOSITE в сборке мертва: без
///     неё правило `GEOSITE,telegram` не совпадает никогда;
///   * правила по приложениям показаны выключенными с причиной, а не списком
///     из десяти выдуманных программ с рабочими на вид тумблерами;
///   * включённый блок рекламы всегда несёт рядом границу метода (домен из
///     первого пакета мимо IP-соединений и ECH), а не только статус списка —
///     список может доехать целиком и метод всё равно кое-что пропустит.
class SplitTunnelScreen extends ConsumerStatefulWidget {
  const SplitTunnelScreen({super.key});

  @override
  ConsumerState<SplitTunnelScreen> createState() => _SplitTunnelScreenState();
}

class _SplitTunnelScreenState extends ConsumerState<SplitTunnelScreen> {
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
    final ads = adBlockStatus(cfg, applied);

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
                'Улучшения',
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
                    'Что добавить к соединению поверх выбранного сервера: '
                    'вырезать рекламу, вести через VPN только нужные сайты, '
                    'выбрать режим для страны.',
                    style: AppType.bodyMd.copyWith(color: c.textMed),
                  ),

                  if (ref.watch(reconnectRequiredProvider)) ...[
                    const SizedBox(height: AppSpace.s4),
                    const ReconnectBanner(),
                  ],

                  // ── Блок рекламы ──────────────────────────────────────────
                  const SectionTitle('Реклама и трекеры'),
                  RowsGroup(
                    children: [
                      CRow(
                        icon: Lucide.shield,
                        label: 'Блокировать рекламу и трекеры',
                        trailing: Switch(
                          value: cfg.blockAds,
                          onChanged: cfgN.setBlockAds,
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: AppSpace.s3),
                  InlineBanner(
                    tone: switch (ads.state) {
                      AdBlockState.working => BannerTone.info,
                      AdBlockState.broken => BannerTone.danger,
                      AdBlockState.off => BannerTone.info,
                      _ => BannerTone.warning,
                    },
                    glyph: ads.isWorking ? Lucide.fileCheck : Lucide.alert,
                    text: ads.message,
                  ),
                  // Граница метода, а не состояния конкретной сборки — стоит
                  // рядом с переключателем всегда, пока он включён, а не
                  // только когда список правил разрешился.
                  //
                  // Текст здесь дублирует `AdBlockLimitNote` из
                  // libs/caramba-core/profile/profile.go: канонический смысл
                  // определён там, но константа заведена в Go и в Dart не
                  // протянута — ни FFI, ни отчёт ядра её не несут (grep по
                  // lib/ на `AdBlockLimitNote` пуст). Провести канал ради
                  // одной статичной фразы означало бы менять контракт ядра
                  // ради строки, которая не зависит от состояния подключения
                  // — поэтому текст здесь только СИНХРОНИЗИРОВАН с ним
                  // вручную, а не подтянут программно; при правке одного
                  // источника проверяйте оба (см. profile/sniffer_test.go).
                  if (cfg.blockAds) ...[
                    const SizedBox(height: AppSpace.s3),
                    const InlineBanner(
                      tone: BannerTone.info,
                      glyph: Lucide.alert,
                      text: 'Блок режет по имени домена из первого пакета '
                          'соединения. Реклама, загруженная по голому IP или '
                          'спрятанная шифрованием имени (ECH), проходит мимо '
                          '— это граница метода, а не сбой конкретного '
                          'списка.',
                    ),
                  ],

                  // ── Сайты ─────────────────────────────────────────────────
                  const SectionTitle('Правила по сайтам'),
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

                  // ── Режим страны ──────────────────────────────────────────
                  const SectionTitle('Режим для страны'),
                  RowsGroup(
                    children: [
                      CRow(
                        icon: Lucide.route,
                        label: 'Набор правил',
                        value: _routeName(cfg.route),
                        chevron: true,
                        onTap: () => showRoutePicker(context, ref),
                      ),
                    ],
                  ),
                  if (cfg.allowSitesActive) ...[
                    const SizedBox(height: AppSpace.s3),
                    const InlineBanner(
                      tone: BannerTone.warning,
                      glyph: Lucide.alert,
                      text: 'Режим для страны сейчас НЕ применяется: включён '
                          'список «только выбранные сайты», и через VPN идёт '
                          'ровно он. Очистите список, чтобы вернуть режим.',
                    ),
                  ],
                  const AppliedRouteCard(),

                  // ── Приложения ────────────────────────────────────────────
                  const SectionTitle('Правила по приложениям'),
                  const RowsGroup(
                    children: [
                      CRow(
                        icon: Lucide.appWindow,
                        label: 'Выбирать приложения',
                        value: 'недоступно',
                        trailing: Switch(value: false, onChanged: null),
                      ),
                    ],
                  ),
                  const SizedBox(height: AppSpace.s3),
                  const InlineBanner(
                    tone: BannerTone.info,
                    glyph: Lucide.alert,
                    text: 'Приложение не умеет перечислять установленные '
                        'программы: платформенного канала для этого нет, а '
                        'список, который здесь стоял раньше, был '
                        'демонстрационным — те тумблеры не меняли ни одного '
                        'байта на проводе. Правила по сайтам работают везде и '
                        'заменяют это здесь и сейчас.',
                  ),
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
        key: const ValueKey('split-tunnel-allow-domains-field'),
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
                onChanged:
                    geositeDead ? null : (_) => cfgN.toggleAllowSite(t.tag),
              ),
            ),
        ],
      ),
      if (geositeDead) ...[
        const SizedBox(height: AppSpace.s3),
        const InlineBanner(
          tone: BannerTone.danger,
          glyph: Lucide.alert,
          text: 'Готовые наборы держатся на базе GEOSITE, а её в этой сборке '
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
        key: const ValueKey('split-tunnel-bypass-domains-field'),
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
        'режиму для страны.',
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

  static String _routeName(int index) {
    const modes = RoutingMode.defaults;
    if (index < 0 || index >= modes.length) return 'не выбран';
    return modes[index].name;
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go('/settings');
    }
  }
}

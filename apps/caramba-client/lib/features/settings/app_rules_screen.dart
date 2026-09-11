/// «Правила по приложениям» — какие программы идут через туннель, а какие мимо.
///
/// Это ВТОРОЙ список того же самого режима, что и «Правила по сайтам»: в ядре
/// `Policy.Split` один, и его `mode` относится сразу к доменам и к процессам.
/// Поэтому переключатель режима здесь тот же самый, и экран об этом говорит
/// прямо — молчаливый второй тумблер поверх одного поля переставлял бы первый.
///
/// Откуда берутся сами приложения, решает платформа, и решение это не про вкус:
///   * Android перечисляет установленное сам, и выбор применяет
///     `VpnService.Builder` (правила `PROCESS-NAME` ядра там мертвы: поиск
///     процесса требует /data/system/packages.xml, куда приложению хода нет);
///   * на десктопе ядро само владеет соединениями и совпадает по ИМЕНИ
///     ПРОЦЕССА, а реестра установленных программ в системе нет — имя берут из
///     файлового диалога или вводят руками;
///   * на iOS выбирать нечего: per-app VPN у Apple существует только в
///     MDM-профиле, и экран показан ЗАКРЫТЫМ с причиной, а не спрятан. Пункт,
///     который исчезает на одной платформе, читается как «у меня сломалось», а
///     честная причина — как граница системы.
///
/// Правило экрана то же, что у списков сайтов: ни одного переключателя без
/// последствия. Поэтому здесь два предупреждения, и оба про реальные «ничего не
/// произойдёт»: пустой список в непустом режиме и режим «только список» без
/// единой цели в правилах по сайтам (см. [_allowNeedsSites]).
library;

import 'dart:typed_data' show Uint8List;

import 'package:caramba_vpn/caramba_vpn.dart' show InstalledApp;
import 'package:flutter/foundation.dart'
    show TargetPlatform, defaultTargetPlatform, kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/installed_apps_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Имя экрана. Строка настроек, которая его открывает, называется так же:
/// «одно имя — два места» здесь уже ловилось тестом на соседнем экране.
const String kAppRulesTitle = 'Правила по приложениям';

/// Сводка строки «Правила по приложениям» в Настройках.
///
/// Считается то, что РЕАЛЬНО уйдёт ядру ([CoreConfig.splitCount]): выбранные
/// при выключенном режиме приложения не считаются, потому что ядру они не
/// уходят. Имя режима в сводке не называется — у режима своя строка выше.
String appRulesSummary(CoreConfig cfg) => switch (cfg.splitMode) {
  SplitMode.off => 'Выключено: списков нет',
  SplitMode.bypassSelected =>
    'Кроме списка · ${_countApps(cfg.splitCount)} мимо VPN',
  SplitMode.onlySelected =>
    'Только список · ${_countApps(cfg.splitCount)} через VPN',
};

String _countApps(int n) {
  final mod100 = n % 100;
  final mod10 = n % 10;
  final word = (mod100 >= 11 && mod100 <= 14)
      ? 'приложений'
      : switch (mod10) {
          1 => 'приложение',
          2 || 3 || 4 => 'приложения',
          _ => 'приложений',
        };
  return '$n $word';
}

/// Режим «только список» выбран, а уехать ядру он не может.
///
/// `_split` в core_policy_mapping понижает пустой allow до `off`: список «через
/// VPN только перечисленное» без единой цели увёл бы мимо туннеля ВЕСЬ трафик.
/// Целями там считаются домены и наборы сайтов, поэтому выбор одних только
/// приложений сейчас до ядра не доходит — и человеку об этом говорят здесь, а
/// не оставляют гадать, почему список не работает.
bool _allowNeedsSites(CoreConfig cfg) =>
    cfg.splitMode == SplitMode.onlySelected && !cfg.allowSitesActive;

class AppRulesScreen extends ConsumerStatefulWidget {
  const AppRulesScreen({super.key});

  @override
  ConsumerState<AppRulesScreen> createState() => _AppRulesScreenState();
}

class _AppRulesScreenState extends ConsumerState<AppRulesScreen> {
  /// Ручной ввод имени процесса (десктоп). Живёт в состоянии экрана, а не в
  /// конфигурации: пока строка не добавлена кнопкой, это черновик, а не выбор.
  final _processCtrl = TextEditingController();

  @override
  void dispose() {
    _processCtrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final cfg = ref.watch(coreConfigProvider);

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
                kAppRulesTitle,
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
                children: _isIos ? _lockedOnIos() : _body(cfg),
              ),
            ),
          ],
        ),
      ),
    );
  }

  // ----------------------------------------------------------------- iOS

  bool get _isIos => !kIsWeb && defaultTargetPlatform == TargetPlatform.iOS;

  List<Widget> _lockedOnIos() => <Widget>[
    const InlineBanner(
      key: ValueKey('app-rules-ios-locked'),
      tone: BannerTone.warning,
      glyph: Lucide.lock,
      text:
          'На iPhone и iPad выбрать приложения нельзя. Per-app VPN у Apple '
          'существует только в управляемом (MDM) профиле, который ставит '
          'организация. Обычное приложение не может ни перечислить '
          'установленные программы, ни направить трафик одной из них мимо '
          'туннеля.',
    ),
    const SizedBox(height: AppSpace.s4),
    Text(
      'Правила по сайтам на iOS работают полностью: списки доменов уходят '
      'ядру и применяются к трафику любого приложения.',
      style: AppType.bodyMd.copyWith(color: context.c.textMed),
    ),
    const SizedBox(height: AppSpace.s4),
    GhostButton(
      label: 'Открыть правила по сайтам',
      icon: Lucide.globe,
      onPressed: () => context.go(AppRoute.siteRules),
    ),
  ];

  // ------------------------------------------------------------ основной

  List<Widget> _body(CoreConfig cfg) {
    final c = context.c;
    return <Widget>[
      Text(
        _isDesktop
            ? 'Список программ поверх общего режима: вести через туннель '
                  'только их или, наоборот, пускать их мимо. Ядро узнаёт '
                  'программу по имени процесса.'
            : 'Список приложений поверх общего режима: вести через туннель '
                  'только их или, наоборот, пускать их мимо.',
        style: AppType.bodyMd.copyWith(color: c.textMed),
      ),

      if (ref.watch(reconnectRequiredProvider)) ...[
        const SizedBox(height: AppSpace.s4),
        const ReconnectBanner(),
      ],

      const SectionTitle('Как применять список'),
      for (final m in SplitMode.values)
        ListItemCard(
          key: ValueKey('app-rules-mode-${m.name}'),
          leading: IBox(_modeGlyph(m)),
          title: m.appsTitle,
          subtitle: m.appsDesc,
          selected: cfg.splitMode == m,
          onTap: () => CsmSettingsBridge.setSplitMode(ref, m),
        ),

      // Режим один на два списка, и это видно на обоих экранах. Умолчать
      // значило бы дать человеку выбрать режим здесь и обнаружить его
      // переставленным на «Правилах по сайтам».
      const SizedBox(height: AppSpace.s2),
      const InlineBanner(
        tone: BannerTone.info,
        glyph: Lucide.alert,
        text:
            'Режим общий с «Правилами по сайтам»: в ядре он один и относится '
            'сразу к сайтам и к приложениям.',
      ),

      if (cfg.splitMode != SplitMode.off) ...[
        if (_allowNeedsSites(cfg)) ...[
          const SizedBox(height: AppSpace.s3),
          const InlineBanner(
            key: ValueKey('app-rules-allow-needs-sites'),
            tone: BannerTone.warning,
            glyph: Lucide.alert,
            text:
                'Пока в «Правилах по сайтам» не выбрано ни одного сайта или '
                'набора, этот режим ядру не уходит вовсе: список «только '
                'выбранные» без единой цели увёл бы мимо туннеля весь трафик. '
                'Добавьте сайт там или выберите «Все через VPN, кроме списка».',
          ),
        ],
        ..._selectedSection(cfg),
        ..._addSection(cfg),
      ],
    ];
  }

  /// Выбранные приложения: что уже в списке и как это убрать.
  List<Widget> _selectedSection(CoreConfig cfg) {
    final c = context.c;
    final selected = cfg.splitApps.toList()..sort();
    // Ярлыки приезжают от системы и только на Android; на прочих платформах
    // карта пуста, и строка честно показывает имя процесса — ровно то, что
    // человек и выбрал.
    final installed =
        ref.watch(installedAppsProvider).valueOrNull ?? const <InstalledApp>[];
    final known = <String, InstalledApp>{
      for (final a in installed) a.packageName: a,
    };

    return <Widget>[
      const SectionTitle('В списке'),
      if (selected.isEmpty)
        InlineBanner(
          key: const ValueKey('app-rules-empty'),
          tone: BannerTone.warning,
          glyph: Lucide.alert,
          text: cfg.splitMode == SplitMode.bypassSelected
              ? 'Список пуст, поэтому правило ничего не меняет: мимо туннеля '
                    'пока не идёт ни одно приложение.'
              : 'Список пуст, поэтому правило ничего не меняет: через туннель '
                    'пока не идёт ни одно приложение.',
        )
      else
        // Карточка, а не строка списка: у приложения есть иконка системы, и
        // строке [CRow] её некуда положить — там слева живёт глиф, а не
        // картинка чужого пакета.
        for (final id in selected)
          ListItemCard(
            key: ValueKey('app-rules-row-$id'),
            leading: appIconOrGlyph(known[id]?.iconPng),
            title: known[id]?.label ?? id,
            subtitle: known.containsKey(id) ? id : null,
            trailing: IconBtn(
              Lucide.trash,
              key: ValueKey('app-rules-remove-$id'),
              size: 36,
              color: c.danger,
              onTap: () =>
                  ref.read(coreConfigProvider.notifier).removeSplitApp(id),
            ),
          ),
    ];
  }

  /// Как добавить — здесь и расходятся платформы.
  List<Widget> _addSection(CoreConfig cfg) {
    final c = context.c;
    if (!_isDesktop) {
      return <Widget>[
        const SizedBox(height: AppSpace.s3),
        GhostButton(
          key: const ValueKey('app-rules-add'),
          label: 'Добавить приложение',
          icon: Lucide.plus,
          onPressed: _openInstalledPicker,
        ),
      ];
    }

    return <Widget>[
      const SizedBox(height: AppSpace.s3),
      GhostButton(
        key: const ValueKey('app-rules-add'),
        label: 'Выбрать программу',
        icon: Lucide.plus,
        onPressed: _pickProcessFile,
      ),
      const SizedBox(height: AppSpace.s2),
      Text(
        'Выберите исполняемый файл программы: на macOS это её значок в '
        '«Программах», на Windows это .exe. Приложение возьмёт оттуда имя '
        'процесса, ровно то, по чему ядро и различает программы.',
        style: AppType.bodySm.copyWith(color: c.textLow),
      ),
      const SectionTitle('Или имя процесса вручную'),
      TextField(
        key: const ValueKey('app-rules-process-field'),
        controller: _processCtrl,
        style: AppType.monoMd.copyWith(color: c.textHi),
        onSubmitted: (_) => _addTypedProcess(),
        decoration: const InputDecoration(hintText: 'chrome.exe'),
      ),
      const SizedBox(height: AppSpace.s2),
      GhostButton(
        key: const ValueKey('app-rules-add-typed'),
        label: 'Добавить имя процесса',
        onPressed: _addTypedProcess,
      ),
    ];
  }

  bool get _isDesktop => isDesktopPlatform;

  void _addTypedProcess() {
    final name = _processCtrl.text.trim();
    if (name.isEmpty) return;
    ref.read(coreConfigProvider.notifier).addSplitApp(name);
    _processCtrl.clear();
  }

  Future<void> _pickProcessFile() async {
    final name = await ref.read(processPickerProvider)();
    if (!mounted || name == null || name.isEmpty) return;
    ref.read(coreConfigProvider.notifier).addSplitApp(name);
  }

  Future<void> _openInstalledPicker() {
    return showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      backgroundColor: Colors.transparent,
      builder: (_) => const _InstalledAppsSheet(),
    );
  }

  /// Глиф режима: тот же набор, что на «Правилах по сайтам», — величина одна.
  static String _modeGlyph(SplitMode m) => switch (m) {
    SplitMode.off => Lucide.shield,
    SplitMode.onlySelected => Lucide.appWindow,
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

/// Пикер установленных приложений (только Android).
///
/// Выбор применяется СРАЗУ по касанию, а лист остаётся открытым: приложений
/// выбирают обычно несколько подряд, и закрывать лист после каждого значило бы
/// заставлять человека открывать его заново.
class _InstalledAppsSheet extends ConsumerStatefulWidget {
  const _InstalledAppsSheet();

  @override
  ConsumerState<_InstalledAppsSheet> createState() =>
      _InstalledAppsSheetState();
}

class _InstalledAppsSheetState extends ConsumerState<_InstalledAppsSheet> {
  final _searchCtrl = TextEditingController();
  String _query = '';

  @override
  void dispose() {
    _searchCtrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final apps = ref.watch(installedAppsProvider);
    final chosen = ref.watch(coreConfigProvider).splitApps;

    return FractionallySizedBox(
      heightFactor: 0.9,
      child: Container(
        decoration: BoxDecoration(
          color: c.bgCanvas,
          borderRadius: const BorderRadius.vertical(top: Radius.circular(20)),
          border: Border.all(color: c.borderSubtle),
        ),
        clipBehavior: Clip.antiAlias,
        child: SafeArea(
          top: false,
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
                  'Установленные приложения',
                  trailing: IconBtn(
                    Lucide.x,
                    onTap: () => Navigator.of(context).pop(),
                  ),
                ),
              ),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: AppSpace.s5),
                child: TextField(
                  key: const ValueKey('app-rules-search'),
                  controller: _searchCtrl,
                  onChanged: (v) => setState(() => _query = v),
                  style: AppType.bodyMd.copyWith(color: c.textHi),
                  decoration: const InputDecoration(hintText: 'Поиск'),
                ),
              ),
              const SizedBox(height: AppSpace.s3),
              Expanded(
                child: apps.when(
                  loading: () => const Center(child: InlineLoading()),
                  error: (e, _) => const Padding(
                    padding: EdgeInsets.all(AppSpace.s5),
                    child: InlineBanner(
                      tone: BannerTone.danger,
                      glyph: Lucide.alert,
                      text:
                          'Система не отдала список установленных приложений. '
                          'Имя пакета можно будет выбрать снова после '
                          'перезапуска приложения.',
                    ),
                  ),
                  data: (list) {
                    final shown = filterInstalledApps(list, _query);
                    if (shown.isEmpty) {
                      return Padding(
                        padding: const EdgeInsets.all(AppSpace.s5),
                        child: InlineBanner(
                          key: const ValueKey('app-rules-picker-empty'),
                          tone: BannerTone.warning,
                          glyph: Lucide.alert,
                          text: list.isEmpty
                              ? 'Эта сборка не умеет перечислять установленные '
                                    'приложения: список отдаёт система, и на '
                                    'этой платформе его нет.'
                              : 'Ничего не найдено по запросу.',
                        ),
                      );
                    }
                    return ListView.builder(
                      padding: const EdgeInsets.fromLTRB(
                        AppSpace.s5,
                        0,
                        AppSpace.s5,
                        AppSpace.s6,
                      ),
                      itemCount: shown.length,
                      itemBuilder: (context, i) {
                        final app = shown[i];
                        return ListItemCard(
                          key: ValueKey('app-rules-pick-${app.packageName}'),
                          leading: appIconOrGlyph(app.iconPng),
                          title: app.label,
                          subtitle: app.packageName,
                          selected: chosen.contains(app.packageName),
                          onTap: () => ref
                              .read(coreConfigProvider.notifier)
                              .toggleSplitApp(app.packageName),
                        );
                      },
                    );
                  },
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Иконка приложения или запасной глиф.
///
/// `errorBuilder` обязателен: байты приходят от ЧУЖОГО пакета, и один
/// испорченный PNG иначе уронил бы весь список исключением декодера. Отсутствие
/// иконки тоже не повод прятать приложение — глиф на её месте честнее пустоты.
Widget appIconOrGlyph(Uint8List? png) {
  if (png == null) return const IBox(Lucide.appWindow);
  return ClipRRect(
    borderRadius: AppRadius.r12,
    child: Image.memory(
      png,
      width: 36,
      height: 36,
      fit: BoxFit.contain,
      gaplessPlayback: true,
      errorBuilder: (_, __, ___) => const IBox(Lucide.appWindow),
    ),
  );
}

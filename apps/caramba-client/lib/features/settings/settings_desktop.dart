/// Настройки на десктопе: индекс разделов слева, форма справа.
///
/// ЭТО ВТОРАЯ РАСКЛАДКА ОДНИХ И ТЕХ ЖЕ НАСТРОЕК, а не второй набор настроек.
/// Провайдеры, сеттеры и мост CSM здесь те же, что в [SettingsScreen], и
/// подписи скопированы оттуда дословно: разъехавшиеся формулировки на двух
/// платформах читаются как две разные настройки, и человек, поменявший
/// значение на маке, не находит его на телефоне. Отличается только форма:
///
///   * значение меняется НА МЕСТЕ ([DesktopPicker]), а не на подэкране и не
///     нижним листом: на телефоне лист занимает пол-экрана и это оправдано,
///     на десктопе он модальным слоем закрывает форму ради выбора между
///     «Авто» и «gVisor»;
///   * длинный экран получил индекс разделов слева: у мобильного списка есть
///     скролл и больше ничего, а здесь до «Приложения» шесть экранов прокрутки;
///   * заголовка экрана нет — его рисует тулбар десктопной оболочки
///     ([DesktopShell]), и второй заголовок под ним был бы повтором.
///
/// Раздел «Приложение» существует только здесь: закрытие окна, автозапуск при
/// входе и запуск без окна — это вопросы, которых у мобильной сборки нет.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/desktop/autostart_service.dart';
import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/desktop_prefs.dart';
import 'package:caramba_client/desktop/desktop_strings.dart';
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/desktop/widgets/desktop_picker.dart';
import 'package:caramba_client/desktop/widgets/form_row.dart';
import 'package:caramba_client/features/csm/config_age_card.dart';
import 'package:caramba_client/features/csm/keep_or_revert_card.dart';
import 'package:caramba_client/features/settings/app_rules_screen.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/features/settings/csm_write_status_note.dart';
import 'package:caramba_client/features/settings/enhancements_summary.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/features/settings/route_picker.dart';
import 'package:caramba_client/features/settings/route_report.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/vpn/core_policy.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Насколько близко к верху формы должен подойти заголовок раздела, чтобы
/// индекс подсветил его. Не ноль: между разделами есть воздух, и на границе
/// подсветка иначе моргала бы туда-сюда на пиксель прокрутки.
const double _kActiveEdge = 24;

/// Подписи кнопок колонки контролов. Каждая обещает РАЗНОЕ: «Открыть» ведёт на
/// экран, «Выбрать» — в список значений, «Изменить» — в лист режима.
const String _kOpen = 'Открыть';
const String _kPick = 'Выбрать';
const String _kChange = 'Изменить';

/// Захват трафика: те же два значения и те же описания, что в листе мобильных
/// настроек. Список закрытый и от состояния не зависит, поэтому он константа.
const List<({String name, String desc, String? icon})> _kTunnelOptions =
    <({String name, String desc, String? icon})>[
      (
        name: 'Системный TUN',
        desc:
            'Весь трафик устройства. Нужны права '
            'администратора или системное расширение.',
        icon: null,
      ),
      (
        name: 'Локальный прокси',
        desc:
            'SOCKS5 и HTTP на 127.0.0.1:7890. Без прав, '
            'трафик направляют приложения или система.',
        icon: null,
      ),
    ];

class SettingsDesktopScreen extends ConsumerStatefulWidget {
  const SettingsDesktopScreen({super.key});

  @override
  ConsumerState<SettingsDesktopScreen> createState() =>
      _SettingsDesktopScreenState();
}

class _SettingsDesktopScreenState extends ConsumerState<SettingsDesktopScreen> {
  final ScrollController _scroll = ScrollController();

  /// Система координат, в которой считаются позиции заголовков: сама форма.
  final GlobalKey _formKey = GlobalKey();

  /// Ключи разделов живут в состоянии, а не создаются в `build`: заново
  /// созданный `GlobalKey` каждый кадр отвязывал бы индекс от формы.
  final Map<String, GlobalKey> _sectionKeys = <String, GlobalKey>{};

  /// Порядок разделов текущего кадра. Он не постоянный: «Проверка и
  /// прозрачность» есть только у профиля с закреплённым ключом, а «Поддержка»
  /// подменяется «Аккаунтом панели» без сессии.
  List<String> _ids = const <String>[];

  int _active = 0;

  @override
  void initState() {
    super.initState();
    _scroll.addListener(_syncActive);
    WidgetsBinding.instance.addPostFrameCallback((_) => _syncActive());
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  GlobalKey _keyFor(String id) =>
      _sectionKeys.putIfAbsent(id, () => GlobalKey());

  /// Активен раздел, чей заголовок ПОСЛЕДНИМ прошёл верхнюю кромку формы.
  ///
  /// Считается по позициям заголовков, а не по накопленным высотам: высота
  /// раздела меняется от содержимого (причина отказа ядра занимает то одну
  /// строку, то три), и любая таблица высот разъехалась бы с формой молча.
  void _syncActive() {
    final form = _formKey.currentContext?.findRenderObject() as RenderBox?;
    if (form == null || !form.attached) return;

    // Short final sections cannot reach the upper edge. At the bottom,
    // select the last section instead of leaving the preceding one active.
    final atBottom =
        _scroll.hasClients &&
        _scroll.position.maxScrollExtent > 0 &&
        _scroll.position.extentAfter <= 1;
    var next = atBottom ? _ids.length - 1 : 0;
    for (var i = 0; !atBottom && i < _ids.length; i++) {
      final box =
          _sectionKeys[_ids[i]]?.currentContext?.findRenderObject()
              as RenderBox?;
      if (box == null || !box.attached) continue;
      if (box.localToGlobal(Offset.zero, ancestor: form).dy <= _kActiveEdge) {
        next = i;
      }
    }
    if (next != _active && mounted) setState(() => _active = next);
  }

  void _jumpTo(String id) {
    final ctx = _sectionKeys[id]?.currentContext;
    if (ctx == null) return;
    unawaited(
      Scrollable.ensureVisible(
        ctx,
        duration: DesktopTokens.panelSlide,
        curve: AppMotion.enter,
      ),
    );
  }

  /// Опции пикера из списка ядра. Форма записи та же, что у `showPickerSheet`:
  /// списки значений не переписываются под вторую платформу.
  List<({String name, String desc, String? icon})> _coreOptions(
    List<CoreOption> options,
  ) => <({String name, String desc, String? icon})>[
    for (final o in options) (name: o.name, desc: o.desc, icon: null),
  ];

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final cfg = ref.watch(coreConfigProvider);
    final cfgN = ref.read(coreConfigProvider.notifier);
    final settings = ref.watch(settingsProvider);
    final settingsN = ref.read(settingsProvider.notifier);

    final protocols = ref.watch(protocolsProvider);
    final modes = ref.watch(routingModesProvider);
    final routeMode = (cfg.route >= 0 && cfg.route < modes.length)
        ? modes[cfg.route]
        : modes.first;
    final isLight = settings.themeMode == ThemeMode.light;
    final tunnelMode = ref.watch(tunnelModeProvider);
    final authed = ref.watch(authProvider).stage == AuthStage.authenticated;
    final prefs = ref.watch(desktopPrefsProvider);
    final prefsN = ref.read(desktopPrefsProvider.notifier);
    final isMac = isMacOSPlatform;
    // Умеет ли ЭТА система автозапуск. Спрашиваем сервис, а не версию ОС из
    // головы: ответ он берёт у самой системы (macOS 12 отвечает отказом
    // канала), и только он знает его наверняка.
    final autostartSupported = ref.watch(autostartSupportedProvider);
    final approvalPending = ref.watch(autostartApprovalPendingProvider);

    final sections = <_Section>[
      _Section(
        id: 'connection',
        title: 'Подключение',
        rows: <Widget>[
          FormRow(
            label: 'Тип подключения',
            description: protocols[cfg.protocol].name,
            provenance: const CsmProvenanceTag(
              settingKey: CsmSettingKey.protocol,
            ),
            control: FormOpenButton(
              label: _kPick,
              onPressed: () => context.go(AppRoute.protocol),
            ),
          ),
          FormRow(
            label: 'Kill-switch',
            provenance: const CsmProvenanceTag(
              settingKey: CsmSettingKey.killSwitch,
            ),
            control: Switch(
              value: cfg.killSwitch,
              onChanged: (v) => CsmSettingsBridge.setKillSwitch(ref, v),
            ),
          ),
          FormRow(
            label: 'Автоподключение',
            control: Switch(
              value: cfg.autoConnect,
              onChanged: cfgN.setAutoConnect,
            ),
          ),
        ],
      ),

      // Раздел отвечает на один вопрос — «какой трафик куда идёт», — и держит
      // его целиком: режим, реклама, списки сайтов. Тот же состав, что на
      // телефоне; расщепить его на десктопе значило бы снова завести два
      // адреса для одного вопроса.
      _Section(
        id: 'traffic',
        title: 'Правила трафика',
        rows: <Widget>[
          FormRow(
            // Строка называется именем листа, который открывает: регрессия
            // «одно имя — два места» закреплена
            // enhancements_naming_regression_test.
            label: kRouteModeSheetTitle,
            description: routeModeLabel(routeMode),
            provenance: const CsmProvenanceTag(
              settingKey: CsmSettingKey.preset,
            ),
            control: FormOpenButton(
              label: _kChange,
              onPressed: () => unawaited(showRoutePicker(context, ref)),
            ),
          ),
          FormRow(
            label: 'Блокировать рекламу и трекеры',
            // Подпись — отчёт ядра, а не слово «включено».
            description: adBlockStatus(
              cfg,
              ref.watch(appliedRouteProvider).valueOrNull,
            ).message,
            control: Switch(value: cfg.blockAds, onChanged: cfgN.setBlockAds),
          ),
          FormRow(
            label: 'Правила по сайтам',
            description: siteRulesSummary(cfg),
            provenance: const CsmProvenanceTag(
              settingKey: CsmSettingKey.splitMode,
            ),
            control: FormOpenButton(
              label: _kOpen,
              onPressed: () => context.go(AppRoute.siteRules),
            ),
          ),
          // На десктопе список приложений это имена ПРОЦЕССОВ (их выбирают
          // файловым диалогом), но строка настроек та же самая и в том же
          // разделе: вопрос «какой трафик куда идёт» один на все платформы.
          FormRow(
            label: kAppRulesTitle,
            description: appRulesSummary(cfg),
            control: FormOpenButton(
              label: _kOpen,
              onPressed: () => context.go(AppRoute.appRules),
            ),
          ),
        ],
        extras: <Widget>[
          // Граница метода, а не состояние сборки: стоит рядом с включённым
          // переключателем всегда. Текст скопирован из мобильных настроек
          // дословно и синхронизирован с `AdBlockLimitNote` в
          // libs/caramba-core/profile/profile.go вручную.
          if (cfg.blockAds)
            const InlineBanner(
              tone: BannerTone.info,
              glyph: Lucide.alert,
              text:
                  'Блок режет по имени домена из первого пакета '
                  'соединения. Реклама, загруженная по голому IP или '
                  'спрятанная шифрованием имени (ECH), проходит мимо — это '
                  'граница метода, а не сбой конкретного списка.',
            ),
        ],
      ),

      _Section(
        id: 'core',
        title: 'Сеть и ядро',
        rows: <Widget>[
          FormRow(
            label: 'Сетевой стек (TUN)',
            description: 'Как приложение поднимает туннель в системе.',
            provenance: const CsmProvenanceTag(settingKey: CsmSettingKey.stack),
            control: DesktopPicker(
              options: _coreOptions(CoreOption.stacks),
              selected: cfg.stack,
              onSelected: (i) => CsmSettingsBridge.setStack(ref, i),
            ),
          ),
          FormRow(
            label: 'DNS-резолвер',
            description: 'Кто резолвит домены внутри туннеля.',
            provenance: const CsmProvenanceTag(
              settingKey: CsmSettingKey.dnsNameservers,
            ),
            control: DesktopPicker(
              options: _coreOptions(CoreOption.dns),
              selected: cfg.dns,
              onSelected: (i) => CsmSettingsBridge.setDns(ref, i),
            ),
          ),
          FormRow(
            label: 'MTU',
            description:
                'Размер пакета. Меньше значение стабильнее, больше быстрее.',
            provenance: const CsmProvenanceTag(settingKey: CsmSettingKey.mtu),
            control: DesktopPicker(
              options: _coreOptions(CoreOption.mtu),
              selected: cfg.mtu,
              onSelected: (i) => CsmSettingsBridge.setMtu(ref, i),
            ),
          ),
          FormRow(
            label: 'Fake-IP',
            provenance: const CsmProvenanceTag(
              settingKey: CsmSettingKey.fakeIp,
            ),
            control: Switch(
              value: cfg.fakeIp,
              onChanged: (v) => CsmSettingsBridge.setFakeIp(ref, v),
            ),
          ),
          FormRow(
            label: 'IPv6',
            provenance: const CsmProvenanceTag(settingKey: CsmSettingKey.ipv6),
            control: Switch(
              value: cfg.ipv6,
              onChanged: (v) => CsmSettingsBridge.setIpv6(ref, v),
            ),
          ),
          FormRow(
            label: 'Захват трафика',
            description:
                'TUN заворачивает весь трафик системы и требует прав. '
                'Прокси поднимает 127.0.0.1:$kMixedPort без прав.',
            control: DesktopPicker(
              options: _kTunnelOptions,
              selected: tunnelMode == TunnelMode.tun ? 0 : 1,
              onSelected: (i) => ref
                  .read(tunnelModeProvider.notifier)
                  .set(i == 0 ? TunnelMode.tun : TunnelMode.proxy),
            ),
          ),
        ],
        // Судьба записи настроек: без неё правка молча не уходила оператору,
        // а экран об этом не говорил ничего.
        extras: const <Widget>[CsmWriteStatusNote()],
      ),

      // Только профилю, который закрепил корневой ключ: четыре строки, каждая
      // из которых открывает пустое состояние, это не прозрачность.
      if (ref.watch(csmProfileStateProvider) != null)
        _Section(
          id: 'transparency',
          title: 'Проверка и прозрачность',
          rows: <Widget>[
            FormRow(
              label: 'Оператор',
              description: 'отпечаток и энроллмент',
              control: FormOpenButton(
                label: _kOpen,
                onPressed: () => context.go(AppRoute.csmOperator),
              ),
            ),
            FormRow(
              label: 'Документы',
              description: 'что проверено',
              control: FormOpenButton(
                label: _kOpen,
                onPressed: () => context.go(AppRoute.csmDocuments),
              ),
            ),
            FormRow(
              label: 'Транспорт',
              description: 'ступени и попытки',
              control: FormOpenButton(
                label: _kOpen,
                onPressed: () => context.go(AppRoute.csmTransport),
              ),
            ),
            FormRow(
              label: 'Что мы отправляем',
              control: FormOpenButton(
                label: _kOpen,
                onPressed: () => context.go(AppRoute.csmDisclosure),
              ),
            ),
          ],
        ),

      _Section(
        id: 'autotune',
        title: 'Автонастройка',
        rows: <Widget>[
          FormRow(
            label: 'Подобрать настройки заново',
            control: FormOpenButton(
              label: _kOpen,
              onPressed: () => context.go(AppRoute.settingsAutotune),
            ),
          ),
        ],
      ),

      // Поддержка живёт в панели: без аккаунта раздел пустой, поэтому в
      // generic-режиме показываем вход вместо тикетов.
      if (authed)
        _Section(
          id: 'support',
          title: 'Поддержка',
          rows: <Widget>[
            FormRow(
              label: 'Запросы в поддержку',
              control: FormOpenButton(
                label: _kOpen,
                onPressed: () => context.go(AppRoute.tickets),
              ),
            ),
          ],
        )
      else
        _Section(
          id: 'account',
          title: 'Аккаунт панели',
          rows: <Widget>[
            FormRow(
              label: 'Войти или подключить панель',
              control: FormOpenButton(
                label: _kOpen,
                onPressed: () => context.go(AppRoute.login),
              ),
            ),
          ],
        ),

      _Section(
        id: 'view',
        title: 'Вид',
        rows: <Widget>[
          FormRow(
            label: 'Светлая тема',
            control: Switch(
              value: isLight,
              onChanged: (v) =>
                  settingsN.setThemeMode(v ? ThemeMode.light : ThemeMode.dark),
            ),
          ),
        ],
      ),

      // Вопросы, которых у мобильной сборки нет вовсе: окно, значок в строке
      // меню, вход в систему.
      _Section(
        id: 'app',
        title: DesktopStrings.settingsAppSection,
        rows: <Widget>[
          FormRow(
            label: DesktopStrings.launchAtLoginTitle,
            // Описание появляется ТОЛЬКО когда система отказала. Подпись «нужна
            // macOS 13 или новее», висящая на любой macOS, врёт каждому, у кого
            // версия новее, — а таких большинство.
            description: autostartSupported
                ? (approvalPending
                      ? DesktopStrings.launchAtLoginNeedsApproval
                      : null)
                : ref.watch(autostartUnavailableMessageProvider),
            control: Switch(
              value: prefs.launchAtLogin,
              // Тумблер, который система не примет, не должен нажиматься:
              // включённый и ничего не делающий он хуже выключенного.
              onChanged: autostartSupported ? prefsN.setLaunchAtLogin : null,
            ),
          ),
          FormRow(
            label: DesktopStrings.onWindowCloseTitle,
            control: DesktopPicker(
              options: <({String name, String desc, String? icon})>[
                (
                  name: DesktopStrings.onWindowCloseHide(isMac: isMac),
                  desc: 'Туннель продолжает работать, окно возвращает значок.',
                  icon: null,
                ),
                (
                  name: DesktopStrings.onWindowCloseQuit,
                  desc: 'Закрытие окна опускает туннель и завершает работу.',
                  icon: null,
                ),
              ],
              selected: prefs.closeToTray ? 0 : 1,
              onSelected: (i) => prefsN.setCloseToTray(i == 0),
            ),
          ),
          FormRow(
            label: DesktopStrings.onLaunchTitle,
            control: DesktopPicker(
              options: <({String name, String desc, String? icon})>[
                (
                  name: DesktopStrings.onLaunchShowWindow,
                  desc: 'Обычный запуск.',
                  icon: null,
                ),
                (
                  name: DesktopStrings.onLaunchTrayOnly(isMac: isMac),
                  desc: 'Запуск без окна, для автозапуска при входе.',
                  icon: null,
                ),
              ],
              selected: prefs.startInTray ? 1 : 0,
              onSelected: (i) => prefsN.setStartInTray(i == 1),
            ),
          ),
        ],
      ),
    ];

    _ids = <String>[for (final s in sections) s.id];
    final active = sections.isEmpty ? 0 : _active.clamp(0, sections.length - 1);

    return Scaffold(
      backgroundColor: c.bgBase,
      body: Padding(
        padding: const EdgeInsets.all(DesktopTokens.contentPad),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            SizedBox(
              width: DesktopTokens.settingsIndexWidth,
              child: SingleChildScrollView(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: <Widget>[
                    for (var i = 0; i < sections.length; i++)
                      _IndexRow(
                        title: sections[i].title,
                        active: i == active,
                        onTap: () => _jumpTo(sections[i].id),
                      ),
                  ],
                ),
              ),
            ),
            const SizedBox(width: DesktopTokens.columnGap),
            Expanded(
              child: Align(
                alignment: Alignment.topLeft,
                child: ConstrainedBox(
                  constraints: const BoxConstraints(
                    maxWidth: DesktopTokens.settingsFormMaxWidth,
                  ),
                  // Форма — не `ListView`: ленивый список не строит разделы,
                  // которых сейчас не видно, а индексу слева нужен их
                  // `BuildContext` — и чтобы подсветить активный, и чтобы
                  // `Scrollable.ensureVisible` вообще нашёл цель. Разделов
                  // восемь, все они дёшевы.
                  child: SingleChildScrollView(
                    key: _formKey,
                    controller: _scroll,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: <Widget>[
                        // Политика ядра применяется при следующем поднятии
                        // туннеля, а не на лету.
                        if (ref.watch(reconnectRequiredProvider)) ...<Widget>[
                          const ReconnectBanner(),
                          const SizedBox(height: AppSpace.s4),
                        ],
                        const CsmConfigAgeCard(),
                        const CsmPendingChangesSection(),
                        for (final s in sections) ...<Widget>[
                          SectionTitle(s.title, key: _keyFor(s.id)),
                          RowsGroup(children: s.rows),
                          for (final extra in s.extras) ...<Widget>[
                            const SizedBox(height: AppSpace.s3),
                            extra,
                          ],
                        ],
                        const SizedBox(height: AppSpace.s6),
                        if (authed)
                          Align(
                            alignment: Alignment.centerLeft,
                            // Кнопка по содержимому, а не во всю форму: 720 px
                            // красного текста читаются как предупреждение, а не
                            // как последняя строка настроек.
                            child: SizedBox(
                              width: 240,
                              child: QuietButton(
                                label: DesktopStrings.signOut,
                                onPressed: () async {
                                  if (ref.read(vpnProvider).isConnected) {
                                    await ref
                                        .read(vpnProvider.notifier)
                                        .disconnect();
                                  }
                                  ref.read(firstRunProvider.notifier).reset();
                                  await ref
                                      .read(authProvider.notifier)
                                      .logout();
                                },
                              ),
                            ),
                          ),
                        const SizedBox(height: AppSpace.s12),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Раздел формы: заголовок, строки в одной группе и то, что стоит под группой.
class _Section {
  /// Устойчивый идентификатор — ключ раздела в [_SettingsDesktopScreenState].
  /// Не заголовок: имя раздела правят, а привязка индекса к форме от этого
  /// ломаться не должна.
  final String id;
  final String title;
  final List<Widget> rows;

  /// Баннеры и заметки под группой строк.
  final List<Widget> extras;

  const _Section({
    required this.id,
    required this.title,
    required this.rows,
    this.extras = const <Widget>[],
  });
}

/// Строка индекса разделов. Активный помечен полосой слева и цветом текста —
/// теми же средствами, что активный пункт сайдбара.
class _IndexRow extends StatelessWidget {
  final String title;
  final bool active;
  final VoidCallback onTap;

  const _IndexRow({
    required this.title,
    required this.active,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Semantics(
      selected: active,
      child: InkWell(
        onTap: onTap,
        borderRadius: AppRadius.r8,
        child: SizedBox(
          height: DesktopTokens.navRowHeight,
          child: Row(
            children: <Widget>[
              Container(
                width: 3,
                height: 18,
                decoration: BoxDecoration(
                  color: active ? c.textHi : const Color(0x00000000),
                  borderRadius: BorderRadius.circular(AppRadius.pill),
                ),
              ),
              const SizedBox(width: AppSpace.s3),
              Expanded(
                child: Text(
                  title,
                  overflow: TextOverflow.ellipsis,
                  style: AppType.bodyMd.copyWith(
                    color: active ? c.textHi : c.textMed,
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

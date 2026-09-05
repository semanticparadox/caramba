import 'dart:convert';
import 'dart:io' show Platform;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/panel_probe.dart';
import 'package:caramba_client/data/subscription_fetch.dart';
import 'package:caramba_client/features/connections/qr_scan_sheet.dart';
import 'package:caramba_client/features/enroll/connect_link.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/features/servers/access_card.dart';
import 'package:caramba_client/state/core_error.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/vpn/vpn_models.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Формат входных данных импорта. Соответствует контракту Go `subimport.Import`
/// (Build A): auto/clash/singbox/v2ray/uri. По умолчанию [auto] — детект.
enum ImportFormat {
  auto('auto', 'Авто', 'Определить формат'),
  clash('clash', 'Clash', 'mihomo / clash YAML'),
  singbox('singbox', 'sing-box', 'sing-box JSON'),
  v2ray('v2ray', 'v2ray', 'base64-список'),
  uri('uri', 'URI', 'vless:// vmess:// ss:// ...');

  final String wire;
  final String label;
  final String desc;
  const ImportFormat(this.wire, this.label, this.desc);

  /// Обратный разбор сохранённого значения провода.
  static ImportFormat fromWire(String wire) {
    for (final f in values) {
      if (f.wire == wire) return f;
    }
    return ImportFormat.auto;
  }
}

/// Что предложить после импорта подписки, которую отдала панель Caramba.
///
/// Список намеренно короткий и состоит только из того, что панель РЕАЛЬНО умеет
/// авторизовать: она выдаёт ссылку подключения через своего бота и гасит её
/// одним запросом. Пункта «ввести инвайт-код» здесь больше нет — именно он был
/// тупиком, потому что кодов панель никому не выпускала.
enum _PanelOffer {
  /// Открыть бота оператора: ссылку подключения выдаёт он.
  openBot,

  /// Открыть экран вставки ссылки `caramba://connect`.
  pasteLink,

  /// Ничего не делать, подписка и так работает.
  none,
}

// ---------------------------------------------------------------------------
// АДРЕС ПАНЕЛИ НА ЭТОМ ЭКРАНЕ НЕ ПОКАЗЫВАЕТСЯ. Почему — ниже, по обоим местам,
// где он показывался раньше. Общее правило одно: адрес панели живёт там, где он
// ПРОВЕРЯЕМ и по нему принимается решение (экран подтверждения `caramba://
// connect`, см. connect_screen.dart), и нигде больше. Здесь он не проверяем и
// решения не несёт: пользователь сам вставил эту ссылку секунду назад.
// ---------------------------------------------------------------------------

/// Заголовок листа «за этой подпиской стоит панель».
///
/// Раньше строка была `'Это подписка панели ${panel.displayName}'`, а
/// `displayName` при пустом брендинге подставлял хост. То есть у оператора,
/// который не заполнил бренд (а это состояние по умолчанию), заголовок печатал
/// адрес его панели крупным шрифтом.
///
/// Что этот адрес здесь доказывал: ничего. Строку в поле ввода набрал сам
/// пользователь, приложение возвращает её ему же. Настоящая проверка адреса
/// стоит одним экраном дальше, на подтверждении ссылки подключения, куда ведут
/// обе кнопки этого листа.
///
/// Имя бренда — заявление оператора, но здесь оно и не выдаётся за большее: это
/// ярлык вопроса «подключать ли панель», а не поле, по которому человек решает,
/// кому доверить устройство.
String panelOfferTitle(PanelProbeResult panel) {
  final brand = panel.brandName;
  if (brand.isEmpty) return 'Это подписка панели Caramba';
  return 'Это подписка панели $brand';
}

/// Имя сохраняемого профиля подписки.
///
/// САМАЯ ДОРОГАЯ УТЕЧКА АДРЕСА БЫЛА ИМЕННО ТУТ, и она была не экранной, а
/// постоянной: при пустом имени функция возвращала `uri.host`, имя уходило в
/// `ConnectionProfile.displayName`, и адрес панели навсегда поселялся в строке
/// «Подписка» на главном экране, в списке подключений и на экране входа. Ни
/// одно из этих мест ничего им не подтверждает — там он просто написан.
///
/// [fromCore] отбрасывается, когда совпадает с хостом источника: панели нередко
/// называют подписку своим доменом, и тогда «имя из конфига» это тот же адрес,
/// зашедший другой дверью. Настоящее имя («Caramba Free») проходит как есть —
/// вырезать его значило бы прятать то, что оператор сам напечатал.
///
/// Одинаковые имена нумеруются: два профиля «Подписка» человек не различит, а
/// различать их надо — это разные конфигурации.
String subscriptionProfileName({
  required String typed,
  String? fromCore,
  String? sourceHost,
  Iterable<String> taken = const <String>[],
}) {
  final own = typed.trim();
  if (own.isNotEmpty) return own;

  final core = fromCore?.trim() ?? '';
  final host = sourceHost?.trim() ?? '';
  if (core.isNotEmpty && core.toLowerCase() != host.toLowerCase()) return core;

  const base = 'Подписка';
  final used = {for (final n in taken) n.trim()};
  if (!used.contains(base)) return base;
  // Границы хватает с запасом: кандидатов здесь `used.length + 2`, занятых
  // имён `used.length`, поэтому свободное найдётся раньше конца.
  for (var n = 2; n <= used.length + 2; n++) {
    final candidate = '$base $n';
    if (!used.contains(candidate)) return candidate;
  }
  return base;
}

/// Экран импорта подписки: вставка URL или сырого текста, QR или файл, плюс
/// выбор формата (auto по умолчанию).
///
/// Порядок шагов важен: сначала «Проверить» — тело подписки загружается (если
/// вставлена ссылка) и уходит в ядро на `importSubscription`, БЕЗ поднятия
/// туннеля. Пользователь видит имя подписки и список узлов до того, как профиль
/// появится в списке; ошибку показываем текстом ядра. Только после этого
/// «Сохранить» заводит [ConnectionProfile] типа [ProfileType.rawSub] с телом,
/// форматом и кэшем узлов и делает его активным.
class ConnectionImportScreen extends ConsumerStatefulWidget {
  /// Ссылка из deeplink `carambaconnect://import?url=...`, подставляется в поле.
  final String? initialUrl;

  const ConnectionImportScreen({this.initialUrl, super.key});

  @override
  ConsumerState<ConnectionImportScreen> createState() =>
      _ConnectionImportScreenState();
}

class _ConnectionImportScreenState
    extends ConsumerState<ConnectionImportScreen> {
  final _nameController = TextEditingController();
  final _sourceController = TextEditingController();
  ImportFormat _format = ImportFormat.auto;
  bool _busy = false;
  String? _error;

  /// Исходный текст отказа — под «Подробности». На экран идёт перевод.
  String? _errorDetail;

  /// Отказ по подписке (трафик/срок/устройства): предлагаем оплату, а не
  /// «повторить» — повтор вернёт тот же ответ.
  bool _errorPayable = false;

  /// Разбор ядра: заполнен после успешной проверки, обнуляется при правке ввода.
  ImportResult? _preview;

  /// Тело подписки, отданное ядру на проверке (его и сохраняем).
  String? _fetchedRaw;

  @override
  void initState() {
    super.initState();
    final url = widget.initialUrl?.trim();
    if (url != null && url.isNotEmpty) _sourceController.text = url;
  }

  @override
  void dispose() {
    _nameController.dispose();
    _sourceController.dispose();
    super.dispose();
  }

  /// QR-скан доступен только там, где есть камера и плагин: Android и iOS.
  bool get _qrAvailable => !kIsWeb && (Platform.isAndroid || Platform.isIOS);

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final preview = _preview;

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: ListView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s12,
          ),
          children: [
            ScreenHead(
              'Импорт подписки',
              trailing: IconBtn(Lucide.arrowLeft, onTap: () => _close(context)),
            ),
            Text(
              'Вставьте ссылку на подписку или сам конфиг. Формат можно оставить '
              'на авто.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s5),

            // Имя профиля (опционально).
            const SectionTitle('Имя'),
            TextField(
              controller: _nameController,
              enabled: !_busy,
              style: AppType.bodyMd.copyWith(color: c.textHi),
              decoration: const InputDecoration(
                hintText: 'Например, «Резерв» (необязательно)',
              ),
            ),
            const SizedBox(height: AppSpace.s5),

            // Источник: URL или сырой конфиг.
            const SectionTitle('Ссылка или конфиг'),
            TextField(
              controller: _sourceController,
              enabled: !_busy,
              minLines: 3,
              maxLines: 8,
              style: AppType.monoMd.copyWith(color: c.textHi),
              onChanged: (_) => _resetPreview(),
              decoration: const InputDecoration(
                hintText: 'https://... или vless://... или YAML/JSON',
              ),
            ),
            const SizedBox(height: AppSpace.s3),

            Row(
              children: [
                Expanded(
                  child: GhostButton(
                    label: _qrAvailable
                        ? 'Сканировать QR'
                        : 'QR: вставьте текст',
                    icon: Lucide.appWindow,
                    onPressed: (_busy || !_qrAvailable) ? null : _scanQr,
                  ),
                ),
                const SizedBox(width: AppSpace.s3),
                Expanded(
                  child: GhostButton(
                    label: 'Из файла',
                    icon: Lucide.inbox,
                    onPressed: _busy ? null : _pickFile,
                  ),
                ),
              ],
            ),
            if (!_qrAvailable) ...[
              const SizedBox(height: AppSpace.s2),
              Text(
                'Камеры здесь нет. Вставьте ссылку или конфиг текстом.',
                style: AppType.bodySm.copyWith(color: c.textLow),
              ),
            ],
            const SizedBox(height: AppSpace.s5),

            // Выбор формата (auto по умолчанию).
            const SectionTitle('Формат'),
            RowsGroup(
              children: [
                CRow(
                  icon: Lucide.layers,
                  label: 'Формат',
                  value: _format.label,
                  chevron: true,
                  onTap: _busy ? null : _pickFormat,
                ),
              ],
            ),
            const SizedBox(height: AppSpace.s2),
            Text(
              _format.desc,
              style: AppType.bodySm.copyWith(color: c.textLow),
            ),
            const SizedBox(height: AppSpace.s6),

            if (_error != null) ...[
              FailureNotice(
                message: _error!,
                technical: _errorDetail,
                onRetry: _errorPayable ? null : _check,
                payable: _errorPayable,
              ),
              const SizedBox(height: AppSpace.s5),
            ],

            if (preview != null) ...[
              _PreviewCard(result: preview),
              const SizedBox(height: AppSpace.s5),
            ],

            FilledButton.icon(
              onPressed: _busy ? null : (preview == null ? _check : _save),
              icon: _busy
                  ? SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        color: c.textOnAccent,
                      ),
                    )
                  : LucideIcon(
                      preview == null ? Lucide.search : Lucide.check,
                      color: c.textOnAccent,
                      size: 18,
                    ),
              label: Text(
                _busy
                    ? 'Проверяю'
                    : (preview == null ? 'Проверить' : 'Сохранить профиль'),
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// Любая правка ввода делает прошлый разбор неактуальным.
  void _resetPreview() {
    if (_error == null && _preview == null) return;
    setState(() {
      _error = null;
      _errorDetail = null;
      _errorPayable = false;
      _preview = null;
      _fetchedRaw = null;
    });
  }

  Future<void> _pickFormat() async {
    const values = ImportFormat.values;
    final selected = await showPickerSheet(
      context: context,
      title: 'Формат импорта',
      subtitle: 'Авто подходит для большинства подписок.',
      selected: values.indexOf(_format),
      options: [
        for (final f in values)
          (name: f.label, desc: f.desc, icon: Lucide.layers),
      ],
    );
    if (selected != null && mounted) {
      setState(() {
        _format = values[selected];
        _preview = null;
        _fetchedRaw = null;
      });
    }
  }

  /// Шаг 1: загрузить тело (если это ссылка) и отдать ядру на разбор.
  Future<void> _check() async {
    final source = _sourceController.text.trim();
    if (source.isEmpty) {
      setState(() => _error = 'Вставьте ссылку или конфиг.');
      return;
    }
    // Ссылку подключения панели легко перепутать со ссылкой подписки: и ту, и
    // другую присылает один и тот же бот в одном сообщении. Разбирать её как
    // конфиг бессмысленно, а ошибка ядра «не разобрать подписку» ничего не
    // объясняет. Уводим туда, где эта ссылка и обрабатывается.
    if (looksLikeConnectLink(source)) {
      context.go(
        Uri(
          path: AppRoute.connect,
          queryParameters: {'link': source},
        ).toString(),
      );
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
      _errorDetail = null;
      _errorPayable = false;
      _preview = null;
    });

    try {
      final looksLikeUrl =
          source.startsWith('http://') || source.startsWith('https://');
      // Ссылку на подписку тянем здесь и сохраняем тело: нативная сторона
      // (subimport.Import) умеет только парсить байты, HTTP-клиента у неё нет.
      final raw = looksLikeUrl ? await fetchSubscriptionBody(source) : source;
      final result = await ref
          .read(vpnConnectionProvider)
          .importSubscription(raw: raw, format: _format.wire);
      if (!mounted) return;
      setState(() {
        _busy = false;
        _fetchedRaw = raw;
        _preview = result;
      });
    } on SubscriptionFetchException catch (e) {
      if (!mounted) return;
      // Владелец увидел здесь «ответ сервера 403» и три часа искал прокси и
      // истёкший токен. Отказ панели теперь называется тем, чем он является.
      final failure = describeFailure(e);
      setState(() {
        _busy = false;
        _error = failure?.text ?? 'Не удалось загрузить подписку: ${e.message}';
        _errorDetail = failure?.technical;
        _errorPayable = failure?.payable ?? false;
      });
    } catch (e) {
      if (!mounted) return;
      final failure = describeFailure(e);
      setState(() {
        _busy = false;
        _error =
            failure?.text ??
            'Не удалось разобрать подписку. Проверьте ссылку, конфиг и формат.';
        _errorDetail = failure?.technical;
        _errorPayable = failure?.payable ?? false;
      });
    }
  }

  /// Шаг 2: завести профиль с телом, форматом и кэшем узлов.

  /// Спрашивает, подключать ли распознанную панель.
  ///
  /// РАНЬШЕ ЗДЕСЬ БЫЛ ТУПИК. Единственная кнопка вела на `/enroll` БЕЗ кода, то
  /// есть на экран «введите инвайт-код», а выпускать эти коды на живой панели
  /// было нечем: таблица пуста, и человек с уже готовым аккаунтом упирался в
  /// требование, которое не может выполнить никто. Владелец наткнулся ровно на
  /// это, вставив ссылку своей подписки.
  ///
  /// Поэтому лист предлагает ТОЛЬКО то, что панель действительно умеет выдать:
  /// ссылку подключения `caramba://connect`, которую даёт бот оператора рядом
  /// со ссылкой на подписку. Адрес бота берётся из брендинга самой панели. Если
  /// панель его не опубликовала, кнопки «открыть бота» НЕТ и вместо неё стоит
  /// прямая фраза о том, что способ подключения оператор не опубликовал:
  /// кнопка, ведущая в никуда, хуже её отсутствия.
  Future<_PanelOffer> _offerPanel(PanelProbeResult panel) async {
    final c = context.c;
    final botUrl = panel.branding.botUrl.trim();
    final answer = await showModalBottomSheet<_PanelOffer>(
      context: context,
      backgroundColor: c.surface1,
      isScrollControlled: true,
      shape: const RoundedRectangleBorder(borderRadius: AppRadius.sheetTop),
      builder: (sheetContext) => SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(AppSpace.s5),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                panelOfferTitle(panel),
                // Имя бренда приходит извне и длину его никто не обещал.
                // Заголовок — не строка «подпись / значение», подделать им
                // соседнее поле нельзя, но развернуться на пол-листа он может.
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: AppType.titleLg.copyWith(color: c.textHi),
              ),
              const SizedBox(height: AppSpace.s3),
              Text(
                'Подписка уже работает и никуда не денется. Если подключить '
                'панель, добавятся смена страны и релэя, выбор протокола, тариф '
                'и устройства.',
                style: AppType.bodyMd.copyWith(color: c.textMed),
              ),
              const SizedBox(height: AppSpace.s3),
              Text(
                botUrl.isEmpty
                    ? 'Подключение делается ссылкой, которую выдаёт бот '
                          'оператора. Адрес бота эта панель не публикует, '
                          'поэтому открыть его отсюда нельзя: возьмите ссылку у '
                          'оператора и вставьте её.'
                    : 'Подключение делается ссылкой из бота оператора: она '
                          'приходит личным сообщением рядом со ссылкой на '
                          'подписку.',
                style: AppType.bodySm.copyWith(color: c.textLow),
              ),
              const SizedBox(height: AppSpace.s5),
              if (botUrl.isNotEmpty) ...[
                FilledButton(
                  onPressed: () =>
                      Navigator.of(sheetContext).pop(_PanelOffer.openBot),
                  child: const Text('Открыть бота за ссылкой'),
                ),
                const SizedBox(height: AppSpace.s2),
                GhostButton(
                  label: 'У меня уже есть ссылка',
                  onPressed: () =>
                      Navigator.of(sheetContext).pop(_PanelOffer.pasteLink),
                ),
              ] else
                FilledButton(
                  onPressed: () =>
                      Navigator.of(sheetContext).pop(_PanelOffer.pasteLink),
                  child: const Text('Вставить ссылку подключения'),
                ),
              const SizedBox(height: AppSpace.s2),
              GhostButton(
                label: 'Пока не нужно',
                onPressed: () =>
                    Navigator.of(sheetContext).pop(_PanelOffer.none),
              ),
            ],
          ),
        ),
      ),
    );
    return answer ?? _PanelOffer.none;
  }

  Future<void> _save() async {
    final preview = _preview;
    final raw = _fetchedRaw;
    if (preview == null || raw == null) return;
    final source = _sourceController.text.trim();
    final now = DateTime.now();
    final profile = ConnectionProfile(
      id: 'cp_${now.millisecondsSinceEpoch}',
      type: ProfileType.rawSub,
      displayName: subscriptionProfileName(
        typed: _nameController.text,
        fromCore: preview.name,
        sourceHost: Uri.tryParse(source)?.host,
        taken: ref
            .read(connectionProfilesProvider)
            .profiles
            .map((p) => p.displayName),
      ),
      source: source,
      rawConfig: raw,
      format: _format.wire,
      servers: preview.servers,
      serversUpdatedMs: now.millisecondsSinceEpoch,
      lastActiveMs: 0,
    );
    await ref.read(connectionProfilesProvider.notifier).add(profile);
    if (!mounted) return;
    showCarambaToast(context, 'Профиль добавлен');

    // Ссылку могла отдать панель Caramba. Тогда предлагаем подключить её:
    // подписка сама по себе даёт только свои узлы, а панель добавляет смену
    // страны, релэя и протокола, тариф и устройства. Отказ ничего не ломает,
    // профиль уже сохранён и работает как обычная подписка.
    final panel = await probeCarambaPanel(source);
    if (!mounted) return;
    if (panel != null) {
      final offer = await _offerPanel(panel);
      if (!mounted) return;
      switch (offer) {
        case _PanelOffer.openBot:
          // Уводим в бота и остаёмся здесь: ссылка придёт в мессенджер, и по
          // возвращении человек либо откроет её (диплинк сам приведёт на экран
          // подтверждения), либо вставит вручную.
          await openExternal(context, panel.branding.botUrl.trim());
          if (!mounted) return;
        case _PanelOffer.pasteLink:
          context.go(AppRoute.connect);
          return;
        case _PanelOffer.none:
          break;
      }
    }
    if (!mounted) return;
    // Generic-режим: пользователь пришёл сюда с экрана входа или по deeplink
    // `carambaconnect://import`, возвращать его в список профилей незачем —
    // ведём на Home, подключаться. Признак — отсутствие сессии панели, а не
    // только флаг режима: по ссылке импорт открывается и до его установки.
    if (ref.read(guestModeProvider) ||
        ref.read(authProvider).stage != AuthStage.authenticated) {
      context.go(AppRoute.home);
      return;
    }
    _close(context);
  }

  Future<void> _scanQr() async {
    final code = await showQrScanSheet(context);
    if (code == null || !mounted) return;
    // QR с приглашением панели встречается тут ровно так же часто, как QR с
    // подпиской: оператор печатает оба. Класть приглашение в поле подписки
    // значит гарантировать невнятную ошибку разбора.
    if (looksLikeConnectLink(code.trim())) {
      context.go(
        Uri(
          path: AppRoute.connect,
          queryParameters: {'link': code.trim()},
        ).toString(),
      );
      return;
    }
    _sourceController.text = code;
    _resetPreview();
    setState(() {});
  }

  /// Читает конфиг из файла. Байты берём сразу (`withData`), чтобы не зависеть
  /// от доступа к пути на Android/iOS. Не-UTF8 файл отклоняем внятным текстом.
  Future<void> _pickFile() async {
    try {
      final picked = await FilePicker.platform.pickFiles(withData: true);
      final file = picked?.files.singleOrNull;
      final bytes = file?.bytes;
      if (bytes == null) return;
      final text = utf8.decode(bytes, allowMalformed: false).trim();
      if (!mounted) return;
      if (text.isEmpty) {
        setState(() => _error = 'Файл пустой.');
        return;
      }
      _sourceController.text = text;
      _resetPreview();
      setState(() {});
    } on FormatException {
      if (!mounted) return;
      setState(
        () => _error = 'Файл не текстовый. Нужен YAML, JSON или список URI.',
      );
    } catch (_) {
      if (!mounted) return;
      setState(() => _error = 'Не удалось прочитать файл.');
    }
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.connections);
    }
  }
}

/// Превью разбора: сколько узлов нашло ядро и какие именно. Показывается до
/// сохранения, чтобы пустая или чужая подписка не превратилась в профиль.
class _PreviewCard extends StatelessWidget {
  final ImportResult result;
  const _PreviewCard({required this.result});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    if (result.servers.isEmpty) {
      return Container(
        padding: const EdgeInsets.all(AppSpace.s4),
        decoration: BoxDecoration(
          color: c.surface1,
          borderRadius: AppRadius.r14,
          border: Border.all(color: c.borderSubtle),
        ),
        child: Row(
          children: [
            LucideIcon(Lucide.alert, color: c.warning, size: 18),
            const SizedBox(width: AppSpace.s3),
            Expanded(
              child: Text(
                'Подписка разобрана, но узлов в ней нет. Сохранять нечего.',
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),
            ),
          ],
        ),
      );
    }

    // Длинный список не разворачиваем целиком: первые узлы дают понять, что
    // подписка та, остальное видно на экране серверов после сохранения.
    const limit = 8;
    final shown = result.servers.take(limit).toList(growable: false);
    final rest = result.servers.length - shown.length;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SectionTitle(
          result.name == null
              ? 'Найдено узлов: ${result.servers.length}'
              : '${result.name} · узлов: ${result.servers.length}',
          padding: const EdgeInsets.only(bottom: AppSpace.s3),
        ),
        RowsGroup(
          children: [
            for (final s in shown)
              CRow(
                label: s.name.isEmpty ? s.id : s.name,
                value: s.country.isEmpty ? s.type : '${s.country} · ${s.type}',
                mono: true,
              ),
          ],
        ),
        if (rest > 0) ...[
          const SizedBox(height: AppSpace.s2),
          Text('и ещё $rest', style: AppType.bodySm.copyWith(color: c.textLow)),
        ],
      ],
    );
  }
}

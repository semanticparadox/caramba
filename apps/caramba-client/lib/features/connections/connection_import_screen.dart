import 'dart:convert';
import 'dart:io' show Platform;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show Clipboard;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/panel_probe.dart';
import 'package:caramba_client/data/subscription_fetch.dart';
import 'package:caramba_client/features/connections/entry_classifier.dart';
import 'package:caramba_client/features/connections/qr_scan_sheet.dart';
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
///
/// ВЫБОР ФОРМАТА БОЛЬШЕ НЕ СПРАШИВАЕТСЯ ЗАРАНЕЕ. Ядро определяет формат по
/// самим байтам (`subimport.detectFormat`: одиночный URI известной схемы, `{`
/// в начале, ключ `proxies:`, декодируемый base64-список), то есть знает о
/// строке строго больше, чем человек, который её вставил. Единственное место
/// догадки там — последний пункт цепочки, и он проваливается ВНЯТНОЙ ошибкой
/// разбора, а не тихой пустотой. Поэтому список форматов показывается только
/// после такой ошибки — как запасной ход, а не как вопрос на входе.
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
///
/// [typed] с экрана больше не приходит: поля «Имя» на входе нет. Параметр
/// остался, потому что переименование живёт на экране списка подключений
/// (`connections_screen.dart`, пункт «Переименовать») и потому что спрашивать
/// имя ДО того, как человек увидел, что вообще импортировалось, — это вопрос,
/// на который нечем ответить. Здесь имя всегда выводится: из конфига, иначе
/// «Подписка N».
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

/// Экран импорта подписки: одно поле, QR и кнопка «Продолжить».
///
/// Тонкая обёртка вокруг [ConnectionEntryForm] — вся работа в форме, потому что
/// ровно та же форма стоит и на первом экране приложения (`login_screen.dart`).
/// Копия здесь означала бы, что `caramba://` из одного входа работает, а из
/// другого — нет.
class ConnectionImportScreen extends ConsumerWidget {
  /// Ссылка из deeplink `carambaconnect://import?url=...`, подставляется в поле.
  final String? initialUrl;

  const ConnectionImportScreen({this.initialUrl, super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: ConnectionEntryForm(
          initialText: initialUrl,
          head: ScreenHead(
            'Подключение',
            trailing: IconBtn(Lucide.arrowLeft, onTap: () => _close(context)),
          ),
          onDone: () => _close(context),
        ),
      ),
    );
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.connections);
    }
  }
}

/// Одно поле, из которого начинается всё подключение.
///
/// ЧТО ЗДЕСЬ ПРОИЗОШЛО. Раньше до первого узла нужно было пройти два экрана и
/// четыре-пять решений: выбрать раздел («Подписка» / «Панель Caramba» / «Код из
/// бота»), придумать имя профиля, выбрать формат из пяти, нажать «Проверить», а
/// потом ещё «Сохранить профиль». Ни одно из этих решений человек принять не
/// может: раздел зависит от того, что за строку ему прислали, имя ему всё равно
/// не с чем сравнивать, формат знает ядро, а «Проверить» и «Сохранить» — это
/// два имени одного намерения.
///
/// Осталось одно поле и одна кнопка. Куда нести строку, решает
/// [classifyEntry]; формат определяет ядро; имя выводится из разобранного
/// конфига; «Продолжить» проверяет, а появившаяся после проверки «Подключить»
/// сохраняет. Всё редкое (файл, код приглашения, код из бота) уехало под «Ещё»
/// — оно не исчезло, потому что исчезнувшая функция неотличима от несуществующей.
///
/// ЗАПАСНОЙ ХОД ПО ФОРМАТУ. Список форматов появляется ТОЛЬКО после того, как
/// ядро не смогло разобрать вставленное. До первой неудачи его нет: вопрос,
/// на который в 99 случаях из 100 правильный ответ «авто», — это не выбор, а
/// налог.
class ConnectionEntryForm extends ConsumerStatefulWidget {
  /// Что подставить в поле сразу (deeplink `carambaconnect://import?url=...`).
  final String? initialText;

  /// Шапка экрана. Разная у двух хозяев формы, поэтому приходит снаружи.
  final Widget head;

  /// Дополнительные пункты под «Ещё». Первый экран кладёт сюда код приглашения
  /// и вход по коду из бота; экрану импорта они не нужны.
  final List<Widget> extras;

  /// Вызывается ПЕРЕД сохранением профиля. Первый экран включает здесь
  /// generic-режим: без него редирект роутера отбросил бы на /login ровно в
  /// момент, когда подключаться уже есть чем.
  final Future<void> Function()? onBeforeSave;

  /// Куда уйти после сохранения, если человек уже внутри приложения.
  final VoidCallback onDone;

  const ConnectionEntryForm({
    required this.head,
    required this.onDone,
    this.initialText,
    this.extras = const <Widget>[],
    this.onBeforeSave,
    super.key,
  });

  @override
  ConsumerState<ConnectionEntryForm> createState() =>
      _ConnectionEntryFormState();
}

class _ConnectionEntryFormState extends ConsumerState<ConnectionEntryForm> {
  final _sourceController = TextEditingController();

  /// Формат. Меняется только через запасной ход после ошибки разбора.
  ImportFormat _format = ImportFormat.auto;

  /// Показывать ли выбор формата: ядро уже один раз не справилось.
  bool _formatOffered = false;

  bool _busy = false;
  bool _moreOpen = false;
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

  /// Что записать в `ConnectionProfile.source`. Для голой ссылки это она сама,
  /// для `carambaconnect://import?url=X` — X, а не обёртка: иначе «Обновить
  /// подписку» на экране подключений качало бы диплинк вместо подписки.
  String _savedSource = '';

  @override
  void initState() {
    super.initState();
    final url = widget.initialText?.trim();
    if (url != null && url.isNotEmpty) _sourceController.text = url;
  }

  @override
  void dispose() {
    _sourceController.dispose();
    super.dispose();
  }

  /// QR-скан доступен только там, где есть камера и плагин: Android и iOS.
  bool get _qrAvailable => !kIsWeb && (Platform.isAndroid || Platform.isIOS);

  /// Есть ли что показывать под «Ещё». На desktop «Из файла» стоит основной
  /// кнопкой вместо QR, и без внешних пунктов раскрывать нечего.
  bool get _hasMore => _qrAvailable || widget.extras.isNotEmpty;

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final preview = _preview;
    final ready = _sourceController.text.trim().isNotEmpty;

    return ListView(
      padding: const EdgeInsets.fromLTRB(
        AppSpace.s5,
        AppSpace.s5,
        AppSpace.s5,
        AppSpace.s12,
      ),
      children: [
        widget.head,
        Text(
          'Вставьте ссылку на подписку, конфиг sing-box или clash, или ссылку '
          'caramba:// — либо отсканируйте QR. Что это, приложение поймёт само.',
          style: AppType.bodyMd.copyWith(color: c.textMed),
        ),
        const SizedBox(height: AppSpace.s5),

        TextField(
          controller: _sourceController,
          enabled: !_busy,
          minLines: 3,
          maxLines: 6,
          style: AppType.monoMd.copyWith(color: c.textHi),
          onChanged: (_) => _onTyped(),
          decoration: const InputDecoration(
            hintText: 'https://... · vless://... · caramba://... · YAML/JSON',
          ),
        ),
        const SizedBox(height: AppSpace.s3),

        // Кнопки стоят друг под другом, а не пополам в одной строке: половины
        // строки на узком экране не хватает на «Сканировать QR» целиком —
        // GhostButton режет подпись [TextOverflow.ellipsis], а dart:ui
        // включает предел в одну строку уже тем, что `ellipsis` задан, даже
        // без явного `maxLines`. Так подпись обрывалась до «Сканировать …».
        // GhostButton общий на всё приложение и здесь не трогается: вместо
        // более широкой колонки, которой всё равно неоткуда взяться в
        // половине строки, каждой кнопке отдаётся вся ширина.
        GhostButton(
          label: 'Вставить',
          icon: Lucide.copy,
          onPressed: _busy ? null : _paste,
        ),
        const SizedBox(height: AppSpace.s3),
        _qrAvailable
            ? GhostButton(
                label: 'Сканировать QR',
                icon: Lucide.appWindow,
                onPressed: _busy ? null : _scanQr,
              )
            : GhostButton(
                label: 'Из файла',
                icon: Lucide.inbox,
                onPressed: _busy ? null : _pickFile,
              ),
        const SizedBox(height: AppSpace.s5),

        FilledButton.icon(
          onPressed: _busy || (!ready && preview == null)
              ? null
              : (preview == null ? _continue : _save),
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
                  preview == null ? Lucide.chevronRight : Lucide.zap,
                  color: c.textOnAccent,
                  size: 18,
                ),
          label: Text(
            _busy
                ? 'Проверяю'
                : (preview == null ? 'Продолжить' : 'Подключить'),
          ),
        ),

        if (_error != null) ...[
          const SizedBox(height: AppSpace.s4),
          FailureNotice(
            message: _error!,
            technical: _errorDetail,
            onRetry: _errorPayable ? null : _continue,
            payable: _errorPayable,
          ),
          // Запасной ход: формат показывается только тут и только после того,
          // как автоопределение уже не справилось.
          if (_formatOffered) ...[
            const SizedBox(height: AppSpace.s2),
            GhostButton(
              label: _format == ImportFormat.auto
                  ? 'Указать формат вручную'
                  : 'Формат: ${_format.label}',
              icon: Lucide.layers,
              onPressed: _busy ? null : _pickFormat,
            ),
          ],
        ],

        if (preview != null) ...[
          const SizedBox(height: AppSpace.s5),
          _PreviewCard(result: preview),
        ],

        if (_hasMore) ...[
          const SizedBox(height: AppSpace.s5),
          GhostButton(
            label: _moreOpen ? 'Скрыть' : 'Ещё',
            icon: _moreOpen ? Lucide.chevronUp : Lucide.chevronDown,
            onPressed:
                _busy ? null : () => setState(() => _moreOpen = !_moreOpen),
          ),
          if (_moreOpen) ...[
            const SizedBox(height: AppSpace.s3),
            if (_qrAvailable) ...[
              GhostButton(
                label: 'Из файла',
                icon: Lucide.inbox,
                onPressed: _busy ? null : _pickFile,
              ),
              const SizedBox(height: AppSpace.s2),
            ],
            for (final w in widget.extras) ...[
              w,
              const SizedBox(height: AppSpace.s2),
            ],
          ],
        ],
      ],
    );
  }

  /// Любая правка ввода делает прошлый разбор неактуальным. Кнопка «Продолжить»
  /// зависит от непустоты поля, поэтому перерисовываемся на каждом символе.
  void _onTyped() {
    setState(() {
      _error = null;
      _errorDetail = null;
      _errorPayable = false;
      _preview = null;
      _fetchedRaw = null;
    });
  }

  void _fail(String message, {String? detail, bool payable = false}) {
    setState(() {
      _busy = false;
      _error = message;
      _errorDetail = detail;
      _errorPayable = payable;
    });
  }

  Future<void> _pickFormat() async {
    const values = ImportFormat.values;
    final selected = await showPickerSheet(
      context: context,
      title: 'Формат конфига',
      subtitle:
          'Обычно определяется сам. Выберите вручную, если разбор не удался.',
      selected: values.indexOf(_format),
      options: [
        for (final f in values)
          (name: f.label, desc: f.desc, icon: Lucide.layers),
      ],
    );
    if (selected == null || !mounted) return;
    setState(() {
      _format = values[selected];
      _preview = null;
      _fetchedRaw = null;
    });
    await _continue();
  }

  /// Единственная кнопка входа. Классифицирует строку и уводит либо на
  /// подтверждение приглашения, либо в энроллмент, либо разбирает подписку.
  Future<void> _continue() async {
    final entry = classifyEntry(_sourceController.text);
    switch (entry.kind) {
      case EntryKind.empty:
        _fail('Вставьте ссылку или конфиг.');
        return;
      case EntryKind.refused:
        // Ссылка наша, но отвергнута, и причина названа: http:// вместо https,
        // пустой код, неразбираемый адрес. Раньше такая строка уезжала в ядро и
        // возвращалась ошибкой разбора YAML.
        _fail(entry.refusalMessage ?? 'Ссылка не распознана.');
        return;
      case EntryKind.connectLink:
      case EntryKind.enrollLink:
        // Ссылку подключения и ссылку подписки бот присылает одним сообщением,
        // и перепутать поля проще простого. Разбирать приглашение как конфиг
        // бессмысленно: уводим туда, где оно и обрабатывается.
        context.go(entry.target!);
        return;
      case EntryKind.subscriptionUrl:
      case EntryKind.configText:
        await _import(entry);
    }
  }

  /// Загрузить тело (если это ссылка) и отдать ядру на разбор. Туннель не
  /// поднимается: человек видит список узлов ДО того, как появится профиль.
  Future<void> _import(EntryClassification entry) async {
    final text = _sourceController.text.trim();
    setState(() {
      _busy = true;
      _error = null;
      _errorDetail = null;
      _errorPayable = false;
      _preview = null;
    });

    final url = entry.url;
    try {
      // Ссылку на подписку тянет приложение: нативная сторона
      // (subimport.Import) умеет только парсить байты, HTTP-клиента у неё нет.
      final raw = url == null ? text : await fetchSubscriptionBody(url);
      final result = await ref
          .read(vpnConnectionProvider)
          .importSubscription(raw: raw, format: _format.wire);
      if (!mounted) return;
      setState(() {
        _busy = false;
        _fetchedRaw = raw;
        _savedSource = url ?? text;
        _preview = result;
      });
    } on SubscriptionFetchException catch (e) {
      if (!mounted) return;
      // Владелец увидел здесь «ответ сервера 403» и три часа искал прокси и
      // истёкший токен. Отказ панели теперь называется тем, чем он является.
      // Формат тут ни при чём: тело даже не доехало, предлагать его выбор
      // значило бы отправить человека чинить не то.
      final failure = describeFailure(e);
      _fail(
        failure?.text ?? 'Не удалось загрузить подписку: ${e.message}',
        detail: failure?.technical,
        payable: failure?.payable ?? false,
      );
    } catch (e) {
      if (!mounted) return;
      // Разбор. Вот теперь запасной ход по формату уместен: байты у ядра были,
      // и не сошлось именно на них.
      final failure = describeFailure(e);
      setState(() => _formatOffered = true);
      _fail(
        failure?.text ??
            'Не удалось разобрать: это не похоже ни на подписку, ни на конфиг.',
        detail: failure?.technical,
        payable: failure?.payable ?? false,
      );
    }
  }

  /// Спрашивает, подключать ли распознанную панель.
  ///
  /// РАНЬШЕ ЗДЕСЬ БЫЛ ТУПИК. Единственная кнопка вела на экран «введите
  /// инвайт-код» БЕЗ кода, а выпускать эти коды на живой панели было нечем:
  /// таблица пуста, и человек с уже готовым аккаунтом упирался в требование,
  /// которое не может выполнить никто. Владелец наткнулся ровно на это, вставив
  /// ссылку своей подписки.
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

  /// Заводит профиль с телом, форматом и кэшем узлов и делает его активным.
  Future<void> _save() async {
    final preview = _preview;
    final raw = _fetchedRaw;
    if (preview == null || raw == null) return;
    final source = _savedSource;
    await widget.onBeforeSave?.call();
    if (!mounted) return;
    final now = DateTime.now();
    final profile = ConnectionProfile(
      id: 'cp_${now.millisecondsSinceEpoch}',
      type: ProfileType.rawSub,
      displayName: subscriptionProfileName(
        // Поля имени на входе нет: имя приходит из разобранного конфига, а
        // переименовать профиль можно на экране подключений, уже видя его.
        typed: '',
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
    // Generic-режим: пользователь пришёл сюда с первого экрана или по deeplink
    // `carambaconnect://import`, возвращать его в список профилей незачем —
    // ведём на Home, подключаться. Признак — отсутствие сессии панели, а не
    // только флаг режима: по ссылке импорт открывается и до его установки.
    if (ref.read(guestModeProvider) ||
        ref.read(authProvider).stage != AuthStage.authenticated) {
      context.go(AppRoute.home);
      return;
    }
    widget.onDone();
  }

  /// Вставка из буфера. Строка кладётся в поле, а не уносится сразу: человек
  /// должен видеть, что именно вставилось, — в буфере нередко лежит не то.
  Future<void> _paste() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text?.trim() ?? '';
    if (!mounted) return;
    if (text.isEmpty) {
      showCarambaToast(context, 'В буфере обмена ничего нет');
      return;
    }
    _sourceController.text = text;
    _onTyped();
  }

  Future<void> _scanQr() async {
    final code = await showQrScanSheet(context);
    if (code == null || !mounted) return;
    final trimmed = code.trim();
    // QR с приглашением панели встречается тут ровно так же часто, как QR с
    // подпиской: оператор печатает оба. Класть приглашение в поле подписки
    // значит гарантировать невнятную ошибку разбора.
    final entry = classifyEntry(trimmed);
    if (entry.isLink) {
      context.go(entry.target!);
      return;
    }
    _sourceController.text = trimmed;
    _onTyped();
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
        _fail('Файл пустой.');
        return;
      }
      _sourceController.text = text;
      _onTyped();
    } on FormatException {
      if (!mounted) return;
      _fail('Файл не текстовый. Нужен YAML, JSON или список URI.');
    } catch (_) {
      if (!mounted) return;
      _fail('Не удалось прочитать файл.');
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

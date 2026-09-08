/// Экран подключения панели по ссылке `caramba://connect`.
///
/// Единственное, что человек здесь делает, это ПОДТВЕРЖДАЕТ. Ввода нет: адрес
/// панели, имя оператора и срок приглашения приходят в самой ссылке. Экран
/// подтверждения существует не для красоты — панелей может быть много, ссылку
/// приносят из мессенджера, и человек обязан понимать, К КОМУ он сейчас
/// привязывает устройство, до того как это случится.
///
/// О ССЫЛКЕ ГОВОРИМ ЧЕСТНО. Она непрозрачная и защищена от искажения
/// контрольной суммой, но НЕ зашифрована и НЕ подписана: у приложения нет
/// общего секрета с оператором, которого оно ещё не видело. Значит, имя
/// оператора в ссылке это ЗАЯВЛЕНИЕ отправителя, и проверяемое здесь — только
/// целостность байт и то, что адрес по https.
///
/// ГДЕ В ПРИЛОЖЕНИИ ВООБЩЕ ВИДЕН АДРЕС ПАНЕЛИ — РЕШЕНО ЗДЕСЬ, ОДИН РАЗ.
/// Решение владельца: при добавлении подписки по `caramba://` адрес панели на
/// дороге не показываем. Прежний довод при этом никуда не делся — origin
/// единственное поле ссылки, за которое ручается не отправитель, а TLS, — и
/// сохранён он не строкой, а действием:
///
///   • [_confirm] — адрес ПОД ДЕЙСТВИЕМ «Показать адрес панели». Не на дороге,
///     но в одном нажатии, и раскрывается вместе с корневым ключом и
///     объяснением, что именно проверяет TLS и почему имя не проверяет никто.
///     Тот, кто пришёл сверять, получает ровно то же, что получал раньше; тот,
///     кто подключается по ссылке своего оператора, не читает лекцию про
///     сертификаты, чтобы нажать «Подключить».
///
///   • [_done] — НЕ ПОКАЗЫВАЕМ, как и прежде. Решение принято экраном раньше,
///     адрес тот же самый (погашение идёт на `link.origin`), изменить или
///     отменить по нему уже нечего. Зато кадр этот висит дольше всех (при
///     проблеме с подпиской — до нажатия кнопки), и именно его отправляют в
///     поддержку скриншотом.
///
/// Тому, кто придёт следующим: «восстановить для симметрии» строку в [_done]
/// нельзя — она ничего не подтверждает; убрать из [_confirm] кнопку «Показать
/// адрес панели» тоже нельзя — вместе с ней уходит единственный способ
/// отличить панель своего оператора от чужой, и остаётся только имя, которое
/// пишет отправитель. Сторожит test/panel_address_exposure_test.dart: скрыт по
/// умолчанию, доступен по действию, отсутствует на [_done].
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show Clipboard;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/features/connections/qr_scan_sheet.dart';
import 'package:caramba_client/features/enroll/connect_controller.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

class ConnectScreen extends ConsumerStatefulWidget {
  /// Сырая ссылка из диплинка (query `link`). `null` — экран вставки.
  final String? initialLink;

  const ConnectScreen({this.initialLink, super.key});

  @override
  ConsumerState<ConnectScreen> createState() => _ConnectScreenState();
}

class _ConnectScreenState extends ConsumerState<ConnectScreen> {
  final _pasteController = TextEditingController();

  /// Раскрыт ли блок с адресом панели и корневым ключом.
  ///
  /// Закрыт по умолчанию: адрес — инструмент сверки, и нужен он тому, кто
  /// пришёл сверять, а не каждому, кто открыл ссылку своего оператора. См.
  /// решение в шапке файла.
  bool _showAddress = false;

  @override
  void initState() {
    super.initState();
    final raw = widget.initialLink?.trim();
    if (raw != null && raw.isNotEmpty) {
      // Разбор после первого кадра: нотифаер autoDispose создаётся при первом
      // чтении провайдера, а читать его из initState до монтирования дерева
      // Riverpod не даёт.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) ref.read(connectProvider.notifier).open(raw);
      });
    }
  }

  @override
  void dispose() {
    _pasteController.dispose();
    super.dispose();
  }

  /// Автопереход в приложение уже запланирован. Гасит повтор: [build]
  /// вызывается не один раз на одном и том же состоянии.
  bool _handoverScheduled = false;

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final s = ref.watch(connectProvider);
    _scheduleHandover(s);

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s6,
            AppSpace.s5,
            AppSpace.s12,
          ),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 460),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: _body(context, s),
            ),
          ),
        ),
      ),
    );
  }

  /// Когда сказать нечего, экран сам уходит в приложение через короткую паузу:
  /// её хватает, чтобы кадр «Панель подключена» отрисовался и человек увидел
  /// имя оператора, к которому подключился. Когда сказать есть что (подписки
  /// нет и названа причина), автоперехода не бывает вовсе — там кнопка.
  void _scheduleHandover(ConnectState s) {
    if (_handoverScheduled) return;
    if (s.stage != ConnectStage.done) return;
    if (s.result?.subscriptionReasonText != null) return;
    _handoverScheduled = true;
    Future<void>.delayed(const Duration(milliseconds: 1200), () {
      if (mounted) _finish();
    });
  }

  List<Widget> _body(BuildContext context, ConnectState s) => switch (s.stage) {
    ConnectStage.needLink => _needLink(context),
    ConnectStage.confirm => _confirm(context, s),
    ConnectStage.refused => _refused(context, s),
    ConnectStage.redeeming => _busy(context, 'Подключаем панель'),
    ConnectStage.failed => _failed(context, s),
    ConnectStage.done => _done(context, s),
  };

  // ---------------------------------------------------------------------------
  // Ссылки нет: объясняем, где её берут, и даём вставить.
  // ---------------------------------------------------------------------------

  List<Widget> _needLink(BuildContext context) {
    final c = context.c;
    return [
      ScreenHead(
        'Подключить панель',
        trailing: IconBtn(Lucide.x, onTap: _close),
      ),
      Text(
        'Ссылку подключения выдаёт бот вашего оператора: она приходит личным '
        'сообщением рядом со ссылкой на подписку. Откройте её или вставьте '
        'сюда.',
        style: AppType.bodyMd.copyWith(color: c.textMed),
      ),
      const SizedBox(height: AppSpace.s5),
      const SectionTitle(
        'Ссылка',
        padding: EdgeInsets.only(bottom: AppSpace.s3),
      ),
      TextField(
        controller: _pasteController,
        minLines: 2,
        maxLines: 5,
        autocorrect: false,
        style: AppType.monoMd.copyWith(color: c.textHi),
        decoration: const InputDecoration(hintText: 'caramba://connect?d=...'),
      ),
      const SizedBox(height: AppSpace.s3),
      Row(
        children: [
          Expanded(
            child: GhostButton(
              label: 'Вставить из буфера',
              icon: Lucide.copy,
              onPressed: _pasteFromClipboard,
            ),
          ),
          if (qrScanSupported) ...[
            const SizedBox(width: AppSpace.s3),
            Expanded(
              child: GhostButton(
                label: 'Сканировать QR',
                icon: Lucide.appWindow,
                onPressed: _scanQr,
              ),
            ),
          ],
        ],
      ),
      if (!qrScanSupported) ...[
        const SizedBox(height: AppSpace.s2),
        Text(
          'Камеры на этой платформе нет, поэтому сканирования QR тут не будет. '
          'Вставьте ссылку текстом.',
          style: AppType.bodySm.copyWith(color: c.textLow),
        ),
      ],
      const SizedBox(height: AppSpace.s5),
      FilledButton(
        onPressed: () =>
            ref.read(connectProvider.notifier).open(_pasteController.text),
        child: const Text('Продолжить'),
      ),
    ];
  }

  // ---------------------------------------------------------------------------
  // Подтверждение: что именно мы собираемся подключить.
  // ---------------------------------------------------------------------------

  List<Widget> _confirm(BuildContext context, ConnectState s) {
    final c = context.c;
    final link = s.link!;
    final nowSec = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    return [
      ScreenHead(
        'Подключить панель',
        trailing: IconBtn(Lucide.x, onTap: _close),
      ),
      Text(
        'Проверьте, к какому оператору вы подключаете это устройство: после '
        'подтверждения серверы, тариф и настройки будет выдавать именно его '
        'панель.',
        style: AppType.bodyMd.copyWith(color: c.textMed),
      ),
      const SizedBox(height: AppSpace.s5),
      RowsGroup(
        children: [
          // Имя — заявление отправителя, и подпись строки говорит об этом
          // прямо. Значение живёт в trailing, а не в value, чтобы задать ему
          // maxLines: 1: CRow рисует value без ограничения строк, и поле с
          // переводом строки нарисовало бы вторую строку, неотличимую от
          // настоящей строки экрана. Разбор такие имена теперь отвергает (см.
          // connect_link.dart), но граница разбора и граница отрисовки — две
          // разные границы, и держать надо обе.
          CRow(
            icon: Lucide.user,
            label: 'Имя из ссылки',
            trailing: _OneLineValue(
              link.operatorName.isEmpty ? 'без имени' : link.operatorName,
            ),
          ),
          CRow(
            icon: Lucide.clock,
            label: 'Приглашение живо',
            value: _remainingText(link.remaining(nowSec)),
            mono: true,
          ),
        ],
      ),
      const SizedBox(height: AppSpace.s3),
      GhostButton(
        label: _showAddress ? 'Скрыть адрес панели' : 'Показать адрес панели',
        icon: Lucide.eye,
        onPressed: () => setState(() => _showAddress = !_showAddress),
      ),
      if (_showAddress) ...[
        const SizedBox(height: AppSpace.s3),
        RowsGroup(
          children: [
            // Единственное поле ссылки, которое приложение способно проверить
            // само: по этому адресу пойдёт TLS, и сертификат либо принадлежит
            // домену, либо соединения не будет. Целиком, без сокращений —
            // сверяют его посимвольно.
            CRow(
              icon: Lucide.lock,
              label: 'Адрес панели',
              value: link.origin,
              valueColor: c.textHi,
              mono: true,
            ),
            // Идентификатор корневого ключа отсутствует у оператора, который не
            // проводил церемонию ключей, и на живой панели сейчас именно так.
            // Строку не прячем: «нет» с названной причиной честнее, чем пустое
            // место, по которому не отличить «нет» от «не показали».
            CRow(
              icon: Lucide.key,
              label: 'Корневой ключ',
              value: link.rootKeyIdHex ?? 'не передан',
              mono: true,
            ),
          ],
        ),
        const SizedBox(height: AppSpace.s2),
        Text(
          link.hasRootKeyId
              ? 'Ссылка называет идентификатор корневого ключа оператора. Сам '
                    'ключ приложение проверит отдельно, когда получит его от '
                    'панели.'
              : 'Оператор не включал подпись документов, поэтому идентификатора '
                    'ключа в ссылке нет. Это обычное состояние, а не сбой.',
          style: AppType.bodySm.copyWith(color: c.textLow),
        ),
        const SizedBox(height: AppSpace.s3),
        // Объяснение живёт под кнопкой вместе с адресом: оно нужно ровно тому,
        // кто адрес раскрыл, и ровно в ту секунду, когда он его сверяет. В
        // свёрнутом виде это был бы совет проверить то, чего на экране нет.
        const InlineBanner(
          glyph: Lucide.lock,
          text:
              'Адрес проверяется: приложение установит к нему TLS-соединение, и '
              'сертификат обязан принадлежать именно этому домену. Имя не '
              'проверяет никто — это текст внутри ссылки. Сверяйте адрес, а не '
              'имя.',
        ),
      ],
      const SizedBox(height: AppSpace.s4),
      // А это видно всегда: сказанное здесь не про сверку адреса, а про саму
      // ссылку, и касается каждого, кто её получил, — включая тех, кто адрес
      // так и не раскрыл.
      const InlineBanner(
        glyph: Lucide.key,
        text:
            'Ссылка не зашифрована и не подписана: кто её получил, тот и '
            'подключится, поэтому не пересылайте её. Если ссылку прислал не '
            'ваш оператор, откройте адрес панели и сверьте его.',
      ),
      const SizedBox(height: AppSpace.s5),
      FilledButton(
        onPressed: () => ref.read(connectProvider.notifier).confirm(),
        child: const Text('Подключить'),
      ),
      const SizedBox(height: AppSpace.s2),
      GhostButton(label: 'Отмена', onPressed: _close),
    ];
  }

  // ---------------------------------------------------------------------------
  // Ссылка не принята. «Всё равно продолжить» здесь нет и быть не может.
  // ---------------------------------------------------------------------------

  List<Widget> _refused(BuildContext context, ConnectState s) {
    final c = context.c;
    return [
      ScreenHead(
        'Ссылка не подошла',
        trailing: IconBtn(Lucide.x, onTap: _close),
      ),
      Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          LucideIcon(Lucide.alert, color: c.danger, size: 18),
          const SizedBox(width: AppSpace.s2),
          Flexible(
            child: Text(
              s.refusalText ?? 'Ссылка не распознана.',
              style: AppType.bodyMd.copyWith(color: c.danger),
            ),
          ),
        ],
      ),
      if (s.detail != null) ...[
        const SizedBox(height: AppSpace.s3),
        Text(
          'Подробность для поддержки: ${s.detail}',
          style: AppType.monoSm.copyWith(color: c.textLow),
        ),
      ],
      const SizedBox(height: AppSpace.s6),
      GhostButton(
        label: 'Вставить другую ссылку',
        icon: Lucide.refresh,
        onPressed: () {
          _pasteController.clear();
          ref.read(connectProvider.notifier).reset();
        },
      ),
    ];
  }

  // ---------------------------------------------------------------------------
  // Панель отказала или не ответила.
  // ---------------------------------------------------------------------------

  List<Widget> _failed(BuildContext context, ConnectState s) {
    final c = context.c;
    return [
      ScreenHead(
        'Не удалось подключить',
        trailing: IconBtn(Lucide.x, onTap: _close),
      ),
      Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          LucideIcon(Lucide.alert, color: c.danger, size: 18),
          const SizedBox(width: AppSpace.s2),
          Flexible(
            child: Text(
              s.error ?? 'Панель не ответила.',
              style: AppType.bodyMd.copyWith(color: c.danger),
            ),
          ),
        ],
      ),
      const SizedBox(height: AppSpace.s6),
      FilledButton(
        onPressed: () => ref.read(connectProvider.notifier).confirm(),
        child: const Text('Повторить'),
      ),
      const SizedBox(height: AppSpace.s2),
      GhostButton(
        label: 'Вставить другую ссылку',
        icon: Lucide.refresh,
        onPressed: () {
          _pasteController.clear();
          ref.read(connectProvider.notifier).reset();
        },
      ),
    ];
  }

  // ---------------------------------------------------------------------------
  // Готово.
  // ---------------------------------------------------------------------------

  List<Widget> _done(BuildContext context, ConnectState s) {
    final c = context.c;
    final r = s.result;
    final problem = r?.subscriptionReasonText;
    return [
      Row(
        children: [
          // Завершение подключения это не статус туннеля: нейтральный textHi,
          // не c.success.
          LucideIcon(Lucide.check, color: c.textHi, size: 26),
          const SizedBox(width: AppSpace.s3),
          Expanded(
            child: Text(
              'Панель подключена',
              style: AppType.headline.copyWith(color: c.textHi),
            ),
          ),
        ],
      ),
      const SizedBox(height: AppSpace.s4),
      RowsGroup(
        children: [
          // СТРОКИ «АДРЕС ПАНЕЛИ» ЗДЕСЬ НЕТ НАМЕРЕННО. На подтверждении она
          // была решением, тут была бы копией: адрес тот же, что человек уже
          // сверил, а отменить по нему нечего — привязка состоялась. Что
          // изменилось за этот шаг, тем и отчитываемся: имя пришло от панели по
          // TLS вместо заявленного ссылкой, и появился статус подписки.
          // Подробнее — в шапке файла.
          //
          // Здесь имя уже пришло ОТ ПАНЕЛИ по TLS (`r.panelName`), и это другое
          // по природе значение, чем то же имя из ссылки. Но откат на имя из
          // ссылки остаётся, а вместе с ним и его происхождение, поэтому
          // ограничение одной строкой стоит и тут.
          CRow(
            icon: Lucide.user,
            label: (r?.panelName.isNotEmpty ?? false)
                ? 'Оператор'
                : 'Имя из ссылки',
            trailing: _OneLineValue(
              (r?.panelName.isNotEmpty ?? false)
                  ? r!.panelName
                  : (s.link?.operatorName ?? ''),
            ),
          ),
          CRow(
            icon: Lucide.layers,
            label: 'Подписка',
            value: r?.subscriptionStatus ?? (problem == null ? '' : 'нет'),
            mono: true,
          ),
        ],
      ),
      if (problem != null) ...[
        const SizedBox(height: AppSpace.s4),
        InlineBanner(tone: BannerTone.warning, text: problem),
        const SizedBox(height: AppSpace.s5),
        FilledButton(
          onPressed: _finish,
          child: const Text('Понятно, продолжить'),
        ),
        // Единственный кадр, где у человека уже есть сессия панели, но с
        // подпиской что-то не так, — и единственный, где «посмотреть тарифы»
        // отвечает ровно на его вопрос. Каталог тянется отдельным запросом
        // этой же сессией: класть его в одноразовую offline-ссылку нельзя,
        // цены меняются, а выданная ссылка — нет.
        const SizedBox(height: AppSpace.s2),
        GhostButton(
          label: 'Посмотреть тарифы',
          icon: Lucide.creditCard,
          onPressed: _openPlans,
        ),
      ] else ...[
        const SizedBox(height: AppSpace.s5),
        const InlineLoading(top: AppSpace.s4),
      ],
    ];
  }

  List<Widget> _busy(BuildContext context, String label) {
    final c = context.c;
    return [
      const SizedBox(height: AppSpace.s8),
      const InlineLoading(),
      const SizedBox(height: AppSpace.s4),
      Text(
        label,
        textAlign: TextAlign.center,
        style: AppType.bodyMd.copyWith(color: c.textMed),
      ),
    ];
  }

  // ---------------------------------------------------------------------------
  // Действия.
  // ---------------------------------------------------------------------------

  Future<void> _pasteFromClipboard() async {
    final data = await Clipboard.getData('text/plain');
    final text = data?.text?.trim() ?? '';
    if (!mounted) return;
    if (text.isEmpty) {
      showCarambaToast(context, 'В буфере пусто');
      return;
    }
    _pasteController.text = text;
    setState(() {});
  }

  Future<void> _scanQr() async {
    final value = await showQrScanSheet(context);
    if (value == null || !mounted) return;
    _pasteController.text = value;
    ref.read(connectProvider.notifier).open(value);
  }

  void _finish() {
    ref.read(connectProvider.notifier).finish();
    context.go(AppRoute.home);
  }

  /// Витрина тарифов вместо «Подключения». Экран накладной (лежит поверх
  /// шелла): сначала снимаем себя, иначе витрина легла бы поверх уже
  /// закрытого потока и «Назад» вернуло бы на него.
  void _openPlans() {
    ref.read(connectProvider.notifier).finish();
    final router = GoRouter.of(context);
    if (router.canPop()) router.pop();
    router.go(AppRoute.plans);
  }

  void _close() {
    ref.read(connectProvider.notifier).reset();
    if (context.canPop()) {
      context.pop();
    } else {
      // Экран накладной; без стека под ним (диплинк холодного старта) уходим
      // на «Подключение».
      context.go(AppRoute.home);
    }
  }

  /// Сколько осталось жить приглашению. Округляем вниз до минуты: секундный
  /// счётчик на экране подтверждения только торопит, а решение он не меняет.
  String _remainingText(Duration left) {
    if (left.inSeconds <= 0) return 'истекло';
    if (left.inMinutes < 1) return 'меньше минуты';
    if (left.inMinutes < 60) return '${left.inMinutes} мин';
    return '${left.inHours} ч';
  }
}

/// Значение строки, которое физически не может стать выше одной строки.
///
/// Существует ради полей, ПРИШЕДШИХ ИЗВНЕ. `CRow.value` рисуется `Text` без
/// `maxLines`: многоточие там ограничивает ширину, но не высоту, поэтому
/// значение с переводом строки разворачивается в несколько строк, выровненных
/// по правому краю ровно там, где стоят значения настоящих строк экрана. Одно
/// поле превращается в поддельную строку «Адрес панели …».
///
/// Разбор ссылки такие символы теперь отвергает (`connect_link.dart`), и это
/// главная защита. Эта — вторая: она не зависит от того, какое поле и из какого
/// источника сюда попадёт завтра.
class _OneLineValue extends StatelessWidget {
  const _OneLineValue(this.text);

  final String text;

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    // Flexible, а не Expanded: CRow ставит значение прямо в свой Row рядом с
    // Expanded(label), и жёсткий Expanded отобрал бы у подписи половину строки
    // даже под короткое имя.
    return Flexible(
      child: Text(
        text,
        maxLines: 1,
        softWrap: false,
        overflow: TextOverflow.ellipsis,
        textAlign: TextAlign.right,
        style: AppType.bodyMd.copyWith(color: c.textHi),
      ),
    );
  }
}

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/features/enroll/enroll_controller.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Экран энроллмента (P2, contract A/B/C).
///
/// Точка входа из deeplink `carambaconnect://enroll?panel=...&code=...`, из
/// ручного ввода (код + URL панели) и из QR (тот же URI). Сначала валидирует
/// код публичным `GET /api/v2/app/enroll/{code}`, показывает имя панели и
/// разовый онбординг-трафик, затем ведёт в регистрацию (email/password) или
/// вход по коду из бота, прокидывая `enroll_code`. Аккаунт обязателен всегда.
class EnrollScreen extends ConsumerStatefulWidget {
  /// URL панели из deeplink (query `panel`). `null` => ручной ввод.
  final String? initialPanel;

  /// Инвайт-код из deeplink (query `code`). `null` => ручной ввод.
  final String? initialCode;

  const EnrollScreen({this.initialPanel, this.initialCode, super.key});

  @override
  ConsumerState<EnrollScreen> createState() => _EnrollScreenState();
}

class _EnrollScreenState extends ConsumerState<EnrollScreen> {
  final _panelController = TextEditingController();
  final _codeController = TextEditingController();

  // Поля регистрации.
  final _emailController = TextEditingController();
  final _passwordController = TextEditingController();
  final _nameController = TextEditingController();

  // Поле входа по коду из бота (альтернатива регистрации).
  final _botCodeController = TextEditingController();

  @override
  void initState() {
    super.initState();
    _panelController.text = widget.initialPanel ?? '';
    _codeController.text = widget.initialCode ?? '';
    // Если deeplink принёс обе части — стартуем валидацию сразу после первого кадра.
    final panel = widget.initialPanel;
    final code = widget.initialCode;
    if (panel != null && panel.isNotEmpty && code != null && code.isNotEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) {
          ref
              .read(enrollProvider.notifier)
              .submitManual(panelUrl: panel, code: code);
        }
      });
    }
  }

  @override
  void dispose() {
    _panelController.dispose();
    _codeController.dispose();
    _emailController.dispose();
    _passwordController.dispose();
    _nameController.dispose();
    _botCodeController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final s = ref.watch(enrollProvider);

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
              children: [
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        kBrandName,
                        style: AppType.titleMd.copyWith(color: c.textHi),
                      ),
                    ),
                    IconBtn(Lucide.x, onTap: _close),
                  ],
                ),
                const SizedBox(height: AppSpace.s6),
                ..._body(context, s),
              ],
            ),
          ),
        ),
      ),
    );
  }

  List<Widget> _body(BuildContext context, EnrollState s) {
    switch (s.stage) {
      case EnrollStage.needInput:
        return _manualInput(context, s);
      case EnrollStage.validating:
        return _busy('Проверяем код энроллмента');
      case EnrollStage.invalid:
        return _invalid(context, s);
      case EnrollStage.valid:
        return _accountStep(context, s);
      case EnrollStage.submitting:
        return _busy('Создаём аккаунт');
      case EnrollStage.done:
        return _done(context, s);
    }
  }

  // --------------------------------------------------------------------------
  // Ручной ввод (код + URL панели) + QR-аффорданс.
  // --------------------------------------------------------------------------

  List<Widget> _manualInput(BuildContext context, EnrollState s) {
    final c = context.c;
    return [
      Text('Энроллмент', style: AppType.headline.copyWith(color: c.textHi)),
      const SizedBox(height: AppSpace.s3),
      Text(
        'Введите инвайт-код и адрес панели, либо откройте ссылку приглашения. '
        'Для входа понадобится аккаунт.',
        style: AppType.bodyMd.copyWith(color: c.textMed),
      ),
      const SizedBox(height: AppSpace.s6),
      SectionTitle('Код', padding: const EdgeInsets.only(bottom: AppSpace.s3)),
      TextField(
        controller: _codeController,
        style: AppType.monoMd.copyWith(color: c.textHi),
        decoration: const InputDecoration(hintText: 'INVITE-XXXX'),
      ),
      const SizedBox(height: AppSpace.s5),
      SectionTitle(
        'URL панели',
        padding: const EdgeInsets.only(bottom: AppSpace.s3),
      ),
      TextField(
        controller: _panelController,
        keyboardType: TextInputType.url,
        autocorrect: false,
        style: AppType.monoMd.copyWith(color: c.textHi),
        decoration: const InputDecoration(hintText: 'https://panel.example'),
      ),
      const SizedBox(height: AppSpace.s3),
      GhostButton(
        label: 'Сканировать QR',
        icon: Lucide.appWindow,
        onPressed: _scanQrStub,
      ),
      if (s.error != null) ...[
        const SizedBox(height: AppSpace.s4),
        _errorLine(context, s.error!),
      ],
      const SizedBox(height: AppSpace.s6),
      FilledButton(onPressed: _submitManual, child: const Text('Продолжить')),
    ];
  }

  // --------------------------------------------------------------------------
  // Невалидный код.
  // --------------------------------------------------------------------------

  List<Widget> _invalid(BuildContext context, EnrollState s) {
    final c = context.c;
    return [
      Text('Код не подошёл', style: AppType.headline.copyWith(color: c.textHi)),
      const SizedBox(height: AppSpace.s4),
      _errorLine(context, s.error ?? 'Код недействителен.'),
      const SizedBox(height: AppSpace.s6),
      GhostButton(
        label: 'Ввести другой код',
        icon: Lucide.refresh,
        onPressed: () => ref.read(enrollProvider.notifier).reset(),
      ),
    ];
  }

  // --------------------------------------------------------------------------
  // Валиден: имя панели + онбординг-трафик + регистрация / вход по коду.
  // --------------------------------------------------------------------------

  List<Widget> _accountStep(BuildContext context, EnrollState s) {
    final c = context.c;
    final v = s.validation;
    final panelName = (v?.panelName != null && v!.panelName!.isNotEmpty)
        ? v.panelName!
        : kBrandName;

    return [
      Text(
        'Создать аккаунт',
        style: AppType.headline.copyWith(color: c.textHi),
      ),
      const SizedBox(height: AppSpace.s3),
      Text(
        'Панель проверена. Заведите аккаунт, чтобы подключиться.',
        style: AppType.bodyMd.copyWith(color: c.textMed),
      ),
      const SizedBox(height: AppSpace.s5),
      RowsGroup(
        children: [
          CRow(icon: Lucide.shield, label: 'Панель', value: panelName),
          if (s.onboardingTrafficMb > 0)
            // Количество трафика — техническое значение, не статус подключения:
            // mono + обычный textHi, без c.success (anti-slop: color = status only).
            CRow(
              icon: Lucide.gift,
              label: 'Стартовый трафик',
              value: _formatMb(s.onboardingTrafficMb),
              valueColor: c.textHi,
              mono: true,
            ),
        ],
      ),
      if (s.onboardingTrafficMb > 0) ...[
        const SizedBox(height: AppSpace.s2),
        Text(
          'Новый неоплаченный аккаунт получит ${_formatMb(s.onboardingTrafficMb)} '
          'стартового трафика один раз.',
          style: AppType.bodySm.copyWith(color: c.textLow),
        ),
      ],
      const SizedBox(height: AppSpace.s6),

      // Регистрация по email/password (свежий аккаунт расходует enroll-код).
      SectionTitle(
        'Регистрация',
        padding: const EdgeInsets.only(bottom: AppSpace.s3),
      ),
      TextField(
        controller: _emailController,
        keyboardType: TextInputType.emailAddress,
        autocorrect: false,
        style: AppType.bodyMd.copyWith(color: c.textHi),
        decoration: const InputDecoration(hintText: 'Email'),
      ),
      const SizedBox(height: AppSpace.s3),
      TextField(
        controller: _passwordController,
        obscureText: true,
        style: AppType.bodyMd.copyWith(color: c.textHi),
        decoration: const InputDecoration(hintText: 'Пароль'),
      ),
      const SizedBox(height: AppSpace.s3),
      TextField(
        controller: _nameController,
        style: AppType.bodyMd.copyWith(color: c.textHi),
        decoration: const InputDecoration(hintText: 'Имя (необязательно)'),
      ),
      if (s.error != null) ...[
        const SizedBox(height: AppSpace.s4),
        _errorLine(context, s.error!),
      ],
      const SizedBox(height: AppSpace.s5),
      FilledButton(onPressed: _register, child: const Text('Создать аккаунт')),

      const SizedBox(height: AppSpace.s6),
      SectionTitle(
        'Уже есть аккаунт',
        padding: const EdgeInsets.only(bottom: AppSpace.s3),
      ),
      Text(
        'Войдите по коду из Telegram-бота этой панели.',
        style: AppType.bodySm.copyWith(color: c.textMed),
      ),
      const SizedBox(height: AppSpace.s3),
      TextField(
        controller: _botCodeController,
        keyboardType: TextInputType.number,
        style: AppType.monoMd.copyWith(color: c.textHi),
        decoration: const InputDecoration(hintText: 'Код из бота (6 цифр)'),
      ),
      const SizedBox(height: AppSpace.s3),
      GhostButton(
        label: 'Войти по коду',
        icon: Lucide.key,
        onPressed: _loginByCode,
      ),
    ];
  }

  // --------------------------------------------------------------------------
  // Готово: аккаунт заведён, показываем онбординг-трафик (contract 3).
  // --------------------------------------------------------------------------

  List<Widget> _done(BuildContext context, EnrollState s) {
    final c = context.c;
    return [
      Row(
        children: [
          // Завершение энроллмента — не статус подключения: нейтральный textHi,
          // а не c.success (anti-slop: color = status only).
          LucideIcon(Lucide.check, color: c.textHi, size: 26),
          const SizedBox(width: AppSpace.s3),
          Expanded(
            child: Text(
              'Аккаунт готов',
              style: AppType.headline.copyWith(color: c.textHi),
            ),
          ),
        ],
      ),
      const SizedBox(height: AppSpace.s4),
      if (s.onboardingTrafficMb > 0)
        Row(
          children: [
            // Иконка подарка — декоративный глиф, не статус: нейтральный textMed.
            LucideIcon(Lucide.gift, color: c.textMed, size: 18),
            const SizedBox(width: AppSpace.s2),
            Flexible(
              child: Text(
                'Начислено ${_formatMb(s.onboardingTrafficMb)} стартового трафика.',
                style: AppType.bodyMd.copyWith(color: c.textMed),
              ),
            ),
          ],
        )
      else
        Text(
          'Можно подключаться.',
          style: AppType.bodyMd.copyWith(color: c.textMed),
        ),
      const SizedBox(height: AppSpace.s6),
      const InlineLoading(top: AppSpace.s4),
    ];
  }

  // --------------------------------------------------------------------------
  // Общие куски.
  // --------------------------------------------------------------------------

  List<Widget> _busy(String label) {
    return [
      const SizedBox(height: AppSpace.s8),
      const InlineLoading(),
      const SizedBox(height: AppSpace.s4),
      Builder(
        builder: (context) {
          final c = context.c;
          return Text(
            label,
            textAlign: TextAlign.center,
            style: AppType.bodyMd.copyWith(color: c.textMed),
          );
        },
      ),
    ];
  }

  Widget _errorLine(BuildContext context, String message) {
    final c = context.c;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        LucideIcon(Lucide.alert, color: c.danger, size: 16),
        const SizedBox(width: AppSpace.s2),
        Flexible(
          child: Text(message, style: AppType.bodySm.copyWith(color: c.danger)),
        ),
      ],
    );
  }

  void _submitManual() {
    FocusScope.of(context).unfocus();
    ref
        .read(enrollProvider.notifier)
        .submitManual(
          panelUrl: _panelController.text,
          code: _codeController.text,
        );
  }

  void _register() {
    final email = _emailController.text.trim();
    final password = _passwordController.text;
    if (email.isEmpty || password.isEmpty) {
      showCarambaToast(context, 'Введите email и пароль');
      return;
    }
    FocusScope.of(context).unfocus();
    ref
        .read(enrollProvider.notifier)
        .registerWithEnroll(
          email: email,
          password: password,
          fullName: _nameController.text.trim(),
        );
  }

  void _loginByCode() {
    final code = _botCodeController.text.replaceAll(RegExp(r'\D'), '');
    if (code.length < 6) {
      showCarambaToast(context, 'Введите 6 цифр кода из бота');
      return;
    }
    FocusScope.of(context).unfocus();
    ref.read(enrollProvider.notifier).loginCodeWithEnroll(botCode: code);
  }

  void _scanQrStub() {
    showCarambaToast(
      context,
      'Сканирование QR появится позже. Пока введите код и URL панели вручную.',
    );
  }

  void _close() {
    ref.read(enrollProvider.notifier).reset();
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.login);
    }
  }

  /// Форматирует МБ в строку (МБ/ГБ), mono-дружелюбно. Технические числа.
  String _formatMb(int mb) {
    if (mb >= 1024 && mb % 1024 == 0) return '${mb ~/ 1024} ГБ';
    if (mb >= 1024) return '${(mb / 1024).toStringAsFixed(1)} ГБ';
    return '$mb МБ';
  }
}

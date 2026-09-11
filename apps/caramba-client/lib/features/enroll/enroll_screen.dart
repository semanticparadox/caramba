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

/// Экран энроллмента по диплинку `carambaconnect://enroll?panel=...&code=...`.
///
/// РУЧНОГО ВВОДА ЗДЕСЬ БОЛЬШЕ НЕТ (раунд 5). Экран держал два поля — инвайт-код
/// и URL панели, — и оба противоречат тому, ради чего всё делалось: адрес
/// панели приложение не показывает и не спрашивает, а код без адреса никуда не
/// ведёт. Практически это был экран, который никто не мог пройти: кодов на
/// живой панели не выпускали, а адрес человек взять неоткуда. Вместе с полями
/// ушли QR-сканер (он жил ради тех же полей; QR со ссылкой подключения
/// сканирует экран `/connect`) и вход 6-значным кодом из бота — режим кода
/// удалён целиком, вплоть до `POST /login/code` на панели.
///
/// Что осталось: ссылка энроллмента приносит и адрес, и код сама, экран их
/// валидирует публичным `GET /api/v2/app/enroll/{code}`, показывает имя панели
/// и разовый онбординг-трафик и ведёт в регистрацию. Аккаунт обязателен всегда.
/// Пришедшему сюда без ссылки экран говорит, где её берут, и уводит на
/// «Подключить панель».
class EnrollScreen extends ConsumerStatefulWidget {
  /// URL панели из deeplink (query `panel`). `null` => ссылки не было.
  final String? initialPanel;

  /// `link_pin` из ссылки энроллмента (query `k`), когда она его несёт.
  /// Закрепляется профилем при успешной валидации.
  final String? initialLinkPin;

  /// Инвайт-код из deeplink (query `code`). `null` => ссылки не было.
  final String? initialCode;

  const EnrollScreen({
    this.initialPanel,
    this.initialCode,
    this.initialLinkPin,
    super.key,
  });

  @override
  ConsumerState<EnrollScreen> createState() => _EnrollScreenState();
}

class _EnrollScreenState extends ConsumerState<EnrollScreen> {
  // Поля регистрации: единственный ввод, который на этом экране остался.
  final _emailController = TextEditingController();
  final _passwordController = TextEditingController();
  final _nameController = TextEditingController();

  @override
  void initState() {
    super.initState();
    // Диплинк принёс обе части — стартуем валидацию сразу после первого кадра.
    final panel = widget.initialPanel;
    final code = widget.initialCode;
    if (panel != null && panel.isNotEmpty && code != null && code.isNotEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) {
          ref
              .read(enrollProvider.notifier)
              .submitManual(
                panelUrl: panel,
                code: code,
                linkPin: widget.initialLinkPin,
              );
        }
      });
    }
  }

  @override
  void dispose() {
    _emailController.dispose();
    _passwordController.dispose();
    _nameController.dispose();
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
        return _noLink(context, s);
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
  // Ссылки нет: говорим, где её берут, и отдаём единственной кнопке.
  // --------------------------------------------------------------------------

  List<Widget> _noLink(BuildContext context, EnrollState s) {
    final c = context.c;
    return [
      Text('Нужна ссылка', style: AppType.headline.copyWith(color: c.textHi)),
      const SizedBox(height: AppSpace.s3),
      Text(
        'Подключение выдаёт бот вашего оператора одной ссылкой: её достаточно '
        'открыть или вставить, вводить ничего не нужно.',
        style: AppType.bodyMd.copyWith(color: c.textMed),
      ),
      if (s.error != null) ...[
        const SizedBox(height: AppSpace.s4),
        _errorLine(context, s.error!),
      ],
      const SizedBox(height: AppSpace.s6),
      FilledButton(
        onPressed: () => context.go(AppRoute.connect),
        child: const Text('Вставить ссылку подключения'),
      ),
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
      // Ввести «другой код» тут нечем: код приходит ссылкой. Просить у
      // оператора новую ссылку и есть единственный осмысленный следующий шаг.
      GhostButton(
        label: 'Вставить другую ссылку',
        icon: Lucide.refresh,
        onPressed: () {
          ref.read(enrollProvider.notifier).reset();
          context.go(AppRoute.connect);
        },
      ),
    ];
  }

  // --------------------------------------------------------------------------
  // Валиден: имя панели + онбординг-трафик + регистрация.
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
      const SectionTitle(
        'Регистрация',
        padding: EdgeInsets.only(bottom: AppSpace.s3),
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

  void _close() {
    ref.read(enrollProvider.notifier).reset();
    // Экран накладной (T1): под ним обычно шелл, туда и возвращаемся. Стека
    // может не быть — холодный старт по ссылке, — тогда уходим на
    // «Подключение», а не на «Аккаунт панели»: человек закрывает энроллмент,
    // а не просит ещё одну дверь для входа.
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.home);
    }
  }

  /// Форматирует МБ в строку (МБ/ГБ), mono-дружелюбно. Технические числа.
  String _formatMb(int mb) {
    if (mb >= 1024 && mb % 1024 == 0) return '${mb ~/ 1024} ГБ';
    if (mb >= 1024) return '${(mb / 1024).toStringAsFixed(1)} ГБ';
    return '$mb МБ';
  }
}

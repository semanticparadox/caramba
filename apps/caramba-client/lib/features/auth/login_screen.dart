import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:url_launcher/url_launcher.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Вход по коду из Telegram-бота (демо §LOGIN).
///
/// 6 mono-боксов под цифры, кнопка «Открыть бота», inline-ошибка под полем,
/// primary «Войти». На успех роутер сам уводит на autotune/home (auth gate).
class LoginScreen extends ConsumerStatefulWidget {
  const LoginScreen({super.key});

  @override
  ConsumerState<LoginScreen> createState() => _LoginScreenState();
}

class _LoginScreenState extends ConsumerState<LoginScreen> {
  static const _len = 6;

  /// Username бота без @. Переопределяется через
  /// `--dart-define=CARAMBA_BOT_USERNAME=...` под конкретный деплой.
  static const _botUsername = String.fromEnvironment(
    'CARAMBA_BOT_USERNAME',
    defaultValue: 'exarobot',
  );

  final _controllers = List.generate(_len, (_) => TextEditingController());
  final _focus = List.generate(_len, (_) => FocusNode());
  String? _localError;

  @override
  void dispose() {
    for (final c in _controllers) {
      c.dispose();
    }
    for (final f in _focus) {
      f.dispose();
    }
    super.dispose();
  }

  String get _code => _controllers.map((c) => c.text).join();

  /// Generic-режим: аккаунт панели не нужен. Флаг ставим ДО перехода, чтобы
  /// редирект роутера уже считал пользователя допущенным в шелл и не отбросил
  /// экран импорта обратно на /login.
  void _startGuestImport() {
    ref.read(guestModeProvider.notifier).enable();
    context.go(AppRoute.connectionImport);
  }

  void _onChanged(int i, String v) {
    if (_localError != null) setState(() => _localError = null);
    // Вставка/автозаполнение целого кода.
    if (v.length > 1) {
      final digits = v.replaceAll(RegExp(r'\D'), '');
      for (var k = 0; k < _len; k++) {
        _controllers[k].text = k < digits.length ? digits[k] : '';
      }
      final next = digits.length.clamp(0, _len - 1);
      _focus[next].requestFocus();
      setState(() {});
      return;
    }
    final clean = v.replaceAll(RegExp(r'\D'), '');
    if (clean != v) {
      _controllers[i].text = clean;
      _controllers[i].selection = TextSelection.collapsed(offset: clean.length);
    }
    if (clean.isNotEmpty && i < _len - 1) {
      _focus[i + 1].requestFocus();
    }
    setState(() {});
  }

  KeyEventResult _onKey(int i, KeyEvent e) {
    if (e is KeyDownEvent &&
        e.logicalKey == LogicalKeyboardKey.backspace &&
        _controllers[i].text.isEmpty &&
        i > 0) {
      _focus[i - 1].requestFocus();
      _controllers[i - 1].clear();
      setState(() {});
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  /// Открывает бота в Telegram: сперва нативное приложение (`tg://`),
  /// иначе web-fallback (`https://t.me/...`). `start=login` подсказывает боту
  /// сразу выдать код для входа.
  Future<void> _openBot() async {
    final tgApp = Uri.parse('tg://resolve?domain=$_botUsername&start=login');
    final web = Uri.parse('https://t.me/$_botUsername?start=login');
    try {
      if (await canLaunchUrl(tgApp)) {
        await launchUrl(tgApp);
        return;
      }
      final ok = await launchUrl(web, mode: LaunchMode.externalApplication);
      if (!ok && mounted) {
        showCarambaToast(context, 'Не удалось открыть Telegram');
      }
    } catch (_) {
      if (mounted) showCarambaToast(context, 'Не удалось открыть Telegram');
    }
  }

  void _verify() {
    final code = _code;
    if (code.length < _len) {
      setState(() => _localError = 'Введите все 6 цифр кода');
      _focus[code.length.clamp(0, _len - 1)].requestFocus();
      return;
    }
    FocusScope.of(context).unfocus();
    ref.read(authProvider.notifier).loginCode(code: code);
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final auth = ref.watch(authProvider);
    final busy = auth.isBusy;
    final error = _localError ?? auth.error;

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s6,
            AppSpace.s5,
            AppSpace.s6,
          ),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 420),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  kBrandName,
                  style: AppType.titleMd.copyWith(color: c.textHi),
                ),
                const SizedBox(height: AppSpace.s6),
                Text('Вход', style: AppType.headline.copyWith(color: c.textHi)),
                const SizedBox(height: AppSpace.s3),
                Text(
                  'Откройте бота @$_botUsername в Telegram и запросите код для входа.',
                  style: AppType.bodyMd.copyWith(color: c.textMed),
                ),
                const SizedBox(height: AppSpace.s5),
                GhostButton(
                  label: 'Открыть бота',
                  icon: Lucide.send,
                  onPressed: busy ? null : _openBot,
                ),
                const SizedBox(height: AppSpace.s6),
                const SectionTitle(
                  'Код из бота',
                  padding: EdgeInsets.only(bottom: AppSpace.s3),
                ),
                Row(
                  children: [
                    for (var i = 0; i < _len; i++) ...[
                      if (i > 0) const SizedBox(width: AppSpace.s2),
                      Expanded(child: _codeBox(i, error != null)),
                    ],
                  ],
                ),
                const SizedBox(height: AppSpace.s3),
                SizedBox(
                  height: 20,
                  child: error == null
                      ? null
                      : Row(
                          children: [
                            LucideIcon(Lucide.alert, color: c.danger, size: 16),
                            const SizedBox(width: AppSpace.s2),
                            Flexible(
                              child: Text(
                                error,
                                style: AppType.bodySm.copyWith(color: c.danger),
                              ),
                            ),
                          ],
                        ),
                ),
                const SizedBox(height: AppSpace.s2),
                FilledButton(
                  onPressed: busy ? null : _verify,
                  child: busy
                      ? SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                            valueColor: AlwaysStoppedAnimation(c.textOnAccent),
                          ),
                        )
                      : const Text('Войти'),
                ),
                const SizedBox(height: AppSpace.s4),
                Text(
                  'Код действует 5 минут и подходит один раз.',
                  style: AppType.bodySm.copyWith(color: c.textMed),
                ),
                const SizedBox(height: AppSpace.s6),
                const SectionTitle(
                  'Есть инвайт-код',
                  padding: EdgeInsets.only(bottom: AppSpace.s3),
                ),
                Text(
                  'Подключаетесь к другой панели по приглашению? Введите инвайт-код.',
                  style: AppType.bodySm.copyWith(color: c.textMed),
                ),
                const SizedBox(height: AppSpace.s3),
                GhostButton(
                  label: 'Энроллмент по коду',
                  icon: Lucide.userPlus,
                  onPressed: busy ? null : () => context.go(AppRoute.enroll),
                ),
                const SizedBox(height: AppSpace.s6),
                const SectionTitle(
                  'Своя подписка',
                  padding: EdgeInsets.only(bottom: AppSpace.s3),
                ),
                Text(
                  'Аккаунт не нужен: вставьте ссылку на подписку или конфиг, и '
                  'приложение подключится по ней.',
                  style: AppType.bodySm.copyWith(color: c.textMed),
                ),
                const SizedBox(height: AppSpace.s3),
                GhostButton(
                  label: 'Импортировать подписку',
                  icon: Lucide.globe,
                  onPressed: busy ? null : _startGuestImport,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _codeBox(int i, bool hasError) {
    final c = context.c;
    return Focus(
      canRequestFocus: false,
      skipTraversal: true,
      onKeyEvent: (_, e) => _onKey(i, e),
      child: TextField(
        controller: _controllers[i],
        focusNode: _focus[i],
        keyboardType: TextInputType.number,
        textInputAction: i == _len - 1
            ? TextInputAction.done
            : TextInputAction.next,
        textAlign: TextAlign.center,
        maxLength: i == 0 ? _len : 1,
        autofillHints: const [AutofillHints.oneTimeCode],
        style: AppType.monoMd.copyWith(
          color: c.textHi,
          fontSize: 22,
          height: 1.0,
        ),
        cursorColor: c.textHi,
        onChanged: (v) => _onChanged(i, v),
        onSubmitted: (_) => _verify(),
        decoration: InputDecoration(
          counterText: '',
          isCollapsed: true,
          contentPadding: const EdgeInsets.symmetric(vertical: 18),
          filled: true,
          fillColor: c.surface1,
          enabledBorder: OutlineInputBorder(
            borderRadius: AppRadius.r12,
            borderSide: BorderSide(
              color: hasError ? c.danger : c.borderSubtle,
              width: 1,
            ),
          ),
          focusedBorder: OutlineInputBorder(
            borderRadius: AppRadius.r12,
            borderSide: BorderSide(
              color: hasError ? c.danger : c.textHi,
              width: 1.5,
            ),
          ),
          border: OutlineInputBorder(
            borderRadius: AppRadius.r12,
            borderSide: BorderSide(color: c.borderSubtle, width: 1),
          ),
        ),
      ),
    );
  }
}

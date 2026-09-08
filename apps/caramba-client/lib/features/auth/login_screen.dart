import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:url_launcher/url_launcher.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// «Аккаунт панели»: накладной экран, а не дверь в приложение.
///
/// ЧТО ЗДЕСЬ БЫЛО. Экран стоял первым и держал форму подключения: человек,
/// только что установивший приложение, упирался в поле ввода раньше, чем видел
/// хоть один экран. Строку для этого поля выдаёт оператор, и у того, кто пришёл
/// посмотреть, её просто нет — дверь оказывалась запертой снаружи.
///
/// ЧТО СТАЛО. Первым идёт шелл с пустой вкладкой «Подключение»: приложение
/// можно обойти целиком до того, как что-то подключать. Сюда приходят по своей
/// воле — из Настроек и из пустых панельных разделов, — и лежит здесь только
/// то, что относится к аккаунту панели: ссылка подключения, код приглашения и
/// вход кодом из бота. Формы одного поля тут нет: её единственный хозяин —
/// экран «Добавить подключение», и две формы означали бы два разных ответа на
/// одну и ту же вставленную строку.
///
/// БОТ. Дефолтного username здесь больше нет. Публичная сборка ни к какому
/// оператору не привязана, и вписанный в код чужой бот — это выдуманный адрес,
/// который приложение выдавало бы за адрес оператора. Ссылка берётся из
/// брендинга подключённой панели; брендированная сборка может задать её через
/// dart-define. Ничего нет — раздела нет, и это сказано словами.
class LoginScreen extends ConsumerStatefulWidget {
  const LoginScreen({super.key});

  @override
  ConsumerState<LoginScreen> createState() => _LoginScreenState();
}

class _LoginScreenState extends ConsumerState<LoginScreen> {
  /// Username бота ТЕКУЩЕЙ панели, без @. Дефолт ПУСТ намеренно (см. шапку
  /// класса). Переопределяется через `--dart-define=CARAMBA_BOT_USERNAME=...`
  /// в брендированной сборке оператора.
  static const _botUsername = String.fromEnvironment('CARAMBA_BOT_USERNAME');

  /// Ссылка на бота панели: сначала то, что отдала сама панель в брендинге,
  /// затем dart-define брендированной сборки. Ничего нет — пусто.
  String get _botLink {
    final fromPanel = ref.read(activeBrandingProvider).botUrl.trim();
    if (fromPanel.isNotEmpty) return fromPanel;
    return _botUsername.isEmpty ? '' : 'https://t.me/$_botUsername';
  }

  /// Экран накладной: крестик возвращает туда, откуда пришли. Стека под нами
  /// может не быть (холодный старт по ссылке) — тогда уходим на «Подключение»,
  /// чтобы закрытие никогда не упиралось в пустоту.
  void _close() {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.home);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;

    // Панель подключена только если её выбрал пользователь. Публичная сборка
    // ни к какому оператору не привязана, поэтому вход по коду из бота и сам
    // бот появляются лишь вместе с панелью.
    final profile = ref.watch(activeConnectionProfileProvider);
    final panelUrl = (profile?.panelUrl ?? '').trim();
    final hasPanel = panelUrl.isNotEmpty || kApiBaseUrl.trim().isNotEmpty;

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
              'Аккаунт панели',
              trailing: IconBtn(Lucide.x, onTap: _close),
            ),
            Text(
              'Аккаунт панели добавляет тарифы, устройства, рефералов и '
              'поддержку. Подключается ссылкой caramba://, которую выдаёт бот '
              'оператора.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s5),
            // Главный путь: ссылку `caramba://` разбирает экран подтверждения,
            // а не это место — здесь только дверь к нему.
            FilledButton(
              onPressed: () => context.go(AppRoute.connect),
              child: const Text('Вставить ссылку подключения'),
            ),
            const SizedBox(height: AppSpace.s2),
            // Ручной код приглашения — для случая, когда код продиктовали
            // голосом и ссылки на руках нет.
            GhostButton(
              label: 'У меня код приглашения',
              icon: Lucide.userPlus,
              onPressed: () => context.go(AppRoute.enroll),
            ),
            // Вход в аккаунт панели кодом из бота. Показывается только когда
            // панель уже известна: без панели ни бота, ни кодов не существует.
            if (hasPanel) ...[
              const SizedBox(height: AppSpace.s5),
              _BotCodeSection(botLink: _botLink),
            ],
          ],
        ),
      ),
    );
  }
}

/// Вход по 6-значному коду из Telegram-бота панели.
///
/// Свёрнут по умолчанию: это самый редкий из входов — им пользуется тот, у кого
/// аккаунт на панели уже есть и кто зачем-то переустановил приложение. Раскрытым
/// он занимал треть первого экрана и создавал впечатление, что код обязателен.
class _BotCodeSection extends ConsumerStatefulWidget {
  /// Ссылка на бота панели. Пустая — кнопки «Открыть бота» нет: кнопка,
  /// ведущая в никуда, хуже её отсутствия.
  final String botLink;

  const _BotCodeSection({required this.botLink});

  @override
  ConsumerState<_BotCodeSection> createState() => _BotCodeSectionState();
}

class _BotCodeSectionState extends ConsumerState<_BotCodeSection> {
  static const _len = 6;

  final _controllers = List.generate(_len, (_) => TextEditingController());
  final _focus = List.generate(_len, (_) => FocusNode());
  bool _open = false;
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
    final link = widget.botLink;
    if (link.isEmpty) return;
    final handle = link.split('/').last.split('?').first;
    final tgApp = Uri.parse('tg://resolve?domain=$handle&start=login');
    final web = Uri.parse('$link?start=login');
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

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        GhostButton(
          label: 'Войти кодом из бота',
          icon: Lucide.key,
          onPressed: () => setState(() => _open = !_open),
        ),
        if (_open) ...[
          const SizedBox(height: AppSpace.s3),
          if (widget.botLink.isNotEmpty) ...[
            Text(
              'Код для входа выдаёт бот панели. Он действует 5 минут и '
              'подходит один раз.',
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s2),
            GhostButton(
              label: 'Открыть бота',
              icon: Lucide.send,
              onPressed: busy ? null : _openBot,
            ),
          ] else
            Text(
              'Код выдаёт бот оператора. Его адрес эта панель не публикует — '
              'возьмите код там, где оформляли подписку.',
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
          const SizedBox(height: AppSpace.s3),
          Row(
            children: [
              for (var i = 0; i < _len; i++) ...[
                if (i > 0) const SizedBox(width: AppSpace.s2),
                Expanded(child: _codeBox(i, error != null)),
              ],
            ],
          ),
          if (error != null) ...[
            const SizedBox(height: AppSpace.s2),
            Row(
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
          ],
          const SizedBox(height: AppSpace.s3),
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
        ],
      ],
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

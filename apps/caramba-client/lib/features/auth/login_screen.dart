import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/router/routes.dart';
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
/// воле — из Настроек и из пустых панельных разделов.
///
/// ПОЧЕМУ ЗДЕСЬ РОВНО ОДНА ДВЕРЬ (раунд 5). Способ подключения теперь один:
/// ссылка `caramba://`, которую выдаёт бот оператора. Вход 6-значным кодом из
/// бота убран целиком (вместе с `POST /login/code` на панели), а «У меня код
/// приглашения» уехал вместе с ним: инвайт-код без ссылки требовал вписать
/// руками адрес панели, а адрес панели приложение не показывает и не
/// спрашивает — именно от этого и уходили. Маршрут `/enroll` остался для
/// диплинка `carambaconnect://enroll`, в котором и код, и адрес приходят
/// сами.
class LoginScreen extends ConsumerStatefulWidget {
  const LoginScreen({super.key});

  @override
  ConsumerState<LoginScreen> createState() => _LoginScreenState();
}

class _LoginScreenState extends ConsumerState<LoginScreen> {
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
              'оператора: её достаточно открыть или вставить, вводить ничего '
              'не нужно.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s5),
            // Главный и единственный путь: ссылку `caramba://` разбирает экран
            // подтверждения, а не это место — здесь только дверь к нему.
            FilledButton(
              onPressed: () => context.go(AppRoute.connect),
              child: const Text('Вставить ссылку подключения'),
            ),
            const SizedBox(height: AppSpace.s3),
            Text(
              'Ссылка не открылась сама? Скопируйте её в боте кнопкой '
              '«Скопировать ссылку» и вставьте здесь.',
              style: AppType.bodySm.copyWith(color: c.textLow),
            ),
          ],
        ),
      ),
    );
  }
}

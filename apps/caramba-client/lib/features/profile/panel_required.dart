/// Пустое состояние панельных разделов, пока аккаунта панели нет.
///
/// Тарифы, устройства, рефералы, партнёрский дашборд, тикеты и уведомления
/// существуют только у аккаунта панели. Сюда попадают двое: тот, кто ещё не
/// подключил ничего вообще, и тот, кто работает по своей подписке. Обоим
/// нужен один и тот же ответ, поэтому вместо 401-ошибок здесь одно действие —
/// дверь на «Аккаунт панели», где подключение и происходит.
library;

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

class PanelRequiredScreen extends StatelessWidget {
  /// Заголовок раздела, в котором пользователь оказался.
  final String title;

  const PanelRequiredScreen({required this.title, super.key});

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
            AppSpace.s20 + AppSpace.s6,
          ),
          children: [
            ScreenHead(title),
            const SizedBox(height: AppSpace.s12),
            Center(
              child: LucideIcon(Lucide.appWindow, color: c.textLow, size: 32),
            ),
            const SizedBox(height: AppSpace.s3),
            Text(
              'Аккаунта панели пока нет',
              textAlign: TextAlign.center,
              style: AppType.titleMd.copyWith(color: c.textHi),
            ),
            const SizedBox(height: AppSpace.s2),
            Text(
              'Тарифы, устройства, рефералы и поддержка появляются вместе с '
              'аккаунтом панели. Его подключает ссылка caramba:// из бота '
              'оператора.',
              textAlign: TextAlign.center,
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s5),
            // Одно действие: код приглашения и вход кодом из бота лежат там же,
            // на «Аккаунте панели», и дублировать их здесь значило бы задавать
            // вопрос «каким способом», на который человек ещё не готов отвечать.
            FilledButton(
              onPressed: () => context.go(AppRoute.login),
              child: const Text('Подключить панель'),
            ),
          ],
        ),
      ),
    );
  }
}

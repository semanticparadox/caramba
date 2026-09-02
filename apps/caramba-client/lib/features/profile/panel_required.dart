/// Пустое состояние панельных разделов в generic-режиме.
///
/// Тарифы, устройства, рефералы, партнёрский дашборд, тикеты и уведомления
/// существуют только у аккаунта панели. Пользователь со своей подпиской в них
/// заходить может, но данных там нет — вместо 401-ошибок показываем, что нужно
/// сделать, чтобы раздел заработал.
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
              'Подключите панель, чтобы видеть тарифы и устройства',
              textAlign: TextAlign.center,
              style: AppType.titleMd.copyWith(color: c.textHi),
            ),
            const SizedBox(height: AppSpace.s2),
            Text(
              'Сейчас вы работаете по своей подписке. Аккаунт панели добавляет '
              'тарифы, устройства, рефералов и поддержку.',
              textAlign: TextAlign.center,
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s5),
            FilledButton(
              onPressed: () => context.go(AppRoute.login),
              child: const Text('Войти в аккаунт'),
            ),
            const SizedBox(height: AppSpace.s2),
            GhostButton(
              label: 'Энроллмент по коду',
              icon: Lucide.userPlus,
              onPressed: () => context.go(AppRoute.enroll),
            ),
          ],
        ),
      ),
    );
  }
}

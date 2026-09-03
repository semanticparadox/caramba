/// Баннер «настройки применятся после переподключения».
///
/// Политика ядра действует со следующего `Up`, поэтому правка при поднятом
/// туннеле не даёт эффекта сразу. Приложение НЕ переподключается само: рвать
/// работающий туннель без спроса недопустимо, особенно на нестабильной сети.
/// Баннер показывается, пока [reconnectRequiredProvider] истинен, и предлагает
/// переподключиться одним действием.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';

class ReconnectBanner extends ConsumerWidget {
  const ReconnectBanner({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
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
              'Новые настройки применятся после переподключения.',
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
          ),
          const SizedBox(width: AppSpace.s3),
          // Кнопка по содержимому, а НЕ во всю ширину: в [Row] рядом с
          // [Expanded] нефлексовый ребёнок получает бесконечную ширину, и
          // `width: double.infinity` внутри него роняет разметку. Баннер
          // обязан пережить собственное появление.
          TextButton(
            style: TextButton.styleFrom(
              foregroundColor: c.textHi,
              minimumSize: const Size(0, 40),
              padding: const EdgeInsets.symmetric(horizontal: AppSpace.s3),
            ),
            onPressed: () async {
              final vpn = ref.read(vpnProvider.notifier);
              await vpn.disconnect();
              await vpn.connect();
            },
            child: const Text('Переподключить'),
          ),
        ],
      ),
    );
  }
}

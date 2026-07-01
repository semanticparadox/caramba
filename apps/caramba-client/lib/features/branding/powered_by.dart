import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/data/models/branding.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';

/// Сдержанный powered-by / upsell блок (P3, contract E).
///
/// Видим ТОЛЬКО когда `branding.upstreamAds == true` (Free-тир или бренд не
/// настроен). Это не маркетинговый баннер: один нейтральный card на surface1,
/// строка «Powered by Caramba Connect» с Lucide-иконкой и одна тихая ссылка.
///
/// АНТИ-СЛОП: нейтральная плашка, textMed/textLow, без акцента, без статус-цвета,
/// без градиента/свечения/эмодзи; копия простая, без em-dash и слоп-слов.
///
/// Сам себя прячет при `upstreamAds == false`, поэтому его можно безусловно
/// ставить на login/home/settings; на Pro c брендом он исчезнет.
class PoweredBy extends ConsumerWidget {
  /// Внешние отступы блока (экран сам решает зазоры).
  final EdgeInsets padding;

  const PoweredBy({
    this.padding = const EdgeInsets.symmetric(vertical: AppSpace.s4),
    super.key,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final branding = ref.watch(activeBrandingProvider);
    if (!branding.upstreamAds) return const SizedBox.shrink();

    final c = context.c;
    return Padding(
      padding: padding,
      child: Container(
        width: double.infinity,
        padding: const EdgeInsets.all(AppSpace.s4),
        decoration: BoxDecoration(
          color: c.surface1,
          borderRadius: AppRadius.r16,
          border: Border.all(color: c.borderSubtle),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Нейтральная иконка щита (защита), НЕ статус-цвет.
            LucideIcon(Lucide.shield, color: c.textMed, size: 18),
            const SizedBox(width: AppSpace.s3),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Powered by $kBrandName',
                    style: AppType.bodySm.copyWith(color: c.textHi),
                  ),
                  const SizedBox(height: AppSpace.s1),
                  Text(
                    'Run your own VPN service on Caramba Connect.',
                    style: AppType.caption.copyWith(
                      color: c.textLow,
                      letterSpacing: 0,
                      height: 1.4,
                    ),
                  ),
                  const SizedBox(height: AppSpace.s2),
                  _LearnMoreLink(branding: branding),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Тихая ссылка «Learn more». Ведёт на support-URL оператора, если он задан,
/// иначе на платформенный bot-URL. Если оба пусты, ссылка скрыта (текст
/// остаётся информативным сам по себе). Нейтральный цвет, без акцента.
class _LearnMoreLink extends StatelessWidget {
  final Branding branding;
  const _LearnMoreLink({required this.branding});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final target = branding.hasSupport
        ? branding.supportUrl
        : (branding.hasBot ? branding.botUrl : kBrandBotUrl);
    if (target.trim().isEmpty) return const SizedBox.shrink();

    return InkWell(
      onTap: () => _open(target),
      borderRadius: AppRadius.r8,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: AppSpace.s1),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              'Learn more',
              style: AppType.caption.copyWith(
                color: c.textMed,
                letterSpacing: 0,
              ),
            ),
            const SizedBox(width: AppSpace.s1),
            LucideIcon(Lucide.externalLink, color: c.textMed, size: 13),
          ],
        ),
      ),
    );
  }

  Future<void> _open(String url) async {
    final uri = Uri.tryParse(url.trim());
    if (uri == null) return;
    if (await canLaunchUrl(uri)) {
      await launchUrl(uri, mode: LaunchMode.externalApplication);
    }
  }
}

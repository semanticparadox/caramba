import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';

/// Active operator logo/name, or the bundled Caramba Connect mark.
/// Custom tenant branding keeps its own logo and text fallback.
class BrandWordmark extends ConsumerWidget {
  /// Высота строки логотипа/текста.
  final double height;

  /// Стиль текстового вордмарка (по умолчанию titleLg/textHi).
  final TextStyle? textStyle;

  const BrandWordmark({this.height = 28, this.textStyle, super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final branding = ref.watch(activeBrandingProvider);
    final c = context.c;
    final name = branding.displayName(kBrandName);
    final style = textStyle ?? AppType.titleLg.copyWith(color: c.textHi);

    if (branding.enabled && branding.hasLogo) {
      return Image.network(
        branding.logoUrl,
        height: height,
        fit: BoxFit.contain,
        semanticLabel: name,
        // На ошибке/пока грузится — нейтральный текстовый вордмарк, без мигания
        // на статус-цвет и без «битой картинки».
        errorBuilder: (_, __, ___) => _text(name, style),
        loadingBuilder: (ctx, child, progress) =>
            progress == null ? child : _text(name, style),
      );
    }
    final normalizedName = name.trim().toLowerCase();
    if (normalizedName != 'caramba' && normalizedName != 'caramba connect') {
      return _text(name, style);
    }
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        ClipRRect(
          borderRadius: BorderRadius.circular(height * 0.22),
          child: Image.asset(
            'assets/brand/caramba-connect.png',
            width: height,
            height: height,
            excludeFromSemantics: true,
          ),
        ),
        const SizedBox(width: 8),
        Flexible(child: _text(name, style)),
      ],
    );
  }

  Widget _text(String name, TextStyle style) =>
      Text(name, style: style, maxLines: 1, overflow: TextOverflow.ellipsis);
}

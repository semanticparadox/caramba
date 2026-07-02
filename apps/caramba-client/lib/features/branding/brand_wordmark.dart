import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';

/// Бренд-вордмарк активного инстанса (P3, contract E).
///
/// Рисует логотип оператора (`logo_url`), если он задан И бренд включён; иначе
/// текстовый вордмарк `branding.displayName(kBrandName)`. Дефолт = «Caramba
/// Connect» текстом, когда бренд выключен/не настроен.
///
/// АНТИ-СЛОП: текстовый вордмарк нейтральный (textHi), без градиента, без
/// свечения. Картинка-логотип рисуется как есть, с graceful-фолбэком на текст
/// при ошибке загрузки. Высота фиксирована, чтобы лого не распирало шапку.
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
        // На ошибке/пока грузится — нейтральный текстовый вордмарк, без мигания
        // на статус-цвет и без «битой картинки».
        errorBuilder: (_, __, ___) => _text(name, style),
        loadingBuilder: (ctx, child, progress) =>
            progress == null ? child : _text(name, style),
      );
    }
    return _text(name, style);
  }

  Widget _text(String name, TextStyle style) =>
      Text(name, style: style, maxLines: 1, overflow: TextOverflow.ellipsis);
}

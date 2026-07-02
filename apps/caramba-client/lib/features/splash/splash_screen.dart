import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/theme/colors.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';

/// Splash / session-probe gate (DESIGN.md §4 — orb idle ring).
///
/// Показывается, пока [AuthStage.unknown]: первичная проверка сессии. Не трогает
/// ни одного защищённого провайдера (servers/subscription/me) — это и есть смысл
/// сплеша: не дёргать API без токена и не мигать authed-UI на холодном старте.
/// Как только сессия резолвится, роутер уводит на `/onboarding` или `/home`.
class SplashScreen extends StatefulWidget {
  const SplashScreen({super.key});

  @override
  State<SplashScreen> createState() => _SplashScreenState();
}

class _SplashScreenState extends State<SplashScreen>
    with SingleTickerProviderStateMixin {
  late final AnimationController _loop;

  @override
  void initState() {
    super.initState();
    _loop = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 3000),
    )..repeat();
  }

  @override
  void dispose() {
    _loop.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final reduceMotion =
        MediaQuery.maybeOf(context)?.disableAnimations ?? false;
    final isDesktop =
        MediaQuery.sizeOf(context).width >= AppBreakpoints.desktop;
    final size = isDesktop ? AppOrb.diameterDesktop : AppOrb.diameterMobile;

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox(
              width: size,
              height: size,
              child: AnimatedBuilder(
                animation: _loop,
                builder: (context, child) {
                  // Только вращающаяся комета-дуга (transform-only). Никакого
                  // breathing-пульса орба — ANTI-SLOP запрещает infinite breathing.
                  return CustomPaint(
                    painter: _SplashRingPainter(
                      colors: c,
                      t: _loop.value,
                      reduceMotion: reduceMotion,
                    ),
                    child: child,
                  );
                },
                child: Center(
                  child: LucideIcon(Lucide.shield, size: 52, color: c.textMed),
                ),
              ),
            ),
            const SizedBox(height: AppSpace.s4),
            Text(kBrandName, style: AppType.titleMd.copyWith(color: c.textHi)),
          ],
        ),
      ),
    );
  }
}

/// Лёгкий idle-ринг сплеша (бренд-кольцо без сетевых вызовов).
class _SplashRingPainter extends CustomPainter {
  final AppColors colors;
  final double t;
  final bool reduceMotion;

  _SplashRingPainter({
    required this.colors,
    required this.t,
    required this.reduceMotion,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final center = size.center(Offset.zero);
    final ringR = size.width / 2 - AppOrb.ringStroke;
    final rect = Rect.fromCircle(center: center, radius: ringR);

    // Бэкграунд-кольцо.
    canvas.drawArc(
      rect,
      0,
      2 * math.pi,
      false,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = AppOrb.ringStroke
        ..color = colors.borderStrong.withValues(alpha: 0.7),
    );

    // Вращающаяся комета-дуга (как connecting), пока резолвим сессию.
    final start = reduceMotion ? -math.pi / 2 : t * 2 * math.pi;
    const sweep = math.pi * 1.35;
    canvas.drawArc(
      rect,
      start,
      sweep,
      false,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = AppOrb.ringStroke
        ..strokeCap = StrokeCap.round
        ..shader = SweepGradient(
          startAngle: start,
          endAngle: start + sweep,
          colors: [
            colors.accent.withValues(alpha: 0),
            colors.accent,
            colors.accentVariant,
          ],
          tileMode: TileMode.decal,
        ).createShader(rect),
    );
  }

  @override
  bool shouldRepaint(covariant _SplashRingPainter old) =>
      old.t != t || old.reduceMotion != reduceMotion;
}

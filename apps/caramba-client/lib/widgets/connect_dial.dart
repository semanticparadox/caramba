import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'package:caramba_client/theme/colors.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/lucide.dart';

/// Дайл подключения (демо `.dial`).
///
/// Нейтральное кольцо, чей цвет несёт статус: borderStrong (отключено),
/// amber вращающаяся дуга (подключение), green кольцо + щит (защищено),
/// red (ошибка). Внутри — face-круг surface1 с глифом. Анимируется только
/// дуга при connecting (transform), уважает reduced-motion.
class ConnectDial extends StatefulWidget {
  final VpnStage stage;
  final String? subLabel;
  final VoidCallback onTap;

  /// Туннель поднят, но оператор доступ закрыл: подписка исчерпана, uuid из
  /// узлов убран, авторизация не проходит ни на одном прокси.
  ///
  /// Отдельный флаг, а не стадия: туннель ДЕЙСТВИТЕЛЬНО поднят, и все, кто
  /// смотрит на `isConnected` (кнопка отключения, экран серверов, баннер
  /// реконнекта), обязаны продолжать видеть сессию. Врёт здесь не стадия, а
  /// слово «Защищено»: щит утверждает, что трафик защищён, а через туннель не
  /// проходит ничего. Туннель при этом НЕ рвётся — открытый канал в обход
  /// подписки был бы хуже закрытого.
  final bool accessBlocked;

  /// Optional keys the atmosphere layer measures against: the chart's home
  /// station is the dial itself, and the boundary bottom plus the quiet lens
  /// are derived from the laid-out connect block rather than from constants.
  final GlobalKey? dialKey;
  final GlobalKey? labelKey;

  const ConnectDial({
    required this.stage,
    required this.onTap,
    this.subLabel,
    this.accessBlocked = false,
    this.dialKey,
    this.labelKey,
    super.key,
  });

  /// Живой туннель без права передавать трафик.
  bool get _blocked => accessBlocked && stage == VpnStage.connected;

  @override
  State<ConnectDial> createState() => _ConnectDialState();
}

class _ConnectDialState extends State<ConnectDial>
    with SingleTickerProviderStateMixin {
  late final AnimationController _spin;
  bool _pressed = false;

  @override
  void initState() {
    super.initState();
    _spin = AnimationController(
      vsync: this,
      duration: const Duration(seconds: 1),
    );
    _syncSpin();
  }

  @override
  void didUpdateWidget(covariant ConnectDial old) {
    super.didUpdateWidget(old);
    if (widget.stage != old.stage) _syncSpin();
  }

  void _syncSpin() {
    final busy =
        widget.stage == VpnStage.connecting ||
        widget.stage == VpnStage.reconnecting;
    if (busy) {
      if (!_spin.isAnimating) _spin.repeat();
    } else {
      _spin.stop();
    }
  }

  @override
  void dispose() {
    _spin.dispose();
    super.dispose();
  }

  ({String text, Color color}) _state(AppColors c) {
    // Слово «Защищено» — единственное утверждение этого экрана, и оно обязано
    // быть правдой. Туннель поднят, трафик не идёт — так и сказано.
    if (widget._blocked) {
      return (text: 'Подключено, но доступ закрыт', color: c.textHi);
    }
    switch (widget.stage) {
      case VpnStage.disconnected:
        return (text: 'Отключено', color: c.textHi);
      case VpnStage.connecting:
        return (text: 'Подключение…', color: c.textHi);
      case VpnStage.reconnecting:
        return (text: 'Переподключение…', color: c.textHi);
      case VpnStage.connected:
        return (text: 'Защищено', color: c.textHi);
      case VpnStage.error:
        return (text: 'Не удалось подключиться', color: c.textHi);
    }
  }

  String _glyph() => switch (widget.stage) {
    VpnStage.connected => widget._blocked ? Lucide.alert : Lucide.shield,
    VpnStage.error => Lucide.alert,
    _ => Lucide.power,
  };

  Color _faceColor(AppColors c) => switch (widget.stage) {
    // Не красный: рвать туннель не нужно и ошибки не произошло — кончился
    // доступ, и это состояние ожидания, а не поломки.
    VpnStage.connected => widget._blocked ? c.warning : c.success,
    VpnStage.error => c.danger,
    VpnStage.connecting || VpnStage.reconnecting => c.warning,
    VpnStage.disconnected => c.textMed,
  };

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final reduceMotion =
        MediaQuery.maybeOf(context)?.disableAnimations ?? false;
    final isDesktop =
        MediaQuery.sizeOf(context).width >= AppBreakpoints.desktop;
    final size = isDesktop ? AppOrb.diameterDesktop : AppOrb.diameterMobile;
    final state = _state(c);
    final faceColor = _faceColor(c);

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        GestureDetector(
          key: widget.dialKey,
          onTapDown: (_) => setState(() => _pressed = true),
          onTapUp: (_) => setState(() => _pressed = false),
          onTapCancel: () => setState(() => _pressed = false),
          onTap: widget.onTap,
          behavior: HitTestBehavior.opaque,
          child: SizedBox(
            width: size,
            height: size,
            child: Stack(
              alignment: Alignment.center,
              children: [
                AnimatedBuilder(
                  animation: _spin,
                  builder: (context, _) => CustomPaint(
                    size: Size.square(size),
                    painter: _RingPainter(
                      stage: widget.stage,
                      blocked: widget._blocked,
                      colors: c,
                      t: reduceMotion ? 0 : _spin.value,
                    ),
                  ),
                ),
                AnimatedScale(
                  scale: _pressed ? 0.98 : 1,
                  duration: AppMotion.micro,
                  child: Container(
                    width: AppOrb.faceMobile,
                    height: AppOrb.faceMobile,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: c.surface1,
                      border: Border.all(
                        color: widget._blocked
                            ? c.warning
                            : widget.stage == VpnStage.connected
                            ? c.success
                            : widget.stage == VpnStage.error
                            ? c.danger
                            : c.borderSubtle,
                      ),
                    ),
                    alignment: Alignment.center,
                    child: LucideIcon(
                      _glyph(),
                      color: faceColor,
                      size: 42,
                      strokeWidth: 1.8,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: AppSpace.s4),
        // The connect block: state label + sub-line. One live region for the
        // screen reader, and the rect the atmosphere layer clears behind.
        Semantics(
          container: true,
          liveRegion: true,
          child: Column(
            key: widget.labelKey,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                state.text,
                style: AppType.titleLg.copyWith(color: state.color),
              ),
              if (widget.subLabel != null) ...[
                const SizedBox(height: AppSpace.s1),
                Text(
                  widget.subLabel!,
                  textAlign: TextAlign.center,
                  style:
                      (widget.stage == VpnStage.connected
                              ? AppType.monoSm
                              : AppType.bodySm)
                          .copyWith(color: c.textMed),
                ),
              ],
            ],
          ),
        ),
      ],
    );
  }
}

class _RingPainter extends CustomPainter {
  final VpnStage stage;
  final bool blocked;
  final AppColors colors;
  final double t;

  _RingPainter({
    required this.stage,
    required this.colors,
    required this.t,
    this.blocked = false,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final center = size.center(Offset.zero);
    final r = size.width / 2 - AppOrb.ringStroke;
    final rect = Rect.fromCircle(center: center, radius: r);

    Color ringColor;
    switch (stage) {
      case VpnStage.connected:
        ringColor = blocked ? colors.warning : colors.success;
      case VpnStage.error:
        ringColor = colors.danger;
      case VpnStage.connecting:
      case VpnStage.reconnecting:
        ringColor = colors.warning;
      case VpnStage.disconnected:
        ringColor = colors.borderStrong;
    }

    if (stage == VpnStage.connecting || stage == VpnStage.reconnecting) {
      // Короткая вращающаяся дуга (как в демо: dash 76 из 534 -> ~14% круга).
      final paint = Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = AppOrb.ringStroke
        ..strokeCap = StrokeCap.round
        ..color = colors.warning;
      final start = t * 2 * math.pi - math.pi / 2;
      const sweep = 0.9; // радиан
      canvas.drawArc(rect, start, sweep, false, paint);
      return;
    }

    canvas.drawArc(
      rect,
      0,
      2 * math.pi,
      false,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = AppOrb.ringStroke
        ..color = ringColor,
    );
  }

  @override
  bool shouldRepaint(covariant _RingPainter old) =>
      old.t != t || old.stage != stage || old.blocked != blocked;
}

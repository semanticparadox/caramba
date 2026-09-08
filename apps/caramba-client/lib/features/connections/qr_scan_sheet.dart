/// Скан QR с подпиской (только Android/iOS).
///
/// Камера есть лишь на мобильных, поэтому вызывающий обязан проверить
/// платформу до вызова [showQrScanSheet] — на desktop экран импорта показывает
/// подсказку «вставьте текст» вместо кнопки. Возвращает содержимое первого
/// распознанного кода или `null`, если пользователь закрыл лист.
library;

import 'dart:io' show Platform;

import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Доступен ли скан QR на текущей платформе.
bool get qrScanSupported => !kIsWeb && (Platform.isAndroid || Platform.isIOS);

/// Открывает полноэкранный лист со сканером. `null` — закрыли без результата.
Future<String?> showQrScanSheet(BuildContext context) {
  if (!qrScanSupported) return Future<String?>.value();
  return showModalBottomSheet<String>(
    context: context,
    isScrollControlled: true,
    backgroundColor: context.c.bgCanvas,
    builder: (_) => const _QrScanSheet(),
  );
}

class _QrScanSheet extends StatefulWidget {
  const _QrScanSheet();

  @override
  State<_QrScanSheet> createState() => _QrScanSheetState();
}

class _QrScanSheetState extends State<_QrScanSheet> {
  final _controller = MobileScannerController(
    detectionSpeed: DetectionSpeed.noDuplicates,
    formats: const [BarcodeFormat.qrCode],
  );

  /// Лист закрывается по первому коду; флаг гасит повторные срабатывания,
  /// пока анимация закрытия ещё идёт.
  bool _handled = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _onDetect(BarcodeCapture capture) {
    if (_handled) return;
    for (final barcode in capture.barcodes) {
      final value = barcode.rawValue?.trim();
      if (value == null || value.isEmpty) continue;
      _handled = true;
      Navigator.of(context).pop(value);
      return;
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final height = MediaQuery.of(context).size.height * 0.8;

    return SizedBox(
      height: height,
      child: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                AppSpace.s5,
                AppSpace.s5,
                AppSpace.s5,
                AppSpace.s3,
              ),
              child: ScreenHead(
                'Сканирование QR',
                trailing: IconBtn(
                  Lucide.x,
                  onTap: () => Navigator.of(context).pop(),
                ),
              ),
            ),
            Expanded(
              child: ClipRRect(
                borderRadius: AppRadius.r14,
                child: MobileScanner(
                  controller: _controller,
                  onDetect: _onDetect,
                  errorBuilder: (context, error) => Center(
                    child: Padding(
                      padding: const EdgeInsets.all(AppSpace.s5),
                      child: Text(
                        'Камера недоступна. Проверьте разрешение и повторите.',
                        textAlign: TextAlign.center,
                        style: AppType.bodyMd.copyWith(color: c.textMed),
                      ),
                    ),
                  ),
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.all(AppSpace.s5),
              child: Text(
                'Наведите камеру на QR с подпиской.',
                textAlign: TextAlign.center,
                style: AppType.bodySm.copyWith(color: c.textLow),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

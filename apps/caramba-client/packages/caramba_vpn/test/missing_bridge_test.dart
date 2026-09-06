/// Отличие «моста нет в сборке» от «вызов не прошёл».
///
/// ПОЧЕМУ ЭТО ОТДЕЛЬНОЕ СВОЙСТВО. Настройки ядра (`setPolicy`, `setTunnelMode`)
/// уходят перед каждым подъёмом, и приложение по итогу решает, говорить ли
/// человеку «переподключитесь, чтобы применить». Сборка без такого моста —
/// не поломка: применённого нет и не будет, звать переподключаться незачем, и
/// баннер, поднятый по такому отказу, не погас бы никогда. Отказ живого моста
/// — наоборот: ядро работает не на том, что выбрал человек, и молчать нельзя.
///
/// Разделяет их ровно эта функция, поэтому у неё есть свой тест: перепутанные
/// стороны дают либо вечный баннер, либо вечную тишину — и обе ошибки видно
/// только на устройстве.
library;

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('канального обработчика нет вовсе — это свойство сборки', () {
    expect(isMissingCoreBridge(MissingPluginException('setPolicy')), isTrue);
  });

  test('символа нет в библиотеке — то же самое на FFI-пути', () {
    expect(
      isMissingCoreBridge(
        const CarambaCoreMissingSymbol(
          'CarambaSetPolicy',
          'libcaramba_core.dylib',
        ),
      ),
      isTrue,
    );
  });

  test('мост ответил ошибкой — это отказ здесь и сейчас', () {
    expect(
      isMissingCoreBridge(PlatformException(code: 'set_policy_failed')),
      isFalse,
    );
    expect(isMissingCoreBridge(StateError('ядро в разборке')), isFalse);
    expect(
      isMissingCoreBridge(const CarambaCoreException('engine: остановка')),
      isFalse,
    );
  });
}

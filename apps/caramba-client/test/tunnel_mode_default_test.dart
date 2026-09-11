// Способ захвата трафика по умолчанию, по платформам.
//
// Заказ владельца: на Windows по умолчанию TUN («не все знают, что такое
// прокси»). Права там даёт манифест раннера, wintun в комплекте. Linux тоже
// TUN: install.sh выдаёт CAP_NET_ADMIN, а отсутствие права называет баннер.
// macOS остаётся на прокси: без Network Extension TUN недоступен.

import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

void main() {
  test('Windows и Linux по умолчанию TUN', () {
    expect(defaultTunnelModeFor(TargetPlatform.windows), TunnelMode.tun);
    expect(defaultTunnelModeFor(TargetPlatform.linux), TunnelMode.tun);
  });

  test('мобильные по умолчанию TUN', () {
    expect(defaultTunnelModeFor(TargetPlatform.android), TunnelMode.tun);
    expect(defaultTunnelModeFor(TargetPlatform.iOS), TunnelMode.tun);
  });

  test('macOS остаётся на прокси: TUN без расширения недоступен', () {
    expect(defaultTunnelModeFor(TargetPlatform.macOS), TunnelMode.proxy);
  });

  test('инъекция платформы перебивает dart:io', () {
    expect(defaultTunnelMode(platform: TargetPlatform.windows), TunnelMode.tun);
    expect(defaultTunnelMode(platform: TargetPlatform.macOS), TunnelMode.proxy);
  });

  test('на тестовом хосте (macOS) дефолт без инъекции это прокси', () {
    // Тесты гоняются на macOS: dart:io отвечает macOS, дефолт прокси. Так же
    // ведут себя все остальные тесты, которые дефолт не подставляют.
    expect(defaultTunnelMode(), TunnelMode.proxy);
  });
}

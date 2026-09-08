import 'dart:convert';

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('CorePolicy.toJson', () {
    test('matches the ABI v2 example field for field', () {
      const policy = CorePolicy(
        protocol: 'auto',
        preset: 'ru-smart',
        relay: 'TR',
        stack: 'gvisor',
        mtu: 1280,
        ipv6: false,
        fakeIp: true,
        killSwitch: true,
        dns: CorePolicyDns(
          nameservers: <String>['https://1.1.1.1/dns-query'],
          fallback: <String>['tls://1.1.1.1:853'],
        ),
        split: CorePolicySplit(
          mode: 'bypass',
          apps: <String>['com.example.app'],
          bypassDomains: <String>['example.com'],
        ),
      );

      // Дословный пример из docs/CORE-ABI-v2.md.
      const abiExample = '''
{"protocol":"auto","preset":"ru-smart","relay":"TR","stack":"gvisor",
 "mtu":1280,"ipv6":false,"fakeIp":true,"killSwitch":true,
 "dns":{"nameservers":["https://1.1.1.1/dns-query"],"fallback":["tls://1.1.1.1:853"]},
 "split":{"mode":"bypass","apps":["com.example.app"],"bypassDomains":["example.com"]}}
''';

      expect(policy.toJson(), jsonDecode(abiExample));
    });

    test('omits null fields entirely so the core keeps current values', () {
      const policy = CorePolicy(preset: 'global');
      expect(policy.toJson(), <String, Object?>{'preset': 'global'});
      expect(CorePolicy.empty.toJson(), isEmpty);
    });

    test('serializes false and 0 rather than dropping them', () {
      const policy = CorePolicy(ipv6: false, killSwitch: false, mtu: 0);
      expect(policy.toJson(), <String, Object?>{
        'mtu': 0,
        'ipv6': false,
        'killSwitch': false,
      });
    });

    test('jsonEncodePolicy produces the wire string', () {
      const policy = CorePolicy(protocol: 'Hysteria2', relay: '');
      expect(
        jsonEncodePolicy(policy),
        '{"protocol":"Hysteria2","relay":""}',
      );
    });

    test('copyWith keeps untouched fields', () {
      const base = CorePolicy(protocol: 'auto', mtu: 1280);
      final next = base.copyWith(mtu: 1400);
      expect(next.protocol, 'auto');
      expect(next.mtu, 1400);
    });
  });

  group('TunnelMode', () {
    test('round-trips its wire value', () {
      expect(TunnelMode.tun.wire, 'tun');
      expect(TunnelMode.proxy.wire, 'proxy');
      expect(TunnelMode.fromWire('proxy'), TunnelMode.proxy);
      expect(TunnelMode.fromWire('tun'), TunnelMode.tun);
    });

    test('unknown or missing values resolve to null', () {
      expect(TunnelMode.fromWire(null), isNull);
      expect(TunnelMode.fromWire(''), isNull);
      expect(TunnelMode.fromWire('wireguard'), isNull);
    });
  });
}

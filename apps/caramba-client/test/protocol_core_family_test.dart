// Граница семейства протоколов принадлежит ЯДРУ, а не пикеру.
//
// `applyProtocol` в libs/caramba-core/profile/profile.go делает ровно две вещи:
// берёт `protocolClashType[Policy.Protocol]` и собирает url-test группу из всех
// прокси, у которых `m["type"]` равен этому типу. Уточнений формы (`reality`,
// `ws`, `grpc`) в сравнении нет вовсе.
//
// Отсюда факт, который экран обязан не скрывать: `VLESS-Reality` и `VLESS` —
// одна строка таблицы ядра (`vless`), и выбор Reality поднимает туннель хоть на
// TLS-инбаунде. Пикер же делит vless на две опции; пока эта таблица не
// изменится, счёт «сколько строк неразличимы» обязан идти по семейству ядра.
//
// Тест сторожит именно таблицу: разъедется она с `coreFamily` — и строка снова
// начнёт обещать точность, которой в ядре нет, молча.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/features/protocol/protocol_screen.dart';

const _reality = ProtocolKey(
  protocol: 'vless',
  transport: 'tcp',
  security: 'reality',
);
const _vlessWs = ProtocolKey(
  protocol: 'vless',
  transport: 'ws',
  security: 'tls',
);
const _hy2 = ProtocolKey(
  protocol: 'hysteria2',
  transport: 'udp',
  security: 'tls',
);

void main() {
  const options = ProtocolOption.defaults;

  group('coreFamily зеркалит protocolClashType ядра', () {
    test('таблица совпадает с profile.go дословно', () {
      // Скопировано из `protocolClashType` (libs/caramba-core/profile/
      // profile.go). Ключ — `Policy.Protocol`, значение — тип, с которым
      // `applyProtocol` сравнивает `m["type"]`.
      const core = <String, String>{
        'AmneziaWG': 'wireguard',
        'VLESS-Reality': 'vless',
        'VLESS': 'vless',
        'Hysteria2': 'hysteria2',
        'TUIC': 'tuic',
        'Shadowsocks': 'ss',
      };
      final mine = <String, String>{
        for (final o in options)
          if (o.id.isNotEmpty) o.id: o.coreFamily,
      };
      expect(mine, core);
    });

    test('у «Авто» семейства нет: это отказ от выбора', () {
      final auto = options.first;
      expect(auto.id, isEmpty);
      expect(auto.coreFamily, isEmpty);
    });
  });

  group('семейство строки пикера', () {
    test('Reality и vless на TLS для ядра — одно семейство', () {
      // Суть находки: два РАЗНЫХ инбаунда, один и тот же отбор в ядре.
      expect(coreFamilyForProtocol(_reality, options), 'vless');
      expect(coreFamilyForProtocol(_vlessWs, options), 'vless');
    });

    test('семейство называется так, как его знает ядро', () {
      // «VLESS», а не «VLESS · Reality»: семейства с таким именем в
      // protocolClashType не существует, и подписать им схлопывание значит
      // соврать ровно тем же способом, только словами.
      expect(
        protocolFamilyTitle(coreFamilyForProtocol(_reality, options)!),
        'VLESS',
      );
      expect(protocolFamilyTitle('ss'), 'Shadowsocks');
      expect(protocolFamilyTitle('wireguard'), 'WireGuard');
    });

    test('разные семейства не сливаются', () {
      expect(coreFamilyForProtocol(_hy2, options), 'hysteria2');
      expect(
        coreFamilyForProtocol(_hy2, options),
        isNot(coreFamilyForProtocol(_reality, options)),
      );
    });

    test('инбаунд, которого ядро попросить не умеет, семейства не имеет', () {
      // `naive` на узле 1 и `trojan`/`vmess` в чужих подписках: строка в
      // списке остаётся, но выбирать её нечем — и приписывать её к чужому
      // семейству нельзя.
      for (final key in const <ProtocolKey>[
        ProtocolKey(protocol: 'naive', transport: 'tcp', security: 'tls'),
        ProtocolKey(protocol: 'trojan', transport: 'tcp', security: 'tls'),
        ProtocolKey(protocol: 'vmess', transport: 'ws', security: 'tls'),
        ProtocolKey(protocol: 'hysteria', transport: 'udp', security: 'tls'),
      ]) {
        expect(
          coreFamilyForProtocol(key, options),
          isNull,
          reason: '${key.label} не должен получать семейство ядра',
        );
      }
    });

    test('AmneziaWG и голый wireguard ядро складывает вместе', () {
      // `protocolClashType["AmneziaWG"] == "wireguard"`, и другого
      // wireguard-семейства у ядра нет: обе формы попадут в одну url-test
      // группу.
      const awg = ProtocolKey(protocol: 'amneziawg');
      const wg = ProtocolKey(protocol: 'wireguard');
      expect(coreFamilyForProtocol(awg, options), 'wireguard');
      expect(coreFamilyForProtocol(wg, options), 'wireguard');
    });

    test('shadowsocks и ss — тоже одно семейство', () {
      expect(
        coreFamilyForProtocol(
          const ProtocolKey(protocol: 'shadowsocks'),
          options,
        ),
        'ss',
      );
      expect(
        coreFamilyForProtocol(const ProtocolKey(protocol: 'ss'), options),
        'ss',
      );
    });
  });
}

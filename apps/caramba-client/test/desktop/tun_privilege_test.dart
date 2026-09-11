// Право на TUN по /proc/self/status и разбор текста отказа ядра.
//
// Ядро при отказе в создании tun-устройства ошибку не возвращает, поэтому
// право проверяется заранее, по эффективной маске возможностей процесса.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/desktop/tun_privilege.dart';

const String _withCap = '''
Name:	caramba_client
Uid:	1000	1000	1000	1000
Gid:	1000	1000	1000	1000
CapInh:	0000000000000000
CapPrm:	0000000000001000
CapEff:	0000000000001000
CapBnd:	000001ffffffffff
''';

const String _withoutCap = '''
Name:	caramba_client
Uid:	1000	1000	1000	1000
CapInh:	0000000000000000
CapPrm:	0000000000000000
CapEff:	0000000000000000
CapBnd:	000001ffffffffff
''';

const String _root = '''
Name:	caramba_client
Uid:	0	0	0	0
CapEff:	000001ffffffffff
''';

void main() {
  group('tunPrivilegeFromProcStatus', () {
    test('cap_net_admin в эффективном наборе это право', () {
      expect(tunPrivilegeFromProcStatus(_withCap), TunPrivilege.granted);
    });

    test('пустая маска это отсутствие права', () {
      expect(tunPrivilegeFromProcStatus(_withoutCap), TunPrivilege.missing);
    });

    test('право только в разрешённом наборе не считается', () {
      const prmOnly = '''
Uid:	1000	1000	1000	1000
CapPrm:	0000000000001000
CapEff:	0000000000000000
''';
      expect(tunPrivilegeFromProcStatus(prmOnly), TunPrivilege.missing);
    });

    test('root имеет право при любой маске', () {
      expect(tunPrivilegeFromProcStatus(_root), TunPrivilege.granted);
      expect(
        tunPrivilegeFromProcStatus('Uid:\t0\t0\t0\t0\nCapEff:\t0\n'),
        TunPrivilege.granted,
      );
    });

    test('без строки CapEff и битая маска это «неизвестно»', () {
      expect(tunPrivilegeFromProcStatus('Uid:\t1000\n'), TunPrivilege.unknown);
      expect(
        tunPrivilegeFromProcStatus('Uid:\t1000\nCapEff:\tzz\n'),
        TunPrivilege.unknown,
      );
      expect(tunPrivilegeFromProcStatus(''), TunPrivilege.unknown);
    });

    test('бит 12 это именно CAP_NET_ADMIN', () {
      expect(kCapNetAdminBit, 12);
      // Соседние биты (CAP_NET_BIND_SERVICE=10, CAP_NET_RAW=13) правом не
      // считаются.
      expect(
        tunPrivilegeFromProcStatus('Uid:\t1000\nCapEff:\t0000000000002400\n'),
        TunPrivilege.missing,
      );
    });
  });

  test(
    'чтение отсутствующего файла отвечает «неизвестно», а не падает',
    () async {
      expect(
        await readLinuxTunPrivilege(path: '/nonexistent/proc/status'),
        TunPrivilege.unknown,
      );
    },
  );

  group('looksLikeTunPermissionFailure', () {
    test('узнаёт отказ в правах на tun', () {
      expect(
        looksLikeTunPermissionFailure(
          'Start TUN listening error: operation not permitted',
        ),
        isTrue,
      );
      expect(
        looksLikeTunPermissionFailure('open /dev/net/tun: permission denied'),
        isTrue,
      );
      expect(looksLikeTunPermissionFailure('wintun: access is denied'), isTrue);
    });

    test('не путает с отказом панели и прочими ошибками', () {
      expect(looksLikeTunPermissionFailure(null), isFalse);
      expect(looksLikeTunPermissionFailure(''), isFalse);
      expect(
        looksLikeTunPermissionFailure('transport: код состояния 403'),
        isFalse,
      );
      expect(
        looksLikeTunPermissionFailure('permission denied'),
        isFalse,
        reason: 'без слова tun это может быть что угодно',
      );
      expect(looksLikeTunPermissionFailure('tun: timeout'), isFalse);
    });
  });
}

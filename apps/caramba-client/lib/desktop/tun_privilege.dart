/// Есть ли у процесса право поднять TUN на десктопе.
///
/// ЗАЧЕМ ПРОВЕРЯТЬ ЗАРАНЕЕ. Ядро (mihomo) при отказе в создании tun-устройства
/// НЕ возвращает ошибку: оно пишет строку в журнал и рапортует «подключено»,
/// а трафик при этом идёт мимо туннеля. С TUN по умолчанию на Linux это
/// случится у каждого, кто запустил бинарь из архива без `install.sh`
/// (тот выдаёт `cap_net_admin`). Поэтому право проверяется ДО подключения, по
/// `/proc/self/status`, и главный экран говорит об этом словами вместо ложного
/// «Защищено».
///
/// Windows: манифест раннера требует администратора, без него процесс не
/// стартует вовсе, проверять нечего. macOS: TUN недоступен по умолчанию и не
/// предлагается. Мобильные: право даёт сама система по запросу.
library;

import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/desktop/desktop_platform.dart';

/// Номер бита CAP_NET_ADMIN в маске возможностей Linux (`linux/capability.h`).
const int kCapNetAdminBit = 12;

enum TunPrivilege {
  /// Право есть (или платформе оно не нужно).
  granted,

  /// Права нет: TUN не поднимется, ядро промолчит.
  missing,

  /// Узнать не удалось: не мешаем, но и не обещаем.
  unknown,
}

/// Разбирает `/proc/self/status` и отвечает, есть ли CAP_NET_ADMIN в
/// ЭФФЕКТИВНОМ наборе (`CapEff`). Именно эффективный: `setcap cap_net_admin+ep`
/// кладёт право туда, а разрешённый набор (`CapPrm`) без `e` ядру бесполезен.
///
/// Root узнаётся по `Uid: 0`: у него все права независимо от маски.
TunPrivilege tunPrivilegeFromProcStatus(String status) {
  String? capEff;
  String? uid;
  for (final raw in status.split('\n')) {
    final line = raw.trim();
    if (line.startsWith('CapEff:')) {
      capEff = line.substring('CapEff:'.length).trim();
    } else if (line.startsWith('Uid:')) {
      uid = line.substring('Uid:'.length).trim().split(RegExp(r'\s+')).first;
    }
  }
  if (uid == '0') return TunPrivilege.granted;
  if (capEff == null) return TunPrivilege.unknown;
  final mask = BigInt.tryParse(capEff, radix: 16);
  if (mask == null) return TunPrivilege.unknown;
  final has = (mask >> kCapNetAdminBit) & BigInt.one == BigInt.one;
  return has ? TunPrivilege.granted : TunPrivilege.missing;
}

/// Читает право с диска. Любой сбой чтения это [TunPrivilege.unknown]: не
/// зная, мешать подключению нельзя.
Future<TunPrivilege> readLinuxTunPrivilege({
  String path = '/proc/self/status',
}) async {
  try {
    return tunPrivilegeFromProcStatus(await File(path).readAsString());
  } catch (_) {
    return TunPrivilege.unknown;
  }
}

/// Право процесса на TUN для текущей платформы. Считается один раз за
/// процесс: права файла между запусками не меняются.
final tunPrivilegeProvider = FutureProvider<TunPrivilege>((ref) async {
  if (!isLinuxPlatform) return TunPrivilege.granted;
  return readLinuxTunPrivilege();
});

/// Похож ли текст ошибки ядра на отказ в правах на TUN.
///
/// Страховка на случай, когда ядро всё же назвало причину (другая сборка, иной
/// путь подъёма): тогда баннер должен появиться и без предварительной
/// проверки. Список слов узкий намеренно: «permission denied» на 403 от
/// панели сюда не попадает, у того текста нет слова tun.
bool looksLikeTunPermissionFailure(String? detail) {
  if (detail == null) return false;
  final low = detail.toLowerCase();
  final aboutTun = low.contains('tun') || low.contains('wintun');
  if (!aboutTun) return false;
  return low.contains('permission') ||
      low.contains('operation not permitted') ||
      low.contains('eperm') ||
      low.contains('cap_net_admin') ||
      low.contains('access is denied') ||
      low.contains('отказано в доступе');
}

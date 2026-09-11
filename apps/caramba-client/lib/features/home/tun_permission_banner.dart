/// Баннер «Нет прав на TUN» на главном экране десктопа.
///
/// ЗАЧЕМ. С TUN по умолчанию на Windows и Linux человек, у которого прав на
/// tun-устройство нет (Linux без `install.sh`), получил бы «Защищено» над
/// мёртвым туннелем: ядро отказ в правах не возвращает. Баннер называет
/// причину до подключения и даёт один клик на выход: переключиться на прокси,
/// который прав не требует.
///
/// Показывается ТОЛЬКО когда выбран TUN и право отсутствует (или ядро само
/// назвало отказ в правах в тексте ошибки). В остальных случаях виджет пуст,
/// поэтому его можно ставить в колонку баннеров безусловно.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/tun_privilege.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Текст баннера. По платформе: на Linux действие «запустить install.sh»
/// реально, на Windows права даёт манифест и отказ значит блокировку
/// адаптера (антивирус, политика).
String tunPermissionBannerText({required bool isLinux}) => isLinux
    ? 'Нет прав на TUN: запустите install.sh из архива (он выдаёт '
          'cap_net_admin) или переключитесь на прокси.'
    : 'Не удалось поднять TUN-адаптер: система или антивирус не дали '
          'создать его. Переключитесь на прокси или проверьте защиту.';

const String kTunPermissionSwitchLabel = 'Прокси';

/// Показывать ли баннер: чистое решение, чтобы его проверял тест без
/// платформы и без диска.
bool shouldShowTunPermissionBanner({
  required TunnelMode mode,
  required TunPrivilege privilege,
  required VpnStatus status,
}) {
  if (mode != TunnelMode.tun) return false;
  if (privilege == TunPrivilege.missing) return true;
  return status.stage == VpnStage.error &&
      looksLikeTunPermissionFailure(status.detail);
}

class TunPermissionBanner extends ConsumerWidget {
  const TunPermissionBanner({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (!isDesktopPlatform) return const SizedBox.shrink();
    final mode = ref.watch(tunnelModeProvider);
    final status = ref.watch(vpnProvider);
    // Пока проверка не завершилась, права считаются неизвестными: баннер о
    // недостающем праве не должен мигнуть на старте у того, у кого право есть.
    final privilege =
        ref.watch(tunPrivilegeProvider).valueOrNull ?? TunPrivilege.unknown;
    if (!shouldShowTunPermissionBanner(
      mode: mode,
      privilege: privilege,
      status: status,
    )) {
      return const SizedBox.shrink();
    }
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.only(bottom: AppSpace.s4),
      child: InlineBanner(
        tone: BannerTone.warning,
        glyph: Lucide.shield,
        text: tunPermissionBannerText(isLinux: isLinuxPlatform),
        // Кнопка по содержимому: в [Row] рядом с [Expanded] кнопка во всю
        // ширину роняет разметку (см. ReconnectBanner).
        trailing: TextButton(
          style: TextButton.styleFrom(
            foregroundColor: c.textHi,
            minimumSize: const Size(0, 40),
            padding: const EdgeInsets.symmetric(horizontal: AppSpace.s3),
          ),
          onPressed: () =>
              ref.read(tunnelModeProvider.notifier).set(TunnelMode.proxy),
          child: const Text(kTunPermissionSwitchLabel),
        ),
      ),
    );
  }
}

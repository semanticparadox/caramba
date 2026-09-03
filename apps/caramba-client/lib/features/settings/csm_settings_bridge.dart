/// Мост между пикерами настроек и словарём настроек CSM/1.
///
/// Экран настроек всегда писал индексы в [coreConfigProvider]; политика ядра
/// собирается из него, и на нём же держится баннер переподключения. С CSM/1
/// у той же правки появляется вторая половина: изменение принимается локально
/// немедленно И встаёт в очередь записи, чтобы уйти оператору по любой
/// доступной ступени (02-SPEC.md 7.8).
///
/// Поэтому каждый сеттер здесь пишет в ОБА места:
///   * [CoreConfig]: то, что уйдёт ядру на следующем `Up`;
///   * [CsmNotifier.setByUser]: то, что уйдёт оператору и переживёт
///     переустановку на втором устройстве.
///
/// Значение вне закрытого словаря во вторую половину просто не попадает:
/// `setByUser` молча его игнорирует (INV-11). Так, `stack = auto` существует в
/// UI, но не существует на проводе, и это правильно: «пусть решает платформа»
/// не то же самое, что назвать стек.
///
/// Ни одно изменение настроек не рвёт работающий туннель. Политика ядра
/// действует со следующего `Up`, приложение поднимает уже существующий баннер
/// переподключения и ждёт человека (02-SPEC.md 7.11).
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_policy_mapping.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';

/// Пишет пользовательскую правку настройки в оба конца.
class CsmSettingsBridge {
  const CsmSettingsBridge._();

  /// Профиль прошёл энроллмент CSM: вторая половина записи имеет смысл.
  static bool isActive(WidgetRef ref) =>
      ref.read(csmProfileStateProvider) != null;

  static void _push(WidgetRef ref, CsmSettingKey key, CsmSettingValue value) {
    _pushAll(ref, <(CsmSettingKey, CsmSettingValue)>[(key, value)]);
  }

  /// Пишет несколько ключей ПО ОЧЕРЕДИ.
  ///
  /// `setByOne` читает текущее состояние профиля в момент вызова и пишет его
  /// целиком, поэтому две записи, выпущенные в один кадр, читают один и тот же
  /// снимок и вторая затирает первую. Единственный ключ, у которого это
  /// проявляется в UI, это DNS: один пикер разворачивается в резолверы и
  /// запасные резолверы, и потеря второго списка означала бы молча половину
  /// применённой настройки.
  static void _pushAll(
    WidgetRef ref,
    List<(CsmSettingKey, CsmSettingValue)> writes,
  ) {
    if (ref.read(csmProfileStateProvider) == null) {
      return;
    }
    final notifier = ref.read(csmNotifierProvider);
    unawaited(() async {
      for (final w in writes) {
        await notifier.setByUser(w.$1, w.$2);
      }
    }());
  }

  /// Протокол. Индекс в [ProtocolOption.defaults]; пустой id это «Авто», а на
  /// проводе он называется `auto`.
  static void setProtocol(WidgetRef ref, int index) {
    ref.read(coreConfigProvider.notifier).setProtocol(index);
    if (index < 0 || index >= ProtocolOption.defaults.length) return;
    final id = ProtocolOption.defaults[index].id;
    _push(ref, CsmSettingKey.protocol, CsmText(id.isEmpty ? 'auto' : id));
  }

  /// Пресет маршрутизации. UI зовёт его `full`, ядро и провод `ru-full`.
  static void setRoute(WidgetRef ref, int index) {
    ref.read(coreConfigProvider.notifier).setRoute(index);
    if (index < 0 || index >= RoutingMode.defaults.length) return;
    final id = RoutingMode.defaults[index].id;
    _push(ref, CsmSettingKey.preset, CsmText(kRoutingPresetWire[id] ?? id));
  }

  /// Relay-вход. Три состояния, не два: «Авто» это пустая строка («не
  /// выбрано, оператор решает»), «Выкл» это литерал `--`, страна это её код
  /// (02-SPEC.md 7.3, Correction 15). `NO` для «без релея» не годится: это
  /// Норвегия.
  static void setRelay(WidgetRef ref, int index, List<Relay> relays) {
    ref.read(coreConfigProvider.notifier).setRelay(index);
    if (index < 0 || index >= relays.length) return;
    final relay = relays[index];
    final String wire;
    if (relay.isOff) {
      wire = kCsmNoRelay;
    } else if (relay.isAuto) {
      wire = '';
    } else {
      wire = (relay.country ?? relay.id ?? '').toUpperCase();
    }
    _push(ref, CsmSettingKey.relay, CsmText(wire));
  }

  /// Сетевой стек. `auto` в словаре провода отсутствует намеренно.
  static void setStack(WidgetRef ref, int index) {
    ref.read(coreConfigProvider.notifier).setStack(index);
    if (index < 0 || index >= CoreOption.stacks.length) return;
    _push(ref, CsmSettingKey.stack, CsmText(CoreOption.stacks[index].id));
  }

  /// MTU. `auto` это ноль, «по умолчанию ядра».
  static void setMtu(WidgetRef ref, int index) {
    ref.read(coreConfigProvider.notifier).setMtu(index);
    if (index < 0 || index >= CoreOption.mtu.length) return;
    final parsed = int.tryParse(CoreOption.mtu[index].id) ?? 0;
    _push(ref, CsmSettingKey.mtu, CsmUint(parsed));
  }

  /// DNS. Пресет пикера разворачивается в два списка резолверов; `auto`
  /// означает «не переопределять» и на провод не уходит.
  static void setDns(WidgetRef ref, int index) {
    ref.read(coreConfigProvider.notifier).setDns(index);
    if (index < 0 || index >= CoreOption.dns.length) return;
    final preset = kDnsPresets[CoreOption.dns[index].id];
    if (preset == null) return;
    // Оба списка уходят ОДНИМ вызовом _pushAll, а не двумя _push: два _push
    // порождают две независимые последовательности, читающие один снимок, и
    // защита, написанная ровно против этого, обходится собственным вызовом.
    _pushAll(ref, [
      (CsmSettingKey.dnsNameservers, CsmTextList(preset.nameservers)),
      (CsmSettingKey.dnsFallback, CsmTextList(preset.fallback)),
    ]);
  }

  static void setKillSwitch(WidgetRef ref, bool value) {
    ref.read(coreConfigProvider.notifier).setKillSwitch(value);
    _push(ref, CsmSettingKey.killSwitch, CsmBoolean(value));
  }

  static void setIpv6(WidgetRef ref, bool value) {
    ref.read(coreConfigProvider.notifier).setIpv6(value);
    _push(ref, CsmSettingKey.ipv6, CsmBoolean(value));
  }

  static void setFakeIp(WidgetRef ref, bool value) {
    ref.read(coreConfigProvider.notifier).setFakeIp(value);
    _push(ref, CsmSettingKey.fakeIp, CsmBoolean(value));
  }

  /// Режим раздельного туннелирования. Уходит ТОЛЬКО режим: сам список
  /// приложений не пересекает границу ни в одну сторону (INV-15), и у него
  /// нет ключа в реестре, поэтому отправить его нечем.
  static void setSplitMode(WidgetRef ref, SplitMode mode) {
    ref.read(coreConfigProvider.notifier).setSplitMode(mode);
    final wire = switch (mode) {
      SplitMode.off => 'off',
      SplitMode.onlySelected => 'allow',
      SplitMode.bypassSelected => 'bypass',
    };
    _push(ref, CsmSettingKey.splitMode, CsmText(wire));
  }
}

/// Метка происхождения значения рядом со строкой настройки (02-SPEC.md 7.6).
///
/// Рендерится, только когда профиль работает по CSM и по ключу есть запись:
/// на профиле без энроллмента происхождения не существует, и рисовать
/// «по умолчанию» там значило бы придумывать факт.
class CsmProvenanceTag extends ConsumerWidget {
  final CsmSettingKey settingKey;

  const CsmProvenanceTag({required this.settingKey, super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    if (ref.watch(csmProfileStateProvider) == null) {
      return const SizedBox.shrink();
    }
    final entry = ref.watch(csmSettingsProvider)[settingKey];
    if (entry == null) {
      return const SizedBox.shrink();
    }
    final operatorSet = entry.src == CsmProvenance.operator && !entry.userSet;
    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: Text(
        csmProvenanceTitle(entry.userSet ? CsmProvenance.user : entry.src),
        style: AppType.monoSm.copyWith(
          color: operatorSet ? c.warning : c.textLow,
        ),
      ),
    );
  }
}

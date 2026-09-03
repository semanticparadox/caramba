/// Перевод пользовательского выбора ([CoreConfig], индексы в списках опций) в
/// политику ядра [CorePolicy] (ABI v2 `CarambaSetPolicy`).
///
/// Единственное место, где индексы UI превращаются в строки провода. Правило:
/// «Авто» в UI означает «ядро решает само», а значит соответствующее поле
/// политики НЕ отправляется (null), кроме протокола — там контракт задаёт явное
/// значение `auto`.
library;

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/vpn/core_policy.dart';

/// Пресеты маршрутизации ядра (`preset` в ABI v2). UI-идентификаторы
/// [RoutingMode] совпадают с ними всюду, кроме `full`: в ядре этот пресет
/// называется `ru-full`.
const Map<String, String> kRoutingPresetWire = <String, String>{
  'full': 'ru-full',
};

/// DNS-резолверы по идентификатору [CoreOption] из пикера. `auto` отсутствует:
/// он означает «не переопределять», то есть `dns: null` в политике.
const Map<String, CorePolicyDns> kDnsPresets = <String, CorePolicyDns>{
  'cloudflare': CorePolicyDns(
    nameservers: <String>['https://1.1.1.1/dns-query'],
    fallback: <String>['tls://1.1.1.1:853'],
  ),
  'google': CorePolicyDns(
    nameservers: <String>['https://8.8.8.8/dns-query'],
    fallback: <String>['tls://8.8.8.8:853'],
  ),
  'adguard': CorePolicyDns(
    nameservers: <String>['https://dns.adguard-dns.com/dns-query'],
    fallback: <String>['tls://dns.adguard-dns.com'],
  ),
};

/// Собирает [CorePolicy] из текущего выбора пользователя.
///
/// [relays] — тот же список, что показывает пикер (панельный, когда загружен,
/// иначе [Relay.defaults]): индекс `config.relay` значим только относительно
/// него. Выход индекса за границы списка трактуется как «не выбрано».
CorePolicy corePolicyFrom(CoreConfig config, List<Relay> relays) {
  return CorePolicy(
    protocol: _protocol(config.protocol),
    preset: _preset(config.route),
    relay: _relay(config.relay, relays),
    stack: _stack(config.stack),
    mtu: _mtu(config.mtu),
    ipv6: config.ipv6,
    fakeIp: config.fakeIp,
    killSwitch: config.killSwitch,
    dns: _dns(config.dns),
    split: _split(config),
  );
}

/// `''` в [ProtocolOption] означает «Авто»; ядро ждёт для этого строку `auto`.
String _protocol(int index) {
  const options = ProtocolOption.defaults;
  if (index < 0 || index >= options.length) return 'auto';
  final id = options[index].id;
  return id.isEmpty ? 'auto' : id;
}

/// Пресет маршрутизации. Пустая строка — валидное значение контракта
/// («без пресета»), поэтому индекс вне диапазона даёт именно её, а не null.
String _preset(int index) {
  const modes = RoutingMode.defaults;
  if (index < 0 || index >= modes.length) return '';
  final id = modes[index].id;
  return kRoutingPresetWire[id] ?? id;
}

/// ISO-2 код relay-входа. «Выкл» и «Авто» одинаково означают «без явного
/// relay»: ядро в обоих случаях получает пустую строку и решает само.
String _relay(int index, List<Relay> relays) {
  if (index < 0 || index >= relays.length) return '';
  final relay = relays[index];
  if (relay.isOff || relay.isAuto) return '';
  final code = relay.country ?? relay.id ?? '';
  return code.toUpperCase();
}

/// `auto` -> null: ядро оставляет стек, выбранный под платформу.
String? _stack(int index) {
  const stacks = CoreOption.stacks;
  if (index < 0 || index >= stacks.length) return null;
  final id = stacks[index].id;
  return id == 'auto' ? null : id;
}

/// `auto` -> null (ядро берёт MTU по протоколу), иначе число из id опции.
int? _mtu(int index) {
  const options = CoreOption.mtu;
  if (index < 0 || index >= options.length) return null;
  return int.tryParse(options[index].id);
}

/// `auto` -> null (DNS из конфигурации сервера).
CorePolicyDns? _dns(int index) {
  const options = CoreOption.dns;
  if (index < 0 || index >= options.length) return null;
  return kDnsPresets[options[index].id];
}

/// Раздельное туннелирование. Режим ядра: `off` | `bypass` (выбранные мимо
/// туннеля) | `allow` (через туннель только выбранные). Домены уходят всегда,
/// когда режим не `off` — список приложений на desktop пока демонстрационный,
/// а домены работают везде.
CorePolicySplit _split(CoreConfig config) {
  final mode = switch (config.splitMode) {
    SplitMode.off => 'off',
    SplitMode.onlySelected => 'allow',
    SplitMode.bypassSelected => 'bypass',
  };
  if (config.splitMode == SplitMode.off) {
    return const CorePolicySplit();
  }
  final apps = config.splitApps.toList()..sort();
  return CorePolicySplit(
    mode: mode,
    apps: apps,
    bypassDomains: config.bypassDomainList,
  );
}

// --------------------------------------------------------------- CSM/1

/// Обратное преобразование [CorePolicy] в индексы пикеров [CoreConfig].
///
/// 02-SPEC.md 7.1 требует, чтобы инверсия `corePolicyFrom` жила В ЭТОМ ЖЕ
/// ФАЙЛЕ: без неё синхронизация настроек однонаправленная, и второе устройство
/// вечно показывает устаревший UI поверх правильного поведения, что читается
/// как баг навсегда.
///
/// Каждое поле независимо: значение, которого нет в списке опций этой сборки,
/// оставляет соответствующий индекс как был. Это правило 02-SPEC.md 7.9
/// последней строки на уровне пикеров.
CoreConfig coreConfigFromPolicy(
  CoreConfig current,
  CorePolicy policy, {
  List<Relay> relays = Relay.defaults,
}) {
  var next = current;

  final protocol = policy.protocol;
  if (protocol != null) {
    final wanted = protocol == 'auto' ? '' : protocol;
    final i = ProtocolOption.defaults.indexWhere((o) => o.id == wanted);
    if (i >= 0) next = next.copyWith(protocol: i);
  }

  final preset = policy.preset;
  if (preset != null) {
    // Ядро знает `ru-full`, UI называет тот же пресет `full`.
    final uiId = kRoutingPresetWire.entries
        .firstWhere(
          (e) => e.value == preset,
          orElse: () => MapEntry(preset, preset),
        )
        .key;
    final i = RoutingMode.defaults.indexWhere((m) => m.id == uiId);
    if (i >= 0) next = next.copyWith(route: i);
  }

  final relay = policy.relay;
  if (relay != null) {
    // Три состояния, не два: пустая строка это «не выбрано, оператор решает» и
    // ложится на «Авто», литерал `--` это явное «без релея» и ложится на
    // «Выкл», код страны ложится на свою страну (02-SPEC.md 7.3).
    final int i;
    if (relay.isEmpty) {
      i = relays.indexWhere((r) => r.isAuto);
    } else if (relay == kCsmNoRelay) {
      i = relays.indexWhere((r) => r.isOff);
    } else {
      i = relays.indexWhere(
        (r) => (r.country ?? r.id ?? '').toUpperCase() == relay.toUpperCase(),
      );
    }
    if (i >= 0) next = next.copyWith(relay: i);
  }

  final stack = policy.stack;
  if (stack != null) {
    final i = CoreOption.stacks.indexWhere((o) => o.id == stack);
    if (i >= 0) next = next.copyWith(stack: i);
  }

  final mtu = policy.mtu;
  if (mtu != null) {
    final i = CoreOption.mtu.indexWhere((o) => o.id == '$mtu');
    if (i >= 0) next = next.copyWith(mtu: i);
  }

  if (policy.ipv6 != null) next = next.copyWith(ipv6: policy.ipv6);
  if (policy.fakeIp != null) next = next.copyWith(fakeIp: policy.fakeIp);
  if (policy.killSwitch != null) {
    next = next.copyWith(killSwitch: policy.killSwitch);
  }

  final dns = policy.dns;
  if (dns != null) {
    for (final e in kDnsPresets.entries) {
      final same =
          e.value.nameservers.length == dns.nameservers.length &&
          e.value.fallback.length == dns.fallback.length &&
          e.value.nameservers.every(dns.nameservers.contains) &&
          e.value.fallback.every(dns.fallback.contains);
      if (!same) continue;
      final i = CoreOption.dns.indexWhere((o) => o.id == e.key);
      if (i >= 0) next = next.copyWith(dns: i);
      break;
    }
  }

  final split = policy.split;
  if (split != null) {
    final mode = switch (split.mode) {
      'allow' => SplitMode.onlySelected,
      'bypass' => SplitMode.bypassSelected,
      _ => SplitMode.off,
    };
    // Список приложений НЕ приходит с провода и не может прийти: INV-15.
    // Локальный список остаётся тем, что выбрал пользователь на этом
    // устройстве.
    next = next.copyWith(splitMode: mode);
  }

  return next;
}

/// Собирает [CorePolicy] из СЛИТОГО состояния настроек CSM.
///
/// Клиент НЕ передаёт `pol` в `SetPolicyJSON`. Он сливает `pol` в своё
/// состояние (см. `csmMergePolicy`), применяет правила происхождения и
/// карточек, и только потом пересобирает политику ЦЕЛИКОМ отсюда
/// (02-SPEC.md 7.11).
///
/// [local] нужен ровно для одного: свой `split.apps` прикрепляется к КАЖДОЙ
/// собираемой политике. `CorePolicySplit.toJson` всегда эмитит `apps`, а
/// `policy_json.go` пересобирает `SplitTunnel` из патча, поэтому отправка
/// операторского `split.mode` без локального списка стёрла бы выбор
/// пользователя, а это ровно тот список, который INV-15 защищает сильнее всех.
/// Отсюда он идёт в ядро и никуда больше.
CorePolicy corePolicyFromCsm(CsmSettings settings, CoreConfig local) {
  String? text(CsmSettingKey key) {
    final v = settings.valueOf(key);
    return v is CsmText ? v.value : null;
  }

  bool? boolean(CsmSettingKey key) {
    final v = settings.valueOf(key);
    return v is CsmBoolean ? v.value : null;
  }

  List<String>? list(CsmSettingKey key) {
    final v = settings.valueOf(key);
    return v is CsmTextList ? v.value : null;
  }

  final mtuValue = settings.valueOf(CsmSettingKey.mtu);
  final nameservers = list(CsmSettingKey.dnsNameservers);
  final fallback = list(CsmSettingKey.dnsFallback);
  final splitMode = text(CsmSettingKey.splitMode);

  final relay = text(CsmSettingKey.relay);

  return CorePolicy(
    protocol: text(CsmSettingKey.protocol),
    preset: text(CsmSettingKey.preset),
    // `--` до ядра не доходит: normalizeRelay принимает только две буквы или
    // пусто, и явного «без релея» в ABI пока нет. Клиент трактует `--` как
    // «не задано» и обязан записать эту деградацию в диагностику
    // (02-SPEC.md 7.3).
    relay: relay == kCsmNoRelay ? '' : relay,
    stack: text(CsmSettingKey.stack),
    mtu: mtuValue is CsmUint ? mtuValue.value : null,
    ipv6: boolean(CsmSettingKey.ipv6),
    fakeIp: boolean(CsmSettingKey.fakeIp),
    killSwitch: boolean(CsmSettingKey.killSwitch),
    dns: nameservers == null && fallback == null
        ? null
        : CorePolicyDns(
            nameservers: nameservers ?? const <String>[],
            fallback: fallback ?? const <String>[],
          ),
    split: splitMode == null
        ? null
        : CorePolicySplit(
            mode: splitMode,
            apps: splitMode == 'off'
                ? const <String>[]
                : (local.splitApps.toList()..sort()),
            bypassDomains: splitMode == 'off'
                ? const <String>[]
                : local.bypassDomainList,
          ),
  );
}

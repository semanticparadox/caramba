/// Перевод пользовательского выбора ([CoreConfig], индексы в списках опций) в
/// политику ядра [CorePolicy] (ABI v2 `CarambaSetPolicy`).
///
/// Единственное место, где индексы UI превращаются в строки провода. Правило:
/// «Авто» в UI означает «ядро решает само», а значит соответствующее поле
/// политики НЕ отправляется (null), кроме протокола — там контракт задаёт явное
/// значение `auto`.
library;

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

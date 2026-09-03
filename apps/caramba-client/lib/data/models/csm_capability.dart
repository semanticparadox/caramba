/// Битовое поле возможностей оператора CSM/1 и правило пересечения.
///
/// Нормативно: 03-WIRE.md 5.1 (назначение битов), 02-SPEC.md 6 (что гейтит
/// каждый бит, правило пересечения, разногласие каталога и директивы).
///
/// `cap` это `bstr(4)`, читаемый как 32-битное поле big-endian. Каталог несёт
/// поле оператора, директива его повторяет.
library;

import 'dart:typed_data';

/// Бит возможности. Значение это номер бита, маска считается из него.
enum CsmCapability {
  /// 0: материал узлов лежит в каталоге, клиент собирает конфигурацию сам.
  perNodeMaterial(0),

  /// 1: директивы приходят запечатанными (0x06).
  sealedDirectives(1),

  /// 2: цепочки релеев реальны (генератор Clash их действительно эмитит).
  relayChaining(2),

  /// 3: доступна запись настроек, они синхронизируются между устройствами.
  settingsWrite(3),

  /// 4: подписанный пул зеркал присутствует (ступень R2).
  mirrorPool(4),

  /// 5: точки DoH присутствуют в каталоге (ступень R3).
  dohEndpoints(5),

  /// 6: хеши правил и geo присутствуют (INV-12 выполним).
  resourceHashes(6),

  /// 7: канал устареваний присутствует.
  deprecationChannel(7),

  /// 8: онбординг-грант трафика доступен.
  onboardingGrant(8),

  /// 9: доступна регистрация второго и последующих устройств.
  deviceEnrollment(9),

  /// 10: `variant` проносится сквозь caramba-sub.
  variantForwarding(10),

  /// 11: флот поддерживает прыжки по портам.
  portHopping(11);

  const CsmCapability(this.bit);

  final int bit;

  int get mask => 1 << bit;
}

/// Биты, утверждающие наличие СОДЕРЖИМОГО каталога, а не политику оператора.
///
/// 02-SPEC.md 6.5: такой бит, поднятый в директиве, но не подкреплённый
/// непустым массивом в связанном каталоге, ОБЯЗАН читаться как ноль. Это
/// констатация факта о байтах, которые клиент держит, а не переопределение
/// политики, и возможность так подарить нельзя.
const Set<CsmCapability> kCsmContentBackedCapabilities = <CsmCapability>{
  CsmCapability.perNodeMaterial,
  CsmCapability.mirrorPool,
  CsmCapability.dohEndpoints,
  CsmCapability.resourceHashes,
};

/// Битовое поле, вкомпилированное в этот клиент (02-SPEC.md 6.2).
///
/// Оно НЕ настраивается, НЕ загружается и НЕ хранится: бит, которого клиент не
/// умеет, равен нулю здесь, и пересечение уводит его в безопасную сторону.
///
/// Биты 10 (`variant`) и 11 (`hop`) сняты намеренно: v1 не показывает контрол
/// варианта (02-SPEC.md 7.3, Correction 3) и рендерер игнорирует `hop`.
const int kCsmClientCapabilities =
    (1 << 0) |
    (1 << 1) |
    (1 << 2) |
    (1 << 3) |
    (1 << 4) |
    (1 << 5) |
    (1 << 6) |
    (1 << 7) |
    (1 << 8) |
    (1 << 9);

/// Разобранное поле возможностей.
class CsmCapabilitySet {
  const CsmCapabilitySet(this.raw);

  /// Пустое поле. Не то же самое, что «поля нет»: отсутствие `cap` на профиле,
  /// закрепившем корневой ключ, это жёсткая ошибка (INV-13), а не ноль.
  static const CsmCapabilitySet none = CsmCapabilitySet(0);

  /// 32-битное поле как целое.
  final int raw;

  /// Читает 4 байта big-endian. Более короткий или длинный вход это не наше
  /// дело: разбор кадра уже отверг бы его на P11.
  factory CsmCapabilitySet.fromBytes(Uint8List bytes) {
    if (bytes.length != 4) {
      return CsmCapabilitySet.none;
    }
    return CsmCapabilitySet(
      (bytes[0] << 24) | (bytes[1] << 16) | (bytes[2] << 8) | bytes[3],
    );
  }

  bool has(CsmCapability c) => (raw & c.mask) != 0;

  /// Правило пересечения: `effective = operator_cap AND client_cap`.
  CsmCapabilitySet intersectWithClient([
    int clientCap = kCsmClientCapabilities,
  ]) => CsmCapabilitySet(raw & clientCap);

  /// Снимает биты, чьё содержимое в связанном каталоге отсутствует или пусто.
  ///
  /// [backing] отвечает на вопрос «есть ли непустой массив за этим битом» и
  /// спрашивается только про биты из [kCsmContentBackedCapabilities].
  CsmCapabilitySet withContentPresence(bool Function(CsmCapability) backing) {
    var v = raw;
    for (final c in kCsmContentBackedCapabilities) {
      if ((v & c.mask) != 0 && !backing(c)) {
        v &= ~c.mask;
      }
    }
    return CsmCapabilitySet(v);
  }

  /// Набор поднятых битов, для диагностики и экрана возможностей.
  Set<CsmCapability> get bits => CsmCapability.values.where(has).toSet();

  String toHex() => raw.toRadixString(16).padLeft(8, '0');

  @override
  bool operator ==(Object other) =>
      other is CsmCapabilitySet && other.raw == raw;

  @override
  int get hashCode => raw.hashCode;

  @override
  String toString() => 'CsmCapabilitySet(0x${toHex()})';
}

/// Причина, по которой контрол или ступень недоступны. Словарь закрытый,
/// 02-SPEC.md 8.1.
enum CsmUnavailableReason {
  userDisabled('user_disabled'),
  notOfferedByOperator('not_offered_by_operator'),
  platformUnsupported('platform_unsupported'),
  notConfigured('not_configured'),
  appVersionUnsupported('app_version_unsupported');

  const CsmUnavailableReason(this.wire);

  final String wire;

  static CsmUnavailableReason? fromWire(String? raw) {
    for (final r in CsmUnavailableReason.values) {
      if (r.wire == raw) {
        return r;
      }
    }
    return null;
  }
}

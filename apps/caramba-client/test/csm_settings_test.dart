// Словарь настроек CSM/1: трёхсторонняя семантика патча, происхождение,
// карточка «Оставить или Вернуть», закрытые словари и запрет split.apps.
//
// Нормативно 02-SPEC.md 7. Каждая группа ниже названа пунктом, который она
// проверяет, потому что проверяется тут не код, а правило.

import 'dart:typed_data';

import 'package:caramba_vpn/csm.dart' show sha256;
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/csm_write.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_policy_mapping.dart';
import 'package:caramba_client/vpn/core_policy.dart';

/// Тело записи с нулевыми nonce и dtp: этому слою ключевой материал не нужен,
/// проверяется форма запроса, а не подпись.
CsmSettingsWrite _write(Map<CsmSettingKey, CsmWantOp> want, {int? ifMatch}) =>
    CsmSettingsWrite(
      nonce: Uint8List(16),
      deviceThumbprint: Uint8List(16),
      want: want,
      ifMatchVersion: ifMatch,
    );

Map<int, CsmPolicyEntry> _pol(
  Map<CsmSettingKey, (CsmSettingValue, CsmProvenance)> entries,
) => <int, CsmPolicyEntry>{
  for (final e in entries.entries)
    e.key.wire: CsmPolicyEntry(e.value.$1, e.value.$2),
};

/// Входы в той форме, в какой их отдаёт панель (`GET /app/relays` + два
/// псевдо-варианта клиента). Выдуманного набора `Relay.defaults` со странами,
/// которых у оператора нет, здесь больше нет; индексы прежние: 0 — Выкл,
/// 1 — Авто, 2 — страна.
final _panelRelays = Relay.fromCountries(<Relay>[
  Relay.fromApiJson(const <String, dynamic>{
    'country_code': 'TR',
    'country_name': 'Турция',
    'node_count': 2,
  }),
]);

void main() {
  group('7.5 трёхсторонняя семантика патча', () {
    test('ключ, которого нет в want, означает «не менять»', () {
      final write = _write(<CsmSettingKey, CsmWantOp>{
        CsmSettingKey.protocol: const CsmWantSet(CsmText('auto')),
      });

      final map = write.toWantMap();
      expect(map.keys, <int>[CsmSettingKey.protocol.wire]);
      expect(map.containsKey(CsmSettingKey.killSwitch.wire), isFalse);
    });

    test('сброс это текстовая строка default для ЛЮБОГО типа ключа', () {
      final write = _write(<CsmSettingKey, CsmWantOp>{
        // Текстовый ключ.
        CsmSettingKey.preset: const CsmWantReset(),
        // Булев ключ: сентинел всё равно текст, формы null не существует.
        CsmSettingKey.killSwitch: const CsmWantReset(),
        // Числовой ключ.
        CsmSettingKey.mtu: const CsmWantReset(),
        // Массив строк.
        CsmSettingKey.dnsNameservers: const CsmWantReset(),
      });

      final map = write.toWantMap();
      for (final key in <CsmSettingKey>[
        CsmSettingKey.preset,
        CsmSettingKey.killSwitch,
        CsmSettingKey.mtu,
        CsmSettingKey.dnsNameservers,
      ]) {
        expect(map[key.wire], kCsmDefaultSentinel, reason: key.name);
        expect(map[key.wire], isA<String>());
      }
    });

    test('значение вне словаря на провод не уходит', () {
      final write = _write(<CsmSettingKey, CsmWantOp>{
        CsmSettingKey.preset: const CsmWantSet(CsmText('full')),
        CsmSettingKey.protocol: const CsmWantSet(CsmText('auto')),
      });

      // UI-идентификатор `full` на проводе не появляется никогда: ядро знает
      // этот пресет как `ru-full` (02-SPEC.md 7.3, Correction 8).
      final map = write.toWantMap();
      expect(map.containsKey(CsmSettingKey.preset.wire), isFalse);
      expect(map[CsmSettingKey.protocol.wire], 'auto');
    });

    test('If-Match несёт ver директивы, которую правит запись', () {
      expect(
        _write(const <CsmSettingKey, CsmWantOp>{}, ifMatch: 412).ifMatchHeader,
        '412',
      );
      expect(_write(const <CsmSettingKey, CsmWantOp>{}).ifMatchHeader, isNull);
    });

    test('предобраз доказательства совпадает с 03-WIRE.md 13.6', () {
      // Пустое тело, PUT, канонический путь. Документ печатает 71 байт и
      // приводит их шестнадцатеричной строкой.
      final message = csmWriteProofMessage(
        method: 'PUT',
        canonicalPath: kCsmWritePathPreferences,
        body: const <int>[],
      );

      expect(message.length, 71);
      final hex = message
          .map((b) => b.toRadixString(16).padLeft(2, '0'))
          .join();
      expect(
        hex,
        '63736d312d777269746500505554002f6170692f76322f6170702f70726566657'
        '2656e63657300e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495'
        '991b7852b855',
      );

      // Дайджест это то, что подписывает сама операция ECDSA, и документ
      // называет его отдельно. Он проверяется здесь по той же причине, по
      // которой закреплён в TestDeviceProofPreImage ядра: реализация, которая
      // хеширует заранее и подписывает дайджест КАК СООБЩЕНИЕ, собрала бы
      // верный прообраз и всё равно выдала подпись, которую панель отвергнет.
      final digest = sha256(
        message,
      ).map((b) => b.toRadixString(16).padLeft(2, '0')).join();
      expect(
        digest,
        '45ae6e7f2d6e63113532b04a140183d21319b6d3af86993c3360c31eb80b3716',
      );

      // Разделителей ровно три и они нулевые: без них PUT и путь склеились бы,
      // и два разных запроса дали бы один прообраз.
      expect(message.where((b) => b == 0).length, 3);
    });

    test('прообраз связывает метод, канонический путь и тело', () {
      Uint8List m(String method, String path, List<int> body) =>
          csmWriteProofMessage(method: method, canonicalPath: path, body: body);

      final base = m('PUT', kCsmWritePathPreferences, const <int>[]);
      expect(m('PUT', kCsmWritePathPreferences, const <int>[]), base);
      expect(m('POST', kCsmWritePathPreferences, const <int>[]), isNot(base));
      expect(m('PUT', kCsmWritePathEnrollCode, const <int>[]), isNot(base));
      expect(m('PUT', kCsmWritePathPreferences, const <int>[1]), isNot(base));
    });
  });

  group('7.6 происхождение и старшинство', () {
    test('src из кадра переносится в состояние без перетолкования', () {
      final merged = csmMergePolicy(
        current: CsmSettings.empty,
        pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
          CsmSettingKey.protocol: (
            const CsmText('auto'),
            CsmProvenance.operator,
          ),
          CsmSettingKey.preset: (
            const CsmText('ru-smart'),
            CsmProvenance.byDefault,
          ),
        }),
        nowMs: 1,
      );

      expect(
        merged.settings[CsmSettingKey.protocol]!.src,
        CsmProvenance.operator,
      );
      expect(
        merged.settings[CsmSettingKey.preset]!.src,
        CsmProvenance.byDefault,
      );
    });

    test('отметка «пользователь ставил» переживает запись оператора', () {
      // Отметка про то, трогал ли ключ пользователь на ЭТОМ устройстве, а не
      // про то, кто выиграл старшинство на панели.
      final current = CsmSettings.empty.setByUser(
        CsmSettingKey.mtu,
        const CsmUint(1420),
      );
      expect(current.isUserSet(CsmSettingKey.mtu), isTrue);

      final merged = csmMergePolicy(
        current: current,
        pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
          CsmSettingKey.mtu: (const CsmUint(1420), CsmProvenance.operator),
        }),
        nowMs: 1,
      );

      // Значение то же, значит карточки нет, но происхождение обновилось.
      expect(merged.cards, isEmpty);
      expect(merged.settings[CsmSettingKey.mtu]!.src, CsmProvenance.operator);
      expect(merged.settings.isUserSet(CsmSettingKey.mtu), isTrue);
    });

    test('пока карточка не отвечена, эффективное значение локальное', () {
      final current = CsmSettings.empty.setByUser(
        CsmSettingKey.killSwitch,
        const CsmBoolean(true),
      );

      final first = csmMergePolicy(
        current: current,
        pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
          CsmSettingKey.killSwitch: (
            const CsmBoolean(false),
            CsmProvenance.operator,
          ),
        }),
        nowMs: 1,
      );
      expect(first.cards, hasLength(1));
      expect(
        (first.settings.valueOf(CsmSettingKey.killSwitch)! as CsmBoolean).value,
        isTrue,
        reason: 'клиент удерживает своё значение',
      );

      // Вторая директива с тем же требованием второй карточки не заводит и
      // локальное значение по-прежнему не трогает.
      final second = csmMergePolicy(
        current: first.settings,
        cards: first.cards,
        pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
          CsmSettingKey.killSwitch: (
            const CsmBoolean(false),
            CsmProvenance.operator,
          ),
        }),
        nowMs: 2,
      );
      expect(second.cards, hasLength(1));
      expect(
        (second.settings.valueOf(CsmSettingKey.killSwitch)! as CsmBoolean)
            .value,
        isTrue,
      );
    });
  });

  group('7.7 карточка «Оставить или Вернуть»', () {
    test('оператор поверх пользовательского значения поднимает карточку', () {
      final current = CsmSettings.empty.setByUser(
        CsmSettingKey.preset,
        const CsmText('ru-smart'),
      );

      final merged = csmMergePolicy(
        current: current,
        pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
          CsmSettingKey.preset: (
            const CsmText('global'),
            CsmProvenance.operator,
          ),
        }),
        nowMs: 10,
      );

      expect(merged.cards, hasLength(1));
      final item = merged.cards.single.items.single;
      expect(item.key, CsmSettingKey.preset);
      expect((item.current as CsmText).value, 'ru-smart');
      expect((item.proposed as CsmText).value, 'global');
      expect(item.src, CsmProvenance.operator);
      expect(item.trigger, CsmCardTrigger.operatorOverwroteUserSet);
      expect(merged.applied, isEmpty);
    });

    test('без отметки пользователя тот же ключ применяется молча', () {
      final current = CsmSettings.empty.setDefault(
        CsmSettingKey.preset,
        const CsmText('ru-smart'),
      );

      final merged = csmMergePolicy(
        current: current,
        pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
          CsmSettingKey.preset: (
            const CsmText('global'),
            CsmProvenance.operator,
          ),
        }),
        nowMs: 10,
      );

      expect(merged.cards, isEmpty);
      expect(merged.applied, <CsmSettingKey>{CsmSettingKey.preset});
    });

    test('сужение защиты поднимает карточку независимо от происхождения', () {
      // Ни один из этих ключей пользователь не трогал, и происхождение у всех
      // не операторское: карточка всё равно обязана подняться.
      final current = CsmSettings.empty
          .setDefault(CsmSettingKey.killSwitch, const CsmBoolean(true))
          .setDefault(CsmSettingKey.splitMode, const CsmText('bypass'))
          .setDefault(
            CsmSettingKey.dnsNameservers,
            const CsmTextList(<String>['https://1.1.1.1/dns-query']),
          );

      final merged = csmMergePolicy(
        current: current,
        pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
          CsmSettingKey.killSwitch: (
            const CsmBoolean(false),
            CsmProvenance.user,
          ),
          CsmSettingKey.splitMode: (
            const CsmText('off'),
            CsmProvenance.byDefault,
          ),
          CsmSettingKey.dnsNameservers: (
            const CsmTextList(<String>['https://8.8.8.8/dns-query']),
            CsmProvenance.user,
          ),
        }),
        nowMs: 10,
      );

      expect(merged.cards, hasLength(3));
      expect(
        merged.cards.expand((c) => c.items).map((i) => i.trigger).toSet(),
        <CsmCardTrigger>{CsmCardTrigger.narrowing},
      );
      expect(merged.applied, isEmpty);
      // Ни одно из сужений не применено: клиент держит своё.
      expect(
        (merged.settings.valueOf(CsmSettingKey.killSwitch)! as CsmBoolean)
            .value,
        isTrue,
      );
    });

    test('расширение защиты карточки не поднимает', () {
      final current = CsmSettings.empty
          .setDefault(CsmSettingKey.killSwitch, const CsmBoolean(false))
          .setDefault(CsmSettingKey.splitMode, const CsmText('off'));

      final merged = csmMergePolicy(
        current: current,
        pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
          CsmSettingKey.killSwitch: (
            const CsmBoolean(true),
            CsmProvenance.operator,
          ),
          CsmSettingKey.splitMode: (
            const CsmText('bypass'),
            CsmProvenance.operator,
          ),
        }),
        nowMs: 10,
      );

      expect(merged.cards, isEmpty);
      expect(merged.applied, hasLength(2));
    });

    test(
      'четвёртая карточка схлопывается в самую старую, а не выбрасывается',
      () {
        var settings = CsmSettings.empty
            .setDefault(
              CsmSettingKey.dnsNameservers,
              const CsmTextList(<String>['https://1.1.1.1/dns-query']),
            )
            .setDefault(
              CsmSettingKey.dnsFallback,
              const CsmTextList(<String>['tls://1.1.1.1:853']),
            )
            .setDefault(CsmSettingKey.killSwitch, const CsmBoolean(true))
            .setDefault(CsmSettingKey.splitMode, const CsmText('bypass'));

        var cards = <CsmPendingChange>[];
        var now = 100;
        for (final entry in <(CsmSettingKey, CsmSettingValue)>[
          (
            CsmSettingKey.dnsNameservers,
            const CsmTextList(<String>['https://8.8.8.8/dns-query']),
          ),
          (
            CsmSettingKey.dnsFallback,
            const CsmTextList(<String>['tls://8.8.8.8:853']),
          ),
          (CsmSettingKey.killSwitch, const CsmBoolean(false)),
          (CsmSettingKey.splitMode, const CsmText('off')),
        ]) {
          final merged = csmMergePolicy(
            current: settings,
            cards: cards,
            pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
              entry.$1: (entry.$2, CsmProvenance.operator),
            }),
            nowMs: now,
          );
          settings = merged.settings;
          cards = merged.cards;
          now += 10;
        }

        // Максимум три висящие карточки, но ни один затронутый ключ не потерян.
        expect(cards, hasLength(kCsmMaxOutstandingCards));
        final keys = cards.expand((c) => c.keys).toSet();
        expect(keys, <CsmSettingKey>{
          CsmSettingKey.dnsNameservers,
          CsmSettingKey.dnsFallback,
          CsmSettingKey.killSwitch,
          CsmSettingKey.splitMode,
        });
        // Схлопнулась именно самая старая, и она стала многоключевой.
        final oldest = cards.reduce((a, b) => a.raisedMs <= b.raisedMs ? a : b);
        expect(oldest.isMultiKey, isTrue);
        expect(oldest.raisedMs, 100);
      },
    );
  });

  group('7.9 клиент не знает значения, которое прислала панель', () {
    test(
      'неизвестное значение игнорируется поштучно, остальные применяются',
      () {
        final merged = csmMergePolicy(
          current: CsmSettings.empty.setByUser(
            CsmSettingKey.preset,
            const CsmText('ru-smart'),
          ),
          pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
            // Десятый пресет, которого эта сборка не знает.
            CsmSettingKey.preset: (
              const CsmText('kz-smart'),
              CsmProvenance.operator,
            ),
            CsmSettingKey.protocol: (
              const CsmText('TUIC'),
              CsmProvenance.operator,
            ),
          }),
          nowMs: 1,
        );

        expect(merged.ignored, hasLength(1));
        expect(merged.ignored.single.key, CsmSettingKey.preset);
        expect(
          merged.ignored.single.reason,
          CsmIgnoredReason.outsideVocabulary,
        );
        // Локальное значение сохранено, неизвестное НЕ сохранено (INV-11).
        expect(
          (merged.settings.valueOf(CsmSettingKey.preset)! as CsmText).value,
          'ru-smart',
        );
        expect(merged.applied, <CsmSettingKey>{CsmSettingKey.protocol});
      },
    );

    test('ключ вне закрытого реестра настроек это unknownKey', () {
      final merged = csmMergePolicy(
        current: CsmSettings.empty,
        pol: <int, CsmPolicyEntry>{
          12: const CsmPolicyEntry(CsmText('x'), CsmProvenance.operator),
        },
        nowMs: 1,
      );

      expect(merged.ignored.single.reason, CsmIgnoredReason.unknownKey);
      expect(merged.settings.entries, isEmpty);
    });

    test('несовпадение типа значения и ключа это malformed', () {
      final merged = csmMergePolicy(
        current: CsmSettings.empty,
        pol: <int, CsmPolicyEntry>{
          CsmSettingKey.killSwitch.wire: const CsmPolicyEntry(
            null,
            CsmProvenance.operator,
          ),
        },
        nowMs: 1,
      );

      expect(merged.ignored.single.reason, CsmIgnoredReason.malformed);
    });

    test(
      'ключ, который оператору писать нельзя, с src=operator игнорируется',
      () {
        for (final key in <CsmSettingKey>[
          CsmSettingKey.stack,
          CsmSettingKey.ipv6,
          CsmSettingKey.fakeIp,
        ]) {
          expect(key.operatorMayWrite, isFalse, reason: key.name);
        }

        final merged = csmMergePolicy(
          current: CsmSettings.empty.setByUser(
            CsmSettingKey.stack,
            const CsmText('gvisor'),
          ),
          pol: _pol(<CsmSettingKey, (CsmSettingValue, CsmProvenance)>{
            CsmSettingKey.stack: (
              const CsmText('system'),
              CsmProvenance.operator,
            ),
            CsmSettingKey.ipv6: (
              const CsmBoolean(true),
              CsmProvenance.operator,
            ),
          }),
          nowMs: 1,
        );

        expect(merged.ignored, hasLength(2));
        expect(merged.ignored.map((e) => e.reason).toSet(), <CsmIgnoredReason>{
          CsmIgnoredReason.operatorMayNotWrite,
        });
        expect(
          (merged.settings.valueOf(CsmSettingKey.stack)! as CsmText).value,
          'gvisor',
        );
        expect(merged.settings.valueOf(CsmSettingKey.ipv6), isNull);
      },
    );
  });

  group('7.3 закрытые словари', () {
    test(
      'протокол: VLESS входит в словарь, хотя комментарий его пропускает',
      () {
        expect(kCsmProtocolVocabulary, contains('VLESS'));
        expect(kCsmProtocolVocabulary, contains('VLESS-Reality'));
        expect(
          csmValueInVocabulary(CsmSettingKey.protocol, const CsmText('VLESS')),
          isTrue,
        );
        expect(
          csmValueInVocabulary(CsmSettingKey.protocol, const CsmText('vless')),
          isFalse,
        );
      },
    );

    test(
      'пресет: девять идентификаторов ядра плюс пустая строка, без full',
      () {
        expect(kCsmPresetVocabulary, hasLength(10));
        expect(kCsmPresetVocabulary, isNot(contains('full')));
        expect(
          csmValueInVocabulary(CsmSettingKey.preset, const CsmText('')),
          isTrue,
          reason: 'пустая строка это валидное «без пресета»',
        );
        expect(
          csmValueInVocabulary(CsmSettingKey.preset, const CsmText('full')),
          isFalse,
        );
      },
    );

    test('релей: три состояния, и пустая строка это НЕ «выключено»', () {
      // Код страны заглавными.
      expect(csmIsRelayValue('TR'), isTrue);
      // Явное «без релея».
      expect(csmIsRelayValue(kCsmNoRelay), isTrue);
      // «Не выбрано, оператор решает».
      expect(csmIsRelayValue(''), isTrue);
      // Всё остальное вне словаря.
      expect(csmIsRelayValue('tr'), isFalse);
      expect(csmIsRelayValue('TUR'), isFalse);
      expect(
        csmIsRelayValue('NO'),
        isTrue,
        reason: 'NO это Норвегия, не сентинел',
      );
    });

    test('mtu: ноль означает «по умолчанию ядра», иначе 576..9000', () {
      expect(csmValueInVocabulary(CsmSettingKey.mtu, const CsmUint(0)), isTrue);
      expect(
        csmValueInVocabulary(CsmSettingKey.mtu, const CsmUint(1420)),
        isTrue,
      );
      expect(
        csmValueInVocabulary(CsmSettingKey.mtu, const CsmUint(575)),
        isFalse,
      );
      expect(
        csmValueInVocabulary(CsmSettingKey.mtu, const CsmUint(9001)),
        isFalse,
      );
    });

    test('DNS: только https и tls, максимум восемь записей', () {
      expect(csmIsResolverUrl('https://1.1.1.1/dns-query'), isTrue);
      expect(csmIsResolverUrl('tls://1.1.1.1:853'), isTrue);
      // INV-8 распространяется на каждую выборку клиента, а DNS это выборка.
      expect(csmIsResolverUrl('http://1.1.1.1/dns-query'), isFalse);
      expect(csmIsResolverUrl('1.1.1.1'), isFalse);
      expect(csmIsResolverUrl('udp://1.1.1.1'), isFalse);

      expect(
        csmValueInVocabulary(
          CsmSettingKey.dnsNameservers,
          CsmTextList(List<String>.filled(9, 'https://a.example/dns-query')),
        ),
        isFalse,
      );
    });

    test('режим раздельного туннелирования: off, bypass, allow', () {
      expect(kCsmSplitModeVocabulary, <String>{'off', 'bypass', 'allow'});
      expect(
        csmValueInVocabulary(CsmSettingKey.splitMode, const CsmText('allow')),
        isTrue,
      );
      expect(
        csmValueInVocabulary(CsmSettingKey.splitMode, const CsmText('onlyMe')),
        isFalse,
      );
    });

    test('значение вне словаря не восстанавливается из хранилища', () {
      // INV-11: клиент хранит и повторяет только то, что может проверить.
      final entry = CsmSettingEntry.fromJson(
        CsmSettingKey.preset,
        <String, Object?>{'v': 'kz-smart', 'src': 2, 'user_set': false},
      );
      expect(entry, isNull);
    });
  });

  group('INV-15 split.apps не пересекает границу ни в одну сторону', () {
    test('в закрытом реестре настроек ключа для split.apps нет', () {
      // Ключи pol это 1..11, и все они заняты. Двенадцатого нет и назначить
      // его нельзя (03-WIRE.md 8.3).
      expect(CsmSettingKey.values.map((k) => k.wire).toSet(), <int>{
        1,
        2,
        3,
        4,
        5,
        6,
        7,
        8,
        9,
        10,
        11,
      });
      expect(
        CsmSettingKey.values.map((k) => k.name),
        isNot(contains('splitApps')),
      );
    });

    test(
      'сериализатор запроса физически не может написать список приложений',
      () {
        final write = _write(const <CsmSettingKey, CsmWantOp>{
          CsmSettingKey.splitMode: CsmWantSet(CsmText('bypass')),
        });

        final map = write.toWantMap();
        expect(map, <int, Object?>{CsmSettingKey.splitMode.wire: 'bypass'});
        // Ни одно значение запроса не является списком идентификаторов
        // приложений, потому что таких значений в реестре нет.
        for (final v in map.values) {
          expect(v, isNot(contains('com.example')));
        }
      },
    );

    test('входящий pol с чужим ключом не заводит список приложений', () {
      final merged = csmMergePolicy(
        current: CsmSettings.empty,
        pol: <int, CsmPolicyEntry>{
          // Кто-то попытался назначить split.apps ключ 12.
          12: const CsmPolicyEntry(
            CsmTextList(<String>['com.example.app']),
            CsmProvenance.operator,
          ),
        },
        nowMs: 1,
      );

      expect(merged.ignored.single.reason, CsmIgnoredReason.unknownKey);
      expect(merged.settings.entries, isEmpty);
      // И ничего похожего не осело в сериализованном состоянии.
      expect(merged.settings.toJson(), isEmpty);
    });

    test('локальный список прикрепляется к политике ядра и только к ней', () {
      // 02-SPEC.md 7.11 пункт 3: CorePolicySplit.toJson всегда эмитит apps, а
      // policy_json.go пересобирает SplitTunnel из патча, поэтому отправка
      // операторского split.mode без локального списка стёрла бы выбор
      // пользователя. Список приезжает СЮДА из локального состояния и никуда
      // больше.
      const local = CoreConfig(
        splitMode: SplitMode.bypassSelected,
        splitApps: <String>{'com.example.app', 'com.example.other'},
        bypassDomains: 'example.com',
      );
      final settings = CsmSettings.empty
          .setDefault(CsmSettingKey.splitMode, const CsmText('bypass'))
          .setByUser(CsmSettingKey.killSwitch, const CsmBoolean(true));

      final policy = corePolicyFromCsm(settings, local);

      expect(policy.split!.mode, 'bypass');
      expect(policy.split!.apps, <String>[
        'com.example.app',
        'com.example.other',
      ]);
      expect(policy.killSwitch, isTrue);

      // А в тело запроса тот же список попасть не может.
      final write = _write(const <CsmSettingKey, CsmWantOp>{
        CsmSettingKey.splitMode: CsmWantSet(CsmText('bypass')),
      });
      expect(write.toWantMap().toString(), isNot(contains('com.example')));
    });
  });

  group('7.1 инверсия corePolicyFrom', () {
    test('строки провода возвращаются в индексы пикеров', () {
      const base = CoreConfig();
      final next = coreConfigFromPolicy(
        base,
        const CorePolicy(
          protocol: 'TUIC',
          preset: 'ru-full',
          relay: 'TR',
          stack: 'gvisor',
          mtu: 1420,
          killSwitch: false,
          ipv6: true,
        ),
        relays: _panelRelays,
      );

      expect(ProtocolOption.defaults[next.protocol].id, 'TUIC');
      // Ядро зовёт этот пресет `ru-full`, UI зовёт его `full`.
      expect(RoutingMode.defaults[next.route].id, 'full');
      expect(_panelRelays[next.relay].country, 'TR');
      expect(CoreOption.stacks[next.stack].id, 'gvisor');
      expect(CoreOption.mtu[next.mtu].id, '1420');
      expect(next.killSwitch, isFalse);
      expect(next.ipv6, isTrue);
    });

    test('три состояния релея ложатся на Выкл, Авто и страну', () {
      const base = CoreConfig();

      // Пустая строка это «оператор решает» -> Авто.
      expect(
        _panelRelays[coreConfigFromPolicy(
              base,
              const CorePolicy(relay: ''),
              relays: _panelRelays,
            ).relay]
            .isAuto,
        isTrue,
      );
      // `--` это явное «без релея» -> Выкл.
      expect(
        _panelRelays[coreConfigFromPolicy(
              base,
              const CorePolicy(relay: kCsmNoRelay),
              relays: _panelRelays,
            ).relay]
            .isOff,
        isTrue,
      );
    });

    test('значение, которого нет в списках сборки, индекс не двигает', () {
      const base = CoreConfig(protocol: 1, route: 2);
      final next = coreConfigFromPolicy(
        base,
        const CorePolicy(protocol: 'Unknown-Proto', preset: 'kz-smart'),
      );

      expect(next.protocol, 1);
      expect(next.route, 2);
    });

    test('политика из CSM не переизобретает `--` для ядра', () {
      // normalizeRelay в ядре принимает только две буквы или пусто, поэтому
      // `--` до него не доезжает и трактуется как «не задано»; деградация
      // записывается в диагностику, а не проглатывается тихо.
      final policy = corePolicyFromCsm(
        CsmSettings.empty.setByUser(CsmSettingKey.relay, const CsmText('--')),
        const CoreConfig(),
      );
      expect(policy.relay, '');
    });
  });

  group('7.8 очередь записей', () {
    test('вторая правка того же ключа заменяет первую', () {
      var queue = CsmWriteQueue.empty;
      queue = queue.enqueue(
        const CsmQueuedWrite(
          key: CsmSettingKey.protocol,
          op: CsmWantSet(CsmText('auto')),
          queuedMs: 1,
        ),
      );
      queue = queue.enqueue(
        const CsmQueuedWrite(
          key: CsmSettingKey.protocol,
          op: CsmWantSet(CsmText('TUIC')),
          queuedMs: 2,
        ),
      );

      expect(queue.length, 1);
      expect(
        ((queue.entries.single.op as CsmWantSet).value as CsmText).value,
        'TUIC',
      );
    });

    test('глубина очереди ограничена 32 записями', () {
      var queue = CsmWriteQueue.empty;
      for (var i = 0; i < 40; i++) {
        queue = queue.enqueue(
          CsmQueuedWrite(
            key: CsmSettingKey.values[i % CsmSettingKey.values.length],
            op: const CsmWantReset(),
            queuedMs: i,
          ),
        );
      }
      expect(queue.length, lessThanOrEqualTo(kCsmWriteQueueDepth));
    });

    test('запись старше семи суток выбрасывается', () {
      const day = 86400 * 1000;
      final queue = CsmWriteQueue.empty.enqueue(
        const CsmQueuedWrite(
          key: CsmSettingKey.protocol,
          op: CsmWantReset(),
          queuedMs: 0,
        ),
      );

      expect(queue.prune(6 * day).length, 1);
      expect(queue.prune(8 * day).length, 0);
    });

    test('nonce старше 300 секунд обязан быть переподписан', () {
      const write = CsmQueuedWrite(
        key: CsmSettingKey.protocol,
        op: CsmWantReset(),
        queuedMs: 0,
        nonceIssuedMs: 1000,
      );

      expect(write.isStaleNonce(1000 + 299 * 1000), isFalse);
      expect(write.isStaleNonce(1000 + 301 * 1000), isTrue);
      // Записи, для которой nonce ещё не выдавался, отправлять нечем.
      expect(
        const CsmQueuedWrite(
          key: CsmSettingKey.protocol,
          op: CsmWantReset(),
          queuedMs: 0,
        ).isStaleNonce(1),
        isTrue,
      );
    });
  });
}

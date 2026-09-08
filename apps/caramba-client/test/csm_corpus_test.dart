// Шлюз слияния для слоя P4: энроллмент, закрепление пина и модель настроек.
//
// Корпус docs/protocol/05-TEST-VECTORS порождён независимой реализацией и
// читается ОТСЮДА С ДИСКА по относительному пути. Фикстуры не копируются в
// пакет: один корпус обслуживает три реализации, и копия это способ разойтись
// с ними незаметно.
//
// Проверяющий кадры (packages/caramba_vpn) сходится с корпусом на вердиктах и
// кодах отказа в своём собственном corpus_test. Здесь проверяется то, что
// лежит НАД ним: что бутстрап-блоб корпуса заводит правильный пин, что код
// энроллмента сворачивает в себя префикс пина, и что подписанная карта `pol`
// корпуса сливается в локальное состояние ровно по правилам 02-SPEC.md 7.

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:caramba_vpn/csm.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_enrollment.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';

const String _corpusRelative = 'docs/protocol/05-TEST-VECTORS';

Directory _findCorpus() {
  var dir = Directory.current.absolute;
  for (var i = 0; i < 8; i++) {
    final candidate = Directory('${dir.path}/$_corpusRelative');
    if (candidate.existsSync()) {
      return candidate;
    }
    final parent = dir.parent;
    if (parent.path == dir.path) {
      break;
    }
    dir = parent;
  }
  throw StateError(
    'CSM/1 corpus not found: expected $_corpusRelative at or above '
    '${Directory.current.path}',
  );
}

void main() {
  final corpus = _findCorpus();
  final vectors =
      jsonDecode(File('${corpus.path}/vectors.json').readAsStringSync())
          as Map<String, dynamic>;
  final all = (vectors['vectors'] as List).cast<Map<String, dynamic>>();
  final contexts = vectors['contexts'] as Map<String, dynamic>;
  final defaultContext = contexts['default'] as Map<String, dynamic>;

  Uint8List fixture(String id) {
    final v = all.firstWhere(
      (e) => e['id'] == id,
      orElse: () => throw StateError('corpus has no vector $id'),
    );
    return File('${corpus.path}/${v['file']}').readAsBytesSync();
  }

  final pinnedPid = defaultContext['pinned_pid'] as String;
  final linkPin = defaultContext['link_pin'] as String;

  group('бутстрап-блоб корпуса заводит пин', () {
    test('pos-b1-min: код сворачивает префикс пина, pid совпадает', () {
      final result = csmParseBootstrap(
        fixture('pos-b1-min'),
        expectedLinkPin: linkPin,
      );

      expect(result.isOk, isTrue, reason: 'блоб корпуса обязан разбираться');
      final b = result.bootstrap!;
      expect(b.pid, pinnedPid);
      expect(b.linkPin, linkPin);
      expect(b.origin, 'https://panel.example.net');
      expect(b.operatorName, 'Exa Networks');
      // Блоб раздаётся вне полосы, значит и пин из него вне полосы (INV-18).
      expect(b.pinOrigin, CsmPinOrigin.outOfBand);
      // 02-SPEC.md 9.2: 20 символов, первые восемь это link_pin[0..8].
      expect(b.code.length, kCsmEnrollCodeLength);
      expect(b.code.substring(0, 8), linkPin.substring(0, 8));
    });

    test('pos-b1-max: 32 зеркала переживают разбор', () {
      final result = csmParseBootstrap(
        fixture('pos-b1-max'),
        expectedLinkPin: linkPin,
      );

      expect(result.isOk, isTrue);
      expect(result.bootstrap!.mirrors.length, 32);
      expect(result.bootstrap!.pid, pinnedPid);
    });

    test('pos-b1-wire85: код не сворачивает пин, энроллмент отказан', () {
      // Correction 5 к 03-WIRE.md 8.5: фикстура документа несёт код
      // "K7QW-3M2P-9XRT", который не начинается с префикса пина. Байты
      // разбираются, но энроллмент по такому блобу состояться не может.
      final result = csmParseBootstrap(
        fixture('pos-b1-wire85'),
        expectedLinkPin: linkPin,
      );

      expect(result.isOk, isFalse);
      expect(result.failure, CsmEnrollFailure.codePinMismatch);
    });

    test('чужой пин это жёсткий отказ без пути «всё равно продолжить»', () {
      final result = csmParseBootstrap(
        fixture('pos-b1-min'),
        expectedLinkPin: 'ZZZZZZZZZZZZZZZZZZZZ',
      );

      expect(result.failure, CsmEnrollFailure.pinMismatch);
      expect(result.bootstrap, isNull);
    });

    test('директива вместо блоба это не блоб, а не «почти блоб»', () {
      final result = csmParseBootstrap(fixture('pos-m1-min'));

      expect(result.failure, CsmEnrollFailure.notABlob);
    });
  });

  group('отбракованные корпусом байты не заводят профиль', () {
    // Каждый отрицательный вектор типа 0x05 обязан остаться отказом и на
    // уровне энроллмента, с кодом из реестра 03-WIRE.md 6.6.
    final blobRejects = all
        .where((v) => v['verdict'] == 'reject' && v['doc_type'] == 5)
        .toList(growable: false);

    test('в корпусе есть отрицательные векторы 0x05', () {
      expect(blobRejects, isNotEmpty);
    });

    for (final v in blobRejects) {
      final id = v['id'] as String;
      final expected = v['code'] as String?;
      test('$id остаётся отказом', () {
        final bytes = File('${corpus.path}/${v['file']}').readAsBytesSync();
        final result = csmParseBootstrap(bytes, expectedLinkPin: linkPin);

        expect(result.isOk, isFalse, reason: '$id принят как блоб');
        if (expected != null &&
            expected.startsWith('E_PARSE_') &&
            result.code != null) {
          // Отказ разбора обязан нести ТОТ ЖЕ код реестра, что и корпус:
          // согласие в том, что документ отвергнут, но по разным причинам,
          // это и есть способ спрятать настоящее расхождение.
          expect(result.code!.wire, expected, reason: id);
        }
      });
    }
  });

  group('подписанная карта pol корпуса сливается по правилам 7', () {
    Map<int, CsmPolicyEntry> polOf(String id) {
      final parsed = csmParse(fixture(id));
      final doc = parsed.document;
      expect(doc, isA<CsmDirective>());
      return csmReadPolicyMap(doc.payload[19]);
    }

    test('pos-m1-typical: неизвестный пресет игнорируется поштучно', () {
      final pol = polOf('pos-m1-typical');
      expect(pol.keys.toList()..sort(), <int>[1, 2, 8, 11]);

      final merged = csmMergePolicy(
        current: CsmSettings.empty,
        pol: pol,
        nowMs: 1000,
      );

      // `bypass-ru` не входит в девять пресетов ядра. По 02-SPEC.md 7.9
      // последней строке клиент игнорирует РОВНО этот ключ, держит своё
      // значение, записывает событие и НЕ отвергает директиву.
      expect(
        merged.ignored.map((e) => e.wireKey),
        contains(CsmSettingKey.preset.wire),
      );
      expect(
        merged.ignored.firstWhere((e) => e.key == CsmSettingKey.preset).reason,
        CsmIgnoredReason.outsideVocabulary,
      );
      expect(merged.settings.valueOf(CsmSettingKey.preset), isNull);

      // Остальные три ключа применяются, с происхождением из кадра.
      expect(
        (merged.settings.valueOf(CsmSettingKey.protocol)! as CsmText).value,
        'auto',
      );
      expect(
        merged.settings[CsmSettingKey.protocol]!.src,
        CsmProvenance.operator,
      );
      expect(
        (merged.settings.valueOf(CsmSettingKey.killSwitch)! as CsmBoolean)
            .value,
        isTrue,
      );
      expect(
        merged.settings[CsmSettingKey.killSwitch]!.src,
        CsmProvenance.user,
      );
      expect(
        (merged.settings.valueOf(CsmSettingKey.splitMode)! as CsmText).value,
        'off',
      );
      expect(
        merged.settings[CsmSettingKey.splitMode]!.src,
        CsmProvenance.byDefault,
      );
      // Первая доставка значений не поднимает карточек: удерживать нечего.
      expect(merged.cards, isEmpty);
    });

    test('pos-m1-max: все одиннадцать ключей, четыре из них не проходят', () {
      final pol = polOf('pos-m1-max');
      expect(pol.length, 11);

      final merged = csmMergePolicy(
        current: CsmSettings.empty,
        pol: pol,
        nowMs: 1000,
      );

      final byKey = <CsmSettingKey, CsmIgnoredReason>{
        for (final e in merged.ignored)
          if (e.key != null) e.key!: e.reason,
      };

      // `bypass-ru` не пресет ядра.
      expect(byKey[CsmSettingKey.preset], CsmIgnoredReason.outsideVocabulary);
      // `nl` в нижнем регистре: три состояния pol[3] это код ЗАГЛАВНЫМИ, `--`
      // и пустая строка, и ничего больше (02-SPEC.md 7.3).
      expect(byKey[CsmSettingKey.relay], CsmIgnoredReason.outsideVocabulary);
      // `1.0.0.1` это голый IP: резолвер обязан быть https (DoH) или tls
      // (DoT), INV-8 распространяется и на DNS.
      expect(
        byKey[CsmSettingKey.dnsFallback],
        CsmIgnoredReason.outsideVocabulary,
      );
      // ipv6 оператору писать нельзя, а в фикстуре он приходит с src = 2.
      expect(byKey[CsmSettingKey.ipv6], CsmIgnoredReason.operatorMayNotWrite);
      expect(merged.ignored.length, 4);

      // Остальные семь применяются.
      expect(merged.applied, <CsmSettingKey>{
        CsmSettingKey.protocol,
        CsmSettingKey.stack,
        CsmSettingKey.mtu,
        CsmSettingKey.fakeIp,
        CsmSettingKey.killSwitch,
        CsmSettingKey.dnsNameservers,
        CsmSettingKey.splitMode,
      });
      // `stack` оператору писать нельзя, но в фикстуре он приходит с src = 3,
      // то есть как значение по умолчанию тенанта, и это законно.
      expect(
        merged.settings[CsmSettingKey.stack]!.src,
        CsmProvenance.byDefault,
      );
      expect(
        (merged.settings.valueOf(CsmSettingKey.mtu)! as CsmUint).value,
        1420,
      );

      // INV-11: ничего из отброшенного не сохраняется.
      final roundTrip = CsmSettings.fromJson(
        jsonDecode(jsonEncode(merged.settings.toJson())),
      );
      expect(roundTrip.valueOf(CsmSettingKey.preset), isNull);
      expect(roundTrip.valueOf(CsmSettingKey.relay), isNull);
      expect(roundTrip.valueOf(CsmSettingKey.dnsFallback), isNull);
    });

    test('pos-m1-norelay: сентинел -- доезжает как есть', () {
      final parsed = csmParse(fixture('pos-m1-norelay'));
      final directive = parsed.document as CsmDirective;

      expect(directive.selection?.relayCountry, kCsmNoRelay);
      // `NO` это Норвегия, и сентинелом быть не может (Correction 4).
      expect(directive.selection?.relayCountry, isNot('NO'));
    });
  });

  group('поле возможностей корпуса', () {
    test('pos-m1-typical несёт 0x00000fff, пересечение снимает 10 и 11', () {
      final parsed = csmParse(fixture('pos-m1-typical'));
      final directive = parsed.document as CsmDirective;
      final operatorCap = CsmCapabilitySet.fromBytes(directive.capabilities);

      expect(operatorCap.raw, 0x00000fff);
      expect(operatorCap.has(CsmCapability.variantForwarding), isTrue);

      final effective = operatorCap.intersectWithClient();
      // Клиент не реализует ни проброс `variant` (v1 не показывает контрол), ни
      // прыжки по портам, поэтому оба бита уходят в ноль в безопасную сторону.
      expect(effective.has(CsmCapability.variantForwarding), isFalse);
      expect(effective.has(CsmCapability.portHopping), isFalse);
      expect(effective.has(CsmCapability.perNodeMaterial), isTrue);
      expect(effective.raw, 0x000003ff);
    });

    test('бит содержимого без массива в каталоге читается нулём', () {
      final parsed = csmParse(fixture('pos-m1-typical'));
      final directive = parsed.document as CsmDirective;
      final operatorCap = CsmCapabilitySet.fromBytes(directive.capabilities);

      // 02-SPEC.md 6.5: бит, поднятый в директиве, но не подкреплённый
      // массивом в связанном каталоге, это ноль. Возможность так не дарится.
      final resolved = operatorCap.withContentPresence(
        (c) => c != CsmCapability.mirrorPool,
      );
      expect(resolved.has(CsmCapability.mirrorPool), isFalse);
      expect(resolved.has(CsmCapability.dohEndpoints), isTrue);
      // Бит, не входящий в содержимое каталога, не трогается.
      expect(resolved.has(CsmCapability.settingsWrite), isTrue);
    });
  });

  group('лестница, поднятая на возможностях корпуса', () {
    test('ступени без данных оператора видимы и выключены с причиной', () {
      final parsed = csmParse(fixture('pos-m1-typical'));
      final directive = parsed.document as CsmDirective;
      final caps = CsmCapabilitySet.fromBytes(directive.capabilities)
          .intersectWithClient()
          .withContentPresence(
            // У этого профиля пула зеркал нет.
            (c) => c != CsmCapability.mirrorPool,
          );

      expect(
        csmRungReasonFor(CsmRung.mirrors, caps),
        CsmUnavailableReason.notOfferedByOperator,
      );
      expect(csmRungReasonFor(CsmRung.doh, caps), isNull);
    });
  });
}

/// Тонкая обёртка над правилом причин, чтобы тест не тянул слой Riverpod.
CsmUnavailableReason? csmRungReasonFor(CsmRung rung, CsmCapabilitySet caps) {
  switch (rung) {
    case CsmRung.mirrors:
      return caps.has(CsmCapability.mirrorPool)
          ? null
          : CsmUnavailableReason.notOfferedByOperator;
    case CsmRung.doh:
      return caps.has(CsmCapability.dohEndpoints)
          ? null
          : CsmUnavailableReason.notOfferedByOperator;
    case CsmRung.cached:
    case CsmRung.direct:
    case CsmRung.tunnel:
    case CsmRung.userProxy:
    case CsmRung.outOfBand:
      return null;
  }
}

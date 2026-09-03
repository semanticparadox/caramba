import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/codec/base32_crockford.dart';
import 'package:caramba_vpn/src/csm/errors.dart';
import 'package:caramba_vpn/src/csm/frame.dart';
import 'package:caramba_vpn/src/csm/ids.dart';

// Бронированная текстовая форма и разбиение под QR, 03-WIRE.md 10.
//
//   CARCAP1.<bid>.<i>/<n>.<data>
//
// Любой отказ читателя это E_PARSE_FRAMING. Семейства E_ARMOR_* нет и заводить
// его нельзя: вызывающему все шесть условий означают одно и то же, перед ним не
// поток кадров, а различать их значило бы дать трём реализациям ещё шесть
// поводов разойтись без всякой операционной пользы.

const int csmArmorChunkBytes = 620;
const int csmArmorStreamMax = 65536;
const int csmArmorMaxChunks = 106;
const int csmArmorMaxFrames = 16;

class _Line {
  const _Line(this.bid, this.index, this.count, this.data);
  final String bid;
  final int index;
  final int count;
  final String data;
}

Never _armorFail(String detail) =>
    csmFail(CsmErrorCode.parseFraming, 'armor', detail);

_Line _parseLine(String raw) {
  final line = raw.replaceAll(RegExp(r'\s'), '');
  if (line.length < 21) {
    _armorFail('armored line is too short');
  }
  if (line.substring(0, 7).toUpperCase() != 'CARCAP1' || line[7] != '.') {
    _armorFail('armored line does not begin with the CARCAP1 prefix');
  }
  final afterBid = line.indexOf('.', 8);
  if (afterBid != 16) {
    _armorFail('bid field is not exactly 8 characters');
  }
  final bid = line.substring(8, 16);
  final slash = line.indexOf('/', 17);
  if (slash < 0) {
    _armorFail('armored line carries no ordinal separator');
  }
  final afterCount = line.indexOf('.', slash + 1);
  if (afterCount < 0) {
    _armorFail('armored line carries no data separator');
  }
  final indexText = line.substring(17, slash);
  final countText = line.substring(slash + 1, afterCount);
  final index = _decimal(indexText);
  final count = _decimal(countText);
  if (index < 1 || count < 1 || count > csmArmorMaxChunks || index > count) {
    _armorFail('ordinal $index of $count is outside the legal range');
  }
  return _Line(bid, index, count, line.substring(afterCount + 1));
}

int _decimal(String text) {
  if (text.isEmpty || text.length > 3) {
    _armorFail('ordinal field is empty or too long');
  }
  if (text.length > 1 && text[0] == '0') {
    _armorFail('ordinal field carries a leading zero');
  }
  for (final c in text.codeUnits) {
    if (c < 0x30 || c > 0x39) {
      _armorFail('ordinal field is not decimal');
    }
  }
  return int.parse(text);
}

/// Читает набор бронированных строк и возвращает поток кадров. Порядок строк
/// произвольный; пропущенный порядковый номер обнаруживается, а не
/// склеивается молча.
Uint8List csmArmorDecode(Iterable<String> lines) {
  final parsed = <_Line>[];
  for (final raw in lines) {
    if (raw.trim().isEmpty) {
      continue;
    }
    parsed.add(_parseLine(raw));
  }
  if (parsed.isEmpty) {
    _armorFail('no armored lines');
  }

  final bid = parsed.first.bid;
  final count = parsed.first.count;
  final slices = <int, Uint8List>{};
  for (final line in parsed) {
    if (line.bid != bid || line.count != count) {
      _armorFail('armored set disagrees on bid or on the chunk count');
    }
    final raw = base32CrockfordDecode(line.data);
    if (raw == null) {
      _armorFail(
        'chunk ${line.index} carries a non-Crockford character or non-zero '
        'trailing pad bits',
      );
    }
    if (line.index != count && raw.length != csmArmorChunkBytes) {
      _armorFail(
        'non-final chunk ${line.index} decodes to ${raw.length} bytes where '
        '$csmArmorChunkBytes is required',
      );
    }
    final existing = slices[line.index];
    if (existing != null && !csmBytesEqual(existing, raw)) {
      _armorFail('ordinal ${line.index} appears twice with different data');
    }
    slices[line.index] = raw;
  }
  for (var i = 1; i <= count; i++) {
    if (!slices.containsKey(i)) {
      _armorFail('ordinal $i of $count is missing');
    }
  }

  final joined = <int>[];
  for (var i = 1; i <= count; i++) {
    joined.addAll(slices[i]!);
  }
  if (joined.length > csmArmorStreamMax) {
    _armorFail('frame stream exceeds the $csmArmorStreamMax byte cap');
  }
  final stream = Uint8List.fromList(joined);
  // Сверка bid делается ДО разбора: без неё сканер не отличит смешанные чанки
  // двух разных наборов, а это самый реальный отказ на практике.
  if (csmBundleId(stream) != bid) {
    _armorFail('bid does not match base32_crockford(sha256(stream)[0..5])');
  }
  return stream;
}

/// Читает бронированный текст целиком и возвращает уже разобранные кадры.
List<Uint8List> csmArmorDecodeFrames(String text) {
  final stream = csmArmorDecode(text.split('\n'));
  final frames = csmSplitFrames(stream);
  if (frames.length > csmArmorMaxFrames) {
    _armorFail('frame stream carries ${frames.length} frames, cap is '
        '$csmArmorMaxFrames');
  }
  return frames;
}

/// Кодирование потока кадров в бронированные строки.
List<String> csmArmorEncode(Uint8List stream) {
  if (stream.length > csmArmorStreamMax) {
    _armorFail('frame stream exceeds the $csmArmorStreamMax byte cap');
  }
  final bid = csmBundleId(stream);
  var count = (stream.length + csmArmorChunkBytes - 1) ~/ csmArmorChunkBytes;
  if (count == 0) {
    count = 1;
  }
  if (count > csmArmorMaxChunks) {
    _armorFail('frame stream needs $count chunks, cap is $csmArmorMaxChunks');
  }
  final out = <String>[];
  for (var i = 1; i <= count; i++) {
    final low = (i - 1) * csmArmorChunkBytes;
    var high = low + csmArmorChunkBytes;
    if (high > stream.length) {
      high = stream.length;
    }
    final data = base32CrockfordEncode(stream.sublist(low, high));
    out.add('CARCAP1.$bid.$i/$count.$data');
  }
  return out;
}

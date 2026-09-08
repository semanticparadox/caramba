/// Привязка состояния CSM к активному профилю подключения.
///
/// Нормативно: 02-SPEC.md 1.2 («каждое хранилище состояния профиля ОБЯЗАНО
/// ключеваться по `pid`»), 7.10 пункт 2 (история попыток локальна), 7.7.1
/// (проверенный набор ресурсов это состояние профиля).
///
/// Без этой привязки состояние CSM переживало смену профиля целиком: страж
/// каталога, курсор истории попыток, сама история и факты о транспорте лежали
/// приложением, а не профилем. Последствия были не косметические. Проверенный
/// отпечаток каталога оператора A решал, поднимать ли карточку на ПЕРВЫЙ
/// каталог оператора B; история попыток A рисовалась на экране транспортов B,
/// хотя заголовок того экрана обещает историю этого профиля; а нативное
/// хранилище CSM было одно на приложение, то есть закреплённый корень,
/// регистрация устройства и монотонные отметки второго оператора ложились
/// поверх первого.
library;

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/features/csm/attempt_history.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/csm_catalog_guard.dart';
import 'package:caramba_client/state/csm_ladder_sync.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/providers.dart';

/// Ключ хранилища CSM в ядре для профиля [profileId].
///
/// Ядро принимает `[a-z0-9_-]` длиной до 64 и складывает ключ в путь, поэтому
/// всё остальное здесь и вычищается: ключ, содержащий точку или косую черту,
/// увёл бы хранилище личности устройства за пределы рабочего каталога.
///
/// Пустой или непригодный идентификатор даёт пустой ключ, а он означает
/// «единственное хранилище в рабочем каталоге», как у установок, заведённых до
/// появления второго оператора.
String csmCoreProfileKey(String? profileId) {
  if (profileId == null || profileId.isEmpty) {
    return '';
  }
  final sb = StringBuffer();
  for (final unit in profileId.toLowerCase().codeUnits) {
    final ch = String.fromCharCode(unit);
    final isDigit = unit >= 0x30 && unit <= 0x39;
    final isLower = unit >= 0x61 && unit <= 0x7a;
    if (isDigit || isLower || ch == '-' || ch == '_') {
      sb.write(ch);
    } else {
      sb.write('-');
    }
    if (sb.length == 64) {
      break;
    }
  }
  return sb.toString();
}

/// Держит состояние CSM синхронным с активным профилем.
class CsmProfileBinder {
  CsmProfileBinder(this._ref);

  final Ref _ref;

  String? _bound;

  /// Профиль, к которому состояние привязано сейчас.
  String? get boundProfileId => _bound;

  /// Переключает всё состояние CSM на профиль [profileId].
  ///
  /// Идемпотентно: повторный вызов с тем же идентификатором ничего не делает,
  /// поэтому вызывать его на каждой пересборке безопасно.
  ///
  /// Порядок такой: сначала выбрасывается то, что принадлежит прежнему
  /// профилю, затем страж поднимает корзину нового, и только потом ядру
  /// говорят, какое хранилище открывать. Обратный порядок оставил бы окно, в
  /// котором ядро уже отвечает за новый профиль, а экраны ещё показывают
  /// прежний.
  Future<void> bind(String? profileId) async {
    if (_bound == profileId) {
      return;
    }
    _bound = profileId;

    // История попыток ЛОКАЛЬНА и хранится на профиль (02-SPEC.md 7.10).
    _ref.read(csmLadderSyncProvider).reset();
    _ref.read(csmAttemptHistoryProvider.notifier).clear();
    // Факты о транспорте приходят из ядра нового профиля, а до первого подъёма
    // их нет. Ложь здесь безопасна только в одну сторону, и это она.
    _ref.read(csmTransportFactsProvider.notifier).state =
        const CsmTransportFacts();
    // Проверенный набор ресурсов и висящие карточки лежат в корзине профиля, и
    // карточка, на которую ещё не ответили, обязана пережить переключение.
    _ref.read(csmCatalogGuardProvider.notifier).bindProfile(profileId);

    try {
      await _ref
          .read(vpnConnectionProvider)
          .csmSelectProfile(csmCoreProfileKey(profileId));
    } on Object {
      // Сборка без ABI v3 либо мост, которого на этой платформе нет. Экраны
      // остаются с тем, что уже поднято, и ничего не выдумывают.
    }
  }
}

final csmProfileBinderProvider = Provider<CsmProfileBinder>(
  CsmProfileBinder.new,
);

/// Наблюдатель, который и запускает привязку.
///
/// Провайдер сам следит за активным профилем, поэтому смотреть на него надо
/// ровно один раз, в корне приложения: без наблюдателя привязка не случалась бы
/// вовсе, а привязка, живущая на экране, не случалась бы, пока экран закрыт.
final csmProfileBindingProvider = Provider<String?>((ref) {
  final id = ref.watch(activeConnectionProfileProvider)?.id;
  // Привязка уезжает в микрозадачу: провайдеру запрещено менять другие
  // провайдеры во время собственной сборки, а привязка меняет сразу четыре.
  // Задержка на один тик здесь безвредна, потому что до неё не происходит
  // ничего, что читало бы состояние нового профиля.
  Future<void>.microtask(() {
    try {
      ref.read(csmProfileBinderProvider).bind(id);
    } on Object {
      // Контейнер успели закрыть между кадром и микрозадачей.
    }
  });
  return id;
});

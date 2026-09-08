/// Какой файл значка показать в строке меню (трее) на каждой стадии.
///
/// ЗАЧЕМ отдельный чистый файл. Значок это единственное, что видно от
/// приложения при спрятанном окне, и он обязан отвечать на вопрос «работает
/// или нет» без открытия меню. Правило выбора картинки поэтому не должно жить
/// внутри сервиса вперемешку с подписками: там его не проверить, а разъехаться
/// с меню оно может незаметно.
///
/// Пять состояний, а не пять стадий: `connecting` и `reconnecting` для глаза
/// одно и то же («идёт работа»), а `connected` при закрытом доступе честнее
/// показать отдельной картинкой, чем щитом, который в этот момент врёт.
library;

import 'package:flutter/foundation.dart';

import 'package:caramba_client/vpn/vpn_status.dart';

/// Имена файлов совпадают с тем, что генерирует `scripts/render-tray-icons.sh`.
const String _idle = 'idle';
const String _busy = 'busy';
const String _connected = 'connected';
const String _blocked = 'blocked';
const String _error = 'error';

/// Состояние значка: одно слово, из которого собирается имя файла.
///
/// Отдельная функция от [trayIconPath] потому, что состояние проверяется
/// тестом само по себе, а путь это уже вопрос платформы и расширения.
String trayIconState(
  VpnStage stage, {
  required bool accessBlocked,
  required bool noConnections,
}) {
  // Подключаться некуда: значок обязан выглядеть как «выключено», а не как
  // ошибка. Отсутствие профиля это не поломка.
  if (noConnections) return _idle;
  return switch (stage) {
    VpnStage.disconnected => _idle,
    VpnStage.connecting => _busy,
    VpnStage.reconnecting => _busy,
    VpnStage.connected => accessBlocked ? _blocked : _connected,
    VpnStage.error => _error,
  };
}

/// Путь ассета значка для этой стадии и этой платформы.
///
/// Windows берёт `.ico` (система сама достаёт из него нужный размер), macOS и
/// Linux берут PNG. Каталоги ровно те три, что объявлены в `pubspec.yaml`:
/// исходный SVG в бандл не попадает.
String trayIconPath(
  VpnStage stage, {
  required bool accessBlocked,
  required bool noConnections,
  required TargetPlatform platform,
}) {
  final state = trayIconState(
    stage,
    accessBlocked: accessBlocked,
    noConnections: noConnections,
  );
  return switch (platform) {
    TargetPlatform.macOS => 'assets/tray/mac/$state.png',
    TargetPlatform.windows => 'assets/tray/win/$state.ico',
    // Не только Linux: на любой другой платформе десктопного значка нет вовсе,
    // и падать здесь незачем. Путь остаётся валидным, значок просто не
    // ставится, потому что сервис на этих платформах не поднимается.
    _ => 'assets/tray/linux/$state.png',
  };
}

/// Отдавать ли картинку системе как template.
///
/// Только macOS: там строка меню перекрашивает чёрно-прозрачный шаблон под
/// светлую и тёмную тему и под выделение сама. На Windows и Linux template
/// нет, и цвет несёт сама картинка.
bool trayIconIsTemplate(TargetPlatform platform) =>
    platform == TargetPlatform.macOS;

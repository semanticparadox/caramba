/// Сшивка двух половин флота: ПРЕДЛОЖЕНИЯ (машины и их инбаунды) и ИНВЕНТАРЯ
/// (то, чем выбор закрепляется).
///
/// Эти половины отвечают на разные вопросы и потому не сливаются в одну.
/// [Offering] знает, что такое машина: в теле подписки узла как сущности нет
/// вовсе, и восстановить её можно только по одинаковому адресу `server:`.
/// [ExitInventory] знает, чем закрепляется выбор: у панели это `node_id`, у
/// импорта — имя прокси, которое читает `connectRaw`. Экрану нужно и то и
/// другое: показать машину, а закрепить узел.
///
/// Функции здесь чистые, поэтому «восемь инбаундов одной машины это один
/// сервер» проверяется тестом, а не разглядыванием списка на устройстве.
library;

import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';

/// Описывают ли обе половины ОДИН источник.
///
/// Разошедшиеся половины хуже отсутствующей: экран показал бы машины одного
/// источника со списком выбора другого, и нажатие ушло бы не туда. Поэтому
/// при расхождении экран остаётся на узлах инвентаря — беднее, но правдиво.
bool fleetSourcesAgree(ExitInventorySource inventory, OfferingSource offering) {
  return switch (inventory) {
    ExitInventorySource.panelRest => offering == OfferingSource.panelRest,
    ExitInventorySource.importedSub =>
      offering == OfferingSource.subscriptionBody,
    ExitInventorySource.csmCatalog => offering == OfferingSource.csmCatalog,
    ExitInventorySource.none => false,
  };
}

/// Узел, которым закрепляется эта машина; `null` — в списке выбора её нет.
///
/// Панель: ключ машины и есть ключ узла (`nodes.id`). Импорт: ключ машины это
/// адрес, а закрепляется ИМЯ ПРОКСИ, поэтому берётся первый инбаунд, который
/// действительно доезжает до конфига, — закреплять имя, которого в теле нет,
/// значило бы показать галочку на выборе, который ядро не увидит.
ExitNode? nodeForExit(ExitOffer exit, List<ExitNode> nodes) {
  ExitNode? byKey(String key) {
    for (final n in nodes) {
      if (n.key == key) return n;
    }
    return null;
  }

  final direct = byKey(exit.key);
  if (direct != null) return direct;

  for (final i in exit.liveInbounds) {
    final name = i.proxyName;
    if (name == null || name.isEmpty) continue;
    final n = byKey(name);
    if (n != null) return n;
  }
  // Ни один живой инбаунд не нашёлся: пробуем остальные — узел, у которого
  // прокси в теле нет, всё равно лучше молчаливо неработающей строки, а его
  // непригодность придёт из самого узла.
  for (final i in exit.inbounds) {
    final name = i.proxyName;
    if (name == null || name.isEmpty) continue;
    final n = byKey(name);
    if (n != null) return n;
  }
  return null;
}

/// Стоит ли закреплённый ключ на ЭТОЙ машине.
///
/// В импорте пин это имя прокси, то есть один из инбаундов машины: сравнение
/// только по ключу машины оставило бы список без единой галочки ровно там, где
/// выбор сделан.
bool exitHoldsKey(ExitOffer exit, String? key) {
  if (key == null || key.isEmpty) return false;
  if (exit.key == key) return true;
  for (final i in exit.inbounds) {
    if (i.proxyName == key) return true;
  }
  return false;
}

/// Имя машины для строки списка.
///
/// У панели это имя узла. У импорта имени машины не существует — есть адрес и
/// есть имена прокси; когда прокси ровно один, его имя И ЕСТЬ единственное
/// имя этой машины, и показывать вместо него голый адрес значит терять то
/// немногое, что источник сказал. Как только прокси больше одного, общего
/// имени у них нет, и строкой становится адрес — иначе машина назвалась бы
/// именем случайного своего инбаунда.
String machineTitleOf(ExitOffer exit) {
  if (exit.inbounds.length == 1) {
    final tag = exit.inbounds.single.tag.trim();
    if (tag.isNotEmpty) return tag;
  }
  final label = exit.label.trim();
  return label.isEmpty ? exit.key : label;
}

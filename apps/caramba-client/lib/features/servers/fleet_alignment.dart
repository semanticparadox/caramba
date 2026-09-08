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
/// У ПАНЕЛИ имя узла есть, его дал оператор, и оно сильнее всего. Проверка на
/// это идёт ПЕРВОЙ: пока правило про единственный инбаунд стояло раньше,
/// панельный узел с одним инбаундом назывался тегом этого инбаунда —
/// «vless-in» вместо «Node #5». Пока выбор шёл через страну, строку машины
/// почти никто не видел; в плоском списке это стало заголовком, и ошибка
/// вылезла.
///
/// У ИМПОРТА имени машины не существует — есть адрес и есть имена прокси;
/// когда прокси ровно один, его имя И ЕСТЬ единственное имя этой машины, и
/// показывать вместо него голый адрес значит терять то немногое, что источник
/// сказал. Как только прокси больше одного, общего имени у них нет, и строкой
/// становится адрес — иначе машина назвалась бы именем случайного своего
/// инбаунда.
String machineTitleOf(ExitOffer exit) {
  final label = exit.label.trim();
  // `panelNodeId` не пуст ровно на панельном пути — там `label` это имя узла,
  // а не адрес, подставленный за неимением имени.
  if (exit.panelNodeId != null && label.isNotEmpty) return label;
  if (exit.inbounds.length == 1) {
    final tag = exit.inbounds.single.tag.trim();
    if (tag.isNotEmpty) return tag;
  }
  // Дальше `label` НЕ рассматривается: вне панели он не имя. Сборщик
  // импортированного тела кладёт в него `host` (offering_builder.dart:214) —
  // тот же адрес, что и в ключе, — потому что другого поля у него нет. Взять
  // его заголовком значит показать человеку «85.215.196.151» вместо имени и
  // заодно вынести адрес узла на экран без нужды.
  //
  // Остаётся СТРАНА. Она честна: источник её знает, и в стране машин обычно
  // одна. Тёзок разводит [disambiguateTitles].
  final country = exit.countryName.trim();
  return country.isEmpty ? exit.key : country;
}

/// Нумерует одинаковые заголовки в пределах списка.
///
/// Имени машины на импортированном пути не существует, и заголовком становится
/// страна; две машины одной страны назывались бы одинаково, и выбор превратился
/// бы в угадывание. Номер приписывается ТОЛЬКО повторам: единственная машина
/// страны остаётся просто «Германией».
List<String> disambiguateTitles(List<String> titles) {
  final seen = <String, int>{};
  for (final t in titles) {
    seen[t] = (seen[t] ?? 0) + 1;
  }
  final used = <String, int>{};
  return <String>[
    for (final t in titles)
      if ((seen[t] ?? 0) < 2) t else '$t #${used[t] = (used[t] ?? 0) + 1}',
  ];
}

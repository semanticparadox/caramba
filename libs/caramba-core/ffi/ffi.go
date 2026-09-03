//go:build cgo

// Package main — cgo c-shared фасад caramba-core для десктопных Flutter-сборок
// (Linux/macOS/Windows) через dart:ffi.
//
// Зачем отдельный от mobile/ слой. Мобильные платформы получают нативные привязки
// через gomobile bind (mobile.Client → AAR/xcframework). На десктопе Flutter
// встраивает ядро как нативную библиотеку и зовёт C-функции через dart:ffi —
// gomobile тут не подходит. Этот пакет компилируется в разделяемую библиотеку:
//
//	CGO_ENABLED=1 go build -tags mihomo -buildmode=c-shared \
//	    -o libcaramba_core.{so,dll,dylib} ./ffi
//
// Сборка также генерирует C-заголовок (по умолчанию рядом, имя = -o без
// расширения + .h); каноничный заголовок зафиксирован в ffi/caramba_core.h и его
// вендорит плагин (см. scripts/build-desktop-lib.sh).
//
// Контракт совпадает с mobile.Client: те же стадии (disconnected/connecting/
// connected/reconnecting/error) и те же формы JSON статуса и трафика, чтобы
// нативный плагин на всех платформах читал одинаковые карты.
//
// Модель владения. API основан на хэндлах: CarambaNew создаёт экземпляр ядра и
// возвращает непрозрачный long-хэндл; последующие вызовы передают его обратно.
// Все возвращаемые char* выделены в Go (C.CString) и принадлежат вызывающему —
// он ОБЯЗАН освободить их через CarambaFreeString. Хэндлы освобождаются
// CarambaFree (он же гасит туннель). Несуществующий хэндл → JSON {"error":...}.
//
// TUN на десктопе. Десктоп НЕ передаёт fd: mihomo сам поднимает TUN-устройство
// (wintun на Windows, utun на macOS, tun на Linux). Поэтому tunFd по умолчанию
// -1; CarambaSetTunFd оставлен для симметрии/тестов, но в норме не вызывается.
// Подъём TUN требует прав (root/CAP_NET_ADMIN на Linux, админ на Windows); на
// Windows рядом с библиотекой должен лежать wintun.dll.
package main

/*
#include <stdlib.h>
*/
import "C"

import (
	"encoding/json"
	"sync"
	"unsafe"

	"github.com/semanticparadox/caramba/libs/caramba-core/mobile"
)

func main() {} // обязателен для -buildmode=c-shared, не вызывается.

// registry — потокобезопасный реестр живых клиентов по числовому хэндлу.
var registry = struct {
	mu   sync.Mutex
	next int64
	byID map[int64]*mobile.Client
}{byID: make(map[int64]*mobile.Client)}

// lookup возвращает клиента по хэндлу (или nil).
func lookup(h C.long) *mobile.Client {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	return registry.byID[int64(h)]
}

// errJSON формирует C-строку с JSON-ошибкой {"error":"..."}. Владение — у
// вызывающего (CarambaFreeString).
func errJSON(msg string) *C.char {
	b, _ := json.Marshal(struct {
		Error string `json:"error"`
	}{Error: msg})
	return C.CString(string(b))
}

// okOrErr возвращает s как C-строку, либо JSON-ошибку, если err != nil.
func okOrErr(s string, err error) *C.char {
	if err != nil {
		return errJSON(err.Error())
	}
	return C.CString(s)
}

// CarambaNew создаёт экземпляр ядра и возвращает хэндл (>0) либо 0 при ошибке
// инициализации. panelURL обязателен; subURL/workDir/tokenPath могут быть пустыми
// (тогда — значения по умолчанию). Параметры — C-строки UTF-8.
//
//export CarambaNew
func CarambaNew(panelURL, subURL, workDir, tokenPath *C.char) C.long {
	cl, err := mobile.NewClient(C.GoString(panelURL), C.GoString(subURL), C.GoString(workDir), C.GoString(tokenPath))
	if err != nil || cl == nil {
		return 0
	}
	registry.mu.Lock()
	registry.next++
	id := registry.next
	registry.byID[id] = cl
	registry.mu.Unlock()
	return C.long(id)
}

// CarambaConfigure разрешает шов аутентификации: инъецирует JWT приложения и UUID
// подписки в ядро (см. mobile.Client.Configure). Возвращает NULL при успехе либо
// C-строку с JSON-ошибкой (владелец освобождает её CarambaFreeString).
//
//export CarambaConfigure
func CarambaConfigure(h C.long, panelURL, subscriptionID, accessToken *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	if err := cl.Configure(C.GoString(panelURL), C.GoString(subscriptionID), C.GoString(accessToken)); err != nil {
		return errJSON(err.Error())
	}
	return nil
}

// CarambaImportSubscription импортирует сырую подписку формата format (auto/clash/
// singbox/v2ray/uri) и сохраняет её как источник подключения (см.
// mobile.Client.ImportSubscription). После этого CarambaUp(h, "") поднимает
// туннель из импортированного конфига без панели и без входа. Возвращает JSON
// метаданных подписки либо JSON-ошибку (владелец освобождает строку через
// CarambaFreeString).
//
//export CarambaImportSubscription
func CarambaImportSubscription(h C.long, raw, format *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	return okOrErr(cl.ImportSubscription(C.GoString(raw), C.GoString(format)))
}

// CarambaSetTunnelMode переключает способ захвата трафика (см.
// mobile.Client.SetTunnelMode): mode = "tun" (или "" / NULL) — системный TUN,
// требующий прав; mode = "proxy" — локальный mixed-инбаунд (SOCKS5+HTTP) на
// 127.0.0.1:port БЕЗ каких-либо привилегий. port <= 0 оставляет порт по
// умолчанию (7890) и значим только в proxy-режиме.
//
// Применяется при следующем CarambaUp. Возвращает NULL при успехе либо
// JSON-ошибку (владелец освобождает строку через CarambaFreeString). После
// переключения CarambaStatus отдаёт поля "mode" и (в proxy-режиме) "mixedPort".
//
//export CarambaSetTunnelMode
func CarambaSetTunnelMode(h C.long, mode *C.char, port C.int) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	if err := cl.SetTunnelMode(C.GoString(mode), int(port)); err != nil {
		return errJSON(err.Error())
	}
	return nil
}

// CarambaSetPolicy применяет политику подключения одной JSON-строкой (см.
// mobile.Client.SetPolicyJSON): protocol, preset, relay, stack, mtu, ipv6,
// fakeIp, killSwitch, dns.{nameservers,fallback}, split.{mode,apps,bypassDomains}.
// Все поля опциональны, неизвестные ключи игнорируются; недопустимое значение
// перечислимого поля возвращает JSON-ошибку с именем этого поля и политику не
// меняет.
//
// Применяется при следующем CarambaUp: если туннель уже поднят, приложение
// обязано переподключиться (CarambaDown + CarambaUp). Возвращает NULL при успехе
// либо JSON-ошибку (владелец освобождает строку через CarambaFreeString).
//
//export CarambaSetPolicy
func CarambaSetPolicy(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	if err := cl.SetPolicyJSON(C.GoString(jsonStr)); err != nil {
		return errJSON(err.Error())
	}
	return nil
}

// CarambaProbe меряет задержку до каждого узла ТЕКУЩЕЙ загруженной конфигурации
// (импортированной подписки либо последнего загруженного профиля панели), не
// поднимая туннель. Возвращает JSON
// {"servers":[{"id","name","type","server","port","country","latencyMs"}]}
// (latencyMs = -1 — узел не ответил) либо JSON-ошибку. Если ничего не загружено,
// вернётся {"servers":[]}. timeoutMs <= 0 — таймаут по умолчанию (3000).
// Владелец освобождает строку через CarambaFreeString.
//
//export CarambaProbe
func CarambaProbe(h C.long, timeoutMs C.int) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	return okOrErr(cl.ProbeJSON(int(timeoutMs)))
}

// CarambaSetTunFd пробрасывает TUN fd в ядро (как на мобильных). На десктопе в
// норме НЕ вызывается: при fd=-1 mihomo поднимает TUN сам. Возвращает NULL при
// успехе либо JSON-ошибку.
//
//export CarambaSetTunFd
func CarambaSetTunFd(h C.long, fd C.int) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	if err := cl.SetTunFd(int(fd)); err != nil {
		return errJSON(err.Error())
	}
	return nil
}

// CarambaUp поднимает туннель и возвращает JSON api.UpResult (или JSON-ошибку).
// serverID необязателен (пусто — выбор панели/автоматика). Для импортированной
// подписки непустой serverID — это ИМЯ узла (поле id из метаданных
// CarambaImportSubscription), которое закрепляется выбором по умолчанию в
// селекторе CARAMBA. Владелец освобождает строку.
//
//export CarambaUp
func CarambaUp(h C.long, serverID *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	return okOrErr(cl.Up(C.GoString(serverID)))
}

// CarambaDown останавливает туннель. Возвращает NULL при успехе либо JSON-ошибку.
//
//export CarambaDown
func CarambaDown(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	if err := cl.Down(); err != nil {
		return errJSON(err.Error())
	}
	return nil
}

// CarambaStatus возвращает плоский JSON статуса
// {stage,detail?,connectedSinceMs,activeProxy?} (CHANNEL CONTRACT), пригодный
// для status-канала плагина. activeProxy — имя узла, выбранного в селекторе
// CARAMBA; присутствует только когда туннель поднят. Владелец освобождает строку.
//
//export CarambaStatus
func CarambaStatus(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	return okOrErr(cl.StatusJSON())
}

// CarambaTraffic возвращает плоский JSON счётчиков {downBps,upBps,downTotal,
// upTotal} (CHANNEL CONTRACT). Десктопный плагин опрашивает ~1 Гц. Владелец
// освобождает строку.
//
//export CarambaTraffic
func CarambaTraffic(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	return okOrErr(cl.TrafficJSON())
}

// CarambaFree гасит туннель и освобождает экземпляр ядра по хэндлу. После вызова
// хэндл недействителен. Безопасен при неизвестном хэндле (ничего не делает).
//
//export CarambaFree
func CarambaFree(h C.long) {
	registry.mu.Lock()
	cl := registry.byID[int64(h)]
	delete(registry.byID, int64(h))
	registry.mu.Unlock()
	if cl != nil {
		_ = cl.Down()
	}
}

// CarambaFreeString освобождает C-строку, ранее возвращённую любой Caramba*-
// функцией. Вызывать ровно один раз на каждую ненулевую возвращённую строку.
//
//export CarambaFreeString
func CarambaFreeString(s *C.char) {
	if s != nil {
		C.free(unsafe.Pointer(s))
	}
}

// --- CSM/1: подписанный манифест и лестница транспортов (ABI v3) ---
//
// Каждый символ принимает и отдаёт JSON строкой, как CarambaSetPolicy.
// Отсутствующий символ обрабатывается тем же CarambaCoreMissingSymbol, что и
// setPolicy с probe сегодня: клиент против старой библиотеки деградирует до
// "CSM недоступен в этой сборке", а не падает.

// CarambaCsmEnroll регистрирует профиль из bootstrap blob либо из origin, кода
// и пина. Возвращает снимок проверенного состояния или {"error":...}.
//
//export CarambaCsmEnroll
func CarambaCsmEnroll(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmEnroll(C.GoString(jsonStr)))
}

// CarambaCsmRefresh выполняет один цикл выборки документов. Отказ не означает
// потерю конфигурации: профиль остаётся на кешированных документах.
//
//export CarambaCsmRefresh
func CarambaCsmRefresh(h C.long, timeoutSec C.int) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmRefresh(int(timeoutSec)))
}

// CarambaCsmState отдаёт личность оператора, состояние проверки документов,
// битовое поле возможностей и возраст конфигурации.
//
//export CarambaCsmState
func CarambaCsmState(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmState())
}

// CarambaCsmLadder отдаёт все скомпилированные ступени и историю попыток.
//
//export CarambaCsmLadder
func CarambaCsmLadder(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmLadder())
}

// CarambaCsmFleet отдаёт флот доверенного каталога: выходы, входы и запись
// relay_chaining. Узлы приходят проекцией (id, ярлык, страна, форма протокола,
// ребро rl) без материала подключения, а отозванный узел — помеченным
// available=false, а не выброшенным из списка.
//
//export CarambaCsmFleet
func CarambaCsmFleet(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmFleet())
}

// CarambaCapabilities отдаёт путь ("raw"|"panel") и то, что на нём доступно, с
// машинной причиной недоступности. Без этого символа обвязка либо прячет
// недоступный элемент управления, либо объясняет его недоступность собственной
// копией правила.
//
//export CarambaCapabilities
func CarambaCapabilities(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.Capabilities())
}

// CarambaCsmSetLadder применяет переключатели и порядок от пользователя.
//
//export CarambaCsmSetLadder
func CarambaCsmSetLadder(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	if err := cl.CsmSetLadder(C.GoString(jsonStr)); err != nil {
		return errJSON(err.Error())
	}
	return C.CString(`{"ok":true}`)
}

// CarambaCsmRequestSettings отправляет изменение настроек как подписанный
// запрос и принимает подписанный ответ как новую директиву.
//
//export CarambaCsmRequestSettings
func CarambaCsmRequestSettings(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmRequestSettings(C.GoString(jsonStr)))
}

// CarambaCsmSelectProfile переключает хранилище CSM на профиль key
// (02-SPEC.md 1.2). Пустой ключ означает единственное хранилище в рабочем
// каталоге, как у установок, заведённых до появления второго оператора.
//
//export CarambaCsmSelectProfile
func CarambaCsmSelectProfile(h C.long, key *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	if err := cl.CsmSelectProfile(C.GoString(key)); err != nil {
		return errJSON(err.Error())
	}
	return C.CString(`{"ok":true}`)
}

// CarambaCsmAnswerCatalogChange передаёт ответ пользователя на карточку смены
// набора rule-set и geo-файлов (02-SPEC.md 7.7.1). Вход {"accept":bool}, выход
// {"answered":bool}.
//
// Без этого символа кнопка "Оставить прежние" не откатывает ничего: страж
// ресурсов живёт в ядре, и ответ, оставшийся в слое Dart, меняет только то, о
// чём приложение спросит в следующий раз.
//
//export CarambaCsmAnswerCatalogChange
func CarambaCsmAnswerCatalogChange(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmAnswerCatalogChange(C.GoString(jsonStr)))
}

// CarambaLoopbackProxyURL отдаёт адрес служебного инбаунда на петле вместе с
// парой логин-пароль текущего подъёма, или пустую строку.
//
//export CarambaLoopbackProxyURL
func CarambaLoopbackProxyURL(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return C.CString("")
	}
	return C.CString(cl.LoopbackProxyURL())
}

// CarambaLadderRequest выполняет произвольный HTTP запрос через лестницу.
//
//export CarambaLadderRequest
func CarambaLadderRequest(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.LadderRequest(C.GoString(jsonStr)))
}

// --- CSM/1: ключи устройства (ABI v3) ---
//
// На десктопе платформенного хранилища ключей нет, поэтому ядро держит их
// программно и ЧЕСТНО докладывает уровень 3. Символы всё равно экспортируются:
// поверхность одна на всех пяти мостах, и приложение не разветвляется по
// платформе ради того, чтобы узнать отпечаток своего устройства.

// CarambaDeviceKeygen заводит или отдаёт уже заведённую личность устройства.
// Вход {"purpose":"sign"|"agree","require_hardware":bool}, выход
// {"spki_b64","agree_pub_b64","dtp_hex","tier":1|2|3,"generation":n}.
// Идемпотентен: повторный вызов отдаёт тот же dtp.
//
//export CarambaDeviceKeygen
func CarambaDeviceKeygen(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.DeviceKeygen(C.GoString(jsonStr)))
}

// CarambaDeviceSign подписывает СООБЩЕНИЕ (не дайджест) ключом подписи
// устройства. Вход {"message_b64"}, выход {"sig_b64","proof_header"}:
// 64 байта r || s с низким s и то же значение как base64url без дополнения,
// готовое для заголовка X-CSM-Proof (03-WIRE.md 13.6).
//
//export CarambaDeviceSign
func CarambaDeviceSign(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.DeviceSign(C.GoString(jsonStr)))
}

// CarambaDeviceAgree выполняет ECDH ключом согласования устройства.
// Вход {"rkv":n,"peer_pub_b64","kdf_info_b64"}, выход
// {"shared_b64","own_pub_b64"}.
//
//export CarambaDeviceAgree
func CarambaDeviceAgree(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.DeviceAgree(C.GoString(jsonStr)))
}

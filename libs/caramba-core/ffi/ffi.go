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

//export CarambaNew
//
// CarambaNew создаёт экземпляр ядра и возвращает хэндл (>0) либо 0 при ошибке
// инициализации. panelURL обязателен; subURL/workDir/tokenPath могут быть пустыми
// (тогда — значения по умолчанию). Параметры — C-строки UTF-8.
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

//export CarambaConfigure
//
// CarambaConfigure разрешает шов аутентификации: инъецирует JWT приложения и UUID
// подписки в ядро (см. mobile.Client.Configure). Возвращает NULL при успехе либо
// C-строку с JSON-ошибкой (владелец освобождает её CarambaFreeString).
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

//export CarambaImportSubscription
//
// CarambaImportSubscription импортирует сырую подписку формата format (auto/clash/
// singbox/v2ray/uri) и сохраняет её как источник подключения (см.
// mobile.Client.ImportSubscription). После этого CarambaUp(h, "") поднимает
// туннель из импортированного конфига без панели и без входа. Возвращает JSON
// метаданных подписки либо JSON-ошибку (владелец освобождает строку через
// CarambaFreeString).
func CarambaImportSubscription(h C.long, raw, format *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	return okOrErr(cl.ImportSubscription(C.GoString(raw), C.GoString(format)))
}

//export CarambaSetTunnelMode
//
// CarambaSetTunnelMode переключает способ захвата трафика (см.
// mobile.Client.SetTunnelMode): mode = "tun" (или "" / NULL) — системный TUN,
// требующий прав; mode = "proxy" — локальный mixed-инбаунд (SOCKS5+HTTP) на
// 127.0.0.1:port БЕЗ каких-либо привилегий. port <= 0 оставляет порт по
// умолчанию (7890) и значим только в proxy-режиме.
//
// Применяется при следующем CarambaUp. Возвращает NULL при успехе либо
// JSON-ошибку (владелец освобождает строку через CarambaFreeString). После
// переключения CarambaStatus отдаёт поля "mode" и (в proxy-режиме) "mixedPort".
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

//export CarambaSetPolicy
//
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

//export CarambaProbe
//
// CarambaProbe меряет задержку до каждого узла ТЕКУЩЕЙ загруженной конфигурации
// (импортированной подписки либо последнего загруженного профиля панели), не
// поднимая туннель. Возвращает JSON
// {"servers":[{"id","name","type","server","port","country","latencyMs"}]}
// (latencyMs = -1 — узел не ответил) либо JSON-ошибку. Если ничего не загружено,
// вернётся {"servers":[]}. timeoutMs <= 0 — таймаут по умолчанию (3000).
// Владелец освобождает строку через CarambaFreeString.
func CarambaProbe(h C.long, timeoutMs C.int) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	return okOrErr(cl.ProbeJSON(int(timeoutMs)))
}

//export CarambaSetTunFd
//
// CarambaSetTunFd пробрасывает TUN fd в ядро (как на мобильных). На десктопе в
// норме НЕ вызывается: при fd=-1 mihomo поднимает TUN сам. Возвращает NULL при
// успехе либо JSON-ошибку.
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

//export CarambaUp
//
// CarambaUp поднимает туннель и возвращает JSON api.UpResult (или JSON-ошибку).
// serverID необязателен (пусто — выбор панели/автоматика). Для импортированной
// подписки непустой serverID — это ИМЯ узла (поле id из метаданных
// CarambaImportSubscription), которое закрепляется выбором по умолчанию в
// селекторе CARAMBA. Владелец освобождает строку.
func CarambaUp(h C.long, serverID *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	return okOrErr(cl.Up(C.GoString(serverID)))
}

//export CarambaDown
//
// CarambaDown останавливает туннель. Возвращает NULL при успехе либо JSON-ошибку.
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

//export CarambaStatus
//
// CarambaStatus возвращает плоский JSON статуса
// {stage,detail?,connectedSinceMs,activeProxy?} (CHANNEL CONTRACT), пригодный
// для status-канала плагина. activeProxy — имя узла, выбранного в селекторе
// CARAMBA; присутствует только когда туннель поднят. Владелец освобождает строку.
func CarambaStatus(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	return okOrErr(cl.StatusJSON())
}

//export CarambaTraffic
//
// CarambaTraffic возвращает плоский JSON счётчиков {downBps,upBps,downTotal,
// upTotal} (CHANNEL CONTRACT). Десктопный плагин опрашивает ~1 Гц. Владелец
// освобождает строку.
func CarambaTraffic(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("caramba: неизвестный хэндл")
	}
	return okOrErr(cl.TrafficJSON())
}

//export CarambaFree
//
// CarambaFree гасит туннель и освобождает экземпляр ядра по хэндлу. После вызова
// хэндл недействителен. Безопасен при неизвестном хэндле (ничего не делает).
func CarambaFree(h C.long) {
	registry.mu.Lock()
	cl := registry.byID[int64(h)]
	delete(registry.byID, int64(h))
	registry.mu.Unlock()
	if cl != nil {
		_ = cl.Down()
	}
}

//export CarambaFreeString
//
// CarambaFreeString освобождает C-строку, ранее возвращённую любой Caramba*-
// функцией. Вызывать ровно один раз на каждую ненулевую возвращённую строку.
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

//export CarambaCsmEnroll
//
// CarambaCsmEnroll регистрирует профиль из bootstrap blob либо из origin, кода
// и пина. Возвращает снимок проверенного состояния или {"error":...}.
func CarambaCsmEnroll(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmEnroll(C.GoString(jsonStr)))
}

//export CarambaCsmRefresh
//
// CarambaCsmRefresh выполняет один цикл выборки документов. Отказ не означает
// потерю конфигурации: профиль остаётся на кешированных документах.
func CarambaCsmRefresh(h C.long, timeoutSec C.int) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmRefresh(int(timeoutSec)))
}

//export CarambaCsmState
//
// CarambaCsmState отдаёт личность оператора, состояние проверки документов,
// битовое поле возможностей и возраст конфигурации.
func CarambaCsmState(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmState())
}

//export CarambaCsmLadder
//
// CarambaCsmLadder отдаёт все скомпилированные ступени и историю попыток.
func CarambaCsmLadder(h C.long) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmLadder())
}

//export CarambaCsmSetLadder
//
// CarambaCsmSetLadder применяет переключатели и порядок от пользователя.
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

//export CarambaCsmRequestSettings
//
// CarambaCsmRequestSettings отправляет изменение настроек как подписанный
// запрос и принимает подписанный ответ как новую директиву.
func CarambaCsmRequestSettings(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.CsmRequestSettings(C.GoString(jsonStr)))
}

//export CarambaLadderRequest
//
// CarambaLadderRequest выполняет произвольный HTTP запрос через лестницу.
func CarambaLadderRequest(h C.long, jsonStr *C.char) *C.char {
	cl := lookup(h)
	if cl == nil {
		return errJSON("неизвестный хэндл")
	}
	return okOrErr(cl.LadderRequest(C.GoString(jsonStr)))
}

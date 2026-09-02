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
// serverID необязателен (пусто — выбор панели). Владелец освобождает строку.
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
// CarambaStatus возвращает плоский JSON статуса {stage,detail?,connectedSinceMs}
// (CHANNEL CONTRACT), пригодный для status-канала плагина. Владелец освобождает
// строку.
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

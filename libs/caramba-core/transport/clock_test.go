package transport

import (
	"testing"
	"time"
)

// 02-SPEC.md 5.5, последний пункт: доверие часам СНИМАЕТСЯ, когда стенные часы
// ушли назад больше чем на 300 секунд относительно монотонного смещения.
// Раньше monoRef и wallRef только присваивались и не читались никогда, поэтому
// один перевод часов назад делал V12 проходимым для сколь угодно старых
// документов, а механизм, ради которого это состояние существует, не срабатывал.
func TestClockTrustRevertsOnBackwardsJump(t *testing.T) {
	dir := t.TempDir()
	f, err := NewFetcher(dir, "226e8a20f699b964", NewLadder(nil), nil)
	if err != nil {
		t.Fatalf("fetcher: %v", err)
	}

	now := time.Unix(1788307200, 0)
	f.SetClock(func() time.Time { return now })

	f.mu.Lock()
	// Так выглядит состояние сразу после принятой директивы.
	f.clockTrusted = true
	f.clockEstimate = now.Unix()
	f.estimateAt = time.Now()
	f.monoRef = f.estimateAt
	f.wallRef = now.Unix()
	f.mu.Unlock()

	// Небольшой ход вперёд доверия не снимает.
	now = now.Add(120 * time.Second)
	f.mu.Lock()
	f.reviewClockLocked()
	trusted := f.clockTrusted
	f.mu.Unlock()
	if !trusted {
		t.Fatal("движение часов вперёд не должно снимать доверие")
	}

	// Прыжок назад на сутки: доверие снимается, факт остаётся в снимке.
	now = now.Add(-24 * time.Hour)
	f.mu.Lock()
	f.reviewClockLocked()
	trusted, changed := f.clockTrusted, f.clockChanged
	f.mu.Unlock()
	if trusted {
		t.Fatal("часы ушли назад на сутки, доверие обязано было сняться")
	}
	if !changed {
		t.Fatal("снятие доверия обязано быть видно в обвязке")
	}
}

// Пока часам доверяют, время шагов свежести берётся из оценки последней
// директивы плюс монотонное время, а не из стенных часов: иначе перевод даты
// пользователем прямо двигает границу V12.
func TestNowComesFromTheEstimateNotTheWallClock(t *testing.T) {
	dir := t.TempDir()
	f, err := NewFetcher(dir, "226e8a20f699b964", NewLadder(nil), nil)
	if err != nil {
		t.Fatalf("fetcher: %v", err)
	}
	wall := time.Unix(2000000000, 0)
	f.SetClock(func() time.Time { return wall })

	f.mu.Lock()
	f.clockTrusted = true
	f.clockEstimate = 1788307200
	f.estimateAt = time.Now()
	got := f.nowTrustedLocked()
	f.mu.Unlock()

	if got < 1788307200 || got > 1788307200+5 {
		t.Fatalf("now = %d, ожидалась оценка 1788307200 плюс секунды, а не стенные часы %d", got, wall.Unix())
	}

	// Пока доверия нет, остаются стенные часы: другого источника просто нет,
	// и шаги свежести на них всё равно не смотрят.
	f.mu.Lock()
	f.clockTrusted = false
	got = f.nowTrustedLocked()
	f.mu.Unlock()
	if got != wall.Unix() {
		t.Fatalf("без доверия now = %d, ожидались стенные часы %d", got, wall.Unix())
	}
}

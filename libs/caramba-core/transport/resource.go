package transport

import (
	"bytes"
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"net/http"
	"sort"
	"strings"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
)

// Отказы ресурсов, инвариант 12 и бит возможностей 6.
var (
	// ErrResourceUnknown означает, что ресурс не назван доверенным каталогом.
	// Загружать его нельзя: подписанного sha256 для него нет, а инвариант 12
	// запрещает применять то, что не сходится с каталогом.
	ErrResourceUnknown = errors.New("transport: ресурс не назван доверенным каталогом")
	// ErrResourceHash это несовпадение sha256. Файл отбрасывается целиком.
	ErrResourceHash = errors.New("transport: sha256 ресурса не совпал с подписанным")
	// ErrResourcesDisabled означает снятый бит 6: клиент НЕ ИМЕЕТ ПРАВА
	// загружать rule-set или geo по URL из каталога и откатывается на
	// встроенные данные mihomo. Отказ загружать это безопасное направление.
	ErrResourcesDisabled = errors.New("transport: хеши ресурсов не предлагаются оператором")
)

// CapResourceHashes это бит 6 битового поля возможностей.
const CapResourceHashes = 6

// ResourceGuard хранит подписанные записи rs и geo и пропускает только те
// байты, чей sha256 совпал.
//
// Страж умеет УДЕРЖИВАТЬ прежний набор. Это не удобство, а то, без чего
// карточка 02-SPEC.md 7.7.1 говорит неправду. Смена набора правил и geo-баз
// это сужение защиты, которое приходит в каталоге, а не в настройке: хеш
// связывает байты, но не связывает того, кто выбрал и путь, и хеш. Пока
// пользователь не ответил, в силе обязан оставаться ПРЕЖНИЙ набор, иначе
// кнопка "Оставить прежние" не откатывает ничего, потому что новый набор уже
// применён к моменту, когда карточка появилась на экране.
type ResourceGuard struct {
	byName  map[string]csm.Resource
	enabled bool
	capBits uint32
	// proposed непуст, когда пришедший каталог назвал другой набор и ответа
	// пользователя ещё нет. Действует при этом byName, а не proposed.
	proposed    map[string]csm.Resource
	proposedCap uint32
	hasProposed bool
}

// NewResourceGuard собирает страж по доверенному каталогу и эффективной
// битовой маске возможностей.
func NewResourceGuard(cat *csm.Catalog, capBits uint32) *ResourceGuard {
	g := &ResourceGuard{byName: resourceSet(cat), capBits: capBits}
	// Правило 02-SPEC.md 6.5: бит, утверждающий наличие содержимого, при
	// пустом массиве считается нулём.
	g.enabled = capBits&(1<<CapResourceHashes) != 0 && len(g.byName) > 0
	return g
}

// resourceSet проецирует rs и geo каталога в карту по имени.
func resourceSet(cat *csm.Catalog) map[string]csm.Resource {
	out := map[string]csm.Resource{}
	if cat == nil {
		return out
	}
	for _, r := range cat.RS {
		out[r.Name] = r
	}
	for _, r := range cat.Geo {
		out[r.Name] = r
	}
	return out
}

// sameSet сравнивает два набора по именам и подписанным хешам.
func sameSet(a, b map[string]csm.Resource) bool {
	if len(a) != len(b) {
		return false
	}
	for n, r := range a {
		o, ok := b[n]
		if !ok || !bytes.Equal(r.Hash, o.Hash) {
			return false
		}
	}
	return true
}

// HoldPrevious оставляет в силе набор предыдущего стража, когда новый каталог
// назвал другой, и записывает новый как ПРЕДЛОЖЕННЫЙ.
//
// Первый в жизни профиля набор принимается молча: сузить относительно ничего
// нельзя, и спрашивать здесь было бы не о чем. Дальше любое расхождение
// удерживается до ответа.
func (g *ResourceGuard) HoldPrevious(prev *ResourceGuard) {
	if g == nil || prev == nil || len(prev.byName) == 0 {
		return
	}
	if sameSet(prev.byName, g.byName) {
		// Набор тот же. Незакрытое предложение прежнего стража, если оно было,
		// переносится: пользователь на него так и не ответил.
		if prev.hasProposed {
			g.proposed, g.proposedCap, g.hasProposed = prev.proposed, prev.proposedCap, true
		}
		return
	}
	g.proposed, g.proposedCap, g.hasProposed = g.byName, g.capBits, true
	g.byName, g.capBits = prev.byName, prev.capBits
	g.enabled = g.capBits&(1<<CapResourceHashes) != 0 && len(g.byName) > 0
}

// PendingCatalogChange сообщает, что набор ресурсов ждёт ответа пользователя.
func (g *ResourceGuard) PendingCatalogChange() bool { return g != nil && g.hasProposed }

// ProposedNames перечисляет ресурсы предложенного набора. Пусто, когда
// предложения нет.
func (g *ResourceGuard) ProposedNames() []string {
	if g == nil || !g.hasProposed {
		return nil
	}
	out := make([]string, 0, len(g.proposed))
	for n := range g.proposed {
		out = append(out, n)
	}
	sort.Strings(out)
	return out
}

// AnswerCatalogChange применяет ответ пользователя.
//
// accept: предложенный набор становится действующим. keep: действующим
// остаётся прежний, а предложение снимается; следующий тот же каталог поднимет
// вопрос заново, и это правильно, потому что оператор всё ещё предлагает то,
// от чего пользователь отказался.
//
// Возвращает false, когда отвечать было не на что.
func (g *ResourceGuard) AnswerCatalogChange(accept bool) bool {
	if g == nil || !g.hasProposed {
		return false
	}
	if accept {
		g.byName, g.capBits = g.proposed, g.proposedCap
		g.enabled = g.capBits&(1<<CapResourceHashes) != 0 && len(g.byName) > 0
	}
	g.proposed, g.proposedCap, g.hasProposed = nil, 0, false
	return true
}

// Enabled сообщает, разрешена ли загрузка ресурсов по каталогу.
func (g *ResourceGuard) Enabled() bool { return g != nil && g.enabled }

// Names перечисляет известные ресурсы.
func (g *ResourceGuard) Names() []string {
	if g == nil {
		return nil
	}
	out := make([]string, 0, len(g.byName))
	for n := range g.byName {
		out = append(out, n)
	}
	sort.Strings(out)
	return out
}

// Entry возвращает подписанную запись ресурса.
func (g *ResourceGuard) Entry(name string) (csm.Resource, bool) {
	if g == nil {
		return csm.Resource{}, false
	}
	r, ok := g.byName[name]
	return r, ok
}

// Check применяет инвариант 12 к уже полученным байтам. Это последняя точка
// перед использованием файла, и она обязана быть пройдена ДО того, как байты
// попадут в mihomo.
func (g *ResourceGuard) Check(name string, data []byte) error {
	if !g.Enabled() {
		return fmt.Errorf("%w: %s", ErrResourcesDisabled, name)
	}
	r, ok := g.byName[name]
	if !ok {
		return fmt.Errorf("%w: %s", ErrResourceUnknown, name)
	}
	if len(r.Hash) != sha256.Size {
		return fmt.Errorf("%w: длина подписанного хеша %d", ErrResourceHash, len(r.Hash))
	}
	sum := sha256.Sum256(data)
	if !bytes.Equal(sum[:], r.Hash) {
		return fmt.Errorf("%w: %s", ErrResourceHash, name)
	}
	return nil
}

// Fetch загружает ресурс по лестнице и применяет Check до возврата байтов.
//
// Порядок здесь и есть смысл функции: сначала правило http, потом загрузка,
// потом хеш, и только потом байты уходят наружу. Функции, которая возвращает
// непроверенные байты, в этом пакете нет.
func (g *ResourceGuard) Fetch(ctx context.Context, l *Ladder, name, origin string) ([]byte, error) {
	if !g.Enabled() {
		return nil, fmt.Errorf("%w: %s", ErrResourcesDisabled, name)
	}
	r, ok := g.byName[name]
	if !ok {
		return nil, fmt.Errorf("%w: %s", ErrResourceUnknown, name)
	}
	// 03-WIRE.md 14.3: ни одно подписанное поле CSM/1 не несёт абсолютный URL.
	// Ветки "а если это уже URL" здесь нет намеренно: гарантия не должна
	// зависеть от проверки, живущей в другом пакете.
	if !strings.HasPrefix(r.URL, "/") {
		return nil, fmt.Errorf("%w: %s: подписанный ресурс обязан быть путём, а не URL", ErrResourceUnknown, name)
	}
	target, err := ResolveAgainst(origin, r.URL)
	if err != nil {
		return nil, err
	}
	// Инвариант 8 применяется и здесь: rule-set и geo идут по тем же правилам,
	// что и манифест.
	if err := CheckFetchURLString(target); err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, target, nil)
	if err != nil {
		return nil, err
	}
	resp, err := l.Do(ctx, req, DoOptions{
		Origin: origin,
		// Ресурс не кадр CSM, поэтому потолок ответа тут не resp_max: он
		// задаётся отдельно и всё равно ограничен.
		MaxBody: ResourceMaxBytes,
		Verify: func(body []byte) error {
			return g.Check(name, body)
		},
	})
	if err != nil {
		return nil, err
	}
	return resp.Body, nil
}

// ResourceMaxBytes это потолок одного файла rule-set или geo. Он не подписан и
// не может быть поднят оператором: подписанное поле, способное только ухудшить
// положение клиента, зажимается на клиенте.
const ResourceMaxBytes uint64 = 16 << 20

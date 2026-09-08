package auth

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"
)

// Tokens хранит пару JWT-токенов, выданных панелью caramba.
type Tokens struct {
	AccessToken  string `json:"access_token"`
	RefreshToken string `json:"refresh_token"`
	// AccessExpiry — момент истечения access-токена (UTC). Может быть нулём,
	// если панель не вернула срок жизни; в этом случае авто-обновление
	// срабатывает только по ответу 401. При инъекции извне (SetTokens) нулевое
	// значение достраивается из claim exp самого JWT — см. jwtExpiry.
	AccessExpiry time.Time `json:"access_expiry,omitempty"`
}

// Valid сообщает, есть ли сессия, которой ещё МОЖНО пользоваться: либо access
// пригоден прямо сейчас, либо его есть чем обновить.
//
// Здесь стояло `t.AccessToken != ""`, и эта мелочь превращала восстановимый 401
// в вечный тупик. Приложение отдавало ядру access-токен на 15 минут; через
// четверть часа он протухал, но Valid() продолжал отвечать «да», IsAuthenticated()
// вслед за ним — тоже, и ядро шло в панель с заведомо мёртвым bearer'ом. Панель
// отвечала 401, обновляться было нечем, и пользователь получал «api: загрузка
// узлов подписки для замера: …» — сообщение, по которому невозможно догадаться,
// что надо просто войти заново. Телефон, полежавший ночь, ловил это всегда;
// любой тест, запущенный через минуту после входа, — никогда.
//
// Условие читается так: refresh есть — сессию можно продлить, значит она жива
// (даже если access уже мёртв или его вовсе нет: AccessToken() обновит перед
// использованием). refresh нет — сессия жива ровно пока жив access, и
// протухший access это НЕ авторизация, сколько бы непустой строки в нём ни
// лежало. Неизвестный срок (AccessExpiry == 0) по-прежнему считается живым:
// соврать в другую сторону значило бы выкинуть работающую сессию.
func (t Tokens) Valid() bool {
	if t.RefreshToken != "" {
		return true
	}
	return t.AccessToken != "" && !t.expired(expirySkew)
}

// expired возвращает true, если срок access-токена известен и уже истёк
// (с небольшим запасом skew).
func (t Tokens) expired(skew time.Duration) bool {
	if t.AccessExpiry.IsZero() {
		return false
	}
	return time.Now().Add(skew).After(t.AccessExpiry)
}

// Store — абстракция персистентного хранилища токенов. Реализации должны быть
// безопасны для конкурентного использования.
type Store interface {
	// Load читает сохранённые токены. Если токенов нет, возвращает нулевой
	// Tokens и nil-ошибку.
	Load() (Tokens, error)
	// Save атомарно сохраняет токены.
	Save(Tokens) error
	// Clear удаляет сохранённые токены (logout).
	Clear() error
}

// FileStore — реализация Store на основе файла в каталоге конфигурации
// пользователя (os.UserConfigDir). Используется по умолчанию для CLI и
// десктопных сборок.
type FileStore struct {
	path string
	mu   sync.Mutex
}

// NewFileStore создаёт файловое хранилище. Если path пуст, путь вычисляется
// как <UserConfigDir>/caramba/tokens.json.
func NewFileStore(path string) (*FileStore, error) {
	if path == "" {
		dir, err := os.UserConfigDir()
		if err != nil {
			return nil, fmt.Errorf("auth: определение каталога конфигурации: %w", err)
		}
		path = filepath.Join(dir, "caramba", "tokens.json")
	}
	return &FileStore{path: path}, nil
}

// Path возвращает путь к файлу токенов.
func (s *FileStore) Path() string { return s.path }

// Load читает токены из файла. Отсутствие файла не является ошибкой.
func (s *FileStore) Load() (Tokens, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	data, err := os.ReadFile(s.path)
	if err != nil {
		if os.IsNotExist(err) {
			return Tokens{}, nil
		}
		return Tokens{}, fmt.Errorf("auth: чтение токенов: %w", err)
	}
	var t Tokens
	if err := json.Unmarshal(data, &t); err != nil {
		return Tokens{}, fmt.Errorf("auth: разбор токенов: %w", err)
	}
	return t, nil
}

// Save атомарно записывает токены: пишем во временный файл и переименовываем.
func (s *FileStore) Save(t Tokens) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if err := os.MkdirAll(filepath.Dir(s.path), 0o700); err != nil {
		return fmt.Errorf("auth: создание каталога токенов: %w", err)
	}
	data, err := json.MarshalIndent(t, "", "  ")
	if err != nil {
		return fmt.Errorf("auth: сериализация токенов: %w", err)
	}
	tmp := s.path + ".tmp"
	if err := os.WriteFile(tmp, data, 0o600); err != nil {
		return fmt.Errorf("auth: запись временного файла токенов: %w", err)
	}
	if err := os.Rename(tmp, s.path); err != nil {
		return fmt.Errorf("auth: переименование файла токенов: %w", err)
	}
	return nil
}

// Clear удаляет файл токенов. Отсутствие файла не является ошибкой.
func (s *FileStore) Clear() error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if err := os.Remove(s.path); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("auth: удаление токенов: %w", err)
	}
	return nil
}

// MemoryStore — реализация Store в памяти. Удобна для тестов и эфемерных
// сессий (например, мобильный процесс, где токены хранит платформа).
type MemoryStore struct {
	mu     sync.Mutex
	tokens Tokens
}

// NewMemoryStore создаёт пустое хранилище в памяти.
func NewMemoryStore() *MemoryStore { return &MemoryStore{} }

func (s *MemoryStore) Load() (Tokens, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.tokens, nil
}

func (s *MemoryStore) Save(t Tokens) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.tokens = t
	return nil
}

func (s *MemoryStore) Clear() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.tokens = Tokens{}
	return nil
}

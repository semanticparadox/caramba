package cli

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/semanticparadox/caramba/libs/caramba-core/auth"
	"github.com/spf13/cobra"
)

// newLoginCommand — вход по email/паролю либо через данные Telegram Login.
func newLoginCommand(g *globalFlags) *cobra.Command {
	var (
		email      string
		password   string
		register   bool
		telegram   bool
		code       string
		tgID       int64
		tgUsername string
		tgAuthDate int64
		tgHash     string
	)

	cmd := &cobra.Command{
		Use:   "login",
		Short: "Войти в аккаунт exarobot",
		Long: "Войти по одноразовому коду из Telegram-бота (--code), по email/паролю\n" +
			"(--email/--password) или через Telegram Login (--telegram с полями\n" +
			"виджета). Код получают командой /login в боте @exarobot (действует 5 минут).\n" +
			"Токены сохраняются в защищённом файле и используются последующими\n" +
			"командами.",
		Example: "  exarobot login --code 123456\n" +
			"  exarobot login --email you@example.com\n" +
			"  exarobot login --register --email you@example.com --password 's3cret'\n" +
			"  exarobot login --telegram --tg-id 123 --tg-hash <hash> --tg-auth-date 1700000000",
		RunE: func(cmd *cobra.Command, args []string) error {
			core, err := g.newCore()
			if err != nil {
				PrintError(err)
				return err
			}
			ctx := context.Background()

			if code != "" {
				if _, err := core.LoginCode(ctx, code); err != nil {
					PrintError(err)
					return err
				}
				printOK("вход по коду выполнен")
				return nil
			}

			if telegram {
				data := auth.TelegramLogin{
					ID:       tgID,
					Username: tgUsername,
					AuthDate: tgAuthDate,
					Hash:     tgHash,
				}
				if data.ID == 0 || data.Hash == "" {
					err := errors.New("для --telegram требуются --tg-id и --tg-hash")
					PrintError(err)
					return err
				}
				if _, err := core.LoginTelegram(ctx, data); err != nil {
					PrintError(err)
					return err
				}
				printOK("вход через Telegram выполнен")
				return nil
			}

			// email/пароль
			if email == "" {
				email, err = prompt("Email: ")
				if err != nil {
					PrintError(err)
					return err
				}
			}
			if password == "" {
				password, err = promptPassword("Пароль: ")
				if err != nil {
					PrintError(err)
					return err
				}
			}
			if email == "" || password == "" {
				err := errors.New("email и пароль обязательны")
				PrintError(err)
				return err
			}

			if register {
				if _, err := core.RegisterEmail(ctx, email, password); err != nil {
					PrintError(err)
					return err
				}
				printOK("аккаунт создан, вход выполнен (%s)", email)
				return nil
			}

			if _, err := core.LoginEmail(ctx, email, password); err != nil {
				PrintError(err)
				return err
			}
			printOK("вход выполнен (%s)", email)
			return nil
		},
	}

	f := cmd.Flags()
	f.StringVar(&code, "code", "", "одноразовый код из Telegram-бота (/login)")
	f.StringVar(&email, "email", "", "email для входа")
	f.StringVar(&password, "password", "", "пароль (если не задан — будет запрошен скрыто)")
	f.BoolVar(&register, "register", false, "зарегистрировать новый аккаунт по email/паролю")
	f.BoolVar(&telegram, "telegram", false, "войти через данные Telegram Login")
	f.Int64Var(&tgID, "tg-id", 0, "Telegram ID (для --telegram)")
	f.StringVar(&tgUsername, "tg-username", "", "Telegram username (для --telegram)")
	f.Int64Var(&tgAuthDate, "tg-auth-date", 0, "auth_date виджета Telegram (для --telegram)")
	f.StringVar(&tgHash, "tg-hash", "", "hash виджета Telegram (для --telegram)")

	return cmd
}

// newLogoutCommand — выход: останавливает туннель и отзывает токены.
func newLogoutCommand(g *globalFlags) *cobra.Command {
	return &cobra.Command{
		Use:   "logout",
		Short: "Выйти из аккаунта (останавливает туннель, отзывает токены)",
		RunE: func(cmd *cobra.Command, args []string) error {
			core, err := g.newCore()
			if err != nil {
				PrintError(err)
				return err
			}
			if err := core.Logout(context.Background()); err != nil {
				PrintError(err)
				return err
			}
			printOK("выход выполнен")
			return nil
		},
	}
}

// prompt читает строку из stdin с приглашением. EOF без данных трактуется как
// пустой ввод (валидация обязательности выполняется выше).
func prompt(label string) (string, error) {
	if label != "" {
		fmt.Print(label)
	}
	r := bufio.NewReader(os.Stdin)
	line, err := r.ReadString('\n')
	if err != nil && !errors.Is(err, io.EOF) {
		return "", err
	}
	return strings.TrimSpace(line), nil
}

// promptPassword читает пароль из stdin. Скрытый ввод (без эха) включается на
// unix-терминалах через stty; при недоступности stty или не-терминальном stdin
// откатывается к обычному чтению строки. Так пакет не тянет внешних зависимостей
// помимо cobra.
func promptPassword(label string) (string, error) {
	fmt.Print(label)
	restore, hidden := disableEcho()
	pw, err := prompt("")
	if hidden {
		restore()
		fmt.Println() // перевод строки, который «съел» отключённый эхо-ввод
	}
	if err != nil {
		return "", err
	}
	return pw, nil
}

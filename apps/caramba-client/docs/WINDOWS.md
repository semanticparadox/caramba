# Caramba Connect на Windows

Как установить, что спросит Windows и почему, и как это собирается.

## Что скачивать

В релизе `client-v<версия>` (и в Telegram-боте, кнопка «Скачать приложение»)
для Windows два файла:

| Файл | Что это |
|------|---------|
| `Caramba-Connect-Setup-x64.exe` | **Инсталлятор** (рекомендуется). Ставит программу в `C:\Program Files\Caramba Connect`, создаёт ярлык в меню Пуск (на рабочем столе — по галочке), добавляет запись в «Приложения и возможности» и регистрирует ссылки `caramba://` и `carambaconnect://`, чтобы ссылка из бота открывала приложение. |
| `Caramba-Connect-Windows-x64-portable.zip` | **Портативная** сборка: та же программа без установки. Внутри папка `Caramba Connect\`, запускать `caramba_client.exe` из неё. Ссылки `caramba://` из браузера и Telegram в этом варианте **не** открывают приложение — ссылку нужно скопировать и вставить в окно «Добавить подключение». |

Требования: Windows 10 или 11, 64-бит (на Windows 11 ARM64 работает через
эмуляцию x64).

## Установка: два предупреждения Windows и что с ними делать

### 1. SmartScreen: «Windows защитила ваш компьютер»

Сборки не подписаны сертификатом (у проекта его нет), поэтому при первом
запуске `Setup.exe` (и `caramba_client.exe` из ZIP) появляется синее окно
SmartScreen.

Нажмите **«Подробнее»**, затем **«Выполнить в любом случае»**.

Если файл скачан из Telegram, окна может не быть: SmartScreen реагирует на
файлы, которые браузер пометил как скачанные из интернета.

### 2. UAC: «Разрешить этому приложению вносить изменения?»

Появляется дважды и оба раза это ожидаемо:

- при **установке** — инсталлятор пишет в `Program Files` и регистрирует
  ссылки в системном реестре;
- при **каждом запуске** приложения — туннель на Windows работает через
  драйвер wintun, а wintun создаёт сетевой адаптер только из процесса с
  правами администратора. Без них приложение открылось бы, но «Подключить»
  молча не поднимал бы туннель, поэтому exe запрашивает права сам
  (`requireAdministrator` в манифесте). Отдельно «Запуск от имени
  администратора» выбирать не нужно.

На учётной записи без прав администратора понадобится пароль администратора
для запуска.

## Подключение по ссылке из бота

После установки инсталлятором:

1. В боте нажмите «Подключить Caramba Connect» и затем «Скопировать ссылку»
   (или нажмите на саму ссылку — Telegram скопирует её).
2. Клик по ссылке `caramba://connect?...` в браузере или Telegram Desktop
   открывает Caramba Connect с экраном подтверждения подключения. Браузер один
   раз спросит «Открыть Caramba Connect?» — подтвердите.
3. Если ссылка не открылась (портативная версия, или ссылку показали как
   текст): откройте приложение, «Добавить подключение» → «Вставить».

Проверить регистрацию схемы можно из PowerShell:

```powershell
Get-ItemProperty 'HKLM:\Software\Classes\caramba\shell\open\command'
# (default) : "C:\Program Files\Caramba Connect\caramba_client.exe" "%1"
```

## Обновление и удаление

- **Обновление**: запустить новый `Setup.exe` поверх — он закроет запущенное
  приложение, заменит файлы в той же папке и сохранит настройки (они лежат в
  профиле пользователя, а не в `Program Files`).
- **Удаление**: «Параметры → Приложения → Caramba Connect → Удалить» или
  ярлык «Удалить Caramba Connect» в меню Пуск. Регистрация ссылок снимается
  вместе с программой.
- Портативную версию просто удаляют папкой; ссылки она не регистрировала.

## Известные ограничения

- Нет подписи кода: SmartScreen будет предупреждать при каждой новой версии.
- Запущенное от администратора окно не принимает перетаскивание файлов из
  проводника (ограничение Windows UIPI); выбор файла через диалог работает.
- Второй клик по ссылке при уже открытом приложении запускает вторую копию:
  пересылка ссылки в уже работающий экземпляр (`SendAppLinkToInstance` из
  app_links в `windows/runner/main.cpp`) пока не подключена.

## Как это собирается

Всё делает `apps/caramba-client/scripts/ci-desktop.sh windows` (job `windows`
в `.github/workflows/client-desktop.yml`, раннер `windows-latest`):

1. `flutter build windows --release` → `build/windows/x64/runner/Release`.
2. Проверка, что в Release есть `caramba_client.exe`, `libcaramba_core.dll`
   и `wintun.dll` (без ядра приложение молча уходит в mock).
3. Staging `build/dist-staging/windows/Caramba Connect/` — копия Release плюс
   рантайм Visual C++ (`msvcp140.dll`, `vcruntime140.dll`,
   `vcruntime140_1.dll`) из Visual Studio раннера: Flutter их в бандл не
   кладёт, а без них exe на чистой Windows не стартует.
4. ZIP из staging (с корневой папкой) →
   `build/dist/Caramba-Connect-Windows-x64-portable.zip`.
5. `ISCC.exe` (Inno Setup 6, предустановлен на образе) по скрипту
   `windows/installer/caramba-connect.iss` с версией из `pubspec.yaml` →
   `build/dist/Caramba-Connect-Setup-x64.exe`.

Локально это работает только на Windows-хосте с Flutter, Go, MinGW и
Inno Setup 6 (путь к `ISCC.exe` можно задать через `CARAMBA_ISCC`). На macOS
компилятора Inno Setup нет, поэтому `.iss` проверяется только в CI.

Имя исполняемого файла остаётся `caramba_client.exe`: `BINARY_NAME` с
пробелом ломает CMake-цели Flutter. Пользователь видит не имя файла, а
`FileDescription` из `Runner.rc` и имя ярлыка — оба «Caramba Connect».

---

## Windows quick guide (EN)

- **Installer** `Caramba-Connect-Setup-x64.exe` (recommended): installs to
  `C:\Program Files\Caramba Connect`, adds Start Menu shortcut and registers
  `caramba://` links so the link from the bot opens the app.
  **Portable** `Caramba-Connect-Windows-x64-portable.zip`: unzip the
  `Caramba Connect` folder and run `caramba_client.exe`; links are not
  registered, paste them via "Add connection".
- **SmartScreen** ("Windows protected your PC"): the builds are unsigned.
  Click **More info → Run anyway**.
- **UAC prompt** on install and on every launch is expected: the tunnel uses
  the wintun driver, which needs administrator rights to create the adapter.
- To connect: copy the link from the bot, click it (or paste it in
  "Add connection → Paste").

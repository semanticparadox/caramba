; caramba-connect.iss — скрипт Inno Setup 6 для инсталлятора Caramba Connect
; на Windows x64.
;
; Зачем инсталлятор, а не только ZIP: владелец спросил «почему на ПК zip, а не
; exe?». Обычный человек ждёт Setup.exe, который сам создаёт папку «Caramba
; Connect» в Program Files, ярлык в меню Пуск и запись в «Приложения и
; возможности». Кроме удобства у инсталлятора есть ОБЯЗАТЕЛЬНАЯ работа, которую
; ZIP сделать не может: регистрация URL-схем caramba:// и carambaconnect:// в
; реестре. Без неё ссылка из Telegram-бота на Windows не открывает приложение,
; и весь путь «ссылка из бота -> экран подключения» на ПК не работает.
;
; Собирается в CI (apps/caramba-client/scripts/ci-desktop.sh windows) так:
;   ISCC.exe /Qp "/DAppVersion=1.0.0" "/DAppBuild=107" ^
;     "/DSourceDir=<staging>\Caramba Connect" "/O<build/dist>" caramba-connect.iss
; На выходе build/dist/Caramba-Connect-Setup-x64.exe. Локально на Mac ISCC
; нет — синтаксис сверен с документацией Inno Setup 6 (jrsoftware.org/ishelp).
;
; Что здесь намеренно НЕ сделано:
;   - подписи кода нет (сертификата у проекта нет): SmartScreen покажет
;     «Windows защитила ваш компьютер» -> «Подробнее» -> «Выполнить в любом
;     случае». Об этом сказано в docs/WINDOWS.md и в подписи к файлу в боте;
;   - имя исполняемого файла остаётся caramba_client.exe: BINARY_NAME с пробелом
;     ломает CMake-цели Flutter, а пользователь видит не имя файла, а
;     FileDescription из Runner.rc и имя ярлыка — оба «Caramba Connect».

; --- параметры, которые передаёт CI через /D --------------------------------
; Значения по умолчанию нужны только для ручной компиляции из IDE Inno Setup:
; в CI все три приходят снаружи и совпадают с pubspec.yaml и каталогом staging.
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef AppBuild
  #define AppBuild "0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\build\windows\x64\runner\Release"
#endif

#define AppName "Caramba Connect"
#define AppExeName "caramba_client.exe"
#define AppPublisher "Caramba"

[Setup]
; AppId сгенерирован один раз и зашит навсегда: по нему Windows понимает, что
; новая версия — это обновление той же программы, а не вторая копия рядом.
; Двойная фигурная скобка — экранирование «{» в Inno Setup, GUID один.
AppId={{afbad275-6b27-4335-a49b-678ed4c0d06b}
AppName={#AppName}
; Версия как в pubspec.yaml и в теге релиза (client-v1.0.0+107): так запись в
; «Приложения и возможности» совпадает с тем, что показывает бот и мини-апп.
AppVersion={#AppVersion}+{#AppBuild}
AppVerName={#AppName} {#AppVersion}+{#AppBuild}
AppPublisher={#AppPublisher}
; В свойствах Setup.exe должна стоять числовая версия, иначе Windows покажет
; 0.0.0.0 и SmartScreen будет ещё подозрительнее к неподписанному файлу.
VersionInfoVersion={#AppVersion}.{#AppBuild}
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} Setup
; Папка установки — «нормальная», как просил владелец: C:\Program Files\Caramba
; Connect. {autopf} в административном режиме это {commonpf}, а вместе с
; 64-битным режимом ниже — именно Program Files, а не Program Files (x86).
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Приложение собрано под x64 и требует Windows 10+ (Flutter Windows). На
; Windows 11 ARM64 x64-сборка работает через эмуляцию — x64compatible это
; разрешает, x64os запретил бы.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
; Права администратора нужны самой программе: wintun не создаст сетевой
; адаптер без них (см. runner/runner.exe.manifest, requireAdministrator).
; Установка в Program Files и запись схем в HKLM тоже требуют повышения, так
; что один UAC-запрос на установку — честная цена.
PrivilegesRequired=admin
; Ярлыки и панель «Приложения и возможности» показывают корабль, а не
; безликую иконку Setup.
SetupIconFile=..\runner\resources\app_icon.ico
UninstallDisplayIcon={app}\{#AppExeName}
UninstallDisplayName={#AppName}
; Windows должна узнать о новых URL-схемах сразу, без перезагрузки.
ChangesAssociations=yes
; Если приложение запущено, Setup закроет его перед заменой файлов, а не
; упадёт на «файл занят».
CloseApplications=yes
RestartApplications=no
OutputBaseFilename=Caramba-Connect-Setup-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Язык мастера подбирается по языку Windows; диалог выбора появляется только
; если подходящего нет.
ShowLanguageDialog=auto
; Каталог вывода задаёт CI ключом /O; значение здесь — для ручной сборки.
OutputDir=..\..\build\dist

[Languages]
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
; Ярлык на рабочем столе — по желанию, галочка снята по умолчанию.
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; Весь каталог staging (exe, data\, libcaramba_core.dll, wintun.dll,
; flutter_windows.dll, плагины, VC++-рантайм) как есть.
; ignoreversion: у DLL Flutter и ядра нет осмысленных версий в ресурсах, и без
; флага Inno мог бы оставить старую копию, решив, что она «новее».
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Registry]
; Регистрация URL-схем. HKA = HKLM в административном режиме (наш случай), а
; Software\Classes под ним — это то, что раньше писали прямо в HKCR; Inno
; прямо не рекомендует HKCR. uninsdeletekey: при удалении программы схемы
; исчезают, иначе Windows продолжит слать ссылки в несуществующий exe.
;
;   caramba://connect?d=<armor>        — приглашение панели из бота (основной путь);
;   carambaconnect://enroll|import?... — старые ссылки бота.
;
; Значение по умолчанию ключа должно начинаться с «URL:», а пустое значение
; «URL Protocol» обязано существовать — иначе Windows не считает ключ схемой.
; Команда получает ссылку как единственный аргумент («%1»); app_links на
; Windows читает её из argv.
Root: HKA; Subkey: "Software\Classes\caramba"; ValueType: string; ValueData: "URL:{#AppName} connect link"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\caramba"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\caramba\DefaultIcon"; ValueType: string; ValueData: "{app}\{#AppExeName},0"
Root: HKA; Subkey: "Software\Classes\caramba\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExeName}"" ""%1"""

Root: HKA; Subkey: "Software\Classes\carambaconnect"; ValueType: string; ValueData: "URL:{#AppName} link"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\carambaconnect"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\carambaconnect\DefaultIcon"; ValueType: string; ValueData: "{app}\{#AppExeName},0"
Root: HKA; Subkey: "Software\Classes\carambaconnect\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExeName}"" ""%1"""

[Run]
; Запуск после установки — галочка на последней странице, по умолчанию снята:
; приложение стартует с правами администратора (manifest), и лучше, чтобы
; человек запустил его сам, чем получил ещё один UAC-запрос сразу после
; установки. runascurrentuser: для postinstall-записей Inno по умолчанию
; сбрасывает повышение (runasoriginaluser), а нашему exe оно как раз нужно.
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: postinstall nowait skipifsilent unchecked runascurrentuser

//! Выдача установщиков Caramba Connect файлом прямо в Telegram: APK для
//! Android, установщик и переносной ZIP для Windows, DMG для macOS, архив для
//! Linux.
//!
//! ЗАЧЕМ. Единственный путь к установщику до сих пор шёл через кнопку-ссылку на
//! домен панели. Домен могут заблокировать, и человек, который в эту минуту
//! сидит в этом самом боте, остаётся без приложения, хотя Telegram у него
//! очевидно работает. Файл, отданный самим ботом, блокировкой домена не
//! отбирается. Модуль по-прежнему зовётся `apk_delivery`, хотя раздаёт файлы
//! всех платформ: переименование плодило бы правки в `mod.rs`, колбэках и
//! диплинках (`/start apk`), которые уже разошлись по мини-аппу.
//!
//! ПОЧЕМУ ЧЕРЕЗ `file_id`, А НЕ ЗАГРУЗКОЙ. Bot API не даёт боту ЗАГРУЖАТЬ файлы
//! больше 50 МБ, а APK весит около 86, десктопные сборки ещё больше. Зато
//! переслать файл, который уже лежит на серверах Telegram, по его `file_id`
//! можно без ограничения размера. Отсюда весь порядок работы: владелец один раз
//! отправляет файл боту как документ, бот по расширению понимает платформу,
//! запоминает `file_id` в своём слоте, и дальше отдаёт файл всем по кнопке,
//! команде `/apk` или диплинку `/start apk` (`/start apk_<платформа>` сразу
//! присылает файл нужной платформы).
//!
//! `file_id` привязан к конкретному боту (другой бот по нему файл не получит),
//! поэтому хранение его в настройках панели не утечка, а просто закладка.
//!
//! `file_id` может протухнуть (файл удалили из чата, Telegram перепаковал
//! хранилище). Мы это НЕ лечим автоматическим стиранием настройки: ошибка одной
//! отправки может быть и сетевой, а молча забытый файл владелец обнаружит
//! только по жалобам. Забывает файл человек кнопкой в админке.

use crate::AppState;
use crate::bot::translations::{Lang, t};
use crate::bot::utils::escape_html;
use crate::settings::SettingsService;
use teloxide::prelude::*;
use teloxide::types::{ChatId, FileId, InputFile, ParseMode};
use tracing::{error, warn};

/// Префикс callback-данных кнопки «прислать файл этой платформы»:
/// `apk_send_<id платформы>`. Голый `apk_send` без хвоста показывает меню.
pub const CALLBACK_PREFIX: &str = "apk_send_";

/// Платформа, для которой бот хранит файл в Telegram.
///
/// Слот на платформу ровно один: новый файл того же расширения замещает
/// предыдущий. Windows представлена двумя слотами, потому что установщик и
/// переносной ZIP нужны разным людям (второй ставят там, где нет прав
/// администратора на установку), и оба приезжают от владельца отдельными
/// документами.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePlatform {
    Android,
    Windows,
    WindowsPortable,
    MacOs,
    Linux,
}

impl FilePlatform {
    /// Порядок показа в меню и в админке: телефоны первыми, потому что по
    /// статистике бота почти все пользователи с телефона.
    pub const ALL: [FilePlatform; 5] = [
        FilePlatform::Android,
        FilePlatform::Windows,
        FilePlatform::WindowsPortable,
        FilePlatform::MacOs,
        FilePlatform::Linux,
    ];

    /// Идентификатор в ключах настроек, callback-данных и диплинках.
    /// Менять нельзя: `android` уже лежит в настройках боевой панели.
    pub fn id(self) -> &'static str {
        match self {
            FilePlatform::Android => "android",
            FilePlatform::Windows => "windows",
            FilePlatform::WindowsPortable => "windows_portable",
            FilePlatform::MacOs => "macos",
            FilePlatform::Linux => "linux",
        }
    }

    pub fn from_id(id: &str) -> Option<FilePlatform> {
        FilePlatform::ALL
            .into_iter()
            .find(|p| p.id() == id.trim().to_ascii_lowercase())
    }

    /// Платформа по расширению присланного файла.
    ///
    /// Расширение сравнивается без учёта регистра: сборки приезжают и как
    /// `app.apk`, и как `Caramba-1.2.APK`. Имя, состоящее из одного расширения,
    /// не считается файлом сборки. `.tgz` принимается наравне с `.tar.gz`,
    /// потому что так архивы называют некоторые упаковщики.
    pub fn from_file_name(name: &str) -> Option<FilePlatform> {
        let name = name.trim().to_ascii_lowercase();
        let by_ext = |ext: &str| name.len() > ext.len() && name.ends_with(ext);
        if by_ext(".apk") {
            Some(FilePlatform::Android)
        } else if by_ext(".exe") {
            Some(FilePlatform::Windows)
        } else if by_ext(".zip") {
            Some(FilePlatform::WindowsPortable)
        } else if by_ext(".dmg") {
            Some(FilePlatform::MacOs)
        } else if by_ext(".tar.gz") || by_ext(".tgz") {
            Some(FilePlatform::Linux)
        } else {
            None
        }
    }

    /// Подпись платформы для кнопок и подтверждений, с эмодзи платформы.
    pub fn label(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (FilePlatform::Android, _) => "📲 Android (APK)",
            (FilePlatform::Windows, Lang::Ru) => "🪟 Windows (установщик)",
            (FilePlatform::Windows, Lang::En) => "🪟 Windows (installer)",
            (FilePlatform::WindowsPortable, Lang::Ru) => "🪟 Windows (переносной ZIP)",
            (FilePlatform::WindowsPortable, Lang::En) => "🪟 Windows (portable ZIP)",
            (FilePlatform::MacOs, _) => "🍎 Mac (DMG)",
            (FilePlatform::Linux, _) => "🐧 Linux (tar.gz)",
        }
    }

    /// Подпись для админки (без эмодзи, они в таблице настроек лишние).
    pub fn admin_label(self) -> &'static str {
        match self {
            FilePlatform::Android => "Android (APK)",
            FilePlatform::Windows => "Windows (установщик .exe)",
            FilePlatform::WindowsPortable => "Windows (переносной .zip)",
            FilePlatform::MacOs => "Mac (.dmg)",
            FilePlatform::Linux => "Linux (.tar.gz)",
        }
    }

    /// Ключ настройки панели для одного поля слота. Те же строки читает и
    /// админка: объявлены здесь один раз, чтобы у бота не было своего мнения об
    /// их написании. Для Android получается прежнее `app_apk_tg_file_id_android`,
    /// поэтому ничего мигрировать не нужно.
    pub fn setting_key(self, field: SettingField) -> String {
        format!("app_apk_tg_{}_{}", field.as_str(), self.id())
    }

    /// Все ключи слота: админка чистит их разом кнопкой «Забыть файл».
    pub fn setting_keys(self) -> [String; 4] {
        [
            self.setting_key(SettingField::FileId),
            self.setting_key(SettingField::FileName),
            self.setting_key(SettingField::FileSize),
            self.setting_key(SettingField::UploadedAt),
        ]
    }

    /// Callback-данные кнопки «прислать файл этой платформы».
    pub fn callback_data(self) -> String {
        format!("{CALLBACK_PREFIX}{}", self.id())
    }

    /// Платформа из callback-данных `apk_send_<id>`; `None` для голого
    /// `apk_send` и любого мусора.
    pub fn from_callback_data(data: &str) -> Option<FilePlatform> {
        data.strip_prefix(CALLBACK_PREFIX)
            .and_then(FilePlatform::from_id)
    }

    /// Платформа из параметра диплинка `/start apk_<id>`; `None` для голого
    /// `apk` и любого другого параметра.
    pub fn from_start_param(param: &str) -> Option<FilePlatform> {
        param
            .trim()
            .to_ascii_lowercase()
            .strip_prefix("apk_")
            .and_then(FilePlatform::from_id)
    }
}

/// Поля слота файла в настройках панели.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingField {
    FileId,
    FileName,
    FileSize,
    UploadedAt,
}

impl SettingField {
    fn as_str(self) -> &'static str {
        match self {
            SettingField::FileId => "file_id",
            SettingField::FileName => "file_name",
            SettingField::FileSize => "file_size",
            SettingField::UploadedAt => "uploaded_at",
        }
    }
}

/// Файл, который бот хранит для платформы. Есть только когда `file_id`
/// записан: остальные поля без него бесполезны.
#[derive(Debug, Clone)]
pub struct StoredFile {
    pub file_id: String,
    pub file_name: String,
}

/// Файл платформы из настроек, если он загружен.
pub async fn stored_file(settings: &SettingsService, platform: FilePlatform) -> Option<StoredFile> {
    let file_id = settings
        .get_or_default(&platform.setting_key(SettingField::FileId), "")
        .await;
    let file_id = file_id.trim().to_string();
    if file_id.is_empty() {
        return None;
    }
    let file_name = settings
        .get_or_default(&platform.setting_key(SettingField::FileName), "")
        .await;
    Some(StoredFile {
        file_id,
        file_name: file_name.trim().to_string(),
    })
}

/// Платформы, для которых файл загружен, в порядке показа.
pub async fn uploaded_platforms(settings: &SettingsService) -> Vec<FilePlatform> {
    let mut out = Vec::new();
    for platform in FilePlatform::ALL {
        let file_id = settings
            .get_or_default(&platform.setting_key(SettingField::FileId), "")
            .await;
        if !file_id.trim().is_empty() {
            out.push(platform);
        }
    }
    out
}

/// Админ ли этот Telegram-аккаунт.
///
/// ЗАЧЕМ ОТДЕЛЬНАЯ ФУНКЦИЯ. Приём файлов обязан пускать ровно тех же людей, что и
/// `/admin`: право «залить установщик всем пользователям бота» не может быть
/// шире права администрировать панель. Раньше запрос жил одной копией внутри
/// ветки `/admin`; вторая копия рано или поздно разъехалась бы с первой.
///
/// Таблица `admins` хранит username, а не tg_id, поэтому пользователь ищется по
/// `tg_id` и сводится с админом по username. Следствие, которое стоит знать:
/// админ без выставленного в Telegram username не опознаётся, ровно как и в
/// `/admin` сегодня.
pub async fn is_bot_admin(pool: &sqlx::PgPool, tg_id: i64) -> bool {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM admins a
            JOIN users u ON u.username = a.username
            WHERE u.tg_id = $1
        )
        "#,
    )
    .bind(tg_id)
    .fetch_one(pool)
    .await
    .unwrap_or(false)
}

/// В какой слот брать этот документ, и брать ли вообще.
///
/// Чистая функция: весь предикат целиком, чтобы его можно было проверить
/// тестом, не поднимая ни бота, ни базу. Условий ровно два и оба обязательны:
/// отправитель админ, и расширение файла принадлежит одной из платформ.
pub(crate) fn capture_platform(is_admin: bool, file_name: Option<&str>) -> Option<FilePlatform> {
    if !is_admin {
        return None;
    }
    file_name.and_then(FilePlatform::from_file_name)
}

/// Размер в мегабайтах с одним знаком, как его называют люди («86.3 МБ»).
///
/// Мегабайт двоичный (1024×1024): именно так размер показывают Telegram и
/// файловые менеджеры, и расхождение с их цифрой читалось бы как «прислали не
/// тот файл».
pub(crate) fn format_mb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

/// Подтверждение владельцу, что файл принят и уже раздаётся.
///
/// Называет платформу, размер и способы получения: единственный способ
/// убедиться, что бот запомнил тот файл и в тот слот, это увидеть их обратно.
pub(crate) fn capture_ack_text(
    lang: Lang,
    platform: FilePlatform,
    file_name: &str,
    bytes: u64,
) -> String {
    let name = escape_html(file_name);
    let mb = format_mb(bytes);
    let btn = t(lang, "app.apk_tg_btn");
    let label = platform.label(lang);
    let id = platform.id();
    match lang {
        Lang::Ru => format!(
            "✅ <b>Файл принят: {label}</b>\n{name}, {mb} МБ.\n\n\
             Теперь кнопка «{btn}», команда /apk и диплинк /start apk_{id} присылают этот файл."
        ),
        Lang::En => format!(
            "✅ <b>File saved: {label}</b>\n{name}, {mb} MB.\n\n\
             The “{btn}” button, the /apk command and the /start apk_{id} deep link now send this file."
        ),
    }
}

/// Сообщение владельцу о неудачной записи настроек.
fn capture_failed_text(lang: Lang) -> &'static str {
    match lang {
        Lang::Ru => "⚠️ Не получилось сохранить файл. Попробуйте отправить его ещё раз.",
        Lang::En => "⚠️ Couldn't save the file. Try sending it again.",
    }
}

/// Подпись к отправляемому файлу.
///
/// ЗАЧЕМ ТАКОЙ ТЕКСТ. Человек получает в чат большой файл и должен понять три
/// вещи: что это, что система сейчас будет ругаться (иначе установка выглядит
/// как отказ), и куда идти дальше. На Android это предупреждение о неизвестном
/// источнике, на Windows SmartScreen и запрос прав администратора (без них не
/// поднять туннель), на macOS отказ открыть неподписанное приложение. Название
/// кнопки подключения берётся из той же таблицы переводов, что и сама кнопка,
/// чтобы подпись не отсылала к пункту меню, который переименовали.
///
/// Имя файла экранируется: оно приходит от владельца, а подпись уходит в HTML
/// parse mode, где голый `<` роняет отправку целиком. Пустое имя (настройка
/// потёрта руками) просто убирает строку, а не рисует «Файл: ».
pub(crate) fn file_caption(lang: Lang, platform: FilePlatform, file_name: &str) -> String {
    let name = file_name.trim();
    let open = t(lang, "menu.open_app");
    let (title, steps) = match (platform, lang) {
        (FilePlatform::Android, Lang::Ru) => (
            "Caramba Connect для Android",
            "Установите APK (Android спросит разрешение на установку из неизвестного \
             источника).",
        ),
        (FilePlatform::Android, Lang::En) => (
            "Caramba Connect for Android",
            "Install the APK (Android will ask for permission to install from an \
             unknown source).",
        ),
        (FilePlatform::Windows, Lang::Ru) => (
            "Caramba Connect для Windows",
            "Запустите установщик. Если Windows SmartScreen предупредит: нажмите \
             «Подробнее», затем «Выполнить в любом случае». Приложение запрашивает \
             права администратора: без них не поднять туннель.",
        ),
        (FilePlatform::Windows, Lang::En) => (
            "Caramba Connect for Windows",
            "Run the installer. If Windows SmartScreen warns you: click \"More info\", \
             then \"Run anyway\". The app asks for administrator rights: the tunnel \
             cannot start without them.",
        ),
        (FilePlatform::WindowsPortable, Lang::Ru) => (
            "Caramba Connect для Windows (переносная версия)",
            "Распакуйте архив в удобную папку и запустите caramba_client.exe от имени \
             администратора: без этих прав не поднять туннель. Если Windows SmartScreen \
             предупредит: нажмите «Подробнее», затем «Выполнить в любом случае».",
        ),
        (FilePlatform::WindowsPortable, Lang::En) => (
            "Caramba Connect for Windows (portable)",
            "Unpack the archive to any folder and run caramba_client.exe as \
             administrator: the tunnel cannot start without those rights. If Windows \
             SmartScreen warns you: click \"More info\", then \"Run anyway\".",
        ),
        (FilePlatform::MacOs, Lang::Ru) => (
            "Caramba Connect для Mac",
            "Откройте DMG и перетащите Caramba Connect в «Программы». Приложение не \
             подписано: при первом запуске нажмите на него правой кнопкой, выберите \
             «Открыть», затем ещё раз «Открыть».",
        ),
        (FilePlatform::MacOs, Lang::En) => (
            "Caramba Connect for Mac",
            "Open the DMG and drag Caramba Connect to Applications. The app is not \
             signed: on first launch right-click it, choose \"Open\", then \"Open\" \
             again.",
        ),
        (FilePlatform::Linux, Lang::Ru) => (
            "Caramba Connect для Linux",
            "Распакуйте архив и запустите install.sh из папки caramba-connect (скрипт \
             попросит пароль sudo: он копирует приложение в /opt и добавляет ярлык).",
        ),
        (FilePlatform::Linux, Lang::En) => (
            "Caramba Connect for Linux",
            "Unpack the archive and run install.sh from the caramba-connect folder (it \
             asks for your sudo password: it copies the app to /opt and adds a launcher).",
        ),
    };
    let file_line = if name.is_empty() {
        String::new()
    } else {
        match lang {
            Lang::Ru => format!("\nФайл: {}", escape_html(name)),
            Lang::En => format!("\nFile: {}", escape_html(name)),
        }
    };
    match lang {
        Lang::Ru => format!(
            "📦 <b>{title}</b>{file_line}\n\n{steps} Затем вернитесь в бот и нажмите «{open}»."
        ),
        Lang::En => format!(
            "📦 <b>{title}</b>{file_line}\n\n{steps} Then come back to the bot and tap “{open}”."
        ),
    }
}

/// Что сказать, когда файла в Telegram нет (или он не отдался).
///
/// Ветвится по тому, есть ли запасная ссылка: обещать «скоро», когда ссылка
/// настроена и кнопка тут же под сообщением, значит спорить с собственной
/// кнопкой.
pub(crate) fn unavailable_text(lang: Lang, has_url: bool) -> &'static str {
    match (lang, has_url) {
        (Lang::Ru, true) => {
            "📦 Этого файла пока нет в Telegram. Скачайте приложение по кнопке ниже."
        }
        (Lang::Ru, false) => {
            "📦 Файлы приложения скоро появятся здесь. Загляните чуть позже или \
             напишите в поддержку."
        }
        (Lang::En, true) => {
            "📦 This file isn't stored in Telegram yet. Download the app with the button below."
        }
        (Lang::En, false) => {
            "📦 The app files will appear here soon. Check back a bit later or \
             contact support."
        }
    }
}

/// Текст над меню выбора платформы (файлы из Telegram).
pub(crate) fn platform_menu_text(lang: Lang) -> &'static str {
    match lang {
        Lang::Ru => {
            "📦 <b>Файлы Caramba Connect в Telegram</b>\n\n\
             Выберите платформу, и бот пришлёт установщик прямо сюда. Работает, даже \
             если сайт недоступен."
        }
        Lang::En => {
            "📦 <b>Caramba Connect files in Telegram</b>\n\n\
             Pick your platform and the bot sends the installer right here. Works even \
             when the website is blocked."
        }
    }
}

/// Текст над общим меню скачивания (ссылки на сайт + файлы из Telegram).
pub(crate) fn download_menu_text(lang: Lang, has_tg_files: bool) -> &'static str {
    match (lang, has_tg_files) {
        (Lang::Ru, true) => {
            "📥 <b>Скачать Caramba Connect</b>\n\n\
             Выберите платформу. Кнопки «в Telegram» присылают файл прямо в этот чат: \
             они работают, даже если сайт недоступен."
        }
        (Lang::Ru, false) => "📥 <b>Скачать Caramba Connect</b>\n\nВыберите платформу.",
        (Lang::En, true) => {
            "📥 <b>Download Caramba Connect</b>\n\n\
             Pick your platform. The \"in Telegram\" buttons send the file right into \
             this chat: they work even when the website is blocked."
        }
        (Lang::En, false) => "📥 <b>Download Caramba Connect</b>\n\nPick your platform.",
    }
}

/// Приём файла от владельца: документ в личке боту.
///
/// Вызывается на КАЖДОМ сообщении с документом. Документы от посторонних и
/// файлы с чужим расширением от админа не порождают ни ответа, ни записи: бот
/// ведёт себя ровно так же, как до этой функции (молча), чтобы случайный файл в
/// чате не превращался в диалог.
pub async fn handle_admin_document(bot: &Bot, msg: &Message, state: &AppState) {
    let Some(doc) = msg.document() else {
        return;
    };
    let tg_id = msg.chat.id.0;
    let is_admin = is_bot_admin(&state.pool, tg_id).await;
    let Some(platform) = capture_platform(is_admin, doc.file_name.as_deref()) else {
        return;
    };

    let file_name = doc
        .file_name
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string();
    let size = doc.file.size as u64;
    let lang = crate::bot::utils::lang_by_tg_id(state, tg_id).await;

    // ПОРЯДОК ЗАПИСИ ВАЖЕН: `file_id` пишется последним, потому что именно он
    // включает кнопку и команду. Оборвись запись посередине, пользователи
    // увидят прежнее состояние, а не кнопку без файла.
    let writes: [(String, String); 3] = [
        (
            platform.setting_key(SettingField::FileName),
            file_name.clone(),
        ),
        (
            platform.setting_key(SettingField::FileSize),
            size.to_string(),
        ),
        (
            platform.setting_key(SettingField::UploadedAt),
            chrono::Utc::now().to_rfc3339(),
        ),
    ];
    for (key, value) in writes {
        if let Err(e) = state.settings.set(&key, &value).await {
            error!("apk capture: failed to store {key}: {e:#}");
            let _ = bot
                .send_message(msg.chat.id, capture_failed_text(lang))
                .await;
            return;
        }
    }
    if let Err(e) = state
        .settings
        .set(&platform.setting_key(SettingField::FileId), &doc.file.id.0)
        .await
    {
        error!("apk capture: failed to store file_id: {e:#}");
        let _ = bot
            .send_message(msg.chat.id, capture_failed_text(lang))
            .await;
        return;
    }

    tracing::info!(
        "apk capture: stored {} ({} bytes) as {} from admin tg_id {}",
        file_name,
        size,
        platform.id(),
        tg_id
    );
    let _ = bot
        .send_message(
            msg.chat.id,
            capture_ack_text(lang, platform, &file_name, size),
        )
        .parse_mode(ParseMode::Html)
        .await
        .map_err(|e| error!("apk capture: failed to confirm to admin: {e}"));
}

/// Вход «файл в Telegram»: кнопка `apk_send`, команда `/apk`, диплинк
/// `/start apk`.
///
/// Все входы намеренно сходятся в одну функцию, иначе текст и поведение при
/// пустом/протухшем `file_id` разъехались бы по трём местам. Что показать,
/// решает число загруженных файлов: ни одного, значит запасной путь со ссылкой;
/// ровно один, значит сразу файл (меню из одной кнопки только добавляет
/// лишний тап); несколько, значит выбор платформы.
pub async fn send_apk(bot: &Bot, chat_id: ChatId, lang: Lang, state: &AppState) {
    let uploaded = uploaded_platforms(&state.settings).await;
    match uploaded.as_slice() {
        [] => send_unavailable(bot, chat_id, lang, state).await,
        [only] => send_file(bot, chat_id, lang, state, *only).await,
        many => {
            let keyboard = crate::bot::keyboards::tg_file_platform_keyboard(lang, many);
            let _ = bot
                .send_message(chat_id, platform_menu_text(lang))
                .parse_mode(ParseMode::Html)
                .reply_markup(keyboard)
                .await
                .map_err(|e| error!("apk send: failed to send platform menu: {e}"));
        }
    }
}

/// Общее меню скачивания: кнопка «📥 Скачать приложение» в главном меню.
///
/// Ссылки на сайт для всех настроенных `app_download_url_*` плюс файлы из
/// Telegram для всех загруженных платформ. Если не настроено ничего, тот же
/// запасной текст, что и у `send_apk`.
pub async fn send_download_menu(bot: &Bot, chat_id: ChatId, lang: Lang, state: &AppState) {
    let uploaded = uploaded_platforms(&state.settings).await;
    let Some(keyboard) =
        crate::bot::keyboards::download_menu_keyboard(&state.settings, lang, &uploaded).await
    else {
        send_unavailable(bot, chat_id, lang, state).await;
        return;
    };
    let _ = bot
        .send_message(chat_id, download_menu_text(lang, !uploaded.is_empty()))
        .parse_mode(ParseMode::Html)
        .reply_markup(keyboard)
        .await
        .map_err(|e| error!("apk send: failed to send download menu: {e}"));
}

/// Отправка файла одной платформы: кнопка `apk_send_<id>`, диплинк
/// `/start apk_<id>`, единственный файл в `send_apk`.
pub async fn send_file(
    bot: &Bot,
    chat_id: ChatId,
    lang: Lang,
    state: &AppState,
    platform: FilePlatform,
) {
    let Some(file) = stored_file(&state.settings, platform).await else {
        send_unavailable(bot, chat_id, lang, state).await;
        return;
    };

    let document = InputFile::file_id(FileId(file.file_id));
    match bot
        .send_document(chat_id, document)
        .caption(file_caption(lang, platform, &file.file_name))
        .parse_mode(ParseMode::Html)
        .await
    {
        Ok(_) => {}
        Err(e) => {
            // Самая вероятная причина: протухший `file_id`. Настройку не
            // трогаем (см. заголовок модуля), человеку даём запасной путь.
            warn!(
                "apk send: send_document by file_id failed for {}: {e}",
                platform.id()
            );
            send_unavailable(bot, chat_id, lang, state).await;
        }
    }
}

/// Запасной путь: объяснение + кнопки-ссылки на сайт, если оператор их настроил.
async fn send_unavailable(bot: &Bot, chat_id: ChatId, lang: Lang, state: &AppState) {
    let keyboard = crate::bot::keyboards::app_download_url_keyboard(&state.settings, lang).await;
    let send = bot
        .send_message(chat_id, unavailable_text(lang, keyboard.is_some()))
        .parse_mode(ParseMode::Html);
    let send = match keyboard {
        Some(kb) => send.reply_markup(kb),
        None => send,
    };
    let _ = send
        .await
        .map_err(|e| error!("apk send: failed to send fallback: {e}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_cyrillic(text: &str) -> bool {
        text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
    }

    /// Платформа узнаётся по расширению без учёта регистра; чужие файлы и имена
    /// из одного расширения не берутся.
    #[test]
    fn platform_is_detected_by_extension() {
        use FilePlatform::*;
        assert_eq!(
            FilePlatform::from_file_name("caramba-connect.apk"),
            Some(Android)
        );
        assert_eq!(
            FilePlatform::from_file_name("Caramba-1.2.APK"),
            Some(Android)
        );
        assert_eq!(FilePlatform::from_file_name("  app.Apk  "), Some(Android));
        assert_eq!(
            FilePlatform::from_file_name("Caramba-Connect-Setup-x64.exe"),
            Some(Windows)
        );
        assert_eq!(
            FilePlatform::from_file_name("Caramba-Connect-Windows-x64-portable.zip"),
            Some(WindowsPortable)
        );
        assert_eq!(
            FilePlatform::from_file_name("Caramba-Connect-macOS-arm64.dmg"),
            Some(MacOs)
        );
        assert_eq!(
            FilePlatform::from_file_name("Caramba-Connect-Linux-x64.tar.gz"),
            Some(Linux)
        );
        assert_eq!(FilePlatform::from_file_name("caramba.tgz"), Some(Linux));

        assert_eq!(FilePlatform::from_file_name("notes.txt"), None);
        assert_eq!(FilePlatform::from_file_name("apk"), None);
        assert_eq!(FilePlatform::from_file_name(".apk"), None);
        assert_eq!(FilePlatform::from_file_name(".tar.gz"), None);
        assert_eq!(FilePlatform::from_file_name("app.apk.rar"), None);
    }

    /// Предикат приёма: админ обязателен даже для правильного файла.
    #[test]
    fn capture_requires_admin() {
        assert_eq!(
            capture_platform(true, Some("caramba-connect.apk")),
            Some(FilePlatform::Android)
        );
        assert_eq!(capture_platform(false, Some("caramba-connect.apk")), None);
        assert_eq!(capture_platform(true, None), None);
        assert_eq!(capture_platform(true, Some("notes.txt")), None);
    }

    /// Ключи настроек Android совпадают с теми, что уже лежат на боевой панели:
    /// иначе загруженный APK «пропал» бы после релиза.
    #[test]
    fn android_setting_keys_are_unchanged() {
        assert_eq!(
            FilePlatform::Android.setting_key(SettingField::FileId),
            "app_apk_tg_file_id_android"
        );
        assert_eq!(
            FilePlatform::Android.setting_keys(),
            [
                "app_apk_tg_file_id_android".to_string(),
                "app_apk_tg_file_name_android".to_string(),
                "app_apk_tg_file_size_android".to_string(),
                "app_apk_tg_uploaded_at_android".to_string(),
            ]
        );
        assert_eq!(
            FilePlatform::WindowsPortable.setting_key(SettingField::UploadedAt),
            "app_apk_tg_uploaded_at_windows_portable"
        );
    }

    /// Идентификаторы ходят по кругу: callback и диплинк восстанавливают ту же
    /// платформу, мусор даёт `None`.
    #[test]
    fn ids_round_trip_through_callback_and_deep_link() {
        for platform in FilePlatform::ALL {
            assert_eq!(FilePlatform::from_id(platform.id()), Some(platform));
            assert_eq!(
                FilePlatform::from_callback_data(&platform.callback_data()),
                Some(platform)
            );
            assert_eq!(
                FilePlatform::from_start_param(&format!("apk_{}", platform.id())),
                Some(platform)
            );
            // Callback-данные Telegram ограничены 64 байтами.
            assert!(platform.callback_data().len() <= 64);
        }
        assert_eq!(FilePlatform::from_callback_data("apk_send"), None);
        assert_eq!(FilePlatform::from_callback_data("apk_send_ios"), None);
        assert_eq!(FilePlatform::from_start_param("apk"), None);
        assert_eq!(
            FilePlatform::from_start_param("APK_MACOS"),
            Some(FilePlatform::MacOs)
        );
        assert_eq!(FilePlatform::from_start_param("ref123"), None);
    }

    /// Размер считается в двоичных мегабайтах, как его показывает сам Telegram.
    #[test]
    fn size_is_formatted_in_binary_megabytes() {
        assert_eq!(format_mb(0), "0.0");
        assert_eq!(format_mb(1024 * 1024), "1.0");
        assert_eq!(format_mb(90_177_536), "86.0");
        // 86 000 000 байт это НЕ 86 МБ, и цифра обязана это показывать.
        assert_eq!(format_mb(86_000_000), "82.0");
    }

    /// Подпись обязана объяснить предупреждение системы и увести обратно в бот.
    #[test]
    fn captions_explain_the_os_warning() {
        let android = file_caption(Lang::Ru, FilePlatform::Android, "caramba-connect-1.2.apk");
        assert!(android.contains("Файл: caramba-connect-1.2.apk"));
        assert!(android.contains("неизвестного источника"));
        assert!(android.contains(t(Lang::Ru, "menu.open_app")));

        let windows = file_caption(Lang::Ru, FilePlatform::Windows, "setup.exe");
        assert!(windows.contains("SmartScreen"));
        assert!(windows.contains("администратора"));

        let portable = file_caption(Lang::Ru, FilePlatform::WindowsPortable, "portable.zip");
        assert!(portable.contains("SmartScreen"));
        assert!(portable.contains("Распакуйте"));

        let macos = file_caption(Lang::Ru, FilePlatform::MacOs, "app.dmg");
        assert!(macos.contains("не подписано"));
        assert!(macos.contains("правой кнопкой"));

        let linux = file_caption(Lang::Ru, FilePlatform::Linux, "app.tar.gz");
        assert!(linux.contains("install.sh"));
    }

    /// Английская ветка не должна протекать русским ни для одной платформы.
    #[test]
    fn english_captions_have_no_cyrillic() {
        for platform in FilePlatform::ALL {
            let caption = file_caption(Lang::En, platform, "caramba-connect-1.2.bin");
            assert!(caption.contains("File: caramba-connect-1.2.bin"));
            assert!(
                !has_cyrillic(&caption),
                "в английской подписи оказалась кириллица: {caption}"
            );
            assert!(!has_cyrillic(platform.label(Lang::En)));
        }
        assert!(file_caption(Lang::En, FilePlatform::MacOs, "x").contains("not signed"));
        assert!(file_caption(Lang::En, FilePlatform::Windows, "x").contains("SmartScreen"));
    }

    /// Имя файла приходит извне и уходит в HTML parse mode: голый `<` уронил бы
    /// отправку целиком.
    #[test]
    fn file_name_is_html_escaped_in_caption_and_ack() {
        let caption = file_caption(Lang::Ru, FilePlatform::Android, "<b>evil</b>.apk");
        assert!(caption.contains("&lt;b&gt;evil&lt;/b&gt;.apk"));
        assert!(!caption.contains("<b>evil"));

        let ack = capture_ack_text(
            Lang::En,
            FilePlatform::Android,
            "<b>evil</b>.apk",
            1024 * 1024,
        );
        assert!(ack.contains("&lt;b&gt;evil&lt;/b&gt;.apk"));
    }

    /// Пустое имя убирает строку целиком, а не рисует пустой хвост.
    #[test]
    fn empty_file_name_drops_the_file_line() {
        for lang in [Lang::Ru, Lang::En] {
            let caption = file_caption(lang, FilePlatform::MacOs, "   ");
            assert!(!caption.contains("Файл:"));
            assert!(!caption.contains("File:"));
        }
    }

    /// Подтверждение владельцу называет платформу, файл, вес и все входы.
    #[test]
    fn ack_names_platform_size_and_entry_points() {
        let ack = capture_ack_text(Lang::Ru, FilePlatform::Windows, "setup.exe", 90_177_536);
        assert!(ack.contains("setup.exe"));
        assert!(ack.contains("86.0 МБ"));
        assert!(ack.contains("Windows (установщик)"));
        assert!(ack.contains(t(Lang::Ru, "app.apk_tg_btn")));
        assert!(ack.contains("/apk"));
        assert!(ack.contains("/start apk_windows"));

        let ack_en = capture_ack_text(Lang::En, FilePlatform::Windows, "setup.exe", 90_177_536);
        assert!(ack_en.contains("86.0 MB"));
        assert!(
            !has_cyrillic(&ack_en),
            "в английском подтверждении оказалась кириллица: {ack_en}"
        );
    }

    /// Без ссылки нельзя звать «нажмите кнопку ниже»: кнопки не будет.
    #[test]
    fn unavailable_text_matches_the_keyboard() {
        assert!(unavailable_text(Lang::Ru, true).contains("кнопке ниже"));
        assert!(!unavailable_text(Lang::Ru, false).contains("кнопке ниже"));
        assert!(unavailable_text(Lang::En, true).contains("button below"));
        assert!(!unavailable_text(Lang::En, false).contains("button below"));
    }

    /// Меню скачивания не обещает файлы «в Telegram», когда их нет.
    #[test]
    fn download_menu_text_mentions_telegram_only_with_files() {
        assert!(download_menu_text(Lang::Ru, true).contains("в Telegram"));
        assert!(!download_menu_text(Lang::Ru, false).contains("в Telegram"));
        assert!(download_menu_text(Lang::En, true).contains("in Telegram"));
        assert!(!download_menu_text(Lang::En, false).contains("in Telegram"));
        assert!(!has_cyrillic(platform_menu_text(Lang::En)));
    }
}

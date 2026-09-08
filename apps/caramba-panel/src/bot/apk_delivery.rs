//! Выдача установщика Caramba Connect (APK) файлом прямо в Telegram.
//!
//! ЗАЧЕМ. Единственный путь к установщику до сих пор шёл через кнопку-ссылку на
//! домен панели. Домен могут заблокировать — и человек, который в эту минуту
//! сидит в этом самом боте, остаётся без приложения, хотя Telegram у него
//! очевидно работает. Файл, отданный самим ботом, блокировкой домена не
//! отбирается.
//!
//! ПОЧЕМУ ЧЕРЕЗ `file_id`, А НЕ ЗАГРУЗКОЙ. Bot API не даёт боту ЗАГРУЖАТЬ файлы
//! больше 50 МБ, а APK весит около 86 — прямая отправка байтов невозможна в
//! принципе. Зато переслать файл, который уже лежит на серверах Telegram, по его
//! `file_id` можно без ограничения размера. Отсюда весь порядок работы: владелец
//! один раз отправляет APK боту как документ, бот запоминает `file_id`, и дальше
//! отдаёт этот файл всем по кнопке, команде `/apk` или диплинку `/start apk`.
//!
//! `file_id` привязан к конкретному боту (другой бот по нему файл не получит),
//! поэтому хранение его в настройках панели — это не утечка, а просто закладка.
//!
//! `file_id` может протухнуть (файл удалили из чата, Telegram перепаковал
//! хранилище). Мы это НЕ лечим автоматическим стиранием настройки: ошибка одной
//! отправки может быть и сетевой, а молча забытый файл владелец обнаружит
//! только по жалобам. Забывает файл человек кнопкой в админке.

use crate::AppState;
use crate::bot::translations::{Lang, t};
use crate::bot::utils::escape_html;
use teloxide::prelude::*;
use teloxide::types::{ChatId, FileId, InputFile, ParseMode};
use tracing::{error, warn};

/// Ключи настроек панели. Те же строки читает и админка (K2) — здесь они
/// объявлены один раз, чтобы у бота не было своего мнения об их написании.
pub const SETTING_APK_FILE_ID: &str = "app_apk_tg_file_id_android";
pub const SETTING_APK_FILE_NAME: &str = "app_apk_tg_file_name_android";
pub const SETTING_APK_FILE_SIZE: &str = "app_apk_tg_file_size_android";
pub const SETTING_APK_UPLOADED_AT: &str = "app_apk_tg_uploaded_at_android";

/// Админ ли этот Telegram-аккаунт.
///
/// ЗАЧЕМ ОТДЕЛЬНАЯ ФУНКЦИЯ. Приём APK обязан пускать ровно тех же людей, что и
/// `/admin`: право «залить установщик всем пользователям бота» не может быть
/// шире права администрировать панель. Раньше запрос жил одной копией внутри
/// ветки `/admin`; вторая копия рано или поздно разъехалась бы с первой.
///
/// Таблица `admins` хранит username, а не tg_id, поэтому пользователь ищется по
/// `tg_id` и сводится с админом по username. Следствие, которое стоит знать:
/// админ без выставленного в Telegram username не опознаётся — ровно как и в
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

/// Брать ли этот документ как новый установщик.
///
/// Чистая функция — весь предикат целиком, чтобы его можно было проверить
/// тестом, не поднимая ни бота, ни базу. Условий ровно два и оба обязательны:
/// отправитель — админ, и имя файла оканчивается на `.apk`. Расширение
/// сравнивается без учёта регистра: Android-сборки приезжают и как `app.apk`,
/// и как `Caramba-1.2.APK`.
pub(crate) fn should_capture(is_admin: bool, file_name: Option<&str>) -> bool {
    is_admin
        && file_name.is_some_and(|name| {
            let name = name.trim();
            name.len() > 4 && name.to_ascii_lowercase().ends_with(".apk")
        })
}

/// Размер в мегабайтах с одним знаком — как его называют люди («86.3 МБ»).
///
/// Мегабайт двоичный (1024×1024): именно так размер показывают Telegram и
/// файловые менеджеры на телефоне, и расхождение с их цифрой читалось бы как
/// «прислали не тот файл».
pub(crate) fn format_mb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

/// Подтверждение владельцу, что файл принят и уже раздаётся.
///
/// Называет и размер, и способы получения: единственный способ убедиться, что
/// бот запомнил именно тот файл, — увидеть его имя и вес обратно.
pub(crate) fn capture_ack_text(lang: Lang, file_name: &str, bytes: u64) -> String {
    let name = escape_html(file_name);
    let mb = format_mb(bytes);
    let btn = t(lang, "app.apk_tg_btn");
    match lang {
        Lang::Ru => format!(
            "✅ <b>APK принят:</b> {name}, {mb} МБ.\n\n\
             Теперь кнопка «{btn}» и команда /apk присылают этот файл."
        ),
        Lang::En => format!(
            "✅ <b>APK saved:</b> {name}, {mb} MB.\n\n\
             The “{btn}” button and the /apk command now send this file."
        ),
    }
}

/// Сообщение владельцу о неудачной записи настроек.
fn capture_failed_text(lang: Lang) -> &'static str {
    match lang {
        Lang::Ru => "⚠️ Не получилось сохранить APK. Попробуйте отправить файл ещё раз.",
        Lang::En => "⚠️ Couldn't save the APK. Try sending the file again.",
    }
}

/// Подпись к отправляемому APK.
///
/// ЗАЧЕМ ТАКОЙ ТЕКСТ. Человек получает в чат файл на 86 МБ и должен понять три
/// вещи: что это, что Android сейчас будет ругаться (иначе установка выглядит
/// как отказ), и куда идти дальше. Название кнопки входа берётся из той же
/// таблицы переводов, что и сама кнопка, — чтобы подпись не отсылала к пункту
/// меню, который переименовали.
///
/// Имя файла экранируется: оно приходит от владельца, а подпись уходит в HTML
/// parse mode, где голый `<` роняет отправку целиком. Пустое имя (настройка
/// потёрта руками) просто убирает строку, а не рисует «Версия из файла: ».
pub(crate) fn apk_caption(lang: Lang, file_name: &str) -> String {
    let name = file_name.trim();
    let open = t(lang, "menu.open_app");
    match lang {
        Lang::Ru => {
            let version_line = if name.is_empty() {
                String::new()
            } else {
                format!("\nВерсия из файла: {}", escape_html(name))
            };
            format!(
                "📦 <b>Caramba Connect для Android</b>{version_line}\n\n\
                 Установите APK (Android спросит разрешение на установку из неизвестного \
                 источника), затем вернитесь в бот и нажмите «{open}»."
            )
        }
        Lang::En => {
            let version_line = if name.is_empty() {
                String::new()
            } else {
                format!("\nFile version: {}", escape_html(name))
            };
            format!(
                "📦 <b>Caramba Connect for Android</b>{version_line}\n\n\
                 Install the APK (Android will ask for permission to install from an \
                 unknown source), then come back to the bot and tap “{open}”."
            )
        }
    }
}

/// Что сказать, когда файла в Telegram нет (или он не отдался).
///
/// Ветвится по тому, есть ли запасная ссылка: обещать «скоро», когда ссылка
/// настроена и кнопка тут же под сообщением, — значит спорить с собственной
/// кнопкой.
pub(crate) fn unavailable_text(lang: Lang, has_url: bool) -> &'static str {
    match (lang, has_url) {
        (Lang::Ru, true) => "📦 Установщик пока не лежит в Telegram. Скачайте его по кнопке ниже.",
        (Lang::Ru, false) => {
            "📦 Установщик для Android скоро появится здесь. Загляните чуть позже или \
             напишите в поддержку."
        }
        (Lang::En, true) => {
            "📦 The installer isn't stored in Telegram yet. Download it with the button below."
        }
        (Lang::En, false) => {
            "📦 The Android installer will appear here soon. Check back a bit later or \
             contact support."
        }
    }
}

/// Приём APK от владельца: документ в личке боту.
///
/// Вызывается на КАЖДОМ сообщении с документом. Документы от посторонних и
/// не-apk от админа не порождают ни ответа, ни записи — бот ведёт себя ровно
/// так же, как до этой функции (молча), чтобы случайный файл в чате не
/// превращался в диалог.
pub async fn handle_admin_document(bot: &Bot, msg: &Message, state: &AppState) {
    let Some(doc) = msg.document() else {
        return;
    };
    let tg_id = msg.chat.id.0;
    let is_admin = is_bot_admin(&state.pool, tg_id).await;
    if !should_capture(is_admin, doc.file_name.as_deref()) {
        return;
    }

    let file_name = doc
        .file_name
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string();
    let size = doc.file.size as u64;
    let lang = crate::bot::utils::lang_by_tg_id(state, tg_id).await;

    // ПОРЯДОК ЗАПИСИ ВАЖЕН: `file_id` пишется последним, потому что именно он
    // включает кнопку и команду. Оборвись запись посередине — пользователи
    // увидят прежнее состояние, а не кнопку без файла.
    let writes: [(&str, String); 3] = [
        (SETTING_APK_FILE_NAME, file_name.clone()),
        (SETTING_APK_FILE_SIZE, size.to_string()),
        (SETTING_APK_UPLOADED_AT, chrono::Utc::now().to_rfc3339()),
    ];
    for (key, value) in writes {
        if let Err(e) = state.settings.set(key, &value).await {
            error!("apk capture: failed to store {key}: {e:#}");
            let _ = bot
                .send_message(msg.chat.id, capture_failed_text(lang))
                .await;
            return;
        }
    }
    if let Err(e) = state
        .settings
        .set(SETTING_APK_FILE_ID, &doc.file.id.0)
        .await
    {
        error!("apk capture: failed to store file_id: {e:#}");
        let _ = bot
            .send_message(msg.chat.id, capture_failed_text(lang))
            .await;
        return;
    }

    tracing::info!(
        "apk capture: stored {} ({} bytes) from admin tg_id {}",
        file_name,
        size,
        tg_id
    );
    let _ = bot
        .send_message(msg.chat.id, capture_ack_text(lang, &file_name, size))
        .parse_mode(ParseMode::Html)
        .await
        .map_err(|e| error!("apk capture: failed to confirm to admin: {e}"));
}

/// Единственная точка выдачи APK: кнопка `apk_send`, команда `/apk`,
/// диплинк `/start apk`.
///
/// Три входа намеренно сходятся в одну функцию — иначе текст подписи и
/// поведение при пустом/протухшем `file_id` разъехались бы по трём местам.
///
/// Аргументы в порядке «куда, на каком языке, чем» — как записано в решении по
/// задаче; `state` последним, потому что он нужен только за настройками.
pub async fn send_apk(bot: &Bot, chat_id: ChatId, lang: Lang, state: &AppState) {
    let file_id = state.settings.get_or_default(SETTING_APK_FILE_ID, "").await;
    let file_id = file_id.trim();
    if file_id.is_empty() {
        send_unavailable(bot, chat_id, lang, state).await;
        return;
    }

    let file_name = state
        .settings
        .get_or_default(SETTING_APK_FILE_NAME, "")
        .await;

    let document = InputFile::file_id(FileId(file_id.to_string()));
    match bot
        .send_document(chat_id, document)
        .caption(apk_caption(lang, &file_name))
        .parse_mode(ParseMode::Html)
        .await
    {
        Ok(_) => {}
        Err(e) => {
            // Самая вероятная причина — протухший `file_id`. Настройку не
            // трогаем (см. заголовок модуля), человеку даём запасной путь.
            warn!("apk send: send_document by file_id failed: {e}");
            send_unavailable(bot, chat_id, lang, state).await;
        }
    }
}

/// Запасной путь: объяснение + кнопка-ссылка, если оператор её настроил.
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

    /// Предикат приёма: оба условия обязательны, регистр расширения не важен.
    #[test]
    fn capture_requires_admin_and_apk_extension() {
        assert!(should_capture(true, Some("caramba-connect.apk")));
        assert!(should_capture(true, Some("Caramba-1.2.APK")));
        assert!(should_capture(true, Some("  app.Apk  ")));

        // Не админ — не принимаем даже правильный файл.
        assert!(!should_capture(false, Some("caramba-connect.apk")));
        // Админ, но не установщик.
        assert!(!should_capture(true, Some("notes.txt")));
        assert!(!should_capture(true, Some("apk")));
        assert!(!should_capture(true, Some("app.apk.zip")));
        // Документ без имени файла.
        assert!(!should_capture(true, None));
        // Файл, у которого имя — одно расширение: имени сборки нет, брать нечего.
        assert!(!should_capture(true, Some(".apk")));
    }

    /// Размер считается в двоичных мегабайтах — как его показывает сам Telegram.
    #[test]
    fn size_is_formatted_in_binary_megabytes() {
        assert_eq!(format_mb(0), "0.0");
        assert_eq!(format_mb(1024 * 1024), "1.0");
        assert_eq!(format_mb(90_177_536), "86.0");
        // 86 000 000 байт — это НЕ 86 МБ, и цифра обязана это показывать.
        assert_eq!(format_mb(86_000_000), "82.0");
    }

    /// Подпись обязана объяснить предупреждение Android и увести обратно в бот.
    #[test]
    fn russian_caption_warns_about_unknown_source() {
        let caption = apk_caption(Lang::Ru, "caramba-connect-1.2.apk");
        assert!(caption.contains("Версия из файла: caramba-connect-1.2.apk"));
        assert!(caption.contains("неизвестного источника"));
        assert!(caption.contains(t(Lang::Ru, "menu.open_app")));
    }

    /// Английская ветка не должна протекать русским.
    #[test]
    fn english_caption_has_no_cyrillic() {
        let caption = apk_caption(Lang::En, "caramba-connect-1.2.apk");
        assert!(caption.contains("File version: caramba-connect-1.2.apk"));
        assert!(caption.contains("unknown source"));
        assert!(
            !caption
                .chars()
                .any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)),
            "в английской подписи оказалась кириллица: {caption}"
        );
    }

    /// Имя файла приходит извне и уходит в HTML parse mode — голый `<` уронил бы
    /// отправку целиком.
    #[test]
    fn file_name_is_html_escaped_in_caption_and_ack() {
        let caption = apk_caption(Lang::Ru, "<b>evil</b>.apk");
        assert!(caption.contains("&lt;b&gt;evil&lt;/b&gt;.apk"));
        assert!(!caption.contains("<b>evil"));

        let ack = capture_ack_text(Lang::En, "<b>evil</b>.apk", 1024 * 1024);
        assert!(ack.contains("&lt;b&gt;evil&lt;/b&gt;.apk"));
    }

    /// Пустое имя убирает строку целиком, а не рисует пустой хвост.
    #[test]
    fn empty_file_name_drops_the_version_line() {
        for lang in [Lang::Ru, Lang::En] {
            let caption = apk_caption(lang, "   ");
            assert!(!caption.contains("Версия из файла"));
            assert!(!caption.contains("File version"));
        }
    }

    /// Подтверждение владельцу называет и файл, и вес, и оба способа выдачи.
    #[test]
    fn ack_names_the_file_size_and_both_entry_points() {
        let ack = capture_ack_text(Lang::Ru, "caramba.apk", 90_177_536);
        assert!(ack.contains("caramba.apk"));
        assert!(ack.contains("86.0 МБ"));
        assert!(ack.contains(t(Lang::Ru, "app.apk_tg_btn")));
        assert!(ack.contains("/apk"));

        let ack_en = capture_ack_text(Lang::En, "caramba.apk", 90_177_536);
        assert!(ack_en.contains("86.0 MB"));
        assert!(
            !ack_en
                .chars()
                .any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)),
            "в английском подтверждении оказалась кириллица: {ack_en}"
        );
    }

    /// Без ссылки нельзя звать «нажмите кнопку ниже» — кнопки не будет.
    #[test]
    fn unavailable_text_matches_the_keyboard() {
        assert!(unavailable_text(Lang::Ru, true).contains("кнопке ниже"));
        assert!(!unavailable_text(Lang::Ru, false).contains("кнопке ниже"));
        assert!(unavailable_text(Lang::En, true).contains("button below"));
        assert!(!unavailable_text(Lang::En, false).contains("button below"));
    }
}

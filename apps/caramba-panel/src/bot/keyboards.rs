use crate::bot::translations::{Lang, t};
use teloxide::types::{
    CopyTextButton, InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup,
};

/// Главное меню (reply keyboard).
///
/// Подписи кнопок локализованы. Нажатие такой кнопки приходит боту обычным
/// текстовым сообщением, поэтому `command.rs::menu_action` распознаёт их на
/// обоих языках (плюс старые английские подписи как legacy-алиасы — у клиентов
/// уже отрисованные клавиатуры не обновляются сами).
pub fn main_menu(lang: Lang, app_mode: bool, always_support: bool) -> KeyboardMarkup {
    if app_mode {
        // Режим «только приложение» обязан оставлять дорогу В приложение.
        //
        // Раньше он прятал всё, кроме поддержки, — включая единственную кнопку,
        // по которой бот отдаёт ссылку caramba://connect. Выпуск ссылки при этом
        // работал и был выкачен, но нажать было негде: на боевой панели в этом
        // режиме за всё время не выдалось ни одного кода. Функция, до которой
        // нельзя дотянуться, ничем не отличается от отсутствующей.
        let mut row = vec![KeyboardButton::new(t(lang, "menu.open_app"))];
        if always_support {
            row.push(KeyboardButton::new(t(lang, "menu.support")));
        }
        return KeyboardMarkup::new(vec![row]).resize_keyboard();
    }

    KeyboardMarkup::new(vec![
        vec![
            KeyboardButton::new(t(lang, "menu.buy")),
            KeyboardButton::new(t(lang, "menu.services")),
        ],
        vec![
            KeyboardButton::new(t(lang, "menu.store")),
            KeyboardButton::new(t(lang, "menu.profile")),
        ],
        vec![
            KeyboardButton::new(t(lang, "menu.referral")),
            KeyboardButton::new(t(lang, "menu.support")),
        ],
        vec![
            KeyboardButton::new(t(lang, "menu.guides")),
            KeyboardButton::new(t(lang, "menu.open_app")),
        ],
    ])
    .resize_keyboard()
}

/// Платформы инструкций в порядке показа; ключ настройки — `guide_url_{id}`.
pub const GUIDE_PLATFORMS: [&str; 7] = [
    "ios", "android", "windows", "macos", "linux", "tv", "router",
];

/// Инлайн-кнопки со ссылками на инструкции (Telegraph). Адреса лежат в
/// настройках панели, чтобы менять их без релиза; пустые пропускаются.
/// `None` — если не опубликована ни одна страница.
pub async fn guides_keyboard(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Option<InlineKeyboardMarkup> {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    let mut row: Vec<InlineKeyboardButton> = Vec::new();
    for id in GUIDE_PLATFORMS {
        let url = settings
            .get_or_default(&format!("guide_url_{id}"), "")
            .await;
        let Ok(parsed) = url.trim().parse::<reqwest::Url>() else {
            continue;
        };
        row.push(InlineKeyboardButton::url(
            t(lang, &format!("guides.{id}")),
            parsed,
        ));
        // Роутер — отдельной строкой, остальные по две.
        if row.len() == 2 || id == "router" {
            rows.push(std::mem::take(&mut row));
        }
    }
    if !row.is_empty() {
        rows.push(row);
    }
    if rows.is_empty() {
        None
    } else {
        Some(InlineKeyboardMarkup::new(rows))
    }
}

/// Одна кнопка «Пошаговая инструкция» — к сообщению со ссылками подписки.
pub async fn guide_index_button(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Option<InlineKeyboardMarkup> {
    let url = settings.get_or_default("guide_url_index", "").await;
    let parsed = url.trim().parse::<reqwest::Url>().ok()?;
    Some(InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::url(t(lang, "guides.index_btn"), parsed),
    ]]))
}

/// Кнопка-ссылка «Скачать для Android».
///
/// Адрес APK задаёт оператор в Settings → «Caramba Connect app — download
/// links»; пусто или не https — кнопки нет. Требование https не косметическое:
/// по кнопке человек ставит себе APK, и отдавать его по открытому каналу
/// значит разрешить подменить установочный файл по дороге. Telegram к тому же
/// не примет URL-кнопку с посторонней схемой.
async fn app_download_url_button(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Option<InlineKeyboardButton> {
    let url = settings
        .get_or_default("app_download_url_android", "")
        .await;
    let parsed = url.trim().parse::<reqwest::Url>().ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    Some(InlineKeyboardButton::url(
        t(lang, "app.download_android_btn"),
        parsed,
    ))
}

/// Только кнопка-ссылка, без выдачи файла.
///
/// Нужна запасному пути в `apk_delivery`: сообщение «файла в Telegram нет» не
/// может нести кнопку «получить файл в Telegram» — она вернула бы человека в
/// то же самое сообщение по кругу.
pub async fn app_download_url_keyboard(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Option<InlineKeyboardMarkup> {
    let button = app_download_url_button(settings, lang).await?;
    Some(InlineKeyboardMarkup::new(vec![vec![button]]))
}

/// Способы забрать приложение — строки для клавиатуры сообщения со ссылкой.
///
/// Две строки, обе необязательные: ссылка на домен панели и выдача APK файлом
/// прямо в Telegram. ПОРЯДОК НЕ СЛУЧАЕН: ссылка отдаёт всегда свежую сборку с
/// сервера, файл в Telegram — ту, что владелец загрузил руками, поэтому ссылка
/// остаётся первой, пока работает. Вторая строка — страховка ровно на тот
/// случай, ради которого всё затевалось: домен заблокирован, а Telegram у
/// человека очевидно работает, раз он читает это сообщение.
///
/// Кнопка выдачи файла появляется только когда `file_id` действительно записан
/// (см. `apk_delivery`): кнопка, которая отвечает «файла нет», хуже отсутствия
/// кнопки. Если не настроено ничего — клавиатуры нет вовсе.
async fn app_download_rows(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Vec<Vec<InlineKeyboardButton>> {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    if let Some(button) = app_download_url_button(settings, lang).await {
        rows.push(vec![button]);
    }
    let has_file = !settings
        .get_or_default(crate::bot::apk_delivery::SETTING_APK_FILE_ID, "")
        .await
        .trim()
        .is_empty();
    if has_file {
        rows.push(vec![InlineKeyboardButton::callback(
            t(lang, "app.apk_tg_btn"),
            "apk_send",
        )]);
    }
    rows
}

/// Предел Telegram на текст кнопки копирования: 1..=256 символов.
const COPY_TEXT_MAX_CHARS: usize = 256;

/// Строка с нативной кнопкой копирования ссылки, если ссылка в предел влезает.
///
/// ЗАЧЕМ ОТДЕЛЬНАЯ КНОПКА. Тап по `<code>` копирует не везде: на части клиентов
/// он открывает меню, а на десктопе выделяет строку целиком вместе с переносами.
/// Кнопка копирует ровно ссылку и ровно в буфер — это единственный путь для
/// человека, у которого схема `caramba://` не перехватывается системой.
///
/// `None` при переполнении: Telegram отвергает такую кнопку и роняет ОТПРАВКУ
/// ВСЕГО сообщения, то есть человек остался бы вообще без ссылки. Ссылка длиннее
/// 256 символов реальна — её длину задаёт имя оператора из настроек.
fn copy_link_row(lang: Lang, link: &str) -> Option<Vec<InlineKeyboardButton>> {
    let len = link.chars().count();
    if len == 0 || len > COPY_TEXT_MAX_CHARS {
        return None;
    }
    Some(vec![InlineKeyboardButton::copy_text_button(
        t(lang, "app.copy_link_btn"),
        CopyTextButton {
            text: link.to_string(),
        },
    )])
}

/// Клавиатура сообщения со ссылкой входа: сначала «Скопировать ссылку», затем
/// способы забрать приложение.
///
/// ПОРЯДОК НЕ СЛУЧАЕН: копирование относится к самому сообщению и нужно каждому,
/// кто его открыл; скачивание — только тому, у кого приложения ещё нет.
pub async fn connect_link_keyboard(
    settings: &crate::settings::SettingsService,
    lang: Lang,
    link: &str,
) -> Option<InlineKeyboardMarkup> {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    if let Some(row) = copy_link_row(lang, link) {
        rows.push(row);
    }
    rows.extend(app_download_rows(settings, lang).await);
    if rows.is_empty() {
        None
    } else {
        Some(InlineKeyboardMarkup::new(rows))
    }
}

/// Выбор языка. Намеренно двуязычная — показывается до того, как язык известен.
pub fn language_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("🇺🇸 English", "set_lang_en"),
        InlineKeyboardButton::callback("🇷🇺 Русский", "set_lang_ru"),
    ]])
}

pub fn terms_keyboard(lang: Lang) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(t(lang, "terms.accept"), "accept_terms"),
        InlineKeyboardButton::callback(t(lang, "terms.decline"), "decline_terms"),
    ]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use teloxide::types::InlineKeyboardButtonKind;

    /// Первой строкой обязана идти именно кнопка копирования со ССЫЛКОЙ внутри:
    /// это единственный путь для человека, у которого `caramba://` не
    /// перехватывается системой.
    #[test]
    fn copy_row_carries_the_link_itself() {
        let link = "caramba://connect?d=ABC123";
        let row = copy_link_row(Lang::Ru, link).expect("обычная ссылка влезает в предел");
        assert_eq!(row.len(), 1);
        match &row[0].kind {
            InlineKeyboardButtonKind::CopyText(copy) => assert_eq!(copy.text, link),
            other => panic!("не кнопка копирования: {other:?}"),
        }
    }

    /// Ссылка длиннее предела обязана оставить сообщение без кнопки, а не
    /// уронить отправку: без сообщения человек остался бы вообще без ссылки.
    #[test]
    fn over_long_link_drops_the_button_instead_of_the_message() {
        let link = format!("caramba://connect?d={}", "A".repeat(COPY_TEXT_MAX_CHARS));
        assert!(copy_link_row(Lang::Ru, &link).is_none());
        // Ровно на пределе кнопка обязана быть: гейт отсекает переполнение, а не
        // «длинные ссылки вообще».
        let edge = "c".repeat(COPY_TEXT_MAX_CHARS);
        assert!(copy_link_row(Lang::En, &edge).is_some());
    }
}

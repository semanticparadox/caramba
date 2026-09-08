use crate::bot::translations::{Lang, t};
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};

/// Главное меню (reply keyboard).
///
/// Подписи кнопок локализованы. Нажатие такой кнопки приходит боту обычным
/// текстовым сообщением, поэтому `command.rs::menu_action` распознаёт их на
/// обоих языках (плюс старые английские подписи как legacy-алиасы — у клиентов
/// уже отрисованные клавиатуры не обновляются сами).
pub fn main_menu(lang: Lang, app_mode: bool, always_support: bool) -> KeyboardMarkup {
    if app_mode {
        let mut row = Vec::new();
        if always_support {
            row.push(KeyboardButton::new(t(lang, "menu.support")));
        }

        if row.is_empty() {
            // Return empty markup or hidden
            return KeyboardMarkup::new(Vec::<Vec<KeyboardButton>>::new()).resize_keyboard();
        } else {
            return KeyboardMarkup::new(vec![row]).resize_keyboard();
        }
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

/// Способы забрать приложение — к сообщению со ссылкой входа.
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
pub async fn app_download_keyboard(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Option<InlineKeyboardMarkup> {
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
    if rows.is_empty() {
        None
    } else {
        Some(InlineKeyboardMarkup::new(rows))
    }
}

/// Инлайн-клавиатура «прислать заново» — висит ТОЛЬКО на сообщении с кодом
/// входа (`command.rs::send_login_code`). По нажатию callback `get_login_code`
/// присылает заново обе части: ссылку и код, — потому что человек нажимает её
/// как раз тогда, когда первая пара уже протухла.
pub fn login_code_keyboard(lang: Lang) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        t(lang, "login.get_code_btn"),
        "get_login_code",
    )]])
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

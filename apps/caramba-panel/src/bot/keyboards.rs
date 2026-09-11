use crate::bot::apk_delivery::FilePlatform;
use crate::bot::translations::{Lang, t, tf};
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
        // Раньше он прятал всё, кроме поддержки, включая единственную кнопку,
        // по которой бот отдаёт ссылку caramba://connect. Выпуск ссылки при этом
        // работал и был выкачен, но нажать было негде: на боевой панели в этом
        // режиме за всё время не выдалось ни одного кода. Функция, до которой
        // нельзя дотянуться, ничем не отличается от отсутствующей.
        //
        // Каждая кнопка своей строкой: владелец не нашёл, где скопировать ссылку,
        // когда она пряталась под «Войти в приложение», поэтому «Подключить»
        // и «Скачать» стоят первыми и на всю ширину, поддержка последней.
        //
        // Третьей строкой «📖 Инструкция» рядом с поддержкой: инструкции
        // существовали и раньше, но в этом режиме до них нельзя было дойти
        // ничем, кроме сообщения онбординга.
        let mut rows = vec![
            vec![KeyboardButton::new(t(lang, "menu.open_app"))],
            vec![KeyboardButton::new(t(lang, "menu.download_app"))],
        ];
        let mut third = vec![KeyboardButton::new(t(lang, "menu.guides"))];
        if always_support {
            third.push(KeyboardButton::new(t(lang, "menu.support")));
        }
        rows.push(third);
        return KeyboardMarkup::new(rows).resize_keyboard();
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

/// Страницы базы знаний после платформ; ключ настройки — `guide_url_{id}`,
/// подпись — `guides.{id}`. Корневая `index` идёт отдельно первой строкой.
pub const GUIDE_PAGES: [&str; 4] = ["app", "plans", "devices", "faq"];

/// Все ключи настроек `guide_url_*`, которые читает бот: корневая, платформы,
/// страницы базы знаний. Вне тестов не используется, как и `translations::KEYS`:
/// на нём держится проверка, что у каждого ключа есть подпись и что список
/// совпадает с полями админки и `docs/guides/pages.json`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn guide_setting_keys() -> Vec<String> {
    std::iter::once("index")
        .chain(GUIDE_PLATFORMS)
        .chain(GUIDE_PAGES)
        .map(|id| format!("guide_url_{id}"))
        .collect()
}

/// Кнопка-ссылка на страницу инструкции `id`, если её адрес задан.
async fn guide_button(
    settings: &crate::settings::SettingsService,
    lang: Lang,
    id: &str,
) -> Option<InlineKeyboardButton> {
    let url = settings
        .get_or_default(&format!("guide_url_{id}"), "")
        .await;
    let parsed = url.trim().parse::<reqwest::Url>().ok()?;
    Some(InlineKeyboardButton::url(
        t(lang, &format!("guides.{id}")),
        parsed,
    ))
}

/// Инлайн-кнопки со ссылками на инструкции (Telegraph). Адреса лежат в
/// настройках панели, чтобы менять их без релиза; пустые пропускаются.
/// `None` — если не опубликована ни одна страница.
///
/// ПОРЯДОК: первой и на всю ширину «📖 Ваши следующие шаги» (корневая
/// страница, с неё начинают), затем платформы по две (роутер отдельно), затем
/// страницы базы знаний по две.
pub async fn guides_keyboard(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Option<InlineKeyboardMarkup> {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    if let Some(index) = guide_button(settings, lang, "index").await {
        rows.push(vec![index]);
    }
    let mut row: Vec<InlineKeyboardButton> = Vec::new();
    for id in GUIDE_PLATFORMS {
        let Some(button) = guide_button(settings, lang, id).await else {
            continue;
        };
        row.push(button);
        // Роутер — отдельной строкой, остальные по две.
        if row.len() == 2 || id == "router" {
            rows.push(std::mem::take(&mut row));
        }
    }
    if !row.is_empty() {
        rows.push(row);
    }
    let mut pages = Vec::new();
    for id in GUIDE_PAGES {
        if let Some(button) = guide_button(settings, lang, id).await {
            pages.push(button);
        }
    }
    rows.extend(two_per_row(pages));
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

/// Платформы ссылок на сайт в порядке показа; ключ настройки
/// `app_download_url_{id}`, подпись кнопки `app.download_{id}_btn`.
pub const APP_DOWNLOAD_PLATFORMS: [&str; 5] = ["android", "ios", "windows", "macos", "linux"];

/// Кнопки-ссылки «Скачать для …» для всех настроенных платформ.
///
/// Адреса задаёт оператор в Settings → «Caramba Connect app — download
/// links»; пусто или не https, значит кнопки нет. Требование https не
/// косметическое: по кнопке человек ставит себе установщик, и отдавать его по
/// открытому каналу значит разрешить подменить файл по дороге. Telegram к тому
/// же не примет URL-кнопку с посторонней схемой.
async fn app_download_url_buttons(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Vec<InlineKeyboardButton> {
    let mut buttons = Vec::new();
    for id in APP_DOWNLOAD_PLATFORMS {
        let url = settings
            .get_or_default(&format!("app_download_url_{id}"), "")
            .await;
        let Ok(parsed) = url.trim().parse::<reqwest::Url>() else {
            continue;
        };
        if parsed.scheme() != "https" {
            continue;
        }
        buttons.push(InlineKeyboardButton::url(
            t(lang, &format!("app.download_{id}_btn")),
            parsed,
        ));
    }
    buttons
}

/// Кнопки-ссылки по две в строке: пять платформ в столбик растягивают
/// сообщение на весь экран телефона, а по две подписи ещё читаются.
fn two_per_row(buttons: Vec<InlineKeyboardButton>) -> Vec<Vec<InlineKeyboardButton>> {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for button in buttons {
        match rows.last_mut() {
            Some(row) if row.len() < 2 => row.push(button),
            _ => rows.push(vec![button]),
        }
    }
    rows
}

/// Только кнопки-ссылки, без выдачи файла.
///
/// Нужна запасному пути в `apk_delivery`: сообщение «файла в Telegram нет» не
/// может нести кнопку «получить файл в Telegram», она вернула бы человека в
/// то же самое сообщение по кругу.
pub async fn app_download_url_keyboard(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Option<InlineKeyboardMarkup> {
    let rows = two_per_row(app_download_url_buttons(settings, lang).await);
    if rows.is_empty() {
        None
    } else {
        Some(InlineKeyboardMarkup::new(rows))
    }
}

/// Меню выбора платформы для файлов из Telegram: одна кнопка на загруженную
/// платформу, каждая своей строкой.
pub fn tg_file_platform_keyboard(lang: Lang, uploaded: &[FilePlatform]) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(
        uploaded
            .iter()
            .map(|platform| {
                vec![InlineKeyboardButton::callback(
                    platform.label(lang),
                    platform.callback_data(),
                )]
            })
            .collect::<Vec<_>>(),
    )
}

/// Общее меню скачивания под «📥 Скачать приложение»: ссылки на сайт для
/// настроенных платформ, затем файлы из Telegram для загруженных.
///
/// ПОРЯДОК НЕ СЛУЧАЕН: ссылка отдаёт всегда свежую сборку с сервера, файл в
/// Telegram ту, что владелец загрузил руками, поэтому ссылки первыми. Файлы
/// подписаны «в Telegram», чтобы отличаться от ссылок на ту же платформу
/// строкой выше. `None`, если не настроено ничего.
pub async fn download_menu_keyboard(
    settings: &crate::settings::SettingsService,
    lang: Lang,
    uploaded: &[FilePlatform],
) -> Option<InlineKeyboardMarkup> {
    let mut rows = two_per_row(app_download_url_buttons(settings, lang).await);
    for platform in uploaded {
        rows.push(vec![InlineKeyboardButton::callback(
            tf(lang, "app.tg_file_platform_btn", &[platform.label(lang)]),
            platform.callback_data(),
        )]);
    }
    if rows.is_empty() {
        None
    } else {
        Some(InlineKeyboardMarkup::new(rows))
    }
}

/// Способы забрать приложение: строки для клавиатуры сообщения со ссылкой.
///
/// Ссылки на сайт для всех настроенных платформ и одна кнопка выдачи файла
/// прямо в Telegram. ПОРЯДОК НЕ СЛУЧАЕН: ссылка отдаёт всегда свежую сборку с
/// сервера, файл в Telegram ту, что владелец загрузил руками, поэтому ссылки
/// остаются первыми, пока работают. Кнопка файла страховка ровно на тот
/// случай, ради которого всё затевалось: домен заблокирован, а Telegram у
/// человека очевидно работает, раз он читает это сообщение.
///
/// Кнопка выдачи файла появляется только когда хоть один `file_id`
/// действительно записан (см. `apk_delivery`): кнопка, которая отвечает
/// «файла нет», хуже отсутствия кнопки. Если не настроено ничего, клавиатуры
/// нет вовсе.
async fn app_download_rows(
    settings: &crate::settings::SettingsService,
    lang: Lang,
) -> Vec<Vec<InlineKeyboardButton>> {
    let mut rows = two_per_row(app_download_url_buttons(settings, lang).await);
    let has_file = !crate::bot::apk_delivery::uploaded_platforms(settings)
        .await
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

    /// Меню app_only: подключить, скачать, третьей строкой инструкция и
    /// поддержка. Без поддержки третья строка остаётся с одной инструкцией.
    #[test]
    fn app_only_menu_keeps_the_guide_reachable() {
        let kb = main_menu(Lang::Ru, true, true);
        let rows: Vec<Vec<String>> = kb
            .keyboard
            .iter()
            .map(|r| r.iter().map(|b| b.text.clone()).collect())
            .collect();
        assert_eq!(
            rows,
            vec![
                vec![t(Lang::Ru, "menu.open_app").to_string()],
                vec![t(Lang::Ru, "menu.download_app").to_string()],
                vec![
                    t(Lang::Ru, "menu.guides").to_string(),
                    t(Lang::Ru, "menu.support").to_string()
                ],
            ]
        );
        let kb = main_menu(Lang::En, true, false);
        assert_eq!(kb.keyboard.len(), 3);
        assert_eq!(kb.keyboard[2].len(), 1);
        assert_eq!(kb.keyboard[2][0].text, t(Lang::En, "menu.guides"));
    }

    /// У каждого ключа настройки есть подпись на обоих языках, и все
    /// двенадцать ключей уникальны: по этому списку сверяются админка и
    /// скрипт публикации.
    #[test]
    fn every_guide_key_has_a_label_and_is_unique() {
        use crate::bot::translations::MISSING_FOR_TESTS;
        let keys = guide_setting_keys();
        assert_eq!(keys.len(), 1 + GUIDE_PLATFORMS.len() + GUIDE_PAGES.len());
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "дубль ключа guide_url_*");
        for id in std::iter::once("index")
            .chain(GUIDE_PLATFORMS)
            .chain(GUIDE_PAGES)
        {
            for lang in [Lang::Ru, Lang::En] {
                assert_ne!(
                    t(lang, &format!("guides.{id}")),
                    MISSING_FOR_TESTS,
                    "нет подписи guides.{id} ({lang:?})"
                );
            }
        }
        assert_eq!(keys[0], "guide_url_index");
        assert_eq!(keys.last().map(String::as_str), Some("guide_url_faq"));
    }

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

    /// Ссылки на сайт раскладываются по две в строке, хвост из одной кнопки
    /// не теряется.
    #[test]
    fn url_buttons_are_laid_out_two_per_row() {
        let mk = |n: usize| {
            (0..n)
                .map(|i| InlineKeyboardButton::callback(format!("b{i}"), format!("c{i}")))
                .collect::<Vec<_>>()
        };
        assert!(two_per_row(mk(0)).is_empty());
        let rows = two_per_row(mk(5));
        assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 2, 1]);
    }

    /// Меню файлов из Telegram: по строке на платформу, callback несёт её id.
    #[test]
    fn tg_file_menu_has_one_row_per_uploaded_platform() {
        let uploaded = [FilePlatform::Android, FilePlatform::MacOs];
        let kb = tg_file_platform_keyboard(Lang::Ru, &uploaded);
        assert_eq!(kb.inline_keyboard.len(), 2);
        match &kb.inline_keyboard[1][0].kind {
            InlineKeyboardButtonKind::CallbackData(data) => assert_eq!(data, "apk_send_macos"),
            other => panic!("не callback-кнопка: {other:?}"),
        }
        assert_eq!(kb.inline_keyboard[0][0].text, "📲 Android (APK)");
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

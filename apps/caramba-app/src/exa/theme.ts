import WebApp from '@twa-dev/sdk'

/** Тема берётся из Telegram: `colorScheme` даёт dark/light, а параметры
 *  `--tg-theme-*` Telegram сам кладёт на :root — токены их подхватывают.
 *  Вне Telegram (dev в браузере) остаётся тёмная брендовая тема. */
export function applyTelegramTheme(): () => void {
    const root = document.documentElement

    const apply = () => {
        const scheme = WebApp.colorScheme === 'light' ? 'light' : 'dark'
        root.dataset.theme = scheme
        // Шапка и фон Mini App — в цвет холста, чтобы сверху не было чужой полосы.
        const bg = scheme === 'light' ? '#e9ecef' : '#0e1013'
        try {
            WebApp.setHeaderColor(bg)
            WebApp.setBackgroundColor(bg)
        } catch {
            /* старые клиенты Telegram без этих методов */
        }
    }

    apply()
    WebApp.onEvent('themeChanged', apply)
    return () => WebApp.offEvent('themeChanged', apply)
}

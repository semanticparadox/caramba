import WebApp from '@twa-dev/sdk'

/**
 * Тонкая обёртка над WebApp.HapticFeedback: вне Telegram (dev в браузере)
 * SDK может кидать — все вызовы обёрнуты в try/catch и безопасны.
 * Использовать на ключевых тапах: покупка, подключение, переключение табов.
 */
export function hapticTap() {
    try {
        WebApp.HapticFeedback.impactOccurred('light')
    } catch {
        /* вне Telegram — тишина */
    }
}

export function hapticSelect() {
    try {
        WebApp.HapticFeedback.selectionChanged()
    } catch {
        /* вне Telegram — тишина */
    }
}

export function hapticSuccess() {
    try {
        WebApp.HapticFeedback.notificationOccurred('success')
    } catch {
        /* вне Telegram — тишина */
    }
}

export function hapticError() {
    try {
        WebApp.HapticFeedback.notificationOccurred('error')
    } catch {
        /* вне Telegram — тишина */
    }
}

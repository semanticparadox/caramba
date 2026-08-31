import WebApp from '@twa-dev/sdk'

type HapticImpactStyle = 'light' | 'medium' | 'heavy' | 'rigid' | 'soft'
type HapticNotificationType = 'success' | 'warning' | 'error'

export const LANGUAGE_STORAGE_KEY = 'exa_lang'

/** Language the app starts in: the user's own choice wins over the Telegram
 *  client's locale — an English Telegram account is not a preference for an
 *  English interface, and most of the audience is Russian-speaking. */
export const getTelegramLanguage = (): string => {
  try {
    const saved = localStorage.getItem(LANGUAGE_STORAGE_KEY)
    if (saved === 'ru' || saved === 'en') return saved
  } catch {
    // private mode / storage disabled — fall through to the Telegram locale
  }
  const raw = WebApp.initDataUnsafe?.user?.language_code || 'ru'
  return raw.toLowerCase().startsWith('ru') ? 'ru' : 'en'
}

export const triggerSelectionHaptic = () => {
  WebApp.HapticFeedback?.selectionChanged?.()
}

export const triggerImpactHaptic = (style: HapticImpactStyle = 'light') => {
  WebApp.HapticFeedback?.impactOccurred?.(style)
}

export const triggerNotificationHaptic = (
  type: HapticNotificationType = 'success',
) => {
  WebApp.HapticFeedback?.notificationOccurred?.(type)
}

export const runTelegramReady = () => {
  WebApp.ready()
  WebApp.expand()
}

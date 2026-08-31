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

/** Screens a notification button can point at.
 *
 *  A notification links in as `https://t.me/<bot>/<app>?startapp=<slug>`, and
 *  Telegram hands the slug to the Mini App as `start_param`. Without this map
 *  the parameter is inert and every button lands on the home screen — which is
 *  what "top up your balance" used to mean in practice: two more taps for the
 *  person to find on their own. Slugs mirror `ButtonTarget::slug()` on the
 *  panel side; an unknown one is ignored rather than guessed at. */
const START_PARAM_ROUTES: Record<string, string> = {
  billing: '/billing',
  topup: '/billing',
  plans: '/plans',
  subscription: '/subscription',
}

/** Route the launch parameter asks for, or null when there is none we know. */
export const getStartRoute = (): string | null => {
  const raw = WebApp.initDataUnsafe?.start_param
  if (typeof raw !== 'string') return null
  return START_PARAM_ROUTES[raw.trim().toLowerCase()] ?? null
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

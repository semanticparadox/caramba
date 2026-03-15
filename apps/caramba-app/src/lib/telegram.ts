import WebApp from '@twa-dev/sdk'

type HapticImpactStyle = 'light' | 'medium' | 'heavy' | 'rigid' | 'soft'
type HapticNotificationType = 'success' | 'warning' | 'error'

export const getTelegramLanguage = (): string => {
  const raw = WebApp.initDataUnsafe?.user?.language_code || 'ru'
  const normalized = raw.toLowerCase()
  if (normalized.startsWith('ru')) return 'ru'
  return 'en'
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

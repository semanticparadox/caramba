import { triggerNotificationHaptic } from './telegram'

export const copyText = async (value: string): Promise<boolean> => {
  try {
    await navigator.clipboard.writeText(value)
    triggerNotificationHaptic('success')
    return true
  } catch {
    // Fallback for non-HTTPS or denied clipboard permission
    try {
      const textarea = document.createElement('textarea')
      textarea.value = value
      textarea.style.position = 'fixed'
      textarea.style.opacity = '0'
      document.body.appendChild(textarea)
      textarea.select()
      const ok = document.execCommand('copy')
      document.body.removeChild(textarea)
      if (ok) triggerNotificationHaptic('success')
      return ok
    } catch {
      return false
    }
  }
}

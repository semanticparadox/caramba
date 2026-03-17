import { triggerNotificationHaptic } from './telegram'

export const copyText = async (value: string) => {
  try {
    await navigator.clipboard.writeText(value)
    triggerNotificationHaptic('success')
  } catch {
    // Fallback for non-HTTPS or denied clipboard permission
    const textarea = document.createElement('textarea')
    textarea.value = value
    textarea.style.position = 'fixed'
    textarea.style.opacity = '0'
    document.body.appendChild(textarea)
    textarea.select()
    document.execCommand('copy')
    document.body.removeChild(textarea)
    triggerNotificationHaptic('success')
  }
}

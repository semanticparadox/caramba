import { triggerNotificationHaptic } from './telegram'

export const copyText = async (value: string) => {
  await navigator.clipboard.writeText(value)
  triggerNotificationHaptic('success')
}

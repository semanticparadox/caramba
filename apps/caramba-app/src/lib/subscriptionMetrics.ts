import type { UserStats, UserSubscription } from '../context/AuthContext'

export type UsageSnapshot = {
  usedBytes: number
  limitBytes: number
  usedGbText: string
  limitLabel: string
  percent: number
  daysLeft: number | null
  activeSubscriptions: UserSubscription[]
}

const BYTES_IN_GB = 1024 * 1024 * 1024

export const formatBytes = (bytes: number, decimals = 2): string => {
  if (!bytes || bytes <= 0) return '0 B'
  const k = 1024
  const dm = decimals < 0 ? 0 : decimals
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + sizes[i]
}

export const getUsageSnapshot = (
  stats: UserStats | null,
  subscriptions: UserSubscription[],
): UsageSnapshot => {
  const activeSubscriptions = subscriptions.filter((sub) => sub.status === 'active')

  if (activeSubscriptions.length > 0) {
    const usedBytes = activeSubscriptions.reduce(
      (acc, sub) => acc + (sub.used_traffic_bytes || 0),
      0,
    )
    const hasFiniteLimits = activeSubscriptions.some((sub) => (sub.traffic_limit_gb || 0) > 0)
    const limitBytes = hasFiniteLimits
      ? activeSubscriptions.reduce(
          (acc, sub) =>
            acc + Math.max(0, sub.traffic_limit_gb || 0) * BYTES_IN_GB,
          0,
        )
      : 0

    const percent = limitBytes > 0 ? Math.min(100, Math.round((usedBytes / limitBytes) * 100)) : 0
    const daysLeft = Math.min(...activeSubscriptions.map((sub) => Math.max(0, sub.days_left || 0)))

    return {
      usedBytes,
      limitBytes,
      usedGbText: (usedBytes / BYTES_IN_GB).toFixed(2),
      limitLabel: limitBytes > 0 ? formatBytes(limitBytes) : '∞',
      percent,
      daysLeft,
      activeSubscriptions,
    }
  }

  const usedBytes = stats?.traffic_used || 0
  const limitBytes = stats?.traffic_limit || 0
  const percent = limitBytes > 0 ? Math.min(100, Math.round((usedBytes / limitBytes) * 100)) : 0

  return {
    usedBytes,
    limitBytes,
    usedGbText: (usedBytes / BYTES_IN_GB).toFixed(2),
    limitLabel: limitBytes > 0 ? formatBytes(limitBytes) : '∞',
    percent,
    daysLeft: stats?.days_left ?? null,
    activeSubscriptions,
  }
}

export const usageProgress = (sub: UserSubscription) => {
  if ((sub.traffic_limit_gb || 0) <= 0) return 0
  const limitBytes = sub.traffic_limit_gb * BYTES_IN_GB
  return Math.min(100, Math.round(((sub.used_traffic_bytes || 0) / limitBytes) * 100))
}

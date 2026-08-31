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

/**
 * The traffic ceiling the SERVER actually enforces for one subscription.
 *
 * `traffic_limit_bytes` is the authoritative figure: it is the plan allowance
 * PLUS the user's bonus traffic (one-off grants from registration, promo codes
 * of type "traffic" and referrals), which is what the panel's quota gates
 * compare against before expiring or throttling. `traffic_limit_gb` alone
 * cannot express a bonus measured in megabytes, so showing it would tell the
 * user a smaller number than the one that governs their access.
 *
 * `null`/absent limit means unlimited; `traffic_limit_gb` is the fallback for
 * an older panel that does not send the byte figure yet.
 */
export const subscriptionLimitBytes = (sub: UserSubscription): number => {
  if (sub.traffic_limit_bytes === null) return 0
  if (typeof sub.traffic_limit_bytes === 'number' && sub.traffic_limit_bytes > 0) {
    return sub.traffic_limit_bytes
  }
  return Math.max(0, sub.traffic_limit_gb || 0) * BYTES_IN_GB
}

const BYTE_UNIT_KEYS = [
  'common.unitB',
  'common.unitKB',
  'common.unitMB',
  'common.unitGB',
  'common.unitTB',
]
const BYTE_UNIT_FALLBACK = ['B', 'KB', 'MB', 'GB', 'TB']

/**
 * `t` необязателен: у lib-слоя своего i18n нет, а вызывать formatBytes могут и
 * из мест без React-контекста. Без него остаются латинские сокращения.
 */
export const formatBytes = (
  bytes: number,
  t?: (key: string) => string,
  decimals = 2,
): string => {
  const unit = (i: number) => (t ? t(BYTE_UNIT_KEYS[i]) : BYTE_UNIT_FALLBACK[i])
  if (!bytes || bytes <= 0) return `0 ${unit(0)}`
  const k = 1024
  const dm = decimals < 0 ? 0 : decimals
  const i = Math.min(
    Math.floor(Math.log(bytes) / Math.log(k)),
    BYTE_UNIT_FALLBACK.length - 1,
  )
  return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + unit(i)
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
    const hasFiniteLimits = activeSubscriptions.some((sub) => subscriptionLimitBytes(sub) > 0)
    const limitBytes = hasFiniteLimits
      ? activeSubscriptions.reduce((acc, sub) => acc + subscriptionLimitBytes(sub), 0)
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
  const limitBytes = subscriptionLimitBytes(sub)
  if (limitBytes <= 0) return 0
  return Math.min(100, Math.round(((sub.used_traffic_bytes || 0) / limitBytes) * 100))
}

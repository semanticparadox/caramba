import type { UserSubscription } from '../../context/AuthContext'
import { subscriptionLimitBytes } from '../../lib/subscriptionMetrics'

/** Состояние главного экрана. Порядок проверки важен: истёкшая подписка
 *  важнее исчерпанного трафика, а «нет подписки» — когда нечего показывать. */
export type ConnectState = 'protected' | 'expired' | 'exhausted' | 'pending' | 'none'

export function pickPrimary(subs: UserSubscription[]): UserSubscription | null {
    if (subs.length === 0) return null
    const active = subs.filter((s) => s.status === 'active')
    if (active.length > 0) {
        // Платная подписка важнее бесплатной; при равенстве — та, что дольше живёт.
        return [...active].sort((a, b) => Number(!!a.is_free) - Number(!!b.is_free) || b.days_left - a.days_left)[0]
    }
    const pending = subs.find((s) => s.status === 'pending')
    if (pending) return pending
    return [...subs].sort((a, b) => Date.parse(b.expires_at) - Date.parse(a.expires_at))[0]
}

export function deriveState(sub: UserSubscription | null): ConnectState {
    if (!sub) return 'none'
    if (sub.status === 'pending') return 'pending'
    if (sub.status !== 'active' || sub.days_left <= 0) return 'expired'
    const limit = subscriptionLimitBytes(sub)
    if (limit > 0 && (sub.used_traffic_bytes || 0) >= limit) return 'exhausted'
    return 'protected'
}

/** Ссылка подписки с авто-релеем для РФ — та же логика, что была на старом
 *  главном экране, чтобы уже выданные ссылки не менялись. */
export function subscriptionUrl(sub: UserSubscription): string {
    const base = sub.subscription_url
    try {
        let relay = localStorage.getItem(`relay_${sub.id}`)
        if (!relay) {
            const tz = Intl.DateTimeFormat().resolvedOptions().timeZone
            const ru = [
                'Europe/Moscow', 'Europe/Samara', 'Europe/Kaliningrad', 'Asia/Yekaterinburg', 'Asia/Omsk',
                'Asia/Novosibirsk', 'Asia/Krasnoyarsk', 'Asia/Irkutsk', 'Asia/Yakutsk', 'Asia/Vladivostok',
                'Asia/Magadan', 'Asia/Kamchatka',
            ]
            relay = ru.includes(tz) ? 'RU' : 'none'
            localStorage.setItem(`relay_${sub.id}`, relay)
        }
        if (relay && relay !== 'auto' && relay !== 'none') {
            const sep = base.includes('?') ? '&' : '?'
            return `${base}${sep}relay_country=${relay}`
        }
    } catch {
        /* localStorage недоступен — отдаём базовую ссылку */
    }
    return base
}

const GB = 1024 ** 3

export function bytesToGb(bytes: number): number {
    return bytes / GB
}

/** «38» / «38,2» — крупная цифра без лишних знаков после запятой. */
export function formatGb(bytes: number, locale: string): string {
    const gb = bytesToGb(bytes)
    const digits = gb >= 100 ? 0 : gb >= 10 ? 1 : 2
    return new Intl.NumberFormat(locale, { maximumFractionDigits: digits }).format(gb)
}

export function formatDate(iso: string, locale: string, withYear = false): string {
    const d = new Date(iso)
    if (Number.isNaN(d.getTime())) return ''
    return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'long', ...(withYear ? { year: 'numeric' } : {}) }).format(d)
}

export function formatDateTime(ts: number | string, locale: string): string {
    const d = typeof ts === 'number' ? new Date(ts < 1e12 ? ts * 1000 : ts) : new Date(ts)
    if (Number.isNaN(d.getTime())) return ''
    return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'long', year: 'numeric' }).format(d)
}

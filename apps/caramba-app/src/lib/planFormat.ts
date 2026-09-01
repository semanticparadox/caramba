import { useTranslation } from 'react-i18next'

/** Общие типы каталога тарифов (ответ /api/client/plans). */
export interface PlanDuration {
    id: number
    duration_days: number
    price: number
    price_cents: number
}

export interface Plan {
    id: number
    name: string
    description: string | null
    traffic_limit_gb: number
    device_limit: number
    durations: PlanDuration[]
    is_free?: boolean
    daily_traffic_mb?: number
    /** Сколько живых серверов даёт план. Считается на сервере по группам узлов. */
    server_count?: number
    /** Коды стран этих серверов — для флагов на карточке. */
    countries?: string[]
}

/** Двухбуквенный код страны → флаг.
 *
 *  Regional Indicator Symbols: 'DE' → 🇩🇪. Без таблицы соответствий и без
 *  картинок — работает для любой страны, которая когда-либо появится у узла,
 *  и не требует правок кода при добавлении новой локации. Мусор на входе
 *  отдаётся обратно как есть, чтобы на карточке было видно проблему в данных,
 *  а не пустое место. */
export function countryFlag(code: string): string {
    const cc = code.trim().toUpperCase()
    if (!/^[A-Z]{2}$/.test(cc)) return code
    return String.fromCodePoint(...[...cc].map((c) => 0x1f1e6 + c.charCodeAt(0) - 65))
}

export interface PaymentProvider {
    id: string
    label: string
    amount?: number
    currency?: string
}

/** Цена из центов → "$X.XX". Раньше дублировалась в Home/Plans/Subscription. */
export function formatPrice(priceCents: number): string {
    const major = Math.floor(priceCents / 100)
    const minor = priceCents % 100
    return `$${major}.${minor.toString().padStart(2, '0')}`
}

/**
 * Хук форматирования длительности через i18n-ключи.
 * Раньше формула жила копиями в Home.tsx, Plans.tsx и Subscription.tsx.
 */
export function useDurationFormatter() {
    const { t } = useTranslation()
    return (days: number): string => {
        if (days === 0) return t('home.trafficOnly')
        if (days === 30) return t('home.month1')
        if (days === 60) return t('home.months2')
        if (days === 90) return t('home.months3')
        if (days === 180) return t('home.months6')
        if (days === 365) return t('home.year1')
        return t('home.days', { count: days })
    }
}

/** Нормализация сырого ответа /api/client/plans в типизированный каталог. */
export function normalizePlans(data: unknown, fallbackName: string): Plan[] {
    if (!Array.isArray(data)) return []
    return data.map((plan: any) => ({
        id: Number(plan?.id || 0),
        name: String(plan?.name || fallbackName),
        description: typeof plan?.description === 'string' ? plan.description : null,
        traffic_limit_gb: Number(plan?.traffic_limit_gb || 0),
        device_limit: Number(plan?.device_limit || 0),
        is_free: Boolean(plan?.is_free),
        daily_traffic_mb: plan?.daily_traffic_mb != null ? Number(plan.daily_traffic_mb) : undefined,
        durations: Array.isArray(plan?.durations)
            ? plan.durations
                .map((dur: any) => ({
                    id: Number(dur?.id || 0),
                    duration_days: Number(dur?.duration_days || 0),
                    price: Number(dur?.price || 0),
                    price_cents: Number(dur?.price_cents ?? dur?.price ?? 0),
                }))
                .filter((dur: PlanDuration) => dur.id > 0)
            : [],
    }))
}

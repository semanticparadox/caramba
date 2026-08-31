import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiUrl } from '../config'
import { normalizePlans, type PaymentProvider, type Plan } from './planFormat'

type UsePlansCatalogOptions = {
    token: string | null
}

/**
 * Загрузка каталога (тарифы + способы оплаты) — единый источник правды.
 * Раньше та же пара запросов дублировалась в Home.tsx и Plans.tsx.
 */
export function usePlansCatalog({ token }: UsePlansCatalogOptions) {
    const { t } = useTranslation()
    const [plans, setPlans] = useState<Plan[]>([])
    const [providers, setProviders] = useState<PaymentProvider[]>([])
    const [loading, setLoading] = useState(false)
    const [loaded, setLoaded] = useState(false)
    const [error, setError] = useState<string | null>(null)

    useEffect(() => {
        if (!token || loaded) return

        let cancelled = false
        const controller = new AbortController()
        const timeout = setTimeout(() => controller.abort(), 12000)

        const loadCatalog = async () => {
            setLoading(true)
            setError(null)
            try {
                const headers = { Authorization: `Bearer ${token}` }
                const [plansRes, providersRes] = await Promise.all([
                    fetch(apiUrl('/api/client/plans'), { headers, signal: controller.signal }),
                    fetch(apiUrl('/api/client/payment/providers'), { headers, signal: controller.signal }),
                ])
                if (cancelled) return

                if (plansRes.ok) {
                    setPlans(normalizePlans(await plansRes.json(), t('home.noName')))
                } else {
                    setPlans([])
                }

                if (providersRes.ok) {
                    const providersData = await providersRes.json()
                    setProviders(Array.isArray(providersData?.providers) ? providersData.providers : [])
                } else {
                    setProviders([])
                }

                setLoaded(true)
            } catch (e: any) {
                if (!cancelled && e?.name !== 'AbortError') {
                    setError(e?.message || t('home.catalogError'))
                }
            } finally {
                clearTimeout(timeout)
                if (!cancelled) setLoading(false)
            }
        }

        void loadCatalog()

        return () => {
            cancelled = true
            clearTimeout(timeout)
            controller.abort()
        }
    }, [token, loaded, t])

    /** Сбросить и перезагрузить каталог (кнопка «Повторить»). */
    const retry = () => {
        setLoaded(false)
        setLoading(false)
    }

    return { plans, providers, loading, loaded, error, retry }
}

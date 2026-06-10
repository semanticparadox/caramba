import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { apiUrl } from '../config'
import { useAuth } from '../context/AuthContext'
import DrawerModal from '../components/DrawerModal'
import ProviderPicker from '../components/ProviderPicker'
import { mapProviderCards } from '../lib/paymentProviders'
import { usePurchase } from '../lib/usePurchase'
import './Plans.css'

interface PlanDuration {
    id: number
    duration_days: number
    price: number
    price_cents: number
}

interface Plan {
    id: number
    name: string
    description: string | null
    traffic_limit_gb: number
    device_limit: number
    durations: PlanDuration[]
}

interface PaymentProvider {
    id: string
    label: string
    amount?: number
    currency?: string
}

export default function Plans() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { token, refreshData, user, error } = useAuth()

    useEffect(() => {
        const handleFocus = () => {
            refreshData()
        }
        window.addEventListener('focus', handleFocus)
        return () => window.removeEventListener('focus', handleFocus)
    }, [refreshData])

    const [plans, setPlans] = useState<Plan[]>([])
    const [providers, setProviders] = useState<PaymentProvider[]>([])
    const [loading, setLoading] = useState(true)
    const [selectedDuration, setSelectedDuration] = useState<PlanDuration | null>(null)
    const [showPayModal, setShowPayModal] = useState(false)
    const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null)

    const providerCards = mapProviderCards(providers, t('home.defaultProviderDesc'))

    const headers = { Authorization: `Bearer ${token}` }

    // Централизованная логика покупки — общая с Home (lib/usePurchase).
    const { purchasing, purchasingProvider, purchase } = usePurchase({ token, onRefresh: refreshData })

    useEffect(() => {
        if (!token) {
            setLoading(false)
            setMessage({
                type: 'error',
                text: error || t('home.authError'),
            })
            return
        }

        let cancelled = false
        const controller = new AbortController()
        const timeout = setTimeout(() => controller.abort(), 12000)

        void (async () => {
            try {
                const plansRes = await fetch(apiUrl('/api/client/plans'), {
                    headers,
                    signal: controller.signal,
                })
                if (cancelled) return
                if (plansRes.ok) {
                    const data = await plansRes.json()
                    setPlans(Array.isArray(data) ? data : [])
                }

                const providersRes = await fetch(apiUrl('/api/client/payment/providers'), {
                    headers,
                    signal: controller.signal,
                })
                if (cancelled) return
                if (providersRes.ok) {
                    const data = await providersRes.json()
                    setProviders(data.providers || [])
                }

                setMessage(null)
            } catch (e: any) {
                if (cancelled) return
                console.error(e)
                setMessage({
                    type: 'error',
                    text: e?.name === 'AbortError' ? t('home.catalogError') : (e?.message || t('home.catalogError')),
                })
            } finally {
                clearTimeout(timeout)
                if (!cancelled) setLoading(false)
            }
        })()

        return () => {
            cancelled = true
            clearTimeout(timeout)
            controller.abort()
        }
    }, [token, error])

    const handleSelectDuration = (duration: PlanDuration) => {
        setSelectedDuration(duration)
        setShowPayModal(true)
        // Re-fetch providers scoped to this duration so each method advertises its
        // effective per-method price/currency (override-aware).
        if (token) {
            void (async () => {
                try {
                    const res = await fetch(
                        apiUrl(`/api/client/payment/providers?duration_id=${duration.id}`),
                        { headers },
                    )
                    if (res.ok) {
                        const data = await res.json()
                        setProviders(data.providers || [])
                    }
                } catch (e) {
                    console.error('provider price fetch failed', e)
                }
            })()
        }
    }

    const handlePurchase = async (providerId: string) => {
        if (!selectedDuration) return

        const durationId = selectedDuration.id
        setMessage(null)

        const result = await purchase({ durationId, provider: providerId })

        switch (result.outcome) {
            case 'success':
                setMessage({ type: 'success', text: t(result.messageKey, result.messageParams) })
                setShowPayModal(false)
                setSelectedDuration(null)
                break
            case 'manual':
                setMessage({
                    type: 'success',
                    text: t('home.paymentCreated', { url: result.invoiceUrl }),
                })
                setShowPayModal(false)
                setSelectedDuration(null)
                break
            case 'error':
                setMessage({
                    type: 'error',
                    text: result.message || (result.messageKey ? t(result.messageKey) : t('home.invoiceError')),
                })
                // Оставляем модал открытым, чтобы пользователь мог выбрать другой способ.
                break
            case 'redirect':
                // UI передан Stars SDK / внешнему checkout — закрываем модал.
                setShowPayModal(false)
                setSelectedDuration(null)
                break
        }
    }

    const formatPrice = (priceCents: number) => {
        const major = Math.floor(priceCents / 100)
        const minor = priceCents % 100
        return `$${major}.${minor.toString().padStart(2, '0')}`
    }

    const formatDuration = (days: number) => {
        if (days === 0) return t('home.trafficOnly')
        if (days === 30) return t('home.month1')
        if (days === 60) return t('home.months2')
        if (days === 90) return t('home.months3')
        if (days === 180) return t('home.months6')
        if (days === 365) return t('home.year1')
        return t('home.days', { count: days })
    }

    if (loading) return <div className="page"><div className="loading">{t('home.loadingPlans')}</div></div>

    return (
        <div className="page plans-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')} aria-label={t('common.close')}>{'<'}</button>
                <h2>{t('home.purchaseTitle')}</h2>
            </header>

            <div className="balance-strip glass-card">
                <div>
                    <p className="strip-label">{t('home.balance')}</p>
                    <p className="strip-note">{t('home.renewSubtitle')}</p>
                </div>
                <span className="balance-val">${((user?.balance || 0)).toFixed(2)}</span>
            </div>

            {message && (
                <div className={`purchase-msg ${message.type}`} role="status" aria-live="polite">
                    {message.text}
                </div>
            )}

            {plans.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-icon">PL</div>
                    <h3>{t('home.noPlans')}</h3>
                    <p>{t('home.noPlansDesc')}</p>
                </div>
            ) : (
                <div className="plans-list">
                    {plans.map((plan) => (
                        <div key={plan.id} className="plan-card glass-card">
                            <div className="plan-header">
                                <div>
                                    <h3 className="plan-name">{plan.name}</h3>
                                    {plan.description && <p className="plan-desc">{plan.description}</p>}
                                </div>
                                <div className="plan-badges">
                                    <span className="plan-badge">{plan.traffic_limit_gb > 0 ? `${plan.traffic_limit_gb} GB` : t('home.unlimited')}</span>
                                    <span className="plan-badge">{plan.device_limit > 0 ? t('home.deviceLimit', { count: plan.device_limit }) : t('home.noDeviceLimit')}</span>
                                </div>
                            </div>

                            <div className="duration-grid">
                                {plan.durations.map((dur) => (
                                    <button
                                        key={dur.id}
                                        className={`duration-btn ${purchasing === dur.id ? 'purchasing' : ''}`}
                                        onClick={() => handleSelectDuration(dur)}
                                        disabled={purchasing !== null}
                                    >
                                        <span className="dur-label">
                                            {formatDuration(dur.duration_days)}
                                        </span>
                                        <span className="dur-price">{formatPrice(dur.price_cents)}</span>
                                        {purchasing === dur.id && <span className="dur-spinner">...</span>}
                                    </button>
                                ))}
                            </div>
                        </div>
                    ))}
                </div>
            )}

            <DrawerModal
                open={showPayModal}
                onClose={() => setShowPayModal(false)}
                title={t('home.selectPayment')}
                subtitle={selectedDuration ? `${formatDuration(selectedDuration.duration_days)} - ${formatPrice(selectedDuration.price_cents)}` : undefined}
                closeLabel={t('common.close')}
                footer={<button className="btn-ghost" onClick={() => setShowPayModal(false)}>{t('common.cancel')}</button>}
            >
                {providers.length === 0 ? (
                    <div className="empty-state drawer-empty">
                        <div className="empty-icon">PM</div>
                        <h3>{t('home.noProviders')}</h3>
                        <p>{t('home.noProvidersDesc')}</p>
                    </div>
                ) : (
                    <ProviderPicker
                        cards={providerCards}
                        onSelect={handlePurchase}
                        busyProviderId={purchasingProvider}
                        showPrice
                    />
                )}
            </DrawerModal>
        </div>
    )
}

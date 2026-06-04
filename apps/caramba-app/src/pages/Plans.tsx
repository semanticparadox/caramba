import { useState, useEffect } from 'react'
import WebApp from '@twa-dev/sdk'
import { useNavigate } from 'react-router-dom'
import { apiUrl } from '../config'
import { useAuth } from '../context/AuthContext'
import DrawerModal from '../components/DrawerModal'
import { mapProviderCards } from '../lib/paymentProviders'
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
    const [purchasing, setPurchasing] = useState<number | null>(null)
    const [selectedDuration, setSelectedDuration] = useState<PlanDuration | null>(null)
    const [showPayModal, setShowPayModal] = useState(false)
    const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null)

    const providerCards = mapProviderCards(providers)

    const headers = { Authorization: `Bearer ${token}` }

    useEffect(() => {
        if (!token) {
            setLoading(false)
            setMessage({
                type: 'error',
                text: error || 'Требуется авторизация. Откройте Mini App повторно из бота.',
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
                    text: e?.name === 'AbortError' ? 'Время загрузки истекло. Попробуйте снова.' : (e?.message || 'Не удалось загрузить тарифы.'),
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

        setPurchasing(selectedDuration.id)
        setMessage(null)
        setShowPayModal(false)

        try {
            const res = await fetch(apiUrl('/api/client/payment/invoice'), {
                method: 'POST',
                headers: { ...headers, 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    duration_id: selectedDuration.id,
                    provider: providerId,
                }),
            })

            if (res.ok) {
                const data = await res.json()
                if (data.invoice_url) {
                    if (providerId === 'manual') {
                        setMessage({ type: 'success', text: `Платеж создан. Загрузите чек сюда: ${data.invoice_url}` })
                        setPurchasing(null)
                        setSelectedDuration(null)
                    } else if (providerId === 'telegram_stars' || providerId === 'stars' || data.invoice_url.includes('t.me/invoice')) {
                        WebApp.openInvoice(data.invoice_url, (status) => {
                            if (status) {
                                void refreshData()
                            }
                            setPurchasing(null)
                            setSelectedDuration(null)
                        })
                        return
                    } else {
                        window.location.href = data.invoice_url
                        return
                    }
                }
                await refreshData()
            } else {
                const err = await res.text()
                setMessage({ type: 'error', text: err || 'Не удалось создать счет' })
            }
        } catch (e) {
            console.error('Purchase error:', e)
            setMessage({ type: 'error', text: 'Сетевая ошибка при создании счета.' })
        } finally {
            if (providerId === 'manual' || !selectedDuration) {
                setPurchasing(null)
                setSelectedDuration(null)
            }
        }
    }

    const formatPrice = (priceCents: number) => {
        const major = Math.floor(priceCents / 100)
        const minor = priceCents % 100
        return `$${major}.${minor.toString().padStart(2, '0')}`
    }

    const formatProviderPrice = (amount?: number, currency?: string): string | null => {
        if (amount == null || !currency) return null
        const major = amount / 100
        const c = currency.toUpperCase()
        if (c === 'USD') return `$${major.toFixed(2)}`
        if (c === 'RUB') return `${major.toFixed(2)} ₽`
        if (c === 'EUR') return `€${major.toFixed(2)}`
        return `${major.toFixed(2)} ${c}`
    }

    const formatDuration = (days: number) => {
        if (days === 0) return 'Только трафик'
        if (days === 30) return '1 месяц'
        if (days === 60) return '2 месяца'
        if (days === 90) return '3 месяца'
        if (days === 180) return '6 месяцев'
        if (days === 365) return '1 год'
        return `${days} дней`
    }

    if (loading) return <div className="page"><div className="loading">Загрузка тарифов...</div></div>

    return (
        <div className="page plans-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')}>{'<'}</button>
                <h2>Тарифы подписки</h2>
            </header>

            <div className="balance-strip glass-card">
                <div>
                    <p className="strip-label">Баланс кошелька</p>
                    <p className="strip-note">Доступен для продлений и апгрейдов</p>
                </div>
                <span className="balance-val">${((user?.balance || 0)).toFixed(2)}</span>
            </div>

            {message && (
                <div className={`purchase-msg ${message.type}`}>
                    {message.text}
                </div>
            )}

            {plans.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-icon">PL</div>
                    <h3>Тарифы пока недоступны</h3>
                    <p>Проверьте позже или обратитесь в поддержку.</p>
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
                                    <span className="plan-badge">{plan.traffic_limit_gb > 0 ? `${plan.traffic_limit_gb} GB` : 'Безлимит'}</span>
                                    <span className="plan-badge">{plan.device_limit > 0 ? `${plan.device_limit} устройств` : 'Без лимита'}</span>
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
                title="Способ оплаты"
                subtitle={selectedDuration ? `${formatDuration(selectedDuration.duration_days)} - ${formatPrice(selectedDuration.price_cents)}` : undefined}
                footer={<button className="btn-ghost" onClick={() => setShowPayModal(false)}>Отмена</button>}
            >
                {providers.length === 0 ? (
                    <div className="empty-state drawer-empty">
                        <div className="empty-icon">PM</div>
                        <h3>Провайдеры оплаты недоступны</h3>
                        <p>Попробуйте позже или обратитесь в поддержку.</p>
                    </div>
                ) : (
                    <div className="provider-list provider-card-list">
                        {providerCards.map((p) => (
                            <button
                                key={p.id}
                                className={`provider-btn provider-card ${p.accent}`}
                                onClick={() => handlePurchase(p.id)}
                            >
                                <span className="provider-card-copy">
                                    <strong>{p.title}</strong>
                                    <small>{p.description}</small>
                                </span>
                                <span className="provider-card-meta">
                                    {(() => {
                                        const price = formatProviderPrice(p.amount, p.currency)
                                        return price ? <span className="provider-price">{price}</span> : null
                                    })()}
                                    {p.badge && <span className="provider-pill">{p.badge}</span>}
                                    <span className="provider-arrow">{'>'}</span>
                                </span>
                            </button>
                        ))}
                    </div>
                )}
            </DrawerModal>
        </div>
    )
}

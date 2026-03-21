import { useEffect, useMemo, useState } from 'react'
import WebApp from '@twa-dev/sdk'
import { useNavigate } from 'react-router-dom'
import { QRCodeSVG } from 'qrcode.react'
import DrawerModal from '../components/DrawerModal'
import { apiUrl } from '../config'
import { useAuth, UserSubscription } from '../context/AuthContext'
import { copyText } from '../lib/copyActions'
import { mapProviderCards } from '../lib/paymentProviders'
import { formatBytes, getUsageSnapshot, usageProgress } from '../lib/subscriptionMetrics'
import './Home.css'

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
    durations?: PlanDuration[]
}

interface PaymentProvider {
    id: string
    label: string
}

type CenterBanner = { type: 'success' | 'error'; text: string } | null

function formatPrice(priceCents: number): string {
    const major = Math.floor(priceCents / 100)
    const minor = priceCents % 100
    return `$${major}.${minor.toString().padStart(2, '0')}`
}

function formatDuration(days: number): string {
    if (days === 0) return 'Только трафик'
    if (days === 30) return '1 месяц'
    if (days === 60) return '2 месяца'
    if (days === 90) return '3 месяца'
    if (days === 180) return '6 месяцев'
    if (days === 365) return '1 год'
    return `${days} дней`
}

export default function Home() {
    const navigate = useNavigate()
    const { userStats: stats, isLoading, user, subscriptions, refreshData, token, error } = useAuth()


    const usage = getUsageSnapshot(stats, subscriptions)

    const activeSubscriptions = useMemo(
        () => subscriptions.filter((sub) => sub.status === 'active'),
        [subscriptions],
    )
    const pendingSubscriptions = useMemo(
        () => subscriptions.filter((sub) => sub.status === 'pending'),
        [subscriptions],
    )

    const hasActiveAccess = activeSubscriptions.length > 0
    const hasPending = pendingSubscriptions.length > 0

    const [copiedSubId, setCopiedSubId] = useState<number | null>(null)
    const [activatingSubId, setActivatingSubId] = useState<number | null>(null)
    const [plans, setPlans] = useState<Plan[]>([])
    const [providers, setProviders] = useState<PaymentProvider[]>([])
    const [catalogLoading, setCatalogLoading] = useState(false)
    const [catalogLoaded, setCatalogLoaded] = useState(false)
    const [showPayModal, setShowPayModal] = useState(false)
    const [selectedDuration, setSelectedDuration] = useState<PlanDuration | null>(null)
    const [purchasingDurationId, setPurchasingDurationId] = useState<number | null>(null)
    const [banner, setBanner] = useState<CenterBanner>(null)

    const [qrSubId, setQrSubId] = useState<number | null>(null)

    const providerCards = mapProviderCards(providers)

    const radius = 44
    const circumference = 2 * Math.PI * radius
    const strokeOffset = circumference - (usage.percent / 100) * circumference

    const focusPurchase = () => {
        const block = document.getElementById('center-purchase')
        if (!block) return
        block.scrollIntoView({ behavior: 'smooth', block: 'start' })
    }

    // Server selection is now done on the dedicated Servers page

    const getSubUrl = (sub: UserSubscription): string => {
        return sub.subscription_url
    }

    // Hiddify определяет тип конфига сам через User-Agent.
    // Передаём ЧИСТУЮ ссылку подписки без ?client= — иначе Hiddify отклоняет импорт.
    // nodeId safe to include — the subscription endpoint handles it.
    const openHiddify = (sub: UserSubscription) => {
        if (!sub.subscription_url) return
        const url = getSubUrl(sub)
        const deepLink = `hiddify://import/${encodeURIComponent(url)}`
        const w = window.open(deepLink, '_blank')
        if (!w) {
            void copyImportLink(sub)
            setBanner({ type: 'success', text: 'Ссылка скопирована — вставьте в Hiddify вручную (Новый профиль → Добавить из буфера).' })
        }
    }

    const openHapp = (sub: UserSubscription) => {
        if (!sub.subscription_url) return
        const url = getSubUrl(sub)
        const deepLink = `sing-box://import-remote-profile?url=${encodeURIComponent(url)}`
        const w = window.open(deepLink, '_blank')
        if (!w) {
            void copyImportLink(sub)
            setBanner({ type: 'success', text: 'Ссылка скопирована — вставьте в Happ вручную.' })
        }
    }

    const copyImportLink = async (sub: UserSubscription) => {
        if (!sub.subscription_url) return
        const url = getSubUrl(sub)
        await copyText(url)
        setCopiedSubId(sub.id)
        setTimeout(() => setCopiedSubId(null), 1600)
    }

    const handleActivate = async (subId: number) => {
        if (!token) return
        setActivatingSubId(subId)
        setBanner(null)

        try {
            const res = await fetch(`/api/client/subscription/${subId}/activate`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}` },
            })

            if (res.ok) {
                const data = await res.json()
                setBanner({
                    type: 'success',
                    text: data?.message || 'Подписка активирована. Можно подключаться в Hiddify.',
                })
                await refreshData()
            } else {
                const errText = await res.text()
                setBanner({ type: 'error', text: errText || 'Не удалось активировать подписку.' })
            }
        } catch {
            setBanner({ type: 'error', text: 'Сетевая ошибка при активации подписки.' })
        } finally {
            setActivatingSubId(null)
        }
    }

    useEffect(() => {
        if (!token || catalogLoaded) return

        let cancelled = false
        const controller = new AbortController()
        const timeout = setTimeout(() => controller.abort(), 12000)

        const loadCatalog = async () => {
            setCatalogLoading(true)
            try {
                const headers = { Authorization: `Bearer ${token}` }
                const [plansRes, providersRes] = await Promise.all([
                    fetch(apiUrl('/api/client/plans'), { headers, signal: controller.signal }),
                    fetch(apiUrl('/api/client/payment/providers'), { headers, signal: controller.signal }),
                ])
                if (cancelled) return

                let loadedPlans: Plan[] = []
                if (plansRes.ok) {
                    const plansData = await plansRes.json()
                    loadedPlans = Array.isArray(plansData)
                        ? plansData.map((plan: any) => ({
                            id: Number(plan?.id || 0),
                            name: String(plan?.name || 'Без названия'),
                            description: typeof plan?.description === 'string' ? plan.description : null,
                            traffic_limit_gb: Number(plan?.traffic_limit_gb || 0),
                            device_limit: Number(plan?.device_limit || 0),
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
                        : []
                    setPlans(loadedPlans)
                } else {
                    setPlans([])
                }

                if (providersRes.ok) {
                    const providersData = await providersRes.json()
                    setProviders(Array.isArray(providersData?.providers) ? providersData.providers : [])
                } else {
                    setProviders([])
                }

                setCatalogLoaded(true)
            } catch (e: any) {
                if (!cancelled && e?.name !== 'AbortError') {
                    setBanner({ type: 'error', text: e?.message || 'Не удалось загрузить тарифы.' })
                }
            } finally {
                clearTimeout(timeout)
                if (!cancelled) setCatalogLoading(false)
            }
        }

        void loadCatalog()

        return () => {
            cancelled = true
            clearTimeout(timeout)
            controller.abort()
        }
    }, [token, catalogLoaded])

    const handleSelectDuration = (duration: PlanDuration) => {
        setSelectedDuration(duration)
        setShowPayModal(true)
    }

    const handlePurchase = async (providerId: string) => {
        if (!selectedDuration || !token) return

        const pickedDuration = selectedDuration
        setPurchasingDurationId(pickedDuration.id)
        setBanner(null)
        setShowPayModal(false)

        try {
            const res = await fetch(apiUrl('/api/client/payment/invoice'), {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    duration_id: pickedDuration.id,
                    provider: providerId,
                }),
            })

            if (res.ok) {
                const data = await res.json()
                if (!data.invoice_url) {
                    setBanner({ type: 'success', text: 'Оплата прошла успешно. Подписка создана.' })
                    await refreshData()
                } else if (data.invoice_url) {
                    if (providerId === 'manual') {
                        setBanner({
                            type: 'success',
                            text: `Платеж создан. Загрузите чек: ${data.invoice_url}`,
                        })
                    } else if (providerId === 'telegram_stars' || providerId === 'stars' || data.invoice_url.includes('t.me/invoice')) {
                        WebApp.openInvoice(data.invoice_url, () => {
                            void refreshData()
                        })
                        return
                    } else {
                        window.location.href = data.invoice_url
                        return
                    }
                }
                await refreshData()
            } else {
                const errText = await res.text()
                setBanner({ type: 'error', text: errText || 'Не удалось создать счет.' })
            }
        } catch {
            setBanner({ type: 'error', text: 'Сетевая ошибка при создании счета.' })
        } finally {
            setPurchasingDurationId(null)
            setSelectedDuration(null)
        }
    }

    const totalDownload = stats?.total_download || 0
    const totalUpload = stats?.total_upload || 0

    return (
        <div className="page home-page">
            <section className="compact-hero glass-card">
                <span className={`state-dot ${hasActiveAccess ? 'is-online' : hasPending ? 'is-waiting' : 'is-idle'}`} />
                <div className="compact-hero-copy">
                    <p className="hero-kicker">Добро пожаловать</p>
                    <h1>{user?.full_name || user?.username || 'EXA ROBOT'}</h1>
                </div>
            </section>

            {banner && (
                <div className={`home-banner ${banner.type}`}>
                    {banner.text}
                </div>
            )}

            {!token && (
                <div className="home-banner error">
                    {error || 'Требуется авторизация. Откройте Mini App из бота заново.'}
                </div>
            )}

            {activeSubscriptions.map((sub) => {
                return (
                    <section key={sub.id} className="sub-card glass-card">
                        <div className="sub-card-head">
                            <div className="sub-card-info">
                                <span className="sub-card-name">{sub.plan_name}</span>
                                <span className="sub-card-days">{sub.days_left > 0 ? `${sub.days_left} дн. осталось` : 'Скоро истечет'}</span>
                            </div>
                            <span className="sub-card-traffic">
                                {sub.used_traffic_gb} / {sub.traffic_limit_gb > 0 ? `${sub.traffic_limit_gb} GB` : '∞'}
                            </span>
                        </div>
                        {sub.traffic_limit_gb > 0 && (
                            <div className="progress-bar-mini">
                                <div className="progress-fill-mini" style={{ width: `${usageProgress(sub)}%` }} />
                            </div>
                        )}
                        <div className="sub-card-stats">
                            <span>↓ {formatBytes(totalDownload)}</span>
                            <span>↑ {formatBytes(totalUpload)}</span>
                        </div>

                        <div className="sub-card-connect">
                            <button className="btn-primary" onClick={() => openHiddify(sub)}>
                                Подключить в Hiddify
                            </button>
                            <button className="btn-primary btn-secondary" onClick={() => openHapp(sub)}>
                                Подключить в Happ
                            </button>
                        </div>
                        <div className="sub-card-actions">
                            <button className="btn-ghost" onClick={() => setQrSubId(qrSubId === sub.id ? null : sub.id)}>
                                Показать QR
                            </button>
                            <button className="btn-ghost" onClick={() => void copyImportLink(sub)}>
                                {copiedSubId === sub.id ? 'Скопировано' : 'Скопировать ссылку'}
                            </button>
                        </div>
                        <div className="sub-card-actions">
                            <button className="btn-ghost" onClick={() => navigate(`/servers/${sub.id}`)}>
                                Выбрать сервер
                            </button>
                            <button className="btn-ghost" onClick={() => navigate(`/devices?sub=${sub.id}`)}>
                                Устройства {sub.active_devices ?? 0}/{(sub.device_limit ?? 0) > 0 ? sub.device_limit : '∞'}
                            </button>
                        </div>

                        {qrSubId === sub.id && (
                            <div className="sub-qr-panel">
                                <div className="sub-qr-wrap">
                                    <QRCodeSVG value={getSubUrl(sub)} size={180} bgColor="#ffffff" fgColor="#0D0D1A" level="M" includeMargin />
                                </div>
                                <div className="sub-qr-url">
                                    <input type="text" readOnly value={getSubUrl(sub)} onClick={e => e.currentTarget.select()} />
                                </div>
                            </div>
                        )}
                    </section>
                )
            })}

            {!hasActiveAccess && !hasPending && (
                <section className="no-sub-cta glass-card">
                    <p>У вас нет активных подписок</p>
                    <button className="btn-primary" onClick={focusPurchase}>
                        Выбрать и купить тариф
                    </button>
                    <button className="btn-ghost" onClick={() => navigate('/promo')}>
                        Промокод / реферал
                    </button>
                </section>
            )}

            {hasPending && (
                <section className="center-module glass-card">
                    <div className="panel-header">
                        <h3>Ожидают активации</h3>
                        <span>Запустите купленные подписки перед подключением</span>
                    </div>

                    <div className="pending-grid">
                        {pendingSubscriptions.map((sub) => (
                            <article key={sub.id} className="pending-row">
                                <div>
                                    <h4>{sub.plan_name}</h4>
                                    <p>{sub.duration_days > 0 ? `${sub.duration_days} дней` : 'Трафик-план без срока'}</p>
                                </div>
                                <button
                                    className="btn-primary"
                                    onClick={() => handleActivate(sub.id)}
                                    disabled={activatingSubId !== null}
                                >
                                    {activatingSubId === sub.id ? 'Активация...' : 'Активировать'}
                                </button>
                            </article>
                        ))}
                    </div>
                </section>
            )}

            <section id="center-purchase" className="center-module glass-card">
                <div className="panel-header">
                    <h3>{hasActiveAccess ? 'Продлить или купить ещё' : 'Покупка доступа'}</h3>
                    <span>{hasActiveAccess ? 'Добавьте новую подписку или продлите существующую' : 'Сначала купите подписку, затем подключитесь через Hiddify'}</span>
                </div>

                    <div className="balance-inline">
                        <span>Баланс</span>
                        <strong>${((user?.balance || stats?.balance || 0)).toFixed(2)}</strong>
                    </div>

                    {catalogLoading ? (
                        <div className="empty-state control-empty">
                            <div className="empty-icon">PL</div>
                            <h3>Загружаем тарифы</h3>
                            <p>Подождите, собираем доступные варианты оплаты.</p>
                        </div>
                    ) : plans.length === 0 ? (
                        <div className="empty-state control-empty">
                            <div className="empty-icon">PL</div>
                            <h3>Тарифы недоступны</h3>
                            <p>Попробуйте обновить данные или обратитесь в поддержку.</p>
                            <button className="btn-secondary" style={{ marginTop: 12 }} onClick={() => { setCatalogLoaded(false); setCatalogLoading(false) }}>
                                Повторить загрузку
                            </button>
                        </div>
                    ) : (
                        <div className="plans-grid">
                            {plans.map((plan) => (
                                <article key={plan.id} className="plan-card-home">
                                    <div className="plan-card-head">
                                        <h4>{plan.name}</h4>
                                        <p>{plan.description || 'Подписка для защищенного подключения.'}</p>
                                    </div>
                                    <div className="plan-card-meta">
                                        <span>{plan.traffic_limit_gb > 0 ? `${plan.traffic_limit_gb} GB` : 'Безлимит'}</span>
                                        <span>{plan.device_limit > 0 ? `${plan.device_limit} устройств` : 'Без лимита'}</span>
                                    </div>
                                    <div className="duration-grid">
                                        {(plan.durations || []).map((dur) => (
                                            <button
                                                key={dur.id}
                                                className="duration-btn"
                                                onClick={() => handleSelectDuration(dur)}
                                                disabled={purchasingDurationId !== null}
                                            >
                                                <span className="dur-label">{formatDuration(dur.duration_days)}</span>
                                                <span className="dur-price">{formatPrice(dur.price_cents)}</span>
                                                {purchasingDurationId === dur.id && <span className="dur-spinner">...</span>}
                                            </button>
                                        ))}
                                        {(plan.durations || []).length === 0 && (
                                            <div className="plan-empty-note">Варианты длительности пока не настроены для этого тарифа.</div>
                                        )}
                                    </div>
                                </article>
                            ))}
                        </div>
                    )}
                </section>

            <section className="home-bento-grid">
                <article className="bento-card bento-traffic glass-card">
                    <div className="bento-head">
                        <h3>Трафик</h3>
                        <span>{usage.percent}%</span>
                    </div>
                    <div className="traffic-ring-wrap">
                        <svg className="traffic-ring" viewBox="0 0 100 100">
                            <circle className="ring-bg" cx="50" cy="50" r={radius} />
                            <circle
                                className="ring-progress"
                                cx="50"
                                cy="50"
                                r={radius}
                                strokeDasharray={circumference}
                                strokeDashoffset={strokeOffset}
                            />
                        </svg>
                        <div className="ring-text-wrap">
                            <span className="ring-percent">{isLoading ? '...' : `${usage.percent}%`}</span>
                            <span className="ring-text">использовано</span>
                        </div>
                    </div>
                    <div className="traffic-values">
                        <span>{isLoading ? '...' : `${usage.usedGbText} GB`}</span>
                        <span>{isLoading ? '...' : usage.limitLabel}</span>
                    </div>
                </article>

                <article className="bento-card glass-card">
                    <div className="bento-head">
                        <h3>Срок доступа</h3>
                        <span>{activeSubscriptions.length} активных</span>
                    </div>
                    <div className="metric-grid">
                        <div>
                            <p>Дней осталось</p>
                            <strong>{isLoading ? '...' : (usage.daysLeft ?? 'Н/Д')}</strong>
                        </div>
                        <div>
                            <p>Подписок</p>
                            <strong>{subscriptions.length}</strong>
                        </div>
                    </div>
                </article>

                <article className="bento-card glass-card">
                    <div className="bento-head">
                        <h3>Передача</h3>
                        <span>За все время</span>
                    </div>
                    <div className="metric-grid">
                        <div>
                            <p>Входящий</p>
                            <strong>{formatBytes(totalDownload)}</strong>
                        </div>
                        <div>
                            <p>Исходящий</p>
                            <strong>{formatBytes(totalUpload)}</strong>
                        </div>
                    </div>
                </article>

            </section>

            <DrawerModal
                open={showPayModal}
                onClose={() => setShowPayModal(false)}
                title="Выберите способ оплаты"
                subtitle={selectedDuration ? `${formatDuration(selectedDuration.duration_days)} • ${formatPrice(selectedDuration.price_cents)}` : undefined}
                footer={<button className="btn-ghost" onClick={() => setShowPayModal(false)}>Отмена</button>}
            >
                {providers.length === 0 ? (
                    <div className="empty-state drawer-empty">
                        <div className="empty-icon">PM</div>
                        <h3>Способы оплаты недоступны</h3>
                        <p>Попробуйте позже или обратитесь в поддержку.</p>
                    </div>
                ) : (
                    <div className="provider-list provider-card-list">
                        {providerCards.map((provider) => (
                            <button
                                key={provider.id}
                                className={`provider-btn provider-card ${provider.accent}`}
                                onClick={() => handlePurchase(provider.id)}
                            >
                                <span className="provider-card-copy">
                                    <strong>{provider.title}</strong>
                                    <small>{provider.description}</small>
                                </span>
                                <span className="provider-card-meta">
                                    {provider.badge && <span className="provider-pill">{provider.badge}</span>}
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

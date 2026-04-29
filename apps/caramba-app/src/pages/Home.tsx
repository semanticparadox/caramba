import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
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
    is_free?: boolean
    daily_traffic_mb?: number
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

export default function Home() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { userStats: stats, isLoading, user, subscriptions, refreshData, token, error } = useAuth()

    // Форматирование длительности через i18n-ключи
    const formatDuration = (days: number): string => {
        if (days === 0) return t('home.trafficOnly')
        if (days === 30) return t('home.month1')
        if (days === 60) return t('home.months2')
        if (days === 90) return t('home.months3')
        if (days === 180) return t('home.months6')
        if (days === 365) return t('home.year1')
        return t('home.days', { count: days })
    }

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
    const [buyAsGift, setBuyAsGift] = useState(false)
    const [giftCode, setGiftCode] = useState<string | null>(null)
    const [copiedGiftCode, setCopiedGiftCode] = useState(false)

    const [qrSubId, setQrSubId] = useState<number | null>(null)

    // Состояние для продления подписки с главной страницы
    const [extendTargetSub, setExtendTargetSub] = useState<UserSubscription | null>(null)
    const [extendingDurationId, setExtendingDurationId] = useState<number | null>(null)

    const providerCards = mapProviderCards(providers)

    const radius = 44
    const circumference = 2 * Math.PI * radius
    const strokeOffset = circumference - (usage.percent / 100) * circumference

    // Цвет кольца трафика: зелёный → жёлтый (80%) → красный (90%+)
    const trafficRingColor =
        usage.percent >= 90 ? 'var(--color-danger, #e53935)'
        : usage.percent >= 80 ? 'var(--color-warning, #f57c00)'
        : 'var(--color-accent, #1976d2)'

    const focusPurchase = () => {
        const block = document.getElementById('center-purchase')
        if (!block) return
        block.scrollIntoView({ behavior: 'smooth', block: 'start' })
    }

    const getSubUrl = (sub: UserSubscription): string => {
        const base = sub.subscription_url
        try {
            let relay = localStorage.getItem(`relay_${sub.id}`)
            // Авто-определение relay по таймзоне для новых пользователей
            if (!relay) {
                const tz = Intl.DateTimeFormat().resolvedOptions().timeZone
                const russianTimezones = [
                    'Europe/Moscow', 'Europe/Samara', 'Europe/Kaliningrad',
                    'Asia/Yekaterinburg', 'Asia/Omsk', 'Asia/Novosibirsk',
                    'Asia/Krasnoyarsk', 'Asia/Irkutsk', 'Asia/Yakutsk',
                    'Asia/Vladivostok', 'Asia/Magadan', 'Asia/Kamchatka',
                ]
                relay = russianTimezones.includes(tz) ? 'RU' : 'none'
                localStorage.setItem(`relay_${sub.id}`, relay)
            }
            if (relay && relay !== 'auto') {
                const sep = base.includes('?') ? '&' : '?'
                return `${base}${sep}relay_country=${relay}`
            }
        } catch { /* ignore */ }
        return base
    }

    // Hiddify определяет тип конфига сам через User-Agent.
    // Передаём ЧИСТУЮ ссылку подписки без ?client= — иначе Hiddify отклоняет импорт.
    const openHiddify = (sub: UserSubscription) => {
        if (!sub.subscription_url) return
        const url = getSubUrl(sub)
        const deepLink = `hiddify://import/${encodeURIComponent(url)}`
        const w = window.open(deepLink, '_blank')
        if (!w) {
            void copyImportLink(sub)
            setBanner({ type: 'success', text: t('home.hiddifyManualCopy') })
        }
    }

    const openHapp = (sub: UserSubscription) => {
        if (!sub.subscription_url) return
        const url = getSubUrl(sub)
        const deepLink = `happ://import/${encodeURIComponent(url)}`
        const w = window.open(deepLink, '_blank')
        if (!w) {
            void copyImportLink(sub)
            setBanner({ type: 'success', text: t('home.happManualCopy') })
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
                    text: data?.message || t('home.subscriptionActivated'),
                })
                await refreshData()
            } else {
                const errText = await res.text()
                setBanner({ type: 'error', text: errText || t('home.activationError') })
            }
        } catch {
            setBanner({ type: 'error', text: t('home.networkActivationError') })
        } finally {
            setActivatingSubId(null)
        }
    }

    // Продление подписки: списывает баланс и сдвигает expires_at
    const handleExtend = async (durationId: number) => {
        if (!extendTargetSub || !token) return
        const subId = extendTargetSub.id
        setExtendingDurationId(durationId)
        setBanner(null)
        try {
            const res = await fetch(apiUrl(`/api/client/subscription/${subId}/extend`), {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ duration_id: durationId }),
            })

            if (res.ok) {
                const data = await res.json()
                const expiresAt = data?.expires_at
                    ? new Date(data.expires_at).toLocaleDateString()
                    : ''
                setBanner({
                    type: 'success',
                    text: t('subscription.extendSuccess', { date: expiresAt }),
                })
                setExtendTargetSub(null)
                await refreshData()
            } else {
                const errText = await res.text()
                const isBalance =
                    errText.toLowerCase().includes('balance') ||
                    errText.toLowerCase().includes('insufficient')
                setBanner({
                    type: 'error',
                    text: isBalance ? t('subscription.insufficientBalance') : errText,
                })
                setExtendTargetSub(null)
            }
        } catch {
            setBanner({ type: 'error', text: t('home.networkInvoiceError') })
            setExtendTargetSub(null)
        } finally {
            setExtendingDurationId(null)
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
                            name: String(plan?.name || t('home.noName')),
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
                    setBanner({ type: 'error', text: e?.message || t('home.catalogError') })
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
        setBuyAsGift(false)
        setGiftCode(null)
        setCopiedGiftCode(false)
        setShowPayModal(true)
    }

    const handlePurchase = async (providerId: string) => {
        if (!selectedDuration || !token) return

        const pickedDuration = selectedDuration
        const isGift = buyAsGift
        setPurchasingDurationId(pickedDuration.id)
        setBanner(null)
        setShowPayModal(false)

        try {
            // Покупка подарочного кода — всегда через balance (баланс списывается напрямую)
            if (isGift && providerId === 'balance') {
                const res = await fetch(apiUrl('/api/client/plans/purchase'), {
                    method: 'POST',
                    headers: {
                        Authorization: `Bearer ${token}`,
                        'Content-Type': 'application/json',
                    },
                    body: JSON.stringify({
                        duration_id: pickedDuration.id,
                        as_gift: true,
                    }),
                })

                if (res.ok) {
                    const data = await res.json()
                    if (data.type === 'gift' && data.gift_code) {
                        setGiftCode(data.gift_code)
                        setBanner({ type: 'success', text: t('home.giftCodeCreated') })
                    }
                    await refreshData()
                } else {
                    const errText = await res.text()
                    setBanner({ type: 'error', text: errText || t('home.invoiceError') })
                }
                return
            }

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
                    setBanner({ type: 'success', text: t('home.paymentSuccess') })
                    await refreshData()
                } else if (data.invoice_url) {
                    if (providerId === 'manual') {
                        setBanner({
                            type: 'success',
                            text: t('home.paymentCreated', { url: data.invoice_url }),
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
                setBanner({ type: 'error', text: errText || t('home.invoiceError') })
            }
        } catch {
            setBanner({ type: 'error', text: t('home.networkInvoiceError') })
        } finally {
            setPurchasingDurationId(null)
            setSelectedDuration(null)
        }
    }

    const handleCopyGiftCode = async () => {
        if (!giftCode) return
        await copyText(giftCode)
        setCopiedGiftCode(true)
        setTimeout(() => setCopiedGiftCode(false), 1600)
    }

    const totalDownload = stats?.total_download || 0
    const totalUpload = stats?.total_upload || 0

    // Имя пользователя — используем полное имя или username из Telegram, никогда технический плейсхолдер
    const displayName = user?.full_name || user?.username || t('common.appName')

    return (
        <div className="page home-page">
            <section className="compact-hero glass-card">
                <span className={`state-dot ${hasActiveAccess ? 'is-online' : hasPending ? 'is-waiting' : 'is-idle'}`} />
                <div className="compact-hero-copy">
                    <p className="hero-kicker">{t('home.welcome')}</p>
                    <h1>{displayName}</h1>
                </div>
            </section>

            {banner && (
                <div className={`home-banner ${banner.type}`}>
                    {banner.text}
                </div>
            )}

            {!token && (
                <div className="home-banner error">
                    {error || t('home.authError')}
                </div>
            )}

            {/* Ошибка загрузки данных при наличии токена — показываем с кнопкой повтора */}
            {token && error && !isLoading && (
                <div className="home-banner error home-banner-retry">
                    <span>{error}</span>
                    <button className="btn-secondary btn-retry-inline" onClick={() => void refreshData()}>
                        {t('home.retryLoad')}
                    </button>
                </div>
            )}

            {activeSubscriptions.map((sub) => {
                return (
                    <section key={sub.id} className="sub-card glass-card">
                        <div className="sub-card-head">
                            <div className="sub-card-info">
                                <span className="sub-card-name">{sub.plan_name}</span>
                                <span className="sub-card-days">
                                    {sub.is_free
                                        ? t('home.freePlanLabel')
                                        : sub.days_left > 0
                                        ? t('home.daysLeft', { count: sub.days_left })
                                        : t('home.expiringSoon')}
                                </span>
                            </div>
                            <span className="sub-card-traffic">
                                {sub.used_traffic_gb} / {sub.traffic_limit_gb > 0 ? `${sub.traffic_limit_gb} GB` : '∞'}
                            </span>
                        </div>
                        {sub.traffic_limit_gb > 0 && (() => {
                                const pct = usageProgress(sub)
                                const barColor = pct >= 100 ? 'var(--color-danger, #e53935)'
                                    : pct >= 90 ? 'var(--color-danger, #e53935)'
                                    : pct >= 80 ? 'var(--color-warning, #f57c00)'
                                    : 'var(--color-success, #43a047)'
                                return (
                                    <div className="progress-bar-mini">
                                        <div
                                            className="progress-fill-mini"
                                            style={{ width: `${pct}%`, background: barColor }}
                                        />
                                    </div>
                                )
                            })()}
                        {sub.is_free && (
                            <div className="free-plan-banner">
                                <span className="free-plan-topup-badge">
                                    +{sub.daily_traffic_mb ?? 50} {t('home.freePlanTopupLabel')}
                                </span>
                                <button
                                    className="btn-primary btn-upgrade-cta"
                                    onClick={focusPurchase}
                                >
                                    {t('home.upgradeToPremium')}
                                </button>
                            </div>
                        )}
                        <div className="sub-card-stats">
                            <span>↓ {formatBytes(totalDownload)}</span>
                            <span>↑ {formatBytes(totalUpload)}</span>
                        </div>

                        <div className="sub-card-connect">
                            <button className="btn-primary" onClick={() => openHiddify(sub)}>
                                {t('home.connectHiddify')}
                            </button>
                            <button className="btn-primary btn-secondary" onClick={() => openHapp(sub)}>
                                {t('home.connectHapp')}
                            </button>
                        </div>
                        <div className="sub-card-actions">
                            <button className="btn-ghost" onClick={() => setQrSubId(qrSubId === sub.id ? null : sub.id)}>
                                {t('home.showQr')}
                            </button>
                            <button className="btn-ghost" onClick={() => void copyImportLink(sub)}>
                                {copiedSubId === sub.id ? t('common.copied') : t('home.copyLink')}
                            </button>
                        </div>
                        <div className="sub-card-actions">
                            <button className="btn-ghost" onClick={() => navigate(`/servers/${sub.id}`)}>
                                {t('home.selectServer')}
                            </button>
                            <button className="btn-ghost" onClick={() => navigate(`/devices?sub=${sub.id}`)}>
                                {t('home.devices')}{' '}
                                {(() => {
                                    const used = sub.active_devices ?? 0
                                    const limit = (sub.device_limit ?? 0) > 0 ? sub.device_limit! : null
                                    const atLimit = limit != null && used >= limit
                                    return (
                                        <span style={atLimit ? { color: 'var(--color-warning, #f57c00)', fontWeight: 600 } : undefined}>
                                            {used}/{limit ?? '∞'}
                                        </span>
                                    )
                                })()}
                            </button>
                        </div>
                        {/* Кнопка продления — только для платных подписок со сроком */}
                        {!sub.is_free && sub.duration_days > 0 && (
                            <div className="sub-card-actions">
                                <button
                                    className="btn-ghost"
                                    onClick={() => setExtendTargetSub(sub)}
                                >
                                    {t('subscription.extend')}
                                </button>
                            </div>
                        )}

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

                        <div className="app-download-section">
                            <p className="app-download-label">{t('home.downloadApp')}</p>
                            <div className="app-download-grid">
                                <a href="https://play.google.com/store/apps/details?id=app.hiddify.com" target="_blank" rel="noopener" className="app-download-btn">
                                    <span className="app-icon">📱</span>
                                    <span>Hiddify Android</span>
                                </a>
                                <a href="https://apps.apple.com/ua/app/hiddify-proxy-vpn/id6596777532" target="_blank" rel="noopener" className="app-download-btn">
                                    <span className="app-icon">🍎</span>
                                    <span>Hiddify iOS</span>
                                </a>
                                <a href="https://github.com/coolcoala/koala-clash" target="_blank" rel="noopener" className="app-download-btn app-download-wide">
                                    <span className="app-icon">💻</span>
                                    <span>Koala Clash — Windows / macOS / Linux</span>
                                </a>
                            </div>
                        </div>
                    </section>
                )
            })}

            {!hasActiveAccess && !hasPending && (
                <section className="no-sub-cta glass-card">
                    <p>{t('home.noSubscription')}</p>
                    <button className="btn-primary" onClick={focusPurchase}>
                        {t('home.choosePlan')}
                    </button>
                    <button className="btn-ghost" onClick={() => navigate('/promo')}>
                        {t('home.promoCode')}
                    </button>
                </section>
            )}

            {hasPending && (
                <section className="center-module glass-card">
                    <div className="panel-header">
                        <h3>{t('home.pendingTitle')}</h3>
                        <span>{t('home.pendingSubtitle')}</span>
                    </div>

                    <div className="pending-grid">
                        {pendingSubscriptions.map((sub) => (
                            <article key={sub.id} className="pending-row">
                                <div>
                                    <h4>{sub.plan_name}</h4>
                                    <p>{sub.duration_days > 0
                                        ? t('home.durationDays', { count: sub.duration_days })
                                        : t('home.trafficOnlyPlan')}
                                    </p>
                                </div>
                                <button
                                    className="btn-primary"
                                    onClick={() => handleActivate(sub.id)}
                                    disabled={activatingSubId !== null}
                                >
                                    {activatingSubId === sub.id ? t('home.activating') : t('home.activate')}
                                </button>
                            </article>
                        ))}
                    </div>
                </section>
            )}

            <section id="center-purchase" className="center-module glass-card">
                <div className="panel-header">
                    <h3>{hasActiveAccess ? t('home.renewTitle') : t('home.purchaseTitle')}</h3>
                    <span>{hasActiveAccess ? t('home.renewSubtitle') : t('home.purchaseSubtitle')}</span>
                </div>

                    <div className="balance-inline">
                        <span>{t('home.balance')}</span>
                        <strong>${((user?.balance || stats?.balance || 0)).toFixed(2)}</strong>
                    </div>

                    {catalogLoading ? (
                        <div className="empty-state control-empty">
                            <div className="empty-icon">PL</div>
                            <h3>{t('home.loadingPlans')}</h3>
                            <p>{t('home.loadingPlansDesc')}</p>
                        </div>
                    ) : plans.length === 0 ? (
                        <div className="empty-state control-empty">
                            <div className="empty-icon">PL</div>
                            <h3>{t('home.noPlans')}</h3>
                            <p>{t('home.noPlansDesc')}</p>
                            <button className="btn-secondary" style={{ marginTop: 12 }} onClick={() => { setCatalogLoaded(false); setCatalogLoading(false) }}>
                                {t('home.retryLoad')}
                            </button>
                        </div>
                    ) : (
                        <div className="plans-grid">
                            {plans.map((plan) => (
                                <article key={plan.id} className="plan-card-home">
                                    <div className="plan-card-head">
                                        <h4>{plan.name}</h4>
                                        <p>{plan.description || t('home.defaultPlanDesc')}</p>
                                    </div>
                                    <div className="plan-card-meta">
                                        <span>{plan.traffic_limit_gb > 0 ? `${plan.traffic_limit_gb} GB` : t('home.unlimited')}</span>
                                        <span>{plan.device_limit > 0 ? t('home.deviceLimit', { count: plan.device_limit }) : t('home.noDeviceLimit')}</span>
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
                                            <div className="plan-empty-note">{t('home.noDurations')}</div>
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
                        <h3>{t('home.trafficTitle')}</h3>
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
                                stroke={trafficRingColor}
                            />
                        </svg>
                        <div className="ring-text-wrap">
                            <span className="ring-percent" style={{ color: trafficRingColor }}>
                                {isLoading ? '...' : `${usage.percent}%`}
                            </span>
                            <span className="ring-text">{t('home.usedPercent')}</span>
                        </div>
                    </div>
                    <div className="traffic-values">
                        <span>{isLoading ? '...' : `${usage.usedGbText} GB`}</span>
                        <span>{isLoading ? '...' : usage.limitLabel}</span>
                    </div>
                </article>

                <article className="bento-card glass-card">
                    <div className="bento-head">
                        <h3>{t('home.accessTitle')}</h3>
                        <span>{t('home.activeCount', { count: activeSubscriptions.length })}</span>
                    </div>
                    <div className="metric-grid">
                        <div>
                            <p>{t('home.daysLeftMetric')}</p>
                            <strong>{isLoading ? '...' : (usage.daysLeft ?? 'N/A')}</strong>
                        </div>
                        <div>
                            <p>{t('home.subscriptionsMetric')}</p>
                            <strong>{subscriptions.length}</strong>
                        </div>
                    </div>
                </article>

                <article className="bento-card glass-card">
                    <div className="bento-head">
                        <h3>{t('home.transferTitle')}</h3>
                        <span>{t('home.allTime')}</span>
                    </div>
                    <div className="metric-grid">
                        <div>
                            <p>{t('home.incoming')}</p>
                            <strong>{formatBytes(totalDownload)}</strong>
                        </div>
                        <div>
                            <p>{t('home.outgoing')}</p>
                            <strong>{formatBytes(totalUpload)}</strong>
                        </div>
                    </div>
                </article>

            </section>

            <DrawerModal
                open={showPayModal}
                onClose={() => { setShowPayModal(false); setBuyAsGift(false) }}
                title={t('home.selectPayment')}
                subtitle={selectedDuration ? `${formatDuration(selectedDuration.duration_days)} • ${formatPrice(selectedDuration.price_cents)}` : undefined}
                footer={<button className="btn-ghost" onClick={() => { setShowPayModal(false); setBuyAsGift(false) }}>{t('common.cancel')}</button>}
            >
                {/* Переключатель «Купить в подарок» */}
                <label className="gift-toggle-row">
                    <span className="gift-toggle-label">{t('home.buyAsGift')}</span>
                    <input
                        type="checkbox"
                        className="gift-toggle-checkbox"
                        checked={buyAsGift}
                        onChange={(e) => setBuyAsGift(e.target.checked)}
                    />
                </label>
                {buyAsGift && (
                    <p className="gift-toggle-hint">{t('home.buyAsGiftHint')}</p>
                )}

                {providers.length === 0 ? (
                    <div className="empty-state drawer-empty">
                        <div className="empty-icon">PM</div>
                        <h3>{t('home.noProviders')}</h3>
                        <p>{t('home.noProvidersDesc')}</p>
                    </div>
                ) : (
                    <div className="provider-list provider-card-list">
                        {providerCards
                            // При покупке как подарок доступна только оплата балансом
                            .filter((p) => !buyAsGift || p.id === 'balance')
                            .map((provider) => (
                            <button
                                key={provider.id}
                                className={`provider-btn provider-card ${provider.accent}`}
                                onClick={() => handlePurchase(provider.id)}
                            >
                                <span className="provider-card-copy">
                                    <strong>{provider.title}</strong>
                                    <small>{buyAsGift ? t('home.giftBalanceNote') : provider.description}</small>
                                </span>
                                <span className="provider-card-meta">
                                    {provider.badge && <span className="provider-pill">{provider.badge}</span>}
                                    <span className="provider-arrow">{'>'}</span>
                                </span>
                            </button>
                        ))}
                        {buyAsGift && !providerCards.some((p) => p.id === 'balance') && (
                            <p className="gift-toggle-hint">{t('home.giftNeedsBalance')}</p>
                        )}
                    </div>
                )}
            </DrawerModal>

            {/* Показываем подарочный код после покупки */}
            {giftCode && (
                <section className="gift-code-result glass-card">
                    <h3>{t('home.giftCodeTitle')}</h3>
                    <p>{t('home.giftCodeInstructions')}</p>
                    <div className="gift-code-box">
                        <code className="gift-code-text">{giftCode}</code>
                        <button className="btn-primary" onClick={() => void handleCopyGiftCode()}>
                            {copiedGiftCode ? t('common.copied') : t('common.copy')}
                        </button>
                    </div>
                    <button className="btn-ghost" onClick={() => setGiftCode(null)}>
                        {t('common.close')}
                    </button>
                </section>
            )}

            {/* Модал выбора длительности продления (открывается с карточки активной подписки) */}
            <DrawerModal
                open={extendTargetSub !== null}
                onClose={() => setExtendTargetSub(null)}
                title={t('subscription.selectDuration')}
                subtitle={extendTargetSub?.plan_name}
                footer={
                    <button className="btn-ghost" onClick={() => setExtendTargetSub(null)}>
                        {t('common.cancel')}
                    </button>
                }
            >
                {(() => {
                    const durations =
                        extendTargetSub
                            ? (plans.find((p) => p.id === extendTargetSub.plan_id)?.durations ?? [])
                            : []
                    if (durations.length === 0) {
                        return (
                            <div className="empty-state drawer-empty">
                                <div className="empty-icon">PL</div>
                                <p>{t('home.noDurations')}</p>
                            </div>
                        )
                    }
                    return (
                        <div className="duration-grid">
                            {durations.map((dur) => (
                                <button
                                    key={dur.id}
                                    className="duration-btn"
                                    onClick={() => void handleExtend(dur.id)}
                                    disabled={extendingDurationId !== null}
                                >
                                    <span className="dur-label">{formatDuration(dur.duration_days)}</span>
                                    <span className="dur-price">{formatPrice(dur.price_cents)}</span>
                                    {extendingDurationId === dur.id && (
                                        <span className="dur-spinner">...</span>
                                    )}
                                </button>
                            ))}
                        </div>
                    )
                })()}
            </DrawerModal>
        </div>
    )
}

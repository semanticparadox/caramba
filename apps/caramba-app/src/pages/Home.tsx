import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { QRCodeSVG } from 'qrcode.react'
import { useAuth, UserSubscription } from '../context/AuthContext'
import { useNotifications } from '../context/NotificationContext'
import { copyText } from '../lib/copyActions'
import { hapticError, hapticSuccess, hapticTap } from '../lib/haptics'
import { formatBytes, getUsageSnapshot } from '../lib/subscriptionMetrics'
import './Home.css'

type HomeBanner = { type: 'success' | 'error'; text: string } | null

/**
 * Главная — игровой дашборд: статус подключения (герой + один CTA),
 * кольцо трафика, дни доступа, быстрые действия.
 * Каталог тарифов живёт на /plans (единый PurchaseFlow), продление — на
 * /subscription: одна задача — одна точка входа.
 */
export default function Home() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { userStats: stats, isLoading, user, subscriptions, refreshData, token, error } = useAuth()
    const { unreadCount } = useNotifications()

    const usage = getUsageSnapshot(stats, subscriptions)

    // 'throttled' — суточная квота бесплатного плана исчерпана (пополнится
    // завтра): подписка должна остаться на главном экране, а не исчезать,
    // будто её нет.
    const activeSubscriptions = useMemo(
        () => subscriptions.filter((sub) => sub.status === 'active' || sub.status === 'throttled'),
        [subscriptions],
    )
    const pendingSubscriptions = useMemo(
        () => subscriptions.filter((sub) => sub.status === 'pending'),
        [subscriptions],
    )

    const hasActiveAccess = activeSubscriptions.length > 0
    const hasPending = pendingSubscriptions.length > 0
    const primarySub = activeSubscriptions[0] ?? null

    const [copiedSubId, setCopiedSubId] = useState<number | null>(null)
    const [activatingSubId, setActivatingSubId] = useState<number | null>(null)
    const [banner, setBanner] = useState<HomeBanner>(null)
    const [showQr, setShowQr] = useState(false)

    const radius = 44
    const circumference = 2 * Math.PI * radius
    const strokeOffset = circumference - (usage.percent / 100) * circumference

    // Цвет кольца трафика: лайм → жёлтый (80%) → красный (90%+)
    const trafficRingColor =
        usage.percent >= 90 ? 'var(--color-danger)'
        : usage.percent >= 80 ? 'var(--color-warning)'
        : 'var(--color-accent)'

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

    const copyImportLink = async (sub: UserSubscription) => {
        if (!sub.subscription_url) return
        const url = getSubUrl(sub)
        await copyText(url)
        hapticSuccess()
        setCopiedSubId(sub.id)
        setTimeout(() => setCopiedSubId(null), 1600)
    }

    // Hiddify определяет тип конфига сам через User-Agent.
    // Передаём ЧИСТУЮ ссылку подписки без ?client= — иначе Hiddify отклоняет импорт.
    const openHiddify = (sub: UserSubscription) => {
        if (!sub.subscription_url) return
        hapticTap()
        const url = getSubUrl(sub)
        const deepLink = `hiddify://import/${encodeURIComponent(url)}`
        const w = window.open(deepLink, '_blank', 'noopener,noreferrer')
        if (!w) {
            void copyImportLink(sub)
            setBanner({ type: 'success', text: t('home.hiddifyManualCopy') })
        }
    }

    const openHapp = (sub: UserSubscription) => {
        if (!sub.subscription_url) return
        hapticTap()
        const url = getSubUrl(sub)
        const deepLink = `happ://import/${encodeURIComponent(url)}`
        const w = window.open(deepLink, '_blank', 'noopener,noreferrer')
        if (!w) {
            void copyImportLink(sub)
            setBanner({ type: 'success', text: t('home.happManualCopy') })
        }
    }

    const handleActivate = async (subId: number) => {
        if (!token) return
        hapticTap()
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
                hapticSuccess()
                await refreshData()
            } else {
                const errText = await res.text()
                setBanner({ type: 'error', text: errText || t('home.activationError') })
                hapticError()
            }
        } catch {
            setBanner({ type: 'error', text: t('home.networkActivationError') })
            hapticError()
        } finally {
            setActivatingSubId(null)
        }
    }

    const totalDownload = stats?.total_download || 0
    const totalUpload = stats?.total_upload || 0

    // Имя пользователя — используем полное имя или username из Telegram, никогда технический плейсхолдер
    const displayName = user?.full_name || user?.username || t('common.appName')

    const heroState = hasActiveAccess ? 'online' : hasPending ? 'waiting' : 'idle'
    const heroStatusText =
        heroState === 'online' ? t('home.statusOnline')
        : heroState === 'waiting' ? t('home.statusWaiting')
        : t('home.statusIdle')

    const primaryDaysLine = primarySub
        ? primarySub.status === 'throttled'
            ? t('home.throttledNotice')
            : primarySub.is_free
            ? t('home.freePlanLabel')
            : primarySub.days_left > 0
            ? t('home.daysLeft', { count: primarySub.days_left })
            : t('home.expiringSoon')
        : null

    const quickActions = [
        { key: 'plans', icon: '⚡', label: t('home.qaPlans'), to: '/plans' },
        { key: 'services', icon: '📦', label: t('home.qaServices'), to: '/subscription' },
        { key: 'servers', icon: '🌍', label: t('home.qaServers'), to: primarySub ? `/servers/${primarySub.id}` : '/servers' },
        { key: 'devices', icon: '📱', label: t('home.qaDevices'), to: primarySub ? `/devices?sub=${primarySub.id}` : '/devices' },
        { key: 'billing', icon: '💰', label: t('home.qaBilling'), to: '/billing' },
        { key: 'guide', icon: '🧭', label: t('home.qaGuide'), to: '/support/connect' },
    ]

    return (
        <div className="page home-page">
            {/* Верхняя строка: маскот + приветствие + колокольчик */}
            <header className="home-topbar">
                <span className="home-mascot" aria-hidden="true">🤖</span>
                <div className="home-topbar-copy">
                    <p className="home-kicker">{t('home.welcome')}</p>
                    <h1>{displayName}</h1>
                </div>
                <button
                    className="home-bell-btn"
                    onClick={() => { hapticTap(); navigate('/notifications') }}
                    aria-label={t('home.notificationsBell')}
                    title={t('home.notificationsBell')}
                >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <path d="M18 9a6 6 0 1 0-12 0c0 5-2 6-2 6h16s-2-1-2-6" />
                        <path d="M10.3 19a2 2 0 0 0 3.4 0" />
                    </svg>
                    {unreadCount > 0 && (
                        <span className="home-bell-badge">{unreadCount > 99 ? '99+' : unreadCount}</span>
                    )}
                </button>
            </header>

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

            {/* Герой: статус подключения + один главный CTA */}
            <section className={`home-hero glass-card is-${heroState}`}>
                <div className="hero-status-row">
                    <span className={`state-dot is-${heroState}`} />
                    <span className="hero-status-text">{heroStatusText}</span>
                </div>

                {primarySub ? (
                    <>
                        <p className="hero-plan-line">
                            <strong>{primarySub.plan_name}</strong>
                            {primaryDaysLine && <span> · {primaryDaysLine}</span>}
                        </p>
                        <button className="btn-primary hero-cta" onClick={() => openHiddify(primarySub)}>
                            {t('home.connectHiddify')}
                        </button>
                        <div className="hero-chips">
                            <button className="hero-chip" onClick={() => openHapp(primarySub)}>
                                Happ
                            </button>
                            <button className="hero-chip" onClick={() => { hapticTap(); setShowQr(!showQr) }}>
                                {t('home.showQr')}
                            </button>
                            <button className="hero-chip" onClick={() => void copyImportLink(primarySub)}>
                                {copiedSubId === primarySub.id ? t('common.copied') : t('home.copyLink')}
                            </button>
                            {!primarySub.is_free && primarySub.duration_days > 0 && (
                                <button className="hero-chip" onClick={() => { hapticTap(); navigate('/subscription') }}>
                                    {t('subscription.extend')}
                                </button>
                            )}
                        </div>

                        {/* Free-plan daily bucket: the panel's monitoring loop subtracts
                            daily_traffic_mb from used_traffic at the UTC day boundary
                            (apps/caramba-panel/src/services/monitoring.rs daily_traffic_topup).
                            Once the user hits the cap, throttling stops further growth, so
                            used_traffic is effectively bounded by the daily allowance. */}
                        {primarySub.is_free && (() => {
                            const topupMb = primarySub.daily_traffic_mb ?? 50
                            const topupBytes = topupMb * 1024 * 1024
                            const usedTodayBytes = Math.min(primarySub.used_traffic_bytes, topupBytes)
                            const remainingMb = Math.max(0, (topupBytes - usedTodayBytes) / (1024 * 1024))
                            return (
                                <div className="hero-free-strip">
                                    <div className="hero-free-info">
                                        <span>{t('home.freeDailyToday', { remaining: remainingMb.toFixed(0) })}</span>
                                        <span className="hero-free-sep">·</span>
                                        <span>{t('home.freeDailyTomorrow', { topup: topupMb })}</span>
                                    </div>
                                    <button className="btn-secondary hero-upgrade-btn" onClick={() => { hapticTap(); navigate('/plans') }}>
                                        {t('home.upgradeToPremium')}
                                    </button>
                                </div>
                            )
                        })()}

                        {showQr && (
                            <div className="sub-qr-panel">
                                <div className="sub-qr-wrap">
                                    <QRCodeSVG value={getSubUrl(primarySub)} size={180} bgColor="#ffffff" fgColor="#0b0d14" level="M" includeMargin />
                                </div>
                                <div className="sub-qr-url">
                                    <input type="text" readOnly value={getSubUrl(primarySub)} onClick={e => e.currentTarget.select()} />
                                </div>
                            </div>
                        )}

                        {activeSubscriptions.length > 1 && (
                            <button className="btn-ghost hero-more-subs" onClick={() => { hapticTap(); navigate('/subscription') }}>
                                {t('home.allSubscriptions', { count: activeSubscriptions.length })}
                            </button>
                        )}
                    </>
                ) : hasPending ? (
                    <>
                        <p className="hero-plan-line">{t('home.pendingSubtitle')}</p>
                        <button
                            className="btn-primary hero-cta"
                            onClick={() => handleActivate(pendingSubscriptions[0].id)}
                            disabled={activatingSubId !== null}
                        >
                            {activatingSubId !== null ? t('home.activating') : t('home.activate')}
                        </button>
                    </>
                ) : (
                    <>
                        <p className="hero-plan-line">{t('home.noSubscription')}</p>
                        <button className="btn-primary hero-cta" onClick={() => { hapticTap(); navigate('/plans') }}>
                            {t('home.choosePlan')}
                        </button>
                        <button className="btn-ghost" onClick={() => { hapticTap(); navigate('/promo') }}>
                            {t('home.promoCode')}
                        </button>
                    </>
                )}
            </section>

            {/* Бенто: кольцо трафика + большие цифры */}
            <section className="home-bento-grid">
                <article className="bento-card bento-traffic glass-card">
                    <div className="bento-head">
                        <h3>{t('home.trafficTitle')}</h3>
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

                <div className="bento-tile-col">
                    <article className="bento-tile glass-card">
                        <p>{t('home.daysLeftMetric')}</p>
                        <strong>{isLoading ? '...' : (usage.daysLeft ?? '∞')}</strong>
                    </article>
                    <article className="bento-tile glass-card">
                        <p>{t('home.subscriptionsMetric')}</p>
                        <strong>{subscriptions.length}</strong>
                    </article>
                </div>

                <article className="bento-transfer glass-card">
                    <div className="transfer-item">
                        <p>↓ {t('home.incoming')}</p>
                        <strong>{formatBytes(totalDownload)}</strong>
                    </div>
                    <div className="transfer-divider" />
                    <div className="transfer-item">
                        <p>↑ {t('home.outgoing')}</p>
                        <strong>{formatBytes(totalUpload)}</strong>
                    </div>
                </article>
            </section>

            {/* Быстрые действия — навигация вместо дублирующих блоков */}
            <section className="quick-actions">
                {quickActions.map((qa) => (
                    <button
                        key={qa.key}
                        className="qa-card glass-card"
                        onClick={() => { hapticTap(); navigate(qa.to) }}
                    >
                        <span className="qa-icon" aria-hidden="true">{qa.icon}</span>
                        <span className="qa-label">{qa.label}</span>
                    </button>
                ))}
            </section>

            {/* Ожидающие активации: список нужен, когда герой занят активной
                подпиской или pending-подписок несколько (герой активирует первую) */}
            {hasPending && (hasActiveAccess || pendingSubscriptions.length > 1) && (
                <section className="pending-module glass-card">
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
        </div>
    )
}

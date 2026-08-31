import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import Icon from '../components/Icon'
import { useNavigate } from 'react-router-dom'
import { QRCodeSVG } from 'qrcode.react'
import WebApp from '@twa-dev/sdk'
import DrawerModal from '../components/DrawerModal'
import { apiUrl } from '../config'
import { useAuth, UserSubscription } from '../context/AuthContext'
import { copyText } from '../lib/copyActions'
import { subscriptionLimitBytes, usageProgress } from '../lib/subscriptionMetrics'
import './Subscription.css'

// Типы для каталога планов (нужны для выбора длительности при продлении)
interface PlanDuration {
    id: number
    duration_days: number
    price: number
    price_cents: number
}

interface Plan {
    id: number
    name: string
    durations?: PlanDuration[]
}

function formatTraffic(gb: number): string {
    if (gb >= 1024) return `${(gb / 1024).toFixed(1)} TB`
    return `${gb} GB`
}

function formatDateTime(value?: string | null): string {
    if (!value) return '--'
    const ts = new Date(value)
    if (Number.isNaN(ts.getTime())) return value
    return ts.toLocaleString()
}

function withClient(url: string, client: string): string {
    const separator = url.includes('?') ? '&' : '?'
    return `${url}${separator}client=${encodeURIComponent(client)}`
}

function withVariant(url: string, client: string, variant?: string) {
    const base = withClient(url, client)
    if (!variant) return base
    return `${base}&variant=${encodeURIComponent(variant)}`
}

// Форматирует цену из центов в строку "$X.XX"
function formatPrice(priceCents: number): string {
    const major = Math.floor(priceCents / 100)
    const minor = priceCents % 100
    return `$${major}.${minor.toString().padStart(2, '0')}`
}

export default function Subscription() {
    const { t } = useTranslation()
    const { subscriptions, isLoading, refreshData, token, error } = useAuth()
    const navigate = useNavigate()
    const [expandedId, setExpandedId] = useState<number | null>(null)
    const [copied, setCopied] = useState<number | null>(null)
    const [copiedVless, setCopiedVless] = useState<number | null>(null)
    const [copiedVariant, setCopiedVariant] = useState<string | null>(null)
    const [activatingId, setActivatingId] = useState<number | null>(null)
    const [giftingId, setGiftingId] = useState<number | null>(null)
    const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null)
    const [selectedVariants, setSelectedVariants] = useState<Record<number, string>>({})

    // Состояние для продления подписки
    const [plans, setPlans] = useState<Plan[]>([])
    const [extendTargetSub, setExtendTargetSub] = useState<UserSubscription | null>(null)
    const [extendingDurationId, setExtendingDurationId] = useState<number | null>(null)

    const sorted = [...subscriptions].sort((a, b) => {
        const order: Record<string, number> = { active: 0, pending: 1, expired: 2 }
        const diff = (order[a.status] ?? 3) - (order[b.status] ?? 3)
        if (diff !== 0) return diff
        return new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
    })

    const primaryImportSub = sorted.find((sub) => sub.status === 'active')

    const activeById = useMemo(
        () => Object.fromEntries(sorted.map((sub) => [sub.id, sub.singbox_variants?.[0]?.id || ''])),
        [sorted],
    )

    const handleCopy = (sub: UserSubscription) => {
        void copyText(sub.subscription_url)
        setCopied(sub.id)
        setTimeout(() => setCopied(null), 2000)
    }

    const handleCopyVless = (sub: UserSubscription) => {
        if (!sub.primary_vless_link) return
        void copyText(sub.primary_vless_link)
        setCopiedVless(sub.id)
        setTimeout(() => setCopiedVless(null), 2000)
    }

    const handleCopyVariant = (sub: UserSubscription) => {
        const variantId = selectedVariants[sub.id]
        if (!variantId) return
        void copyText(withVariant(sub.subscription_url, 'singbox', variantId))
        setCopiedVariant(`${sub.id}:${variantId}`)
        setTimeout(() => setCopiedVariant(null), 2000)
    }

    const openExternal = (url: string) => {
        if (url.startsWith('http://') || url.startsWith('https://')) {
            // Используем WebApp SDK напрямую — открывает внешний браузер внутри Telegram
            try { WebApp.openLink(url) } catch { window.open(url, '_blank', 'noopener,noreferrer') }
        } else {
            // Deep-link (hiddify://, happ://, singbox://) — открываем как обычную ссылку
            const w = window.open(url, '_blank', 'noopener,noreferrer')
            if (!w) {
                // Браузер заблокировал popup — копируем URL из deep-link в буфер обмена
                const subUrl = url.match(/import\/(.+)/)?.[1]
                if (subUrl) {
                    const decoded = decodeURIComponent(subUrl)
                    void copyText(decoded)
                    setMessage({ type: 'success', text: t('subscription.linkCopiedManual') })
                } else {
                    window.location.href = url
                }
            }
        }
    }

    const handleActivate = async (subId: number) => {
        if (!token) return

        setActivatingId(subId)
        setMessage(null)
        try {
            const res = await fetch(`/api/client/subscription/${subId}/activate`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}` },
            })

            if (res.ok) {
                const data = await res.json()
                setMessage({
                    type: 'success',
                    text: data?.message || t('subscription.activatedSuccess'),
                })
                await refreshData()
                setExpandedId(subId)
            } else {
                const err = await res.text()
                setMessage({ type: 'error', text: err || t('subscription.activationError') })
            }
        } catch {
            setMessage({ type: 'error', text: t('subscription.networkActivationError') })
        } finally {
            setActivatingId(null)
        }
    }

    const handleConvertToGift = async (subId: number) => {
        if (!token) return

        setGiftingId(subId)
        setMessage(null)
        try {
            const res = await fetch(`/api/client/subscription/${subId}/gift`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}` },
            })

            if (res.ok) {
                const data = await res.json()
                const code = data?.code ? ` ${data.code}` : ''
                if (data?.code) {
                    navigator.clipboard.writeText(data.code).catch(() => undefined)
                }
                setMessage({
                    type: 'success',
                    text: t('subscription.giftCreated', { code: code.trim() }),
                })
                await refreshData()
            } else {
                const err = await res.text()
                setMessage({ type: 'error', text: err || t('subscription.giftError') })
            }
        } catch {
            setMessage({ type: 'error', text: t('subscription.networkGiftError') })
        } finally {
            setGiftingId(null)
        }
    }

    const toggleExpand = (id: number) => {
        setExpandedId(expandedId === id ? null : id)
    }

    useEffect(() => {
        setSelectedVariants((current) => {
            const next = { ...current }
            for (const [subId, variantId] of Object.entries(activeById)) {
                if (!next[Number(subId)] && variantId) next[Number(subId)] = variantId
            }
            return next
        })
    }, [activeById])

    // Ленивая загрузка каталога планов — только когда открывается модал продления
    const loadPlansIfNeeded = async () => {
        if (plans.length > 0 || !token) return
        try {
            const res = await fetch(apiUrl('/api/client/plans'), {
                headers: { Authorization: `Bearer ${token}` },
            })
            if (res.ok) {
                const data = await res.json()
                const loaded: Plan[] = Array.isArray(data)
                    ? data.map((p: any) => ({
                          id: Number(p?.id || 0),
                          name: String(p?.name || ''),
                          durations: Array.isArray(p?.durations)
                              ? p.durations.map((d: any) => ({
                                    id: Number(d?.id || 0),
                                    duration_days: Number(d?.duration_days || 0),
                                    price: Number(d?.price || 0),
                                    price_cents: Number(d?.price_cents ?? d?.price ?? 0),
                                }))
                              : [],
                      }))
                    : []
                setPlans(loaded)
            }
        } catch {
            // Тихая ошибка — модал покажет пустой список
        }
    }

    const handleOpenExtend = (sub: UserSubscription) => {
        setExtendTargetSub(sub)
        void loadPlansIfNeeded()
    }

    const handleExtend = async (durationId: number) => {
        if (!extendTargetSub || !token) return
        const subId = extendTargetSub.id
        setExtendingDurationId(durationId)
        setMessage(null)
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
                setMessage({
                    type: 'success',
                    text: t('subscription.extendSuccess', { date: expiresAt }),
                })
                setExtendTargetSub(null)
                await refreshData()
            } else {
                const errText = await res.text()
                // Специальное сообщение для нехватки баланса
                const isBalance =
                    errText.toLowerCase().includes('balance') ||
                    errText.toLowerCase().includes('insufficient')
                // Модал НЕ закрываем: ошибка показывается внутри него — иначе
                // пользователь видит «ничего не произошло» (сообщение на
                // странице остаётся за пределами экрана под модалом).
                setMessage({
                    type: 'error',
                    text: isBalance ? t('subscription.insufficientBalance') : errText,
                })
            }
        } catch {
            setMessage({ type: 'error', text: t('subscription.networkActivationError') })
        } finally {
            setExtendingDurationId(null)
        }
    }

    // Форматирование длительности (аналогично Home.tsx)
    const formatDuration = (days: number): string => {
        if (days === 0) return t('home.trafficOnly')
        if (days === 30) return t('home.month1')
        if (days === 60) return t('home.months2')
        if (days === 90) return t('home.months3')
        if (days === 180) return t('home.months6')
        if (days === 365) return t('home.year1')
        return t('home.days', { count: days })
    }

    // Дюрации для текущей подписки в модале продления
    const extendDurations: PlanDuration[] = extendTargetSub
        ? (plans.find((p) => p.id === extendTargetSub.plan_id)?.durations ?? [])
        : []

    // Метка статуса подписки
    const statusBadgeLabel = (status: string): string => {
        if (status === 'active') return t('subscription.statusActive')
        if (status === 'pending') return t('subscription.statusPending')
        // 'throttled' — суточная квота бесплатного плана исчерпана, завтра
        // пополнится сама; называть это «Истекла» значит врать пользователю.
        if (status === 'throttled') return t('subscription.statusThrottled')
        return t('subscription.statusExpired')
    }

    if (isLoading) return <div className="page"><div className="loading">{t('subscription.loading')}</div></div>

    if (!token) {
        return (
            <div className="page sub-page">
                <header className="page-header">
                    <button className="back-button" onClick={() => navigate('/')}><Icon name="chevron-left" /></button>
                    <h2>{t('subscription.title')}</h2>
                </header>
                <div className="empty-state">
                    <div className="empty-icon">🔐</div>
                    <h3>{t('subscription.authRequired')}</h3>
                    <p>{error || t('subscription.authNote')}</p>
                </div>
            </div>
        )
    }

    return (
        <div className="page sub-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')}><Icon name="chevron-left" /></button>
                <h2>{t('subscription.myServices')}</h2>
                {subscriptions.length > 0 && (
                    <span className="badge badge-success">
                        {t('subscription.activeCount', { count: subscriptions.filter((s) => s.status === 'active').length })}
                    </span>
                )}
            </header>

            <button className="btn-ghost sub-guide-link" onClick={() => navigate('/support/connect')}>
                {t('subscription.guideLink')}
            </button>

            {message && (
                <div className={`purchase-msg ${message.type}`}>
                    {message.text}
                </div>
            )}

            {sorted.length > 0 && (
                <section className="sub-connect-focus glass-card">
                    <p className="sub-connect-kicker">{t('subscription.primaryPathLabel')}</p>
                    <h3>{t('subscription.primaryPathTitle')}</h3>
                    <p>{t('subscription.primaryPathDesc')}</p>
                    <button
                        className="btn-primary"
                        onClick={() => setExpandedId((primaryImportSub ?? sorted[0]).id)}
                    >
                        {expandedId === (primaryImportSub ?? sorted[0]).id
                            ? t('subscription.importAlreadyOpen')
                            : t('subscription.openImport')}
                    </button>
                </section>
            )}

            {sorted.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-icon">📦</div>
                    <h3>{t('subscription.noSubscriptions')}</h3>
                    <p>{t('subscription.noSubscriptionsDesc')}</p>
                    <button className="btn-primary" onClick={() => navigate('/plans')}>
                        {t('subscription.openPlans')}
                    </button>
                </div>
            ) : (
                <div className="subs-list">
                    {sorted.map((sub, index) => (
                        <div key={sub.id} className={`sub-card glass-card ${sub.status}`}>
                            <div className="sub-header" onClick={() => toggleExpand(sub.id)}>
                                <div className="sub-header-left">
                                    <span className="sub-number">#{index + 1}</span>
                                    <div className="sub-plan-info">
                                        <span className="sub-plan-name">{sub.plan_name}</span>
                                        <span className={`badge badge-${sub.status === 'active' ? 'success' : sub.status === 'pending' || sub.status === 'throttled' ? 'warning' : 'error'}`}>
                                            {statusBadgeLabel(sub.status)}
                                        </span>
                                    </div>
                                </div>
                                <span className="expand-arrow"><Icon name={expandedId === sub.id ? 'chevron-down' : 'chevron-right'} size={14} /></span>
                            </div>

                            {/* Потолок берём из subscriptionLimitBytes: он включает
                                бонусный трафик, т.е. совпадает с тем, по чему
                                панель реально отключает. */}
                            {(() => {
                                const limitBytes = subscriptionLimitBytes(sub)
                                const limitGb = limitBytes / (1024 * 1024 * 1024)
                                return (
                                    <div className="sub-traffic">
                                        <div className="traffic-bar-row">
                                            <span>{t('subscription.traffic')}</span>
                                            <div className="traffic-bar-right">
                                                <span>{sub.used_traffic_gb} GB / {limitBytes > 0 ? formatTraffic(limitGb) : '∞'}</span>
                                                {/* Остаток трафика — только для тарифов с лимитом */}
                                                {limitBytes > 0 && (() => {
                                                    const usedGb = parseFloat(sub.used_traffic_gb) || 0
                                                    const remaining = Math.max(0, limitGb - usedGb)
                                                    return (
                                                        <span className="traffic-remaining-hint">
                                                            {t('home.trafficRemaining', { remaining: remaining.toFixed(1) })}
                                                        </span>
                                                    )
                                                })()}
                                            </div>
                                        </div>
                                        {limitBytes > 0 && (
                                            <div className="progress-bar-mini">
                                                <div
                                                    className="progress-fill-mini"
                                                    style={{ width: `${usageProgress(sub)}%` }}
                                                />
                                            </div>
                                        )}
                                    </div>
                                )
                            })()}

                            <div className="sub-meta-row">
                                {sub.status === 'active' ? (
                                    <>
                                        <span>
                                            {sub.days_left > 0
                                                ? t('home.daysLeft', { count: sub.days_left })
                                                : sub.duration_days === 0
                                                    ? t('subscription.noExpiry')
                                                    : t('home.expiringSoon')}
                                        </span>
                                        <span className="sub-date">
                                            {sub.duration_days > 0
                                                ? new Date(sub.expires_at).toLocaleDateString()
                                                : t('subscription.trafficPlan')}
                                        </span>
                                    </>
                                ) : sub.status === 'pending' ? (
                                    <span>
                                        {sub.duration_days > 0
                                            ? t('subscription.pendingDays', { count: sub.duration_days })
                                            : t('subscription.trafficPlan')}
                                    </span>
                                ) : (
                                    <span>{t('subscription.statusExpired')}</span>
                                )}
                            </div>

                            {sub.note && (
                                <div className="sub-note">
                                    {sub.note}
                                </div>
                            )}

                            <div className="sub-extra-row">
                                <span>{t('subscription.lastConfigUpdate')}: {formatDateTime(sub.last_sub_access)}</span>
                            </div>

                            {/* Автопродление: read-only индикатор. Бэкенд отдаёт auto_renew,
                                но client-эндпоинта для переключения нет — управление в боте.
                                Показываем только для платных подписок со сроком. */}
                            {sub.status === 'active' && sub.duration_days > 0 && !sub.is_free && (
                                <div className="auto-renew-row" onClick={(e) => e.stopPropagation()}>
                                    <div className="auto-renew-copy">
                                        <span className="auto-renew-label">{t('subscription.autoRenew')}</span>
                                        <span className="auto-renew-hint">{t('subscription.autoRenewManageBot')}</span>
                                    </div>
                                    <span
                                        className={`auto-renew-state ${sub.auto_renew ? 'is-on' : 'is-off'}`}
                                        role="status"
                                        aria-label={`${t('subscription.autoRenew')}: ${sub.auto_renew ? t('subscription.autoRenewOn') : t('subscription.autoRenewOff')}`}
                                    >
                                        {sub.auto_renew ? t('subscription.autoRenewOn') : t('subscription.autoRenewOff')}
                                    </span>
                                </div>
                            )}

                            {sub.status === 'active' && (
                                <div className="sub-actions">
                                    <button
                                        className="btn-text"
                                        onClick={(e) => { e.stopPropagation(); navigate(`/servers/${sub.id}`) }}
                                    >
                                        {t('subscription.nodeOptimization')}
                                    </button>
                                    <button
                                        className="btn-text"
                                        onClick={(e) => { e.stopPropagation(); navigate(`/servers/${sub.id}?magic=1`) }}
                                    >
                                        {t('common.magicOptimize')}
                                    </button>
                                    {/* Кнопка продления — только для платных активных подписок со сроком */}
                                    {sub.duration_days > 0 && (
                                        <button
                                            className="btn-text"
                                            onClick={(e) => { e.stopPropagation(); handleOpenExtend(sub) }}
                                        >
                                            {t('subscription.extend')}
                                        </button>
                                    )}
                                </div>
                            )}
                            {sub.status === 'pending' && (
                                <div className="sub-actions">
                                    <button
                                        className="btn-text"
                                        onClick={(e) => {
                                            e.stopPropagation()
                                            handleActivate(sub.id)
                                        }}
                                        disabled={activatingId !== null || giftingId !== null}
                                    >
                                        {activatingId === sub.id ? t('home.activating') : t('home.activate')}
                                    </button>
                                    <button
                                        className="btn-text"
                                        onClick={(e) => {
                                            e.stopPropagation()
                                            handleConvertToGift(sub.id)
                                        }}
                                        disabled={activatingId !== null || giftingId !== null}
                                    >
                                        {giftingId === sub.id ? t('subscription.creatingGift') : t('subscription.makeGift')}
                                    </button>
                                </div>
                            )}

                            {expandedId === sub.id && sub.status === 'active' && (
                                <div className="sub-expanded">
                                    <div className="qr-wrapper">
                                        <QRCodeSVG
                                            value={sub.subscription_url}
                                            size={160}
                                            bgColor="#ffffff"
                                            fgColor="#0D0D1A"
                                            level="M"
                                            includeMargin
                                        />
                                    </div>
                                    <p className="qr-hint">{t('subscription.qrHint')}</p>

                                    <div className="link-row">
                                        <input type="text" readOnly value={sub.subscription_url} onClick={(e) => e.currentTarget.select()} />
                                        <button
                                            className={`btn-secondary copy-btn ${copied === sub.id ? 'copied' : ''}`}
                                            onClick={() => handleCopy(sub)}
                                        >
                                            {copied === sub.id ? t('common.copied') : t('common.copy')}
                                        </button>
                                    </div>

                                    {sub.primary_vless_link && (
                                        <div className="link-row">
                                            <input type="text" readOnly value={sub.primary_vless_link ?? ''} onClick={(e) => e.currentTarget.select()} />
                                            <button
                                                className={`btn-secondary copy-btn ${copiedVless === sub.id ? 'copied' : ''}`}
                                                onClick={() => handleCopyVless(sub)}
                                            >
                                                {copiedVless === sub.id ? t('common.copied') : 'VLESS'}
                                            </button>
                                        </div>
                                    )}

                                    <div className="import-path-block">
                                        <div className="import-path-head">
                                            <h4>{t('subscription.importTitle')}</h4>
                                            <span>{t('subscription.importDesc')}</span>
                                        </div>
                                        <div className="app-links-grid app-links-primary-grid">
                                            <button
                                                className="btn-primary btn-app"
                                                onClick={() => openExternal(`hiddify://import/${encodeURIComponent(sub.subscription_url)}`)}
                                            >
                                                {t('subscription.openHiddify')}
                                            </button>
                                            <button
                                                className="btn-secondary btn-app"
                                                onClick={() => openExternal(withVariant(sub.subscription_url, 'singbox', selectedVariants[sub.id]))}
                                            >
                                                {t('subscription.openSingbox')}
                                            </button>
                                        </div>
                                    </div>

                                    <div className="app-links-grid">
                                        <button
                                            className="btn-text btn-app"
                                            onClick={() => openExternal(`happ://add?url=${encodeURIComponent(withClient(sub.subscription_url, 'happ'))}`)}
                                        >
                                            Happ
                                        </button>
                                        <button
                                            className="btn-text btn-app"
                                            onClick={() => navigate(`/servers/${sub.id}`)}
                                        >
                                            {t('subscription.allVariants')}
                                        </button>
                                    </div>

                                    {!!sub.singbox_variants?.length && (
                                        <details className="advanced-variants">
                                            <summary>{t('subscription.advancedVariants')}</summary>
                                            <div className="variant-picker-block">
                                                <div className="variant-picker-head">
                                                    <h4>{t('subscription.manualProfileSelect')}</h4>
                                                    <span>{t('subscription.manualProfileDesc')}</span>
                                                </div>
                                                <div className="variant-picker-list">
                                                    {sub.singbox_variants.map((variant) => (
                                                        <button
                                                            key={variant.id}
                                                            className={`variant-inline-card ${selectedVariants[sub.id] === variant.id ? 'active' : ''}`}
                                                            onClick={() => setSelectedVariants((current) => ({ ...current, [sub.id]: variant.id }))}
                                                        >
                                                            <span className="variant-inline-title">{variant.label}</span>
                                                            <span className="variant-inline-meta">
                                                                {variant.transport} · {variant.relay ? t('servers.viaRelay') : t('servers.directRoute')}
                                                            </span>
                                                        </button>
                                                    ))}
                                                </div>
                                                <button className="btn-secondary variant-copy-btn" onClick={() => handleCopyVariant(sub)}>
                                                    {copiedVariant === `${sub.id}:${selectedVariants[sub.id]}`
                                                        ? t('subscription.variantCopied')
                                                        : t('subscription.copyVariant')}
                                                </button>
                                            </div>
                                        </details>
                                    )}
                                </div>
                            )}
                        </div>
                    ))}
                </div>
            )}

            {/* Модал выбора длительности для продления подписки */}
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
                {message?.type === 'error' && (
                    <div className="purchase-msg error">{message.text}</div>
                )}
                {extendDurations.length === 0 ? (
                    <div className="empty-state drawer-empty">
                        <div className="empty-icon">⚡</div>
                        <p>{t('home.noDurations')}</p>
                    </div>
                ) : (
                    <div className="duration-grid">
                        {extendDurations.map((dur) => (
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
                )}
            </DrawerModal>
        </div>
    )
}

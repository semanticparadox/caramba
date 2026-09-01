import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import WebApp from '@twa-dev/sdk'
import { useAuth } from '../../context/AuthContext'
import { apiUrl } from '../../config'
import { hapticError, hapticSuccess } from '../../lib/haptics'
import { formatPrice, type Plan, type PlanDuration } from '../../lib/planFormat'
import { usePlansCatalog } from '../../lib/usePlansCatalog'
import { usePurchase } from '../../lib/usePurchase'
import { useBotPayment } from '../../lib/useBotPayment'
import { subscriptionLimitBytes } from '../../lib/subscriptionMetrics'
import Mascot from '../Mascot'
import { ExaIcon, StarFilled } from '../icons'
import { Button, CountryChip, Pill, Sheet } from '../ui'
import { formatDate, pickPrimary } from '../lib/subscription'
import { useToast } from '../lib/useToast'

type Step = 'plan' | 'method' | 'waiting' | 'success'

/** Оплата в три шага поверх главного экрана: тариф и срок → способ → успех.
 *  Только реально включённые провайдеры; цены — из каталога панели. */
export default function Pay() {
    const { t, i18n } = useTranslation()
    const navigate = useNavigate()
    const toast = useToast()
    const [params] = useSearchParams()
    const { token, subscriptions, refreshData } = useAuth()
    const { plans, providers, loading } = usePlansCatalog({ token })
    const { purchase, purchasing } = usePurchase({ token, onRefresh: refreshData })
    const { botPayment, startBotPayment, clearBotPayment } = useBotPayment({ token, onRefresh: refreshData })
    const locale = i18n.language

    const [step, setStep] = useState<Step>('plan')
    const [planId, setPlanId] = useState<number | null>(null)
    const [durationId, setDurationId] = useState<number | null>(null)
    const [provider, setProvider] = useState<string | null>(null)
    const [activating, setActivating] = useState(false)

    const paid = useMemo(() => plans.filter((p) => !p.is_free && p.durations.length > 0), [plans])
    const free = useMemo(() => plans.find((p) => p.is_free) ?? null, [plans])
    const recommended = useMemo(() => paid.find((p) => /gold/i.test(p.name)) ?? paid[0] ?? null, [paid])

    // Первичный выбор: рекомендуемый тариф и средний срок (в макете — 90 дней).
    useEffect(() => {
        if (planId !== null || paid.length === 0) return
        const upgrade = params.get('upgrade') === '1'
        const plan = upgrade ? recommended : (recommended ?? paid[0])
        if (!plan) return
        setPlanId(plan.id)
        const durs = [...plan.durations].sort((a, b) => a.duration_days - b.duration_days)
        setDurationId((durs[1] ?? durs[0]).id)
    }, [paid, recommended, planId, params])

    // Способы: показываем только те, что панель объявила включёнными.
    const methods = useMemo(() => {
        const known = ['stars', 'nowpayments', 'balance']
        const list = providers.filter((p) => known.includes(p.id))
        return list.length ? list : providers
    }, [providers])
    useEffect(() => {
        if (provider === null && methods.length) setProvider(methods[0].id)
    }, [methods, provider])

    const plan = paid.find((p) => p.id === planId) ?? null
    const duration = plan?.durations.find((d) => d.id === durationId) ?? null

    // Ждём подтверждение: сравниваем дату окончания подписки до и после.
    const before = useRef<string | null>(null)
    const sub = useMemo(() => pickPrimary(subscriptions), [subscriptions])
    useEffect(() => {
        if (step !== 'waiting') return
        if (botPayment?.status === 'completed') {
            hapticSuccess()
            setStep('success')
            return
        }
        if (botPayment?.status === 'failed' || botPayment?.status === 'expired') {
            hapticError()
            toast(t('exa.pay.failed'))
            clearBotPayment()
            setStep('method')
            return
        }
        if (sub && before.current && sub.expires_at !== before.current) {
            hapticSuccess()
            setStep('success')
        }
    }, [step, sub, botPayment, clearBotPayment, toast, t])

    // Пока ждём Stars (без сессии) — опрашиваем профиль сами.
    useEffect(() => {
        if (step !== 'waiting' || botPayment) return
        const id = setInterval(() => void refreshData(), 3000)
        const stop = setTimeout(() => clearInterval(id), 120000)
        return () => {
            clearInterval(id)
            clearTimeout(stop)
        }
    }, [step, botPayment, refreshData])

    const pay = async () => {
        if (!duration || !provider || purchasing !== null) return
        before.current = sub?.expires_at ?? null
        const result = await purchase({ durationId: duration.id, provider })
        switch (result.outcome) {
            case 'success':
                hapticSuccess()
                await refreshData()
                setStep('success')
                break
            case 'redirect':
                setStep('waiting')
                break
            case 'bot_link':
                startBotPayment(result.invoiceUrl, result.sessionId)
                if (/^https?:\/\/t\.me\//.test(result.invoiceUrl)) WebApp.openTelegramLink(result.invoiceUrl)
                else WebApp.openLink(result.invoiceUrl)
                setStep('waiting')
                break
            case 'manual':
                WebApp.openLink(result.invoiceUrl)
                setStep('waiting')
                break
            default:
                hapticError()
                toast(result.messageKey ? t(result.messageKey) : (result.message ?? t('exa.common.error')))
        }
    }

    // Бесплатный тариф: у него одна «длительность» с нулевой ценой — тот же
    // счёт, что и у платных, но панель активирует его без оплаты.
    const startFree = async () => {
        if (!free || !token || activating) return
        const dur = free.durations[0]
        setActivating(true)
        try {
            if (dur) {
                const result = await purchase({ durationId: dur.id, provider: 'balance' })
                if (result.outcome === 'success' || result.outcome === 'redirect') {
                    await refreshData()
                    hapticSuccess()
                    toast(t('exa.pay.activated'))
                    navigate('/', { replace: true })
                    return
                }
            }
            // Подписка уже есть и ждёт активации — активируем её.
            const pending = subscriptions.find((s) => s.status === 'pending')
            if (pending) {
                const res = await fetch(apiUrl(`/api/client/subscription/${pending.id}/activate`), {
                    method: 'POST',
                    headers: { Authorization: `Bearer ${token}` },
                })
                if (res.ok) {
                    await refreshData()
                    hapticSuccess()
                    toast(t('exa.pay.activated'))
                    navigate('/', { replace: true })
                    return
                }
            }
            hapticError()
            toast(t('exa.common.error'))
        } finally {
            setActivating(false)
        }
    }

    const close = () => navigate(-1)
    const daysLabel = (d: PlanDuration) => t('exa.common.days', { count: d.duration_days })
    const perMonth = (d: PlanDuration) => formatPrice(Math.round(d.price_cents / Math.max(1, d.duration_days / 30)))
    const starsAmount = (id: string) => methods.find((m) => m.id === id)?.amount ?? null

    // ---------- успех: полноэкранно, с маскотом ----------
    if (step === 'success') {
        const s = pickPrimary(subscriptions)
        const limit = s ? subscriptionLimitBytes(s) : 0
        return (
            <div className="exa-screen" style={{ paddingBottom: 24 }}>
                <div className="exa-empty" style={{ gap: 28, paddingBottom: 0 }}>
                    <Mascot state="shield" width={200} />
                    <div>
                        <div className="exa-success__title">{t('exa.pay.success')}</div>
                        {s ? <div className="exa-empty__text">{t('exa.pay.successUntil', { plan: s.plan_name, date: formatDate(s.expires_at, locale, true) })}</div> : null}
                    </div>
                    {s ? (
                        <div className="exa-facts">
                            <div>
                                <span className="exa-display">{s.days_left}</span>
                                <small>{t('exa.pay.successDays')}</small>
                            </div>
                            <div>
                                <span className="exa-display">{s.device_limit ?? '—'}</span>
                                <small>{t('exa.pay.successDevices')}</small>
                            </div>
                            <div>
                                <span className="exa-display">{limit > 0 ? `${Math.round(limit / 1024 ** 3)} ${t('exa.common.gb')}` : '∞'}</span>
                                <small>{t('exa.pay.successTraffic')}</small>
                            </div>
                        </div>
                    ) : null}
                </div>
                <Button onClick={() => navigate('/', { replace: true })}>{t('exa.pay.toHome')}</Button>
            </div>
        )
    }

    return (
        <div className="exa-screen">
            {/* Шаг 1 — тариф */}
            <Sheet open={step === 'plan'} title={t('exa.pay.plan')} aside={t('exa.pay.step', { n: 1 })} onClose={close}>
                {loading && plans.length === 0 ? <div className="exa-loading">{t('exa.common.loading')}</div> : null}
                {!loading && paid.length === 0 ? <p className="exa-muted exa-center">{t('exa.pay.noPlans')}</p> : null}
                {paid.map((p) => (
                    <PlanCard
                        key={p.id}
                        plan={p}
                        selected={p.id === planId}
                        recommended={p.id === recommended?.id}
                        durationId={p.id === planId ? durationId : null}
                        onPick={(d) => {
                            setPlanId(p.id)
                            setDurationId(d.id)
                        }}
                        daysLabel={daysLabel}
                        perMonth={perMonth}
                    />
                ))}
                {free ? (
                    <button type="button" className="exa-card exa-card--row" style={{ minHeight: 56, background: 'var(--exa-code-bg)' }} disabled={activating} onClick={() => void startFree()}>
                        <span className="exa-row__body" style={{ gap: 2 }}>
                            <span className="exa-row__title">
                                <span>{t('exa.pay.tryFree')}</span>
                            </span>
                            <span className="exa-row__meta" style={{ fontSize: 12 }}>
                                {t('exa.pay.devices', { count: free.device_limit })}
                                {free.daily_traffic_mb ? ` · ${t('exa.pay.freeDaily', { mb: free.daily_traffic_mb })}` : free.traffic_limit_gb ? ` · ${t('exa.pay.gb', { gb: free.traffic_limit_gb })}` : ''}
                                {free.server_count ? ` · ${t('exa.pay.servers', { count: free.server_count })}` : ''}
                            </span>
                        </span>
                        <ExaIcon name="chevron" size={20} className="exa-linkrow__chevron" />
                    </button>
                ) : null}
                <Button disabled={!plan || !duration} onClick={() => setStep('method')}>
                    {plan && duration ? t('exa.pay.next', { plan: plan.name, days: daysLabel(duration), price: formatPrice(duration.price_cents) }) : t('exa.common.next')}
                </Button>
            </Sheet>

            {/* Шаг 2 — способ */}
            <Sheet open={step === 'method'} title={t('exa.pay.method')} aside={t('exa.pay.step', { n: 2 })} onClose={() => setStep('plan')}>
                {plan && duration ? (
                    <div className="exa-summary">
                        <span>
                            {plan.name} · {daysLabel(duration)}
                        </span>
                        <b>{formatPrice(duration.price_cents)}</b>
                    </div>
                ) : null}
                {methods.map((m) => {
                    const id = m.id
                    const isStars = id === 'stars'
                    const isCrypto = id === 'nowpayments'
                    const label = isStars ? t('exa.pay.stars') : isCrypto ? t('exa.pay.crypto') : id === 'balance' ? t('exa.pay.balance') : m.label
                    const hint = isStars ? t('exa.pay.starsHint') : isCrypto ? t('exa.pay.cryptoHint') : id === 'balance' ? t('exa.pay.balanceHint') : ''
                    const amount = starsAmount(id)
                    return (
                        <button key={id} type="button" className={`exa-method${provider === id ? ' is-on' : ''}`} onClick={() => setProvider(id)}>
                            <span className="exa-method__icon">
                                <ExaIcon name={isStars ? 'stars' : isCrypto ? 'crypto' : 'pay'} />
                            </span>
                            <span className="exa-method__body">
                                <span style={{ fontWeight: 600 }}>{label}</span>
                                {hint ? <small>{hint}</small> : null}
                            </span>
                            {isStars && amount ? (
                                <span className="exa-method__price">
                                    <StarFilled size={16} color="var(--exa-ember)" />
                                    {amount}
                                </span>
                            ) : duration ? (
                                <span className="exa-method__price">{isCrypto ? '≈ ' : ''}{formatPrice(duration.price_cents)}</span>
                            ) : null}
                        </button>
                    )
                })}
                <p className="exa-muted" style={{ lineHeight: 1.45, padding: '0 2px' }}>
                    {t('exa.pay.note')}
                </p>
                <Button disabled={!provider || !duration || purchasing !== null} onClick={() => void pay()}>
                    {purchasing !== null ? t('exa.pay.paying') : t('exa.pay.payNow')}
                    {provider === 'stars' && starsAmount('stars') ? (
                        <>
                            <StarFilled size={16} color="var(--exa-on-ember)" /> {starsAmount('stars')}
                        </>
                    ) : null}
                </Button>
            </Sheet>

            {/* Ожидание подтверждения */}
            <Sheet open={step === 'waiting'} title={t('exa.pay.waiting')} onClose={() => { clearBotPayment(); setStep('method') }}>
                <div className="exa-loading">{t('exa.common.loading')}</div>
                {botPayment?.invoiceUrl ? (
                    <Button variant="secondary" size="md" onClick={() => WebApp.openLink(botPayment.invoiceUrl)}>
                        {t('exa.pay.openBot')}
                    </Button>
                ) : null}
                <Button variant="ghost" size="md" onClick={() => void refreshData()}>
                    {t('exa.common.done')}
                </Button>
            </Sheet>
        </div>
    )
}

function PlanCard({
    plan,
    selected,
    recommended,
    durationId,
    onPick,
    daysLabel,
    perMonth,
}: {
    plan: Plan
    selected: boolean
    recommended: boolean
    durationId: number | null
    onPick: (d: PlanDuration) => void
    daysLabel: (d: PlanDuration) => string
    perMonth: (d: PlanDuration) => string
}) {
    const { t } = useTranslation()
    const durs = [...plan.durations].sort((a, b) => a.duration_days - b.duration_days)
    return (
        <section className={`exa-card exa-plan${selected ? ' is-selected' : ''}`}>
            <div className="exa-card__head" style={{ alignItems: 'center' }}>
                <span className="exa-display">{plan.name}</span>
                {recommended ? <Pill tone="accent">{t('exa.common.recommended')}</Pill> : null}
            </div>
            <div className="exa-plan__facts">
                <span>
                    <b>{plan.device_limit}</b> {t('exa.pay.devices', { count: plan.device_limit }).replace(/^\d+\s*/, '')}
                </span>
                <span>{plan.traffic_limit_gb > 0 ? <><b>{plan.traffic_limit_gb}</b> {t('exa.common.gb')}</> : <b>{t('exa.common.unlimited')}</b>}</span>
                {plan.countries?.length ? (
                    <span className="exa-plan__chips">
                        {plan.countries.slice(0, 4).map((c) => (
                            <CountryChip key={c} code={c} small />
                        ))}
                    </span>
                ) : null}
            </div>
            <div className="exa-durations">
                {durs.map((d) => (
                    <button key={d.id} type="button" className={`exa-duration${durationId === d.id ? ' is-on' : ''}`} onClick={() => onPick(d)}>
                        <small>{daysLabel(d)}</small>
                        <b>{formatPrice(d.price_cents)}</b>
                        <em>{t('exa.pay.perMonth', { price: perMonth(d) })}</em>
                    </button>
                ))}
            </div>
        </section>
    )
}

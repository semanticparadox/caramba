import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import WebApp from '@twa-dev/sdk'
import { useAuth } from '../../context/AuthContext'
import { apiUrl } from '../../config'
import { copyText } from '../../lib/copyActions'
import { hapticError, hapticSuccess, hapticTap } from '../../lib/haptics'
import { LANGUAGE_STORAGE_KEY } from '../../lib/telegram'
import { formatPrice } from '../../lib/planFormat'
import { ExaIcon } from '../icons'
import { Button, IconButton, LinkRow, Pill, ScreenHeader, Segmented, Toggle } from '../ui'
import { formatDateTime, pickPrimary } from '../lib/subscription'
import { useToast } from '../lib/useToast'

interface Payment {
    id: number
    amount: number
    method: string
    status: string
    created_at: number
    plan_name?: string | null
    duration_days?: number | null
}

interface ReferralStats {
    referral_code: string
    referred_count: number
    referral_link: string
    total_earned_usd: number
}

type Category = 'payment' | 'subscription' | 'device' | 'referral' | 'support_ticket' | 'system_maintenance'
type Channel = 'bot_dm' | 'mini_app'
interface PrefRow {
    category: Category
    channel: Channel
    enabled: boolean
}
const ALL_CATEGORIES: Category[] = ['payment', 'subscription', 'device', 'referral', 'support_ticket', 'system_maintenance']
const ALL_CHANNELS: Channel[] = ['bot_dm', 'mini_app']
/** В профиле показываем три переключателя — те, что касаются человека напрямую. */
const SHOWN: { cat: Category; key: string }[] = [
    { cat: 'subscription', key: 'exa.profile.notifySubscription' },
    { cat: 'payment', key: 'exa.profile.notifyPayment' },
    { cat: 'device', key: 'exa.profile.notifyDevice' },
]

const APP_VERSION = '1.0'

export default function Profile() {
    const { t, i18n } = useTranslation()
    const navigate = useNavigate()
    const toast = useToast()
    const [params, setParams] = useSearchParams()
    const { token, user, userStats, subscriptions } = useAuth()
    const sub = useMemo(() => pickPrimary(subscriptions), [subscriptions])
    const locale = i18n.language

    // ---------- платежи ----------
    const [payments, setPayments] = useState<Payment[]>([])
    useEffect(() => {
        if (!token) return
        void fetch(apiUrl('/api/client/user/payments'), { headers: { Authorization: `Bearer ${token}` } })
            .then((r) => (r.ok ? r.json() : []))
            .then((d) => setPayments(Array.isArray(d) ? d : (d?.payments ?? [])))
            .catch(() => {})
    }, [token])

    // ---------- промо и рефералы ----------
    const [ref, setRef] = useState<ReferralStats | null>(null)
    const [promoOpen, setPromoOpen] = useState(params.get('promo') === '1')
    const [promo, setPromo] = useState('')
    const [promoBusy, setPromoBusy] = useState(false)
    const promoInput = useRef<HTMLInputElement>(null)
    useEffect(() => {
        if (!token) return
        void fetch(apiUrl('/api/client/user/referrals'), { headers: { Authorization: `Bearer ${token}` } })
            .then((r) => (r.ok ? r.json() : null))
            .then((d) => d && setRef(d.stats ?? d))
            .catch(() => {})
    }, [token])
    useEffect(() => {
        if (promoOpen) promoInput.current?.focus()
    }, [promoOpen])

    const redeem = async () => {
        const code = promo.trim()
        if (!token || !code || promoBusy) return
        setPromoBusy(true)
        try {
            const res = await fetch(apiUrl('/api/client/promo/redeem'), {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
                body: JSON.stringify({ code }),
            })
            if (res.ok) {
                const data = await res.json().catch(() => null)
                hapticSuccess()
                toast(data?.message || t('exa.profile.promoApplied'))
                setPromo('')
                setPromoOpen(false)
                if (params.has('promo')) setParams({}, { replace: true })
            } else {
                hapticError()
                toast((await res.text()) || t('exa.common.error'))
            }
        } catch {
            hapticError()
            toast(t('exa.common.error'))
        } finally {
            setPromoBusy(false)
        }
    }

    // ---------- уведомления ----------
    const [prefs, setPrefs] = useState<Map<string, boolean>>(new Map())
    const key = (c: Category, ch: Channel) => `${c}::${ch}`
    useEffect(() => {
        if (!token) return
        void fetch(apiUrl('/api/client/notification-preferences'), { headers: { Authorization: `Bearer ${token}` } })
            .then((r) => (r.ok ? r.json() : []))
            .then((rows: PrefRow[]) => {
                const m = new Map<string, boolean>()
                for (const p of rows) m.set(key(p.category, p.channel), p.enabled)
                setPrefs(m)
            })
            .catch(() => {})
    }, [token])

    const savePrefs = useCallback(
        async (next: Map<string, boolean>) => {
            if (!token) return
            const body: PrefRow[] = []
            for (const category of ALL_CATEGORIES)
                for (const channel of ALL_CHANNELS) body.push({ category, channel, enabled: next.get(key(category, channel)) ?? true })
            const res = await fetch(apiUrl('/api/client/notification-preferences'), {
                method: 'PUT',
                headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
                body: JSON.stringify(body),
            })
            if (!res.ok) {
                hapticError()
                toast(t('exa.common.error'))
            }
        },
        [token, toast, t],
    )

    const toggle = (cat: Category, on: boolean) => {
        const next = new Map(prefs)
        // Категория включается и в боте, и в мини-приложении — одним переключателем.
        for (const ch of ALL_CHANNELS) next.set(key(cat, ch), on)
        setPrefs(next)
        void savePrefs(next)
    }
    const isOn = (cat: Category) => prefs.get(key(cat, 'bot_dm')) ?? true

    // ---------- язык ----------
    const lang = (i18n.language || 'ru').startsWith('en') ? 'en' : 'ru'
    const setLang = (code: 'ru' | 'en') => {
        try {
            localStorage.setItem(LANGUAGE_STORAGE_KEY, code)
        } catch {
            /* приватный режим */
        }
        void i18n.changeLanguage(code)
    }

    // ---------- поддержка ----------
    // support_url в панели хранится как username бота-поддержки (так его читает
    // и Telegram-бот). Всё, что не похоже на ссылку или username, — мусор из
    // формы, тогда ведём в чат самого бота, а не в браузер.
    const openSupport = () => {
        hapticTap()
        const raw = (userStats?.support_url || '').trim()
        const bot = userStats?.bot_username ? `https://t.me/${userStats.bot_username}` : null
        let url: string | null = null
        if (/^https?:\/\//.test(raw)) url = raw
        else {
            const name = raw.replace(/^@/, '')
            if (/^[A-Za-z0-9_]{5,32}$/.test(name) && !/undefined|null/i.test(name)) url = `https://t.me/${name}`
        }
        url = url || bot
        if (!url) return
        if (/^https?:\/\/t\.me\//.test(url)) WebApp.openTelegramLink(url)
        else WebApp.openLink(url)
    }

    const methodLabel = (m: string) => {
        const id = (m || '').toLowerCase()
        if (id.includes('star')) return t('exa.profile.method.stars')
        if (id.includes('now') || id.includes('crypto')) return t('exa.profile.method.nowpayments')
        if (id === 'balance') return t('exa.profile.method.balance')
        if (id === 'manual') return t('exa.profile.method.manual')
        return m
    }

    return (
        <div className="exa-screen">
            <ScreenHeader title={t('exa.profile.title')} aside={user?.username ? `@${user.username}` : user?.full_name} />

            <LinkRow
                icon={<ExaIcon name="devices" size={22} />}
                title={t('exa.profile.devices')}
                aside={sub && sub.device_limit ? t('exa.home.devicesOf', { n: sub.active_devices ?? 0, of: sub.device_limit }) : undefined}
                onClick={() => navigate('/devices')}
            />

            <section className="exa-card exa-card--list">
                <div className="exa-row__head">
                    <ExaIcon name="pay" size={22} />
                    <span>{t('exa.profile.payments')}</span>
                </div>
                {payments.length === 0 ? (
                    <div className="exa-row" style={{ minHeight: 48 }}>
                        <span className="exa-muted">{t('exa.profile.noPayments')}</span>
                    </div>
                ) : (
                    payments.slice(0, 5).map((p) => (
                        <div key={p.id} className="exa-row" style={{ minHeight: 48, padding: '11px 16px' }}>
                            <span className="exa-row__body" style={{ gap: 1 }}>
                                <span style={{ fontSize: 14 }}>
                                    {p.plan_name ? `${p.plan_name}${p.duration_days ? ` · ${t('exa.common.days', { count: p.duration_days })}` : ''}` : methodLabel(p.method)}
                                </span>
                                <span className="exa-row__meta" style={{ fontSize: 12 }}>
                                    {formatDateTime(p.created_at, locale)} · {methodLabel(p.method)}
                                    {p.status && p.status !== 'completed' ? <Pill tone={p.status === 'failed' ? 'danger' : 'neutral'}>{p.status}</Pill> : null}
                                </span>
                            </span>
                            <span className="exa-display" style={{ fontSize: 14 }}>
                                {formatPrice(Math.round(Math.abs(p.amount) * 100))}
                            </span>
                        </div>
                    ))
                )}
            </section>

            <section className="exa-card exa-card--list">
                <div className="exa-row__head">
                    <ExaIcon name="promo" size={22} />
                    <span>{t('exa.profile.promo')}</span>
                    <Button variant="secondary" size="sm" block={false} onClick={() => setPromoOpen((v) => !v)}>
                        {t('exa.profile.enterPromo')}
                    </Button>
                </div>
                {promoOpen || ref?.referral_link ? (
                <div style={{ padding: '12px 16px', display: 'grid', gap: 10 }}>
                    {promoOpen ? (
                        <div style={{ display: 'flex', gap: 8 }}>
                            <input
                                ref={promoInput}
                                className="exa-rename"
                                placeholder={t('exa.profile.promoPlaceholder')}
                                value={promo}
                                onChange={(e) => setPromo(e.target.value.toUpperCase())}
                                onKeyDown={(e) => e.key === 'Enter' && void redeem()}
                                autoCapitalize="characters"
                                autoCorrect="off"
                            />
                            <Button size="sm" block={false} disabled={promoBusy || !promo.trim()} onClick={() => void redeem()}>
                                {t('exa.profile.promoApply')}
                            </Button>
                        </div>
                    ) : null}
                    {ref?.referral_link ? (
                        <>
                            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                                <span className="exa-code is-line" style={{ flex: 1 }}>
                                    {ref.referral_link.replace(/^https?:\/\//, '')}
                                </span>
                                <IconButton
                                    label={t('exa.common.copy')}
                                    className="is-sm"
                                    onClick={async () => {
                                        if (await copyText(ref.referral_link)) {
                                            hapticSuccess()
                                            toast(t('exa.common.copied'))
                                        }
                                    }}
                                >
                                    <ExaIcon name="copy" size={18} />
                                </IconButton>
                            </div>
                            <div className="exa-stat-pair">
                                <span>
                                    <b>{ref.referred_count}</b>
                                    <small>{t('exa.profile.invited')}</small>
                                </span>
                                <span>
                                    <b>{formatPrice(Math.round((ref.total_earned_usd || 0) * 100))}</b>
                                    <small>{t('exa.profile.earned')}</small>
                                </span>
                            </div>
                        </>
                    ) : null}
                </div>
                ) : null}
            </section>

            <section className="exa-card exa-card--list">
                {SHOWN.map((row, i) => (
                    <div key={row.cat} className="exa-row" style={{ minHeight: 48, padding: '12px 16px' }}>
                        {i === 0 ? <ExaIcon name="bell" size={22} style={{ color: 'var(--exa-text-2)' }} /> : <span style={{ width: 22 }} />}
                        <span style={{ flex: 1, fontSize: 15 }}>{t(row.key)}</span>
                        <Toggle on={isOn(row.cat)} onChange={(v) => toggle(row.cat, v)} label={t(row.key)} />
                    </div>
                ))}
                <div className="exa-row" style={{ minHeight: 48, padding: '12px 16px' }}>
                    <ExaIcon name="language" size={22} style={{ color: 'var(--exa-text-2)' }} />
                    <span style={{ flex: 1, fontSize: 15 }}>{t('exa.profile.language')}</span>
                    <Segmented
                        compact
                        value={lang}
                        onChange={setLang}
                        options={[
                            { value: 'ru', label: 'RU' },
                            { value: 'en', label: 'EN' },
                        ]}
                    />
                </div>
            </section>

            <LinkRow icon={<ExaIcon name="support" size={22} />} title={t('exa.profile.support')} onClick={openSupport} />

            <div className="exa-legal">EXA {APP_VERSION}</div>
        </div>
    )
}

import { useCallback, useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { apiUrl } from '../../config'
import { useAuth } from '../../context/AuthContext'
import { useNotifications } from '../../context/NotificationContext'
import { hapticTap } from '../../lib/haptics'
import { ExaIcon, type ExaIconName } from '../icons'
import { Button, Pill, ScreenHeader, Segmented } from '../ui'

type Category = 'payment' | 'subscription' | 'device' | 'referral' | 'support_ticket' | 'system_maintenance'
type Severity = 'info' | 'warning' | 'error'

interface Notification {
    id: number
    category: Category
    severity: Severity
    title: string
    body: string
    payload_json: Record<string, string> | null
    status: 'unread' | 'read'
    created_at: string
}

const ICON: Record<Category, ExaIconName> = {
    payment: 'pay',
    subscription: 'shield',
    device: 'devices',
    referral: 'gift',
    support_ticket: 'support',
    system_maintenance: 'server',
}

type Tab = 'all' | 'unread' | 'read'
const POLL_MS = 60_000

/** Список уведомлений: три фильтра, иконки категорий из набора EXA,
 *  непрочитанные помечены эмбером. Открывается по колокольчику на главном. */
export default function Notifications() {
    const { t, i18n } = useTranslation()
    const navigate = useNavigate()
    const { token } = useAuth()
    const { refreshUnreadCount } = useNotifications()
    const [items, setItems] = useState<Notification[]>([])
    const [loading, setLoading] = useState(true)
    const [tab, setTab] = useState<Tab>('all')

    const load = useCallback(
        async (signal?: AbortSignal) => {
            if (!token) return
            try {
                const status = tab === 'all' ? '' : `status=${tab}&`
                const res = await fetch(apiUrl(`/api/client/notifications?${status}limit=50&offset=0`), {
                    headers: { Authorization: `Bearer ${token}` },
                    signal,
                })
                if (res.ok) {
                    const data = await res.json()
                    setItems(Array.isArray(data) ? data : [])
                }
            } catch {
                /* сеть — покажем то, что есть */
            } finally {
                if (!signal?.aborted) setLoading(false)
            }
        },
        [token, tab],
    )

    useEffect(() => {
        setLoading(true)
        const ctrl = new AbortController()
        void load(ctrl.signal)
        const timer = setInterval(() => void load(ctrl.signal), POLL_MS)
        return () => {
            ctrl.abort()
            clearInterval(timer)
        }
    }, [load])

    const openOne = async (n: Notification) => {
        hapticTap()
        if (n.status === 'unread' && token) {
            void fetch(apiUrl(`/api/client/notifications/${n.id}/read`), { method: 'POST', headers: { Authorization: `Bearer ${token}` } }).catch(() => {})
            setItems((list) => list.map((x) => (x.id === n.id ? { ...x, status: 'read' } : x)))
            refreshUnreadCount()
        }
        const url = n.payload_json?.url
        if (url) navigate(url)
    }

    const markAll = async () => {
        if (!token) return
        hapticTap()
        void fetch(apiUrl('/api/client/notifications/read-all'), { method: 'POST', headers: { Authorization: `Bearer ${token}` } }).catch(() => {})
        setItems((list) => list.map((x) => ({ ...x, status: 'read' })))
        refreshUnreadCount()
    }

    const unread = items.filter((n) => n.status === 'unread').length
    const shown = tab === 'all' ? items : items.filter((n) => n.status === tab)
    const when = (iso: string) => {
        const mins = Math.max(0, Math.floor((Date.now() - new Date(iso).getTime()) / 60000))
        if (mins < 2) return t('exa.devices.now')
        if (mins < 60) return t('exa.devices.minutesAgo', { count: mins })
        const h = Math.floor(mins / 60)
        if (h < 24) return t('exa.devices.hoursAgo', { count: h })
        return new Intl.DateTimeFormat(i18n.language, { day: 'numeric', month: 'short' }).format(new Date(iso))
    }

    return (
        <div className="exa-screen">
            <ScreenHeader title={t('exa.notifications.title')} aside={unread > 0 ? t('exa.notifications.unread', { count: unread }) : undefined} />
            <Segmented
                value={tab}
                onChange={setTab}
                options={[
                    { value: 'all', label: t('exa.notifications.all') },
                    { value: 'unread', label: t('exa.notifications.unreadTab') },
                    { value: 'read', label: t('exa.notifications.archive') },
                ]}
            />
            {unread > 0 ? (
                <Button variant="ghost" size="md" onClick={() => void markAll()}>
                    {t('exa.notifications.markAll')}
                </Button>
            ) : null}
            {loading && items.length === 0 ? <div className="exa-loading">{t('exa.common.loading')}</div> : null}
            {!loading && shown.length === 0 ? (
                <div className="exa-empty" style={{ gap: 12 }}>
                    <ExaIcon name="bell" size={40} style={{ color: 'var(--exa-hint)' }} />
                    <div className="exa-empty__text">{t('exa.notifications.empty')}</div>
                </div>
            ) : null}
            {shown.length > 0 ? (
                <section className="exa-card exa-card--list">
                    {shown.map((n) => (
                        <button key={n.id} type="button" className="exa-row is-tappable" style={{ alignItems: 'flex-start', minHeight: 64 }} onClick={() => void openOne(n)}>
                            <span className="exa-device-avatar" style={{ color: n.severity === 'error' ? 'var(--exa-danger)' : n.severity === 'warning' ? 'var(--exa-warning)' : 'var(--exa-text)' }}>
                                <ExaIcon name={ICON[n.category] ?? 'bell'} size={22} />
                            </span>
                            <span className="exa-row__body" style={{ gap: 3 }}>
                                <span className="exa-row__title">
                                    <span style={{ whiteSpace: 'normal', fontWeight: n.status === 'unread' ? 600 : 500 }}>{n.title}</span>
                                    {n.status === 'unread' ? <Pill tone="accent">{t('exa.notifications.new')}</Pill> : null}
                                </span>
                                <span className="exa-row__meta" style={{ display: 'block', whiteSpace: 'normal', lineHeight: 1.4 }}>
                                    {n.body}
                                </span>
                                <span className="exa-row__meta" style={{ fontSize: 12 }}>{when(n.created_at)}</span>
                            </span>
                        </button>
                    ))}
                </section>
            ) : null}
        </div>
    )
}

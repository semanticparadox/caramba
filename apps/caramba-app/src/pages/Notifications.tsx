import { useEffect, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import Icon from '../components/Icon'
import { useNavigate } from 'react-router-dom'
import { apiUrl } from '../config'
import { useAuth } from '../context/AuthContext'
import { useNotifications } from '../context/NotificationContext'
import './Notifications.css'

// Интервал авто-обновления списка
const POLL_MS = 60_000

type NotifSeverity = 'info' | 'warning' | 'error'
type NotifCategory =
    | 'payment'
    | 'subscription'
    | 'device'
    | 'referral'
    | 'support_ticket'
    | 'system_maintenance'

interface Notification {
    id: number
    category: NotifCategory
    severity: NotifSeverity
    title: string
    body: string
    payload_json: Record<string, string> | null
    status: 'unread' | 'read'
    created_at: string
    read_at: string | null
}

// Аббревиатуры-иконки для категорий (без внешних зависимостей)
// Ключи, а не готовые метки: «PAY»/«SUB» ничего не говорят русскому
// пользователю, а сокращения в кружке должны читаться на его языке.
const CATEGORY_ABBR_KEY: Record<NotifCategory, string> = {
    payment: 'notifications.abbrPayment',
    subscription: 'notifications.abbrSubscription',
    device: 'notifications.abbrDevice',
    referral: 'notifications.abbrReferral',
    support_ticket: 'notifications.abbrTicket',
    system_maintenance: 'notifications.abbrSystem',
}

function relativeTime(iso: string, t: (key: string, opts?: object) => string): string {
    const diff = Date.now() - new Date(iso).getTime()
    const minutes = Math.floor(diff / 60_000)
    if (minutes < 2) return t('notifications.timeJustNow')
    if (minutes < 60) return t('notifications.timeMinutes', { count: minutes })
    const hours = Math.floor(minutes / 60)
    if (hours < 24) return t('notifications.timeHours', { count: hours })
    const days = Math.floor(hours / 24)
    return t('notifications.timeDays', { count: days })
}

type TabFilter = 'all' | 'unread' | 'read'

export default function Notifications() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { token } = useAuth()
    const { refreshUnreadCount } = useNotifications()

    const [notifications, setNotifications] = useState<Notification[]>([])
    const [loading, setLoading] = useState(true)
    const [error, setError] = useState<string | null>(null)
    const [tab, setTab] = useState<TabFilter>('all')

    // Загружаем уведомления с заданным фильтром
    const fetchNotifications = useCallback(
        async (signal?: AbortSignal) => {
            if (!token) return
            try {
                const statusParam = tab === 'all' ? '' : `status=${tab}&`
                const res = await fetch(
                    apiUrl(`/api/client/notifications?${statusParam}limit=50&offset=0`),
                    { headers: { Authorization: `Bearer ${token}` }, signal },
                )
                if (signal?.aborted) return
                if (res.ok) {
                    const data = await res.json()
                    setNotifications(Array.isArray(data) ? data : [])
                    setError(null)
                } else {
                    setError(t('notifications.fetchError'))
                }
            } catch (e: any) {
                if (e?.name === 'AbortError') return
                setError(t('notifications.fetchError'))
            } finally {
                setLoading(false)
            }
        },
        [token, tab, t],
    )

    // Первичная загрузка + авто-обновление
    useEffect(() => {
        setLoading(true)
        const controller = new AbortController()
        void fetchNotifications(controller.signal)

        const timer = setInterval(() => {
            void fetchNotifications(controller.signal)
        }, POLL_MS)

        return () => {
            controller.abort()
            clearInterval(timer)
        }
    }, [fetchNotifications])

    const handleMarkRead = async (notif: Notification) => {
        if (!token) return
        // Если уже прочитано — не делаем лишний запрос
        if (notif.status === 'read') {
            // Переходим по ссылке из payload_json если есть
            const url = notif.payload_json?.url
            if (url) navigate(url)
            return
        }

        try {
            await fetch(apiUrl(`/api/client/notifications/${notif.id}/read`), {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}` },
            })
        } catch { /* тихая ошибка */ }

        // Оптимистичное обновление
        setNotifications((prev) =>
            prev.map((n) => (n.id === notif.id ? { ...n, status: 'read' } : n)),
        )
        refreshUnreadCount()

        const url = notif.payload_json?.url
        if (url) navigate(url)
    }

    const handleMarkAll = async () => {
        if (!token) return
        try {
            await fetch(apiUrl('/api/client/notifications/read-all'), {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}` },
            })
        } catch { /* тихая ошибка */ }

        setNotifications((prev) => prev.map((n) => ({ ...n, status: 'read' })))
        refreshUnreadCount()
    }

    const unreadCount = notifications.filter((n) => n.status === 'unread').length

    // Клиентская фильтрация для вкладки «Архив» (прочитанные) = 'read'
    const displayed = tab === 'all'
        ? notifications
        : notifications.filter((n) => n.status === tab)

    return (
        <div className="page notifications-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')} aria-label={t('common.cancel')}>
                    <Icon name="chevron-left" />
                </button>
                <h2>
                    {t('notifications.title')}
                    {unreadCount > 0 && (
                        <span className="unread-badge">{unreadCount}</span>
                    )}
                </h2>
                <div style={{ width: 38 }} />
            </header>

            {/* Фильтр-табы */}
            <div className="notif-tabs">
                <button
                    className={`notif-tab${tab === 'all' ? ' active' : ''}`}
                    onClick={() => setTab('all')}
                >
                    {t('notifications.tabAll')}
                </button>
                <button
                    className={`notif-tab${tab === 'unread' ? ' active' : ''}`}
                    onClick={() => setTab('unread')}
                >
                    {t('notifications.tabUnread')}
                </button>
                <button
                    className={`notif-tab${tab === 'read' ? ' active' : ''}`}
                    onClick={() => setTab('read')}
                >
                    {t('notifications.tabArchive')}
                </button>
            </div>

            {/* Кнопка «Прочитать все» */}
            {unreadCount > 0 && (
                <div className="notif-mark-all-row">
                    <button className="notif-mark-all-btn" onClick={() => void handleMarkAll()}>
                        {t('notifications.markAllRead')}
                    </button>
                </div>
            )}

            {/* Ошибка загрузки */}
            {error && (
                <div className="home-banner error">{error}</div>
            )}

            {/* Лоадер */}
            {loading && !error && (
                <div className="loading">{t('app.loading')}</div>
            )}

            {/* Пустое состояние */}
            {!loading && !error && displayed.length === 0 && (
                <div className="empty-state glass-card">
                    <div className="empty-icon">🔔</div>
                    <h3>{t('notifications.emptyTitle')}</h3>
                    <p>{t('notifications.emptyDesc')}</p>
                </div>
            )}

            {/* Список уведомлений */}
            {!loading && displayed.length > 0 && (
                <ul className="notif-list" style={{ listStyle: 'none', padding: 0 }}>
                    {displayed.map((notif) => (
                        <li key={notif.id}>
                            <button
                                className={`notif-item${notif.status === 'unread' ? ' is-unread' : ''}`}
                                onClick={() => void handleMarkRead(notif)}
                            >
                                <div className="notif-cat-icon">
                                    {CATEGORY_ABBR_KEY[notif.category]
                                        ? t(CATEGORY_ABBR_KEY[notif.category])
                                        : notif.category.slice(0, 3).toUpperCase()}
                                </div>
                                <div className="notif-content">
                                    <span className="notif-title">{notif.title}</span>
                                    <span className="notif-body">{notif.body}</span>
                                </div>
                                <div className="notif-meta">
                                    <span className={`notif-dot ${notif.severity}`} />
                                    <span className="notif-time">
                                        {relativeTime(notif.created_at, t)}
                                    </span>
                                </div>
                            </button>
                        </li>
                    ))}
                </ul>
            )}

            {/* Ссылка на настройки уведомлений */}
            <button
                className="notif-prefs-link"
                onClick={() => navigate('/notifications/preferences')}
            >
                <span>{t('notifications.preferencesLink')}</span>
                <span><Icon name="chevron-right" size={14} /></span>
            </button>
        </div>
    )
}

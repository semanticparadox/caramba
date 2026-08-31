import { useEffect, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import Icon from '../components/Icon'
import { useNavigate } from 'react-router-dom'
import { apiUrl } from '../config'
import { useAuth } from '../context/AuthContext'
import './Tickets.css'

const POLL_MS = 60_000

export type TicketStatus = 'open' | 'in_progress' | 'awaiting_user' | 'resolved' | 'closed'

export interface TicketSummary {
    id: number
    category: string
    subject: string
    status: TicketStatus
    created_at: string
    updated_at: string
    last_message_preview: string | null
    unread_for_user: boolean
}

function relativeTime(iso: string, t: (key: string, opts?: object) => string): string {
    const diff = Date.now() - new Date(iso).getTime()
    const minutes = Math.floor(diff / 60_000)
    if (minutes < 2) return t('tickets.timeJustNow')
    if (minutes < 60) return t('tickets.timeMinutes', { count: minutes })
    const hours = Math.floor(minutes / 60)
    if (hours < 24) return t('tickets.timeHours', { count: hours })
    const days = Math.floor(hours / 24)
    return t('tickets.timeDays', { count: days })
}

export default function Tickets() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { token } = useAuth()

    const [tickets, setTickets] = useState<TicketSummary[]>([])
    const [loading, setLoading] = useState(true)
    const [error, setError] = useState<string | null>(null)

    const fetchTickets = useCallback(
        async (signal?: AbortSignal) => {
            if (!token) return
            try {
                const res = await fetch(apiUrl('/api/client/tickets'), {
                    headers: { Authorization: `Bearer ${token}` },
                    signal,
                })
                if (signal?.aborted) return
                if (res.ok) {
                    const data = await res.json()
                    setTickets(Array.isArray(data) ? data : [])
                    setError(null)
                } else {
                    setError(t('tickets.fetchError'))
                }
            } catch (e: any) {
                if (e?.name === 'AbortError') return
                setError(t('tickets.fetchError'))
            } finally {
                setLoading(false)
            }
        },
        [token, t],
    )

    useEffect(() => {
        setLoading(true)
        const controller = new AbortController()
        void fetchTickets(controller.signal)

        const timer = setInterval(() => void fetchTickets(controller.signal), POLL_MS)

        return () => {
            controller.abort()
            clearInterval(timer)
        }
    }, [fetchTickets])

    return (
        <div className="page tickets-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/support')} aria-label={t('common.cancel')}>
                    <Icon name="chevron-left" />
                </button>
                <h2>{t('tickets.title')}</h2>
                <div style={{ width: 38 }} />
            </header>

            {error && <div className="home-banner error">{error}</div>}

            {loading && !error && (
                <div className="loading">{t('app.loading')}</div>
            )}

            {!loading && !error && tickets.length === 0 && (
                <div className="empty-state glass-card">
                    <div className="empty-icon">🎫</div>
                    <h3>{t('tickets.emptyTitle')}</h3>
                    <p>{t('tickets.emptyDesc')}</p>
                </div>
            )}

            {!loading && tickets.length > 0 && (
                <ul className="tickets-list" style={{ listStyle: 'none', padding: 0 }}>
                    {tickets.map((ticket) => (
                        <li key={ticket.id}>
                            <button
                                className={`ticket-row${ticket.unread_for_user ? ' has-unread' : ''}`}
                                onClick={() => navigate(`/tickets/${ticket.id}`)}
                            >
                                <div className="ticket-row-head">
                                    <span className="ticket-subject">{ticket.subject}</span>
                                    <span className={`ticket-status-pill ${ticket.status}`}>
                                        {t(`tickets.status_${ticket.status}`)}
                                    </span>
                                </div>
                                {ticket.last_message_preview && (
                                    <span className="ticket-preview">{ticket.last_message_preview}</span>
                                )}
                                <div className="ticket-row-foot">
                                    <span className="ticket-time">
                                        {relativeTime(ticket.updated_at, t)}
                                    </span>
                                    <span style={{ fontSize: '0.68rem', color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                                        {t(`tickets.category_${ticket.category}` as const)}
                                    </span>
                                </div>
                            </button>
                        </li>
                    ))}
                </ul>
            )}

            {/* Плавающая кнопка создания тикета */}
            <button
                className="fab-new-ticket"
                onClick={() => navigate('/tickets/new')}
            >
                + {t('tickets.newTicket')}
            </button>
        </div>
    )
}

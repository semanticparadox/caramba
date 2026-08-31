import { useEffect, useRef, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import Icon from '../components/Icon'
import { useNavigate, useParams } from 'react-router-dom'
import { apiUrl } from '../config'
import { useAuth } from '../context/AuthContext'
import type { TicketStatus } from './Tickets'
import './Tickets.css'

// Автообновление переписки
const POLL_MS = 15_000

type SenderRole = 'user' | 'admin' | 'system'

interface AttachmentJson {
    id?: number
    url?: string
    filename?: string
    content_type?: string
}

interface TicketMessage {
    id: number
    sender_role: SenderRole
    body: string
    attachments_json: AttachmentJson[] | null
    created_at: string
}

interface TicketDetail {
    id: number
    category: string
    subject: string
    status: TicketStatus
    created_at: string
    updated_at: string
}

// Максимальный размер вложения 10 МБ (байт)
const MAX_ATTACH_BYTES = 10 * 1024 * 1024
const ALLOWED_TYPES = ['image/png', 'image/jpeg', 'image/webp', 'application/pdf']

interface PendingAttachment {
    id: number  // attachment_id из POST /attach
    filename: string
    content_type: string
}

export default function TicketDetail() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { id } = useParams<{ id: string }>()
    const { token } = useAuth()

    const [ticket, setTicket] = useState<TicketDetail | null>(null)
    const [messages, setMessages] = useState<TicketMessage[]>([])
    const [loading, setLoading] = useState(true)
    const [error, setError] = useState<string | null>(null)

    // Reply form state
    const [replyBody, setReplyBody] = useState('')
    const [pendingAttachments, setPendingAttachments] = useState<PendingAttachment[]>([])
    const [uploading, setUploading] = useState(false)
    const [sending, setSending] = useState(false)
    const [replyError, setReplyError] = useState<string | null>(null)
    const fileInputRef = useRef<HTMLInputElement>(null)
    const messagesBottomRef = useRef<HTMLDivElement>(null)

    // Последний известный ID сообщения — для оптимизации обновлений
    const lastMsgIdRef = useRef<number>(0)

    const fetchDetail = useCallback(
        async (signal?: AbortSignal, silent = false) => {
            if (!token || !id) return
            try {
                const res = await fetch(apiUrl(`/api/client/tickets/${id}`), {
                    headers: { Authorization: `Bearer ${token}` },
                    signal,
                })
                if (signal?.aborted) return
                if (res.ok) {
                    const data = await res.json()
                    setTicket(data.ticket ?? null)
                    const msgs: TicketMessage[] = Array.isArray(data.messages) ? data.messages : []
                    setMessages(msgs)
                    if (msgs.length > 0) {
                        lastMsgIdRef.current = msgs[msgs.length - 1].id
                    }
                    setError(null)
                } else {
                    if (!silent) setError(t('tickets.fetchError'))
                }
            } catch (e: any) {
                if (e?.name === 'AbortError') return
                if (!silent) setError(t('tickets.fetchError'))
            } finally {
                if (!silent) setLoading(false)
            }
        },
        [token, id, t],
    )

    useEffect(() => {
        setLoading(true)
        const controller = new AbortController()
        void fetchDetail(controller.signal, false)

        // Авто-обновление: тихое (silent = true) чтобы не показывать лоадер
        const timer = setInterval(() => void fetchDetail(controller.signal, true), POLL_MS)

        return () => {
            controller.abort()
            clearInterval(timer)
        }
    }, [fetchDetail])

    // Скролл вниз при появлении новых сообщений
    useEffect(() => {
        messagesBottomRef.current?.scrollIntoView({ behavior: 'smooth' })
    }, [messages.length])

    const handleAttachFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
        const file = e.target.files?.[0]
        if (!file || !token || !id) return

        // Сбрасываем input чтобы можно было повторно выбрать тот же файл
        e.target.value = ''

        if (!ALLOWED_TYPES.includes(file.type)) {
            setReplyError(t('tickets.attachTypeError'))
            return
        }
        if (file.size > MAX_ATTACH_BYTES) {
            setReplyError(t('tickets.attachSizeError'))
            return
        }

        setUploading(true)
        setReplyError(null)

        try {
            const form = new FormData()
            form.append('file', file)

            const res = await fetch(apiUrl(`/api/client/tickets/${id}/attach`), {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}` },
                body: form,
            })

            if (res.ok) {
                const data = await res.json()
                setPendingAttachments((prev) => [
                    ...prev,
                    {
                        id: data.attachment_id,
                        filename: file.name,
                        content_type: file.type,
                    },
                ])
            } else {
                setReplyError(t('tickets.attachUploadError'))
            }
        } catch {
            setReplyError(t('tickets.attachUploadError'))
        } finally {
            setUploading(false)
        }
    }

    const removeAttachment = (attachId: number) => {
        setPendingAttachments((prev) => prev.filter((a) => a.id !== attachId))
    }

    const handleSend = async () => {
        if (!token || !id) return
        const bodyTrimmed = replyBody.trim()
        if (!bodyTrimmed) return

        setSending(true)
        setReplyError(null)

        try {
            const res = await fetch(apiUrl(`/api/client/tickets/${id}/messages`), {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    body: bodyTrimmed,
                    attachment_ids: pendingAttachments.map((a) => a.id),
                }),
            })

            if (res.ok) {
                setReplyBody('')
                setPendingAttachments([])
                // Обновляем переписку сразу после отправки
                void fetchDetail(undefined, true)
            } else {
                const errText = await res.text()
                setReplyError(errText || t('tickets.sendError'))
            }
        } catch {
            setReplyError(t('tickets.sendError'))
        } finally {
            setSending(false)
        }
    }

    const isClosed = ticket?.status === 'closed' || ticket?.status === 'resolved'

    function msgBubbleClass(role: SenderRole): string {
        if (role === 'user') return 'ticket-msg user-msg'
        if (role === 'system') return 'ticket-msg system-msg'
        return 'ticket-msg admin-msg'
    }

    function formatMsgTime(iso: string): string {
        const d = new Date(iso)
        return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
    }

    return (
        <div className="page ticket-detail-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/tickets')} aria-label={t('common.cancel')}>
                    <Icon name="chevron-left" />
                </button>
                <h2 style={{ fontSize: '0.95rem' }}>
                    {ticket ? ticket.subject : t('tickets.title')}
                </h2>
                <div style={{ width: 38 }} />
            </header>

            {error && <div className="home-banner error">{error}</div>}
            {loading && !error && <div className="loading">{t('app.loading')}</div>}

            {ticket && (
                <section className="ticket-detail-header glass-card">
                    <span className="ticket-detail-title">{ticket.subject}</span>
                    <div className="ticket-detail-meta">
                        <span className={`ticket-status-pill ${ticket.status}`}>
                            {t(`tickets.status_${ticket.status}`)}
                        </span>
                        <span>{t(`tickets.category_${ticket.category}`)}</span>
                        <span>{new Date(ticket.created_at).toLocaleDateString()}</span>
                    </div>
                </section>
            )}

            {messages.length > 0 && (
                <div className="ticket-messages">
                    {messages.map((msg) => (
                        <div key={msg.id} className={msgBubbleClass(msg.sender_role)}>
                            <div className="ticket-msg-bubble">
                                {msg.body}
                                {/* Вложения */}
                                {Array.isArray(msg.attachments_json) && msg.attachments_json.length > 0 && (
                                    <div className="ticket-msg-attachments">
                                        {msg.attachments_json.map((att, i) => {
                                            if (!att.url) return null
                                            const isImage = att.content_type?.startsWith('image/')
                                            if (isImage) {
                                                return (
                                                    <a key={i} href={att.url} target="_blank" rel="noopener noreferrer">
                                                        <img
                                                            className="ticket-attach-img"
                                                            src={att.url}
                                                            alt={att.filename ?? t('tickets.attachmentAlt')}
                                                        />
                                                    </a>
                                                )
                                            }
                                            return (
                                                <a
                                                    key={i}
                                                    className="ticket-attach-link"
                                                    href={att.url}
                                                    target="_blank"
                                                    rel="noopener noreferrer"
                                                >
                                                    PDF {att.filename ?? t('tickets.attachment')}
                                                </a>
                                            )
                                        })}
                                    </div>
                                )}
                            </div>
                            <span className="ticket-msg-time">{formatMsgTime(msg.created_at)}</span>
                        </div>
                    ))}
                    <div ref={messagesBottomRef} />
                </div>
            )}

            {/* Reply-форма или сообщение о закрытии */}
            {isClosed ? (
                <div className="ticket-closed-note">
                    {t('tickets.closedNote')}
                </div>
            ) : (
                <div className="ticket-reply-wrap">
                    {/* Превью прикреплённых файлов */}
                    {pendingAttachments.length > 0 && (
                        <div className="ticket-pending-attachments">
                            {pendingAttachments.map((att) => (
                                <span key={att.id} className="ticket-pending-attach-chip">
                                    {att.filename}
                                    <button
                                        type="button"
                                        onClick={() => removeAttachment(att.id)}
                                        aria-label={t('common.close')}
                                    >
                                        x
                                    </button>
                                </span>
                            ))}
                        </div>
                    )}

                    {replyError && (
                        <div className="ticket-reply-error">{replyError}</div>
                    )}

                    <div className="ticket-reply-row">
                        <textarea
                            className="ticket-reply-textarea"
                            placeholder={t('tickets.replyPlaceholder')}
                            value={replyBody}
                            onChange={(e) => setReplyBody(e.target.value)}
                            rows={1}
                            disabled={sending}
                            onKeyDown={(e) => {
                                if (e.key === 'Enter' && !e.shiftKey) {
                                    e.preventDefault()
                                    void handleSend()
                                }
                            }}
                        />
                        <button
                            type="button"
                            className="ticket-reply-attach-btn"
                            onClick={() => fileInputRef.current?.click()}
                            disabled={uploading || sending}
                            title={t('tickets.attachFile')}
                        >
                            {uploading ? '...' : '@'}
                        </button>
                        <button
                            type="button"
                            className="ticket-reply-send-btn"
                            onClick={() => void handleSend()}
                            disabled={!replyBody.trim() || sending}
                        >
                            <Icon name="chevron-right" size={14} />
                        </button>
                    </div>

                    <input
                        ref={fileInputRef}
                        type="file"
                        accept={ALLOWED_TYPES.join(',')}
                        style={{ display: 'none' }}
                        onChange={(e) => void handleAttachFile(e)}
                    />
                </div>
            )}
        </div>
    )
}

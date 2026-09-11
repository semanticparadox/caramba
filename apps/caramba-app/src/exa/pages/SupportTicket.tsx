import { useCallback, useEffect, useRef, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useAuth } from '../../context/AuthContext'
import { useNotifications } from '../../context/NotificationContext'
import { hapticError, hapticSuccess, hapticTap } from '../../lib/haptics'
import { ExaIcon } from '../icons'
import { Button, Pill, ScreenHeader } from '../ui'
import { useToast } from '../lib/useToast'
import { fetchTicket, replyTicket, ticketIsOpen, type TicketDetail, type TicketMessage } from '../lib/supportApi'

/** Пока тикет открыт, переписка перечитывается раз в 15 секунд: push в
 *  мини-аппе нет, а ответ поддержки должен появиться без переоткрытия. */
const POLL_MS = 15_000
const BODY_MAX = 5000

/** «Запросы в поддержку › тикет»: переписка и ответ. Сюда же ведёт тап по
 *  уведомлению «Ответ по тикету» (`payload_json.url = /support/{id}`). */
export default function SupportTicket() {
    const { t, i18n } = useTranslation()
    const navigate = useNavigate()
    const toast = useToast()
    const { id } = useParams()
    const ticketId = Number(id)
    const { token } = useAuth()
    const { refreshUnreadCount } = useNotifications()
    const [detail, setDetail] = useState<TicketDetail | null>(null)
    const [missing, setMissing] = useState(false)
    const [loading, setLoading] = useState(true)
    const [draft, setDraft] = useState('')
    const [busy, setBusy] = useState(false)
    const [flash, setFlash] = useState(false)
    const seenSupport = useRef<number | null>(null)
    const endRef = useRef<HTMLDivElement>(null)

    const load = useCallback(
        async (signal?: AbortSignal) => {
            if (!token || !Number.isFinite(ticketId) || ticketId <= 0) {
                setMissing(true)
                setLoading(false)
                return
            }
            try {
                const next = await fetchTicket(token, ticketId, signal)
                if (signal?.aborted) return
                if (!next) {
                    setMissing(true)
                    return
                }
                // Новый ответ поддержки между опросами — плашка и прокрутка вниз.
                const support = next.messages.filter((m) => m.sender_role !== 'user').length
                const seen = seenSupport.current
                seenSupport.current = support
                if (seen !== null && support > seen) {
                    setFlash(true)
                    window.setTimeout(() => setFlash(false), 4000)
                    window.setTimeout(() => endRef.current?.scrollIntoView({ behavior: 'smooth' }), 50)
                }
                setDetail(next)
                // Первое открытие: панель отметила тикет прочитанным, а
                // уведомление могло погаснуть по тапу — обновляем колокольчик
                // один раз, не на каждом опросе.
                if (seen === null) refreshUnreadCount()
            } catch {
                /* сеть — оставляем предыдущую ленту */
            } finally {
                if (!signal?.aborted) setLoading(false)
            }
        },
        [token, ticketId, refreshUnreadCount],
    )

    useEffect(() => {
        const ctrl = new AbortController()
        void load(ctrl.signal)
        return () => ctrl.abort()
    }, [load])

    const status = detail?.ticket.status
    const open = status ? ticketIsOpen(status) : false

    useEffect(() => {
        if (!open) return
        const ctrl = new AbortController()
        const timer = window.setInterval(() => void load(ctrl.signal), POLL_MS)
        return () => {
            ctrl.abort()
            window.clearInterval(timer)
        }
    }, [open, load])

    const send = async () => {
        const body = draft.trim()
        if (!token || !body || busy || !detail) return
        setBusy(true)
        try {
            const result = await replyTicket(token, ticketId, body.slice(0, BODY_MAX))
            if (result === 'ok') {
                hapticSuccess()
                setDraft('')
                await load()
                window.setTimeout(() => endRef.current?.scrollIntoView({ behavior: 'smooth' }), 50)
            } else {
                hapticError()
                toast(result === 'closed' ? t('exa.support.closedHint') : t('exa.common.error'))
                if (result === 'closed') await load()
            }
        } finally {
            setBusy(false)
        }
    }

    const time = (iso: string) =>
        new Intl.DateTimeFormat(i18n.language, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' }).format(new Date(iso))

    const bubble = (m: TicketMessage) => {
        const mine = m.sender_role === 'user'
        return (
            <div key={m.id} style={{ display: 'flex', flexDirection: 'column', alignItems: mine ? 'flex-end' : 'flex-start', gap: 3 }}>
                <div
                    style={{
                        maxWidth: '82%',
                        padding: '10px 12px',
                        borderRadius: 14,
                        background: mine ? 'var(--exa-elevated)' : 'var(--exa-surface)',
                        border: '1px solid var(--exa-border-strong)',
                        whiteSpace: 'pre-wrap',
                        wordBreak: 'break-word',
                        fontSize: 'var(--exa-text-md)',
                    }}
                >
                    {m.body}
                </div>
                <span className="exa-row__meta" style={{ fontSize: 12 }}>
                    {mine ? null : `${t('exa.support.fromSupport')} · `}
                    {time(m.created_at)}
                </span>
            </div>
        )
    }

    return (
        <div className="exa-screen">
            <ScreenHeader title={detail?.ticket.subject || t('exa.support.ticketTitle', { id: ticketId })} aside={status ? <Pill tone={open ? 'accent' : 'neutral'}>{t(`exa.support.status.${status}`)}</Pill> : undefined} />
            <Button
                variant="ghost"
                size="sm"
                block={false}
                icon={<ExaIcon name="back" size={18} />}
                onClick={() => {
                    hapticTap()
                    navigate('/support')
                }}
            >
                {t('exa.support.backToList')}
            </Button>

            {flash ? (
                <div className="exa-muted" style={{ textAlign: 'center' }}>
                    {t('exa.support.newReply')}
                </div>
            ) : null}

            {loading && !detail ? <div className="exa-loading">{t('exa.common.loading')}</div> : null}
            {missing ? (
                <div className="exa-empty" style={{ gap: 12 }}>
                    <ExaIcon name="support" size={40} style={{ color: 'var(--exa-hint)' }} />
                    <div className="exa-empty__text">{t('exa.support.notFound')}</div>
                </div>
            ) : null}

            {detail ? (
                <section className="exa-card" style={{ display: 'grid', gap: 12 }}>
                    {detail.messages.length === 0 ? <div className="exa-muted">{t('exa.support.noMessages')}</div> : detail.messages.map(bubble)}
                    <div ref={endRef} />
                </section>
            ) : null}

            {detail && open ? (
                <section className="exa-card" style={{ display: 'grid', gap: 8 }}>
                    <textarea
                        className="exa-rename"
                        style={{ width: '100%', height: 90, padding: 10, resize: 'vertical' }}
                        placeholder={t('exa.support.replyPlaceholder')}
                        value={draft}
                        maxLength={BODY_MAX}
                        onChange={(e) => setDraft(e.target.value)}
                        disabled={busy}
                    />
                    <Button size="md" onClick={() => void send()} disabled={busy || draft.trim().length === 0}>
                        {t('exa.support.send')}
                    </Button>
                </section>
            ) : null}
            {detail && !open ? <div className="exa-muted" style={{ textAlign: 'center' }}>{t('exa.support.closedHint')}</div> : null}
        </div>
    )
}

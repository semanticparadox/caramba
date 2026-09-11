import { useCallback, useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useAuth } from '../../context/AuthContext'
import { hapticError, hapticSuccess, hapticTap } from '../../lib/haptics'
import { ExaIcon } from '../icons'
import { Button, Pill, ScreenHeader } from '../ui'
import { useToast } from '../lib/useToast'
import {
    TICKET_CATEGORIES,
    createTicket,
    fetchTickets,
    type TicketCategory,
    type TicketStatus,
    type TicketSummary,
} from '../lib/supportApi'

const SUBJECT_MAX = 200
const BODY_MAX = 5000

/** «Профиль › Запросы в поддержку»: список тикетов и форма нового запроса.
 *
 *  Тот же контракт, что у бота и Flutter-приложения, поэтому тикет, начатый
 *  здесь, виден везде, а ответ поддержки приходит и сюда, и в Telegram.
 *  Раньше `/support` молча редиректил в профиль, и ссылка из уведомления
 *  «Ответ по тикету» выбрасывала человека на главный экран. */
export default function Support() {
    const { t, i18n } = useTranslation()
    const navigate = useNavigate()
    const toast = useToast()
    const { token } = useAuth()
    const [tickets, setTickets] = useState<TicketSummary[]>([])
    const [loading, setLoading] = useState(true)
    const [composing, setComposing] = useState(false)
    const [category, setCategory] = useState<TicketCategory>('general')
    const [subject, setSubject] = useState('')
    const [body, setBody] = useState('')
    const [busy, setBusy] = useState(false)

    const load = useCallback(
        async (signal?: AbortSignal) => {
            if (!token) {
                setLoading(false)
                return
            }
            try {
                setTickets(await fetchTickets(token, signal))
            } catch {
                /* сеть — покажем то, что есть */
            } finally {
                if (!signal?.aborted) setLoading(false)
            }
        },
        [token],
    )

    useEffect(() => {
        const ctrl = new AbortController()
        void load(ctrl.signal)
        return () => ctrl.abort()
    }, [load])

    const canSubmit = subject.trim().length > 0 && body.trim().length > 0 && !busy

    const submit = async () => {
        if (!token || !canSubmit) return
        setBusy(true)
        try {
            const created = await createTicket(token, {
                category,
                subject: subject.trim().slice(0, SUBJECT_MAX),
                body: body.trim().slice(0, BODY_MAX),
            })
            if (created) {
                hapticSuccess()
                toast(t('exa.support.sent'))
                setSubject('')
                setBody('')
                setComposing(false)
                navigate(`/support/${created.id}`)
            } else {
                hapticError()
                toast(t('exa.common.error'))
            }
        } catch {
            hapticError()
            toast(t('exa.common.error'))
        } finally {
            setBusy(false)
        }
    }

    const statusLabel = (s: TicketStatus) => t(`exa.support.status.${s}`)
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
            <ScreenHeader title={t('exa.support.title')} />
            <p className="exa-muted" style={{ margin: 0 }}>
                {t('exa.support.intro')}
            </p>

            {composing ? (
                <section className="exa-card" style={{ display: 'grid', gap: 10 }}>
                    <label className="exa-muted" htmlFor="ticket-category">
                        {t('exa.support.category')}
                    </label>
                    <select
                        id="ticket-category"
                        className="exa-rename"
                        style={{ width: '100%', height: 40 }}
                        value={category}
                        onChange={(e) => setCategory(e.target.value as TicketCategory)}
                        disabled={busy}
                    >
                        {TICKET_CATEGORIES.map((c) => (
                            <option key={c} value={c}>
                                {t(`exa.support.categories.${c}`)}
                            </option>
                        ))}
                    </select>
                    <input
                        className="exa-rename"
                        style={{ width: '100%', height: 40 }}
                        placeholder={t('exa.support.subjectPlaceholder')}
                        value={subject}
                        maxLength={SUBJECT_MAX}
                        onChange={(e) => setSubject(e.target.value)}
                        disabled={busy}
                    />
                    <textarea
                        className="exa-rename"
                        style={{ width: '100%', height: 120, padding: 10, resize: 'vertical' }}
                        placeholder={t('exa.support.bodyPlaceholder')}
                        value={body}
                        maxLength={BODY_MAX}
                        onChange={(e) => setBody(e.target.value)}
                        disabled={busy}
                    />
                    <div style={{ display: 'flex', gap: 8 }}>
                        <Button variant="secondary" size="md" onClick={() => setComposing(false)} disabled={busy}>
                            {t('exa.common.cancel')}
                        </Button>
                        <Button size="md" onClick={() => void submit()} disabled={!canSubmit}>
                            {t('exa.support.send')}
                        </Button>
                    </div>
                </section>
            ) : (
                <Button
                    size="md"
                    onClick={() => {
                        hapticTap()
                        setComposing(true)
                    }}
                >
                    {t('exa.support.newTicket')}
                </Button>
            )}

            {loading && tickets.length === 0 ? <div className="exa-loading">{t('exa.common.loading')}</div> : null}
            {!loading && tickets.length === 0 ? (
                <div className="exa-empty" style={{ gap: 12 }}>
                    <ExaIcon name="support" size={40} style={{ color: 'var(--exa-hint)' }} />
                    <div className="exa-empty__text">{t('exa.support.empty')}</div>
                </div>
            ) : null}
            {tickets.length > 0 ? (
                <section className="exa-card exa-card--list">
                    {tickets.map((tk) => (
                        <button
                            key={tk.id}
                            type="button"
                            className="exa-row is-tappable"
                            style={{ alignItems: 'flex-start', minHeight: 64 }}
                            onClick={() => {
                                hapticTap()
                                navigate(`/support/${tk.id}`)
                            }}
                        >
                            <span className="exa-row__body" style={{ gap: 3 }}>
                                <span className="exa-row__title">
                                    <span style={{ fontWeight: tk.unread_for_user > 0 ? 600 : 500 }}>{tk.subject}</span>
                                    {tk.unread_for_user > 0 ? <Pill tone="accent">{t('exa.support.new')}</Pill> : null}
                                </span>
                                {tk.last_message_preview ? (
                                    <span className="exa-row__meta" style={{ display: 'block', whiteSpace: 'normal', lineHeight: 1.4 }}>
                                        {tk.last_message_preview}
                                    </span>
                                ) : null}
                                <span className="exa-row__meta" style={{ fontSize: 12 }}>
                                    #{tk.id} · {statusLabel(tk.status)} · {when(tk.updated_at)}
                                </span>
                            </span>
                            <ExaIcon name="chevron" size={20} style={{ color: 'var(--exa-hint)' }} />
                        </button>
                    ))}
                </section>
            ) : null}
        </div>
    )
}

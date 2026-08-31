import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import Icon from '../components/Icon'
import { useNavigate } from 'react-router-dom'
import { apiUrl } from '../config'
import { useAuth } from '../context/AuthContext'
import './Tickets.css'

type TicketCategory = 'billing' | 'connection' | 'device' | 'feature_request' | 'other'

const CATEGORIES: TicketCategory[] = [
    'billing',
    'connection',
    'device',
    'feature_request',
    'other',
]

const BODY_MIN = 10

export default function TicketNew() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { token } = useAuth()

    const [category, setCategory] = useState<TicketCategory>('connection')
    const [subject, setSubject] = useState('')
    const [body, setBody] = useState('')
    const [submitting, setSubmitting] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const subjectTrimmed = subject.trim()
    const bodyTrimmed = body.trim()
    const isValid = subjectTrimmed.length > 0 && bodyTrimmed.length >= BODY_MIN

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault()
        if (!token || !isValid) return

        setSubmitting(true)
        setError(null)

        try {
            const res = await fetch(apiUrl('/api/client/tickets'), {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    category,
                    subject: subjectTrimmed,
                    body: bodyTrimmed,
                }),
            })

            if (res.ok) {
                const data = await res.json()
                // После создания — переходим на страницу тикета
                navigate(`/tickets/${data.id}`, { replace: true })
            } else {
                const errText = await res.text()
                setError(errText || t('tickets.createError'))
            }
        } catch {
            setError(t('tickets.createError'))
        } finally {
            setSubmitting(false)
        }
    }

    return (
        <div className="page ticket-new-page">
            <header className="page-header">
                <button
                    className="back-button"
                    onClick={() => navigate('/tickets')}
                    aria-label={t('common.cancel')}
                >
                    <Icon name="chevron-left" />
                </button>
                <h2>{t('tickets.newTitle')}</h2>
                <div style={{ width: 38 }} />
            </header>

            {error && <div className="home-banner error">{error}</div>}

            <form className="ticket-form glass-card" style={{ padding: 'var(--space-4)' }} onSubmit={(e) => void handleSubmit(e)}>
                <div className="form-field">
                    <label className="form-label" htmlFor="ticket-category">
                        {t('tickets.fieldCategory')}
                    </label>
                    <select
                        id="ticket-category"
                        className="form-select"
                        value={category}
                        onChange={(e) => setCategory(e.target.value as TicketCategory)}
                    >
                        {CATEGORIES.map((cat) => (
                            <option key={cat} value={cat}>
                                {t(`tickets.category_${cat}`)}
                            </option>
                        ))}
                    </select>
                </div>

                <div className="form-field">
                    <label className="form-label" htmlFor="ticket-subject">
                        {t('tickets.fieldSubject')}
                    </label>
                    <input
                        id="ticket-subject"
                        type="text"
                        placeholder={t('tickets.subjectPlaceholder')}
                        value={subject}
                        onChange={(e) => setSubject(e.target.value)}
                        maxLength={200}
                        required
                    />
                </div>

                <div className="form-field">
                    <label className="form-label" htmlFor="ticket-body">
                        {t('tickets.fieldBody')}
                    </label>
                    <textarea
                        id="ticket-body"
                        className="form-textarea"
                        placeholder={t('tickets.bodyPlaceholder')}
                        value={body}
                        onChange={(e) => setBody(e.target.value)}
                        rows={5}
                        required
                    />
                    {bodyTrimmed.length > 0 && bodyTrimmed.length < BODY_MIN && (
                        <span className="form-hint">
                            {t('tickets.bodyTooShort', { min: BODY_MIN })}
                        </span>
                    )}
                </div>

                <button
                    type="submit"
                    className="btn-primary"
                    disabled={submitting || !isValid}
                >
                    {submitting ? t('tickets.submitting') : t('tickets.submit')}
                </button>

                <button
                    type="button"
                    className="btn-ghost"
                    onClick={() => navigate('/tickets')}
                    disabled={submitting}
                >
                    {t('common.cancel')}
                </button>
            </form>
        </div>
    )
}

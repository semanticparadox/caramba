import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import Icon from '../components/Icon'
import { useNavigate } from 'react-router-dom'
import { apiUrl } from '../config'
import { useAuth } from '../context/AuthContext'
import './Notifications.css'

type Category =
    | 'payment'
    | 'subscription'
    | 'device'
    | 'referral'
    | 'support_ticket'
    | 'system_maintenance'

type Channel = 'bot_dm' | 'mini_app'

interface PrefRow {
    category: Category
    channel: Channel
    enabled: boolean
}

// Все известные категории и каналы (по контракту API)
const ALL_CATEGORIES: Category[] = [
    'payment',
    'subscription',
    'device',
    'referral',
    'support_ticket',
    'system_maintenance',
]

const ALL_CHANNELS: Channel[] = ['bot_dm', 'mini_app']

// Строим карту быстрого доступа: category+channel → enabled
type PrefMap = Map<string, boolean>

function makeKey(cat: Category, ch: Channel) {
    return `${cat}::${ch}`
}

function buildMap(prefs: PrefRow[]): PrefMap {
    const m = new Map<string, boolean>()
    for (const p of prefs) {
        m.set(makeKey(p.category, p.channel), p.enabled)
    }
    return m
}

function mapToArray(m: PrefMap): PrefRow[] {
    const result: PrefRow[] = []
    for (const cat of ALL_CATEGORIES) {
        for (const ch of ALL_CHANNELS) {
            result.push({
                category: cat,
                channel: ch,
                enabled: m.get(makeKey(cat, ch)) ?? true,
            })
        }
    }
    return result
}

export default function NotificationPreferences() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { token } = useAuth()

    const [prefMap, setPrefMap] = useState<PrefMap>(new Map())
    const [loading, setLoading] = useState(true)
    const [saving, setSaving] = useState(false)
    const [saveError, setSaveError] = useState<string | null>(null)
    const [saveSuccess, setSaveSuccess] = useState(false)

    useEffect(() => {
        if (!token) return
        const controller = new AbortController()

        const fetchPrefs = async () => {
            try {
                const res = await fetch(apiUrl('/api/client/notification-preferences'), {
                    headers: { Authorization: `Bearer ${token}` },
                    signal: controller.signal,
                })
                if (res.ok) {
                    const data: PrefRow[] = await res.json()
                    setPrefMap(buildMap(Array.isArray(data) ? data : []))
                }
            } catch (e: any) {
                if (e?.name === 'AbortError') return
            } finally {
                setLoading(false)
            }
        }

        void fetchPrefs()
        return () => controller.abort()
    }, [token])

    const toggle = (cat: Category, ch: Channel) => {
        const key = makeKey(cat, ch)
        setPrefMap((prev) => {
            const next = new Map(prev)
            next.set(key, !(prev.get(key) ?? true))
            return next
        })
    }

    const handleSave = async () => {
        if (!token) return
        setSaving(true)
        setSaveError(null)
        setSaveSuccess(false)

        const body = mapToArray(prefMap)
        try {
            const res = await fetch(apiUrl('/api/client/notification-preferences'), {
                method: 'PUT',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify(body),
            })
            if (res.ok) {
                setSaveSuccess(true)
                setTimeout(() => setSaveSuccess(false), 2500)
            } else {
                setSaveError(t('notifications.prefsSaveError'))
            }
        } catch {
            setSaveError(t('notifications.prefsSaveError'))
        } finally {
            setSaving(false)
        }
    }

    return (
        <div className="page notifications-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/notifications')} aria-label={t('common.cancel')}>
                    <Icon name="chevron-left" />
                </button>
                <h2>{t('notifications.prefsTitle')}</h2>
                <div style={{ width: 38 }} />
            </header>

            {saveSuccess && (
                <div className="home-banner success">{t('notifications.prefsSaved')}</div>
            )}
            {saveError && (
                <div className="home-banner error">{saveError}</div>
            )}

            {loading ? (
                <div className="loading">{t('app.loading')}</div>
            ) : (
                <section className="glass-card" style={{ padding: 'var(--space-4)', overflowX: 'auto' }}>
                    <table style={{ width: '100%', borderCollapse: 'collapse' }}>
                        <thead>
                            <tr>
                                {/* Пустая ячейка для заголовка категории */}
                                <th style={{ padding: '6px 8px', textAlign: 'left', fontSize: '0.68rem', textTransform: 'uppercase', letterSpacing: '0.06em', color: 'var(--text-muted)', fontWeight: 700 }}>
                                    {t('notifications.prefsCategoryCol')}
                                </th>
                                {ALL_CHANNELS.map((ch) => (
                                    <th
                                        key={ch}
                                        style={{ padding: '6px 8px', textAlign: 'center', fontSize: '0.68rem', textTransform: 'uppercase', letterSpacing: '0.06em', color: 'var(--text-muted)', fontWeight: 700 }}
                                    >
                                        {t(`notifications.channel_${ch}`)}
                                    </th>
                                ))}
                            </tr>
                        </thead>
                        <tbody>
                            {ALL_CATEGORIES.map((cat, i) => (
                                <tr
                                    key={cat}
                                    style={{ borderTop: i > 0 ? '1px solid var(--border-subtle)' : 'none' }}
                                >
                                    <td style={{ padding: '12px 8px', fontSize: '0.82rem', color: 'var(--text-secondary)', fontWeight: 600 }}>
                                        {t(`notifications.category_${cat}`)}
                                    </td>
                                    {ALL_CHANNELS.map((ch) => (
                                        <td key={ch} style={{ padding: '12px 8px', textAlign: 'center' }}>
                                            <input
                                                type="checkbox"
                                                checked={prefMap.get(makeKey(cat, ch)) ?? true}
                                                onChange={() => toggle(cat, ch)}
                                                style={{ width: 18, height: 18, cursor: 'pointer', accentColor: 'var(--accent-primary)' }}
                                            />
                                        </td>
                                    ))}
                                </tr>
                            ))}
                        </tbody>
                    </table>
                </section>
            )}

            <button
                className="btn-primary"
                onClick={() => void handleSave()}
                disabled={saving || loading}
            >
                {saving ? t('notifications.prefsSaving') : t('notifications.prefsSave')}
            </button>
        </div>
    )
}

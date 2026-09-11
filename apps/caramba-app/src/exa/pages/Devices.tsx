import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useAuth } from '../../context/AuthContext'
import { apiUrl } from '../../config'
import { hapticError, hapticSuccess, hapticTap } from '../../lib/haptics'
import { ExaIcon, deviceIconName } from '../icons'
import { Button, IconButton, Pill, ScreenHeader } from '../ui'
import { accountDeviceLimit, pickPrimary } from '../lib/subscription'
import { useToast } from '../lib/useToast'

/** Устройство аккаунта. Ответ панели: /api/client/devices. */
interface DeviceEntry {
    id: number
    display_name: string
    /** То же имя под старым ключом — пока панель отдаёт оба. */
    device_name: string
    platform: string | null
    client_device_id: string | null
    last_ip: string
    last_seen_at: string
    first_seen_at: string
    online: boolean
    is_current: boolean
}

/** Подпись платформы: android → Android. Незнакомое значение показываем как есть. */
const PLATFORM_LABELS: Record<string, string> = {
    android: 'Android',
    ios: 'iOS',
    macos: 'macOS',
    windows: 'Windows',
    linux: 'Linux',
}

/** «Профиль › Устройства»: привязки аккаунта, переименование, отвязка.
 *
 *  Список читается по аккаунту, а не по подписке: устройство принадлежит
 *  человеку и не должно исчезать при смене тарифа. Раньше здесь стоял
 *  pickPrimary и запросы шли на /subscription/{id}/devices — устройства,
 *  оставшиеся на прежней строке подписки, из кабинета были недоступны вообще,
 *  включая отвязку. */
export default function Devices() {
    const { t } = useTranslation()
    const toast = useToast()
    const { token, subscriptions, refreshData } = useAuth()
    const sub = useMemo(() => pickPrimary(subscriptions), [subscriptions])
    const limit = useMemo(() => accountDeviceLimit(subscriptions), [subscriptions])
    const [devices, setDevices] = useState<DeviceEntry[]>([])
    const [loading, setLoading] = useState(true)
    const [editing, setEditing] = useState<number | null>(null)
    const [draft, setDraft] = useState('')
    const [busy, setBusy] = useState<number | 'all' | null>(null)

    const nameOf = (d: DeviceEntry) => d.display_name || d.device_name || ''

    const load = useCallback(async () => {
        if (!token) {
            setLoading(false)
            return
        }
        try {
            const res = await fetch(apiUrl('/api/client/devices'), {
                headers: { Authorization: `Bearer ${token}` },
            })
            if (res.ok) {
                const data = await res.json()
                setDevices(Array.isArray(data) ? data : (data?.devices ?? []))
            }
        } finally {
            setLoading(false)
        }
    }, [token])

    useEffect(() => {
        void load()
    }, [load])

    const relative = (iso: string) => {
        const mins = Math.max(0, Math.floor((Date.now() - new Date(iso).getTime()) / 60000))
        if (mins < 1) return t('exa.devices.now')
        if (mins < 60) return t('exa.devices.minutesAgo', { count: mins })
        const hours = Math.floor(mins / 60)
        if (hours < 24) return t('exa.devices.hoursAgo', { count: hours })
        return t('exa.devices.daysAgo', { count: Math.floor(hours / 24) })
    }

    /** «Android · 5 минут назад» — платформа полезнее строки User-Agent. */
    const meta = (d: DeviceEntry) => {
        const platform = d.platform ? (PLATFORM_LABELS[d.platform] ?? d.platform) : null
        const seen = d.online ? t('exa.devices.online') : relative(d.last_seen_at)
        return platform ? `${platform} · ${seen}` : seen
    }

    const rename = async (d: DeviceEntry) => {
        if (!token) return
        const name = draft.trim()
        setEditing(null)
        if (!name || name === nameOf(d)) return
        const res = await fetch(apiUrl(`/api/client/devices/${d.id}/name`), {
            method: 'PUT',
            headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
            body: JSON.stringify({ name }),
        })
        if (res.ok) {
            hapticSuccess()
            setDevices((list) =>
                list.map((x) => (x.id === d.id ? { ...x, display_name: name, device_name: name } : x)),
            )
        } else {
            hapticError()
            toast(t('exa.common.error'))
        }
    }

    const kick = async (d: DeviceEntry) => {
        if (!token || busy) return
        if (!window.confirm(t('exa.devices.disconnectConfirm'))) return
        hapticTap()
        setBusy(d.id)
        const res = await fetch(apiUrl(`/api/client/devices/${d.id}`), {
            method: 'DELETE',
            headers: { Authorization: `Bearer ${token}` },
        })
        setBusy(null)
        if (res.ok) {
            hapticSuccess()
            setDevices((list) => list.filter((x) => x.id !== d.id))
            void refreshData()
        } else {
            hapticError()
            toast(t('exa.common.error'))
        }
    }

    const kickAll = async () => {
        if (!token || busy) return
        if (!window.confirm(t('exa.devices.killAllConfirm'))) return
        setBusy('all')
        const res = await fetch(apiUrl('/api/client/devices/kill-all'), {
            method: 'POST',
            headers: { Authorization: `Bearer ${token}` },
        })
        setBusy(null)
        if (res.ok) {
            hapticSuccess()
            setDevices([])
            void refreshData()
        } else {
            hapticError()
            toast(t('exa.common.error'))
        }
    }

    const free = Math.max(0, limit - devices.length)

    return (
        <div className="exa-screen">
            <ScreenHeader
                title={t('exa.devices.title')}
                aside={sub ? `${limit > 0 ? t('exa.home.devicesOf', { n: devices.length, of: limit }) : devices.length} · ${sub.plan_name}` : undefined}
            />
            {loading ? <div className="exa-loading">{t('exa.common.loading')}</div> : null}
            {!loading && devices.length > 0 ? (
                <section className="exa-card exa-card--list">
                    {devices.map((d) => (
                        <div key={d.id} className="exa-row" style={{ minHeight: 68 }}>
                            <span className="exa-device-avatar">
                                <ExaIcon name={deviceIconName(nameOf(d))} size={22} />
                            </span>
                            {editing === d.id ? (
                                <input
                                    className="exa-rename"
                                    autoFocus
                                    value={draft}
                                    maxLength={40}
                                    onChange={(e) => setDraft(e.target.value)}
                                    onBlur={() => void rename(d)}
                                    onKeyDown={(e) => {
                                        if (e.key === 'Enter') void rename(d)
                                        if (e.key === 'Escape') setEditing(null)
                                    }}
                                />
                            ) : (
                                <div className="exa-row__body">
                                    <div className="exa-row__title">
                                        <span>{nameOf(d) || t('exa.devices.unknown')}</span>
                                        {d.is_current ? <Pill tone="accent">{t('exa.devices.thisDevice')}</Pill> : null}
                                    </div>
                                    <div className="exa-row__meta">{meta(d)}</div>
                                </div>
                            )}
                            <IconButton
                                label={t('exa.devices.rename')}
                                className="is-ghost is-sm"
                                onClick={() => {
                                    setDraft(nameOf(d))
                                    setEditing(d.id)
                                }}
                            >
                                <ExaIcon name="pencil" size={20} />
                            </IconButton>
                            <Button variant="danger" size="sm" block={false} disabled={busy !== null} onClick={() => void kick(d)}>
                                {t('exa.devices.disconnect')}
                            </Button>
                        </div>
                    ))}
                </section>
            ) : null}
            {!loading && free > 0 ? (
                <div className="exa-free-slot">
                    <span className="exa-device-avatar is-empty" />
                    <span>{t('exa.devices.freeSlot')}</span>
                </div>
            ) : null}
            {!loading && devices.length === 0 ? <p className="exa-muted exa-center">{t('exa.devices.empty')}</p> : null}
            {devices.length > 0 ? (
                <Button variant="danger" size="md" disabled={busy !== null} onClick={() => void kickAll()}>
                    {t('exa.devices.killAll')}
                </Button>
            ) : null}
            <p className="exa-muted exa-center" style={{ lineHeight: 1.45 }}>
                {t('exa.devices.note')}
            </p>
        </div>
    )
}

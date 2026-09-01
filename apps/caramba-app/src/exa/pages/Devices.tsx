import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useAuth } from '../../context/AuthContext'
import { apiUrl } from '../../config'
import { hapticError, hapticSuccess, hapticTap } from '../../lib/haptics'
import { ExaIcon, deviceIconName } from '../icons'
import { Button, IconButton, Pill, ScreenHeader } from '../ui'
import { pickPrimary } from '../lib/subscription'
import { useToast } from '../lib/useToast'

interface DeviceEntry {
    id: number
    device_name: string
    last_ip: string
    last_seen_at: string
    first_seen_at: string
    is_current: boolean
}

/** «Профиль › Устройства»: список аренд, переименование, отключение. */
export default function Devices() {
    const { t } = useTranslation()
    const toast = useToast()
    const { token, subscriptions, refreshData } = useAuth()
    const sub = useMemo(() => pickPrimary(subscriptions), [subscriptions])
    const [devices, setDevices] = useState<DeviceEntry[]>([])
    const [loading, setLoading] = useState(true)
    const [editing, setEditing] = useState<number | null>(null)
    const [draft, setDraft] = useState('')
    const [busy, setBusy] = useState<number | 'all' | null>(null)

    const load = useCallback(async () => {
        if (!token || !sub) {
            setLoading(false)
            return
        }
        try {
            const res = await fetch(apiUrl(`/api/client/subscription/${sub.id}/devices`), {
                headers: { Authorization: `Bearer ${token}` },
            })
            if (res.ok) {
                const data = await res.json()
                setDevices(Array.isArray(data) ? data : (data?.devices ?? []))
            }
        } finally {
            setLoading(false)
        }
    }, [token, sub])

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

    const rename = async (d: DeviceEntry) => {
        if (!token || !sub) return
        const name = draft.trim()
        setEditing(null)
        if (!name || name === d.device_name) return
        const res = await fetch(apiUrl(`/api/client/subscription/${sub.id}/devices/${d.id}/name`), {
            method: 'PUT',
            headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
            body: JSON.stringify({ name }),
        })
        if (res.ok) {
            hapticSuccess()
            setDevices((list) => list.map((x) => (x.id === d.id ? { ...x, device_name: name } : x)))
        } else {
            hapticError()
            toast(t('exa.common.error'))
        }
    }

    const kick = async (d: DeviceEntry) => {
        if (!token || !sub || busy) return
        hapticTap()
        setBusy(d.id)
        const res = await fetch(apiUrl(`/api/client/subscription/${sub.id}/devices/${d.id}`), {
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
        if (!token || !sub || busy) return
        if (!window.confirm(t('exa.devices.killAllConfirm'))) return
        setBusy('all')
        const res = await fetch(apiUrl(`/api/client/subscription/${sub.id}/devices/kill-all`), {
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

    const limit = sub?.device_limit ?? 0
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
                                <ExaIcon name={deviceIconName(d.device_name)} size={22} />
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
                                        <span>{d.device_name || t('exa.devices.unknown')}</span>
                                        {d.is_current ? <Pill tone="accent">{t('exa.devices.thisDevice')}</Pill> : null}
                                    </div>
                                    <div className="exa-row__meta">{relative(d.last_seen_at)}</div>
                                </div>
                            )}
                            {d.is_current ? (
                                <IconButton
                                    label={t('exa.devices.rename')}
                                    className="is-ghost is-sm"
                                    onClick={() => {
                                        setDraft(d.device_name)
                                        setEditing(d.id)
                                    }}
                                >
                                    <ExaIcon name="pencil" size={20} />
                                </IconButton>
                            ) : (
                                <Button variant="danger" size="sm" block={false} disabled={busy !== null} onClick={() => void kick(d)}>
                                    {t('exa.devices.disconnect')}
                                </Button>
                            )}
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
            {devices.length > 1 ? (
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

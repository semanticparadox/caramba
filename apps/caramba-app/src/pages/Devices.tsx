import { useState, useEffect, useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { useAuth } from '../context/AuthContext'
import './Devices.css'

interface DeviceEntry {
    id: number
    device_name: string
    last_ip: string
    last_seen_at: string
    first_seen_at: string
    is_current: boolean
}

function deviceIcon(name: string): string {
    const lower = name.toLowerCase()
    if (lower.includes('mobile') || lower.includes('android') || lower.includes('ios') || lower.includes('iphone')) return '📱'
    if (lower.includes('desktop') || lower.includes('windows') || lower.includes('macos') || lower.includes('linux')) return '💻'
    if (lower.includes('tv') || lower.includes('android tv')) return '📺'
    if (lower.includes('router') || lower.includes('openwrt')) return '📡'
    return '📱'
}

// Валидация имени: 1-32 символа, без управляющих символов
function sanitizeName(raw: string): string {
    return raw
        .split('')
        .filter(c => c.charCodeAt(0) >= 32)
        .join('')
        .trim()
        .slice(0, 32)
}

export default function Devices() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const [searchParams] = useSearchParams()
    const { token, subscriptions } = useAuth()
    const subId = Number(searchParams.get('sub'))
    const sub = subscriptions.find(s => s.id === subId)

    const [devices, setDevices] = useState<DeviceEntry[]>([])
    const [loading, setLoading] = useState(true)
    const [fetchError, setFetchError] = useState<string | null>(null)
    const [kickingId, setKickingId] = useState<number | null>(null)
    const [kickError, setKickError] = useState<{ id: number; msg: string } | null>(null)

    // Kill-all state
    const [killAllConfirming, setKillAllConfirming] = useState(false)
    const [killAllBusy, setKillAllBusy] = useState(false)
    const [killAllError, setKillAllError] = useState<string | null>(null)

    // Rename state: tracks which device is in edit mode and its draft value
    const [renamingId, setRenamingId] = useState<number | null>(null)
    const [renameDraft, setRenameDraft] = useState('')
    const [renameError, setRenameError] = useState<string | null>(null)
    const renameInputRef = useRef<HTMLInputElement>(null)

    // Форматирование времени через i18n-ключи вместо хардкодных строк
    const timeAgo = (iso: string): string => {
        const diff = Date.now() - new Date(iso).getTime()
        const mins = Math.floor(diff / 60000)
        if (mins < 1) return t('devices.timeJustNow')
        if (mins < 60) return t('devices.timeMinutes', { count: mins })
        const hours = Math.floor(mins / 60)
        if (hours < 24) return t('devices.timeHours', { count: hours })
        const days = Math.floor(hours / 24)
        return t('devices.timeDays', { count: days })
    }

    const fetchDevices = useCallback(async () => {
        if (!token || !subId) return
        setFetchError(null)
        try {
            const res = await fetch(`/api/client/subscription/${subId}/devices`, {
                headers: { Authorization: `Bearer ${token}` },
            })
            if (res.ok) {
                setDevices(await res.json())
            } else {
                setFetchError(t('devices.fetchError'))
                setDevices([])
            }
        } catch {
            setFetchError(t('devices.fetchNetworkError'))
            setDevices([])
        } finally { setLoading(false) }
    }, [token, subId, t])

    useEffect(() => {
        fetchDevices()
        const interval = setInterval(fetchDevices, 30000)
        return () => clearInterval(interval)
    }, [fetchDevices])

    // Фокус на поле ввода при открытии редактирования имени
    useEffect(() => {
        if (renamingId !== null && renameInputRef.current) {
            renameInputRef.current.focus()
            renameInputRef.current.select()
        }
    }, [renamingId])

    const kickDevice = async (deviceId: number) => {
        if (!token || !subId) return
        setKickingId(deviceId)
        setKickError(null)
        try {
            const res = await fetch(`/api/client/subscription/${subId}/devices/${deviceId}`, {
                method: 'DELETE',
                headers: { Authorization: `Bearer ${token}` },
            })
            if (res.ok) {
                setDevices(prev => prev.filter(d => d.id !== deviceId))
            } else {
                setKickError({ id: deviceId, msg: t('devices.kickError') })
            }
        } catch {
            setKickError({ id: deviceId, msg: t('devices.kickNetworkError') })
        } finally { setKickingId(null) }
    }

    // Отключить все устройства — с подтверждением
    const killAllDevices = async () => {
        if (!token || !subId) return
        setKillAllBusy(true)
        setKillAllError(null)
        setKillAllConfirming(false)
        try {
            const res = await fetch(`/api/client/subscription/${subId}/devices/kill-all`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}` },
            })
            if (res.ok) {
                setDevices([])
                // Отложенный рефетч через 5 секунд — подтверждаем что список пуст
                setTimeout(() => { void fetchDevices() }, 5000)
            } else {
                setKillAllError(t('devices.killAllError'))
            }
        } catch {
            setKillAllError(t('devices.killAllNetworkError'))
        } finally { setKillAllBusy(false) }
    }

    // Начало редактирования имени устройства
    const startRename = (device: DeviceEntry) => {
        setRenamingId(device.id)
        setRenameDraft(device.device_name)
        setRenameError(null)
    }

    // Сохранить имя устройства (PUT /api/client/subscription/{sub_id}/devices/{id}/name)
    const saveRename = async (deviceId: number) => {
        if (!token || !subId) return
        const name = sanitizeName(renameDraft)
        setRenamingId(null)

        // Оптимистичное обновление локального состояния
        setDevices(prev => prev.map(d =>
            d.id === deviceId ? { ...d, device_name: name || d.device_name } : d
        ))

        try {
            const res = await fetch(`/api/client/subscription/${subId}/devices/${deviceId}/name`, {
                method: 'PUT',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ name }),
            })
            if (!res.ok) {
                setRenameError(t('devices.renameError'))
                // Откат к серверным данным при ошибке
                void fetchDevices()
            }
        } catch {
            setRenameError(t('devices.renameError'))
            void fetchDevices()
        }
    }

    const cancelRename = () => {
        setRenamingId(null)
        setRenameDraft('')
    }

    const deviceLimit = (sub?.device_limit ?? 0) > 0 ? sub!.device_limit : null
    const planName = sub?.plan_name || t('home.noSubscription')

    // Цвет счётчика и баннер зависят от заполненности лимита
    const deviceLimitStatus: 'ok' | 'at-limit' | 'over-limit' =
        deviceLimit == null ? 'ok'
        : devices.length > deviceLimit ? 'over-limit'
        : devices.length >= deviceLimit ? 'at-limit'
        : 'ok'

    if (!token || !subId) {
        return (
            <div className="page devices-page">
                <header className="page-header">
                    <button className="back-button" onClick={() => navigate('/')}>{'<'}</button>
                    <h2>{t('devices.title')}</h2>
                </header>
                <div className="devices-empty">
                    <div className="empty-icon">🔌</div>
                    <p>{t('devices.noSubscription')}</p>
                </div>
            </div>
        )
    }

    return (
        <div className="page devices-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')}>{'<'}</button>
                <h2>{t('devices.title')}</h2>
                <span className="badge badge-success">{planName}</span>
            </header>

            <div className="devices-summary">
                <span>{t('devices.activeDevices')}</span>
                <span className={`count count--${deviceLimitStatus}`}>
                    {devices.length} / {deviceLimit ?? '∞'}
                </span>
            </div>

            {/* Баннер лимита устройств — показываем только когда лимит задан и достигнут */}
            {deviceLimit != null && deviceLimitStatus !== 'ok' && (
                <div className={`devices-limit-banner devices-limit-banner--${deviceLimitStatus}`}>
                    {deviceLimitStatus === 'over-limit'
                        ? t('devices.overLimitWarning', { current: devices.length, max: deviceLimit })
                        : t('devices.atLimitWarning', { current: devices.length, max: deviceLimit })}
                </div>
            )}

            {/* Ошибка загрузки устройств — показываем с кнопкой повтора */}
            {fetchError && (
                <div className="devices-error">
                    <p>{fetchError}</p>
                    <button className="btn-secondary" onClick={() => { setLoading(true); void fetchDevices() }}>
                        {t('devices.retry')}
                    </button>
                </div>
            )}

            {/* Ошибка при отключении устройства */}
            {kickError && (
                <div className="devices-error">
                    <p>{kickError.msg}</p>
                    <button className="btn-secondary" onClick={() => { void kickDevice(kickError.id); setKickError(null) }}>
                        {t('devices.retry')}
                    </button>
                    <button className="btn-ghost" style={{ marginTop: 4 }} onClick={() => setKickError(null)}>
                        {t('common.close')}
                    </button>
                </div>
            )}

            {/* Ошибки переименования и kill-all */}
            {(renameError || killAllError) && (
                <div className="devices-error">
                    <p>{renameError || killAllError}</p>
                    <button className="btn-ghost" style={{ marginTop: 4 }} onClick={() => { setRenameError(null); setKillAllError(null) }}>
                        {t('common.close')}
                    </button>
                </div>
            )}

            {loading ? (
                <div className="loading">{t('devices.loading')}</div>
            ) : !fetchError && devices.length === 0 ? (
                <div className="devices-empty">
                    <div className="empty-icon">🔌</div>
                    <p>{t('devices.noDevices')}</p>
                </div>
            ) : (
                <div className="devices-list">
                    {devices.map(device => (
                        <div key={device.id} className={`device-card${device.is_current ? ' current' : ''}`}>
                            <div className="device-icon">{deviceIcon(device.device_name)}</div>
                            <div className="device-info">
                                {/* Inline rename: иконка карандаша рядом с именем */}
                                {renamingId === device.id ? (
                                    <input
                                        ref={renameInputRef}
                                        className="device-name-input"
                                        value={renameDraft}
                                        maxLength={32}
                                        placeholder={t('devices.renamePlaceholder')}
                                        onChange={e => setRenameDraft(e.target.value)}
                                        onBlur={() => void saveRename(device.id)}
                                        onKeyDown={e => {
                                            if (e.key === 'Enter') void saveRename(device.id)
                                            if (e.key === 'Escape') cancelRename()
                                        }}
                                    />
                                ) : (
                                    <div className="device-name-row">
                                        <span className="device-name">{device.device_name || t('devices.unknown')}</span>
                                        <button
                                            className="btn-rename"
                                            onClick={() => startRename(device)}
                                            title="Rename"
                                            aria-label="Rename device"
                                        >
                                            ✏️
                                        </button>
                                    </div>
                                )}
                                <div className="device-meta">
                                    <span>IP: {device.last_ip}</span>
                                    <span>{timeAgo(device.last_seen_at)}</span>
                                </div>
                            </div>
                            <div className="device-actions">
                                {device.is_current ? (
                                    <span className="badge-current">{t('devices.current')}</span>
                                ) : (
                                    <button
                                        className="btn-kick"
                                        onClick={() => kickDevice(device.id)}
                                        disabled={kickingId === device.id}
                                    >
                                        {kickingId === device.id ? '...' : t('devices.kick')}
                                    </button>
                                )}
                            </div>
                        </div>
                    ))}
                </div>
            )}

            {/* Кнопка «Отключить всё» — только когда есть хотя бы одно устройство */}
            {!loading && devices.length > 0 && (
                <div className="devices-kill-all-wrap">
                    {killAllConfirming ? (
                        <div className="devices-kill-all-confirm">
                            <p className="devices-kill-all-confirm-text">{t('devices.killAllConfirm')}</p>
                            <div className="devices-kill-all-confirm-actions">
                                <button
                                    className="btn-kick"
                                    onClick={() => void killAllDevices()}
                                    disabled={killAllBusy}
                                >
                                    {killAllBusy ? '...' : t('devices.killAll')}
                                </button>
                                <button
                                    className="btn-ghost"
                                    onClick={() => setKillAllConfirming(false)}
                                    disabled={killAllBusy}
                                >
                                    {t('common.cancel')}
                                </button>
                            </div>
                        </div>
                    ) : (
                        <button
                            className="btn-kill-all"
                            onClick={() => setKillAllConfirming(true)}
                        >
                            {t('devices.killAll')}
                        </button>
                    )}
                </div>
            )}

            <p className="devices-note">
                {t('devices.note')}
            </p>
        </div>
    )
}

import { useState, useEffect, useCallback } from 'react'
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

function timeAgo(iso: string): string {
    const diff = Date.now() - new Date(iso).getTime()
    const mins = Math.floor(diff / 60000)
    if (mins < 1) return 'только что'
    if (mins < 60) return `${mins} мин. назад`
    const hours = Math.floor(mins / 60)
    if (hours < 24) return `${hours} ч. назад`
    const days = Math.floor(hours / 24)
    return `${days} дн. назад`
}

function deviceIcon(name: string): string {
    const lower = name.toLowerCase()
    if (lower.includes('mobile') || lower.includes('android') || lower.includes('ios') || lower.includes('iphone')) return '📱'
    if (lower.includes('desktop') || lower.includes('windows') || lower.includes('macos') || lower.includes('linux')) return '💻'
    if (lower.includes('tv') || lower.includes('android tv')) return '📺'
    if (lower.includes('router') || lower.includes('openwrt')) return '📡'
    return '📱'
}

export default function Devices() {
    const navigate = useNavigate()
    const [searchParams] = useSearchParams()
    const { token, subscriptions } = useAuth()
    const subId = Number(searchParams.get('sub'))
    const sub = subscriptions.find(s => s.id === subId)

    const [devices, setDevices] = useState<DeviceEntry[]>([])
    const [loading, setLoading] = useState(true)
    const [kickingId, setKickingId] = useState<number | null>(null)

    const fetchDevices = useCallback(async () => {
        if (!token || !subId) return
        try {
            const res = await fetch(`/api/client/subscription/${subId}/devices`, {
                headers: { Authorization: `Bearer ${token}` },
            })
            if (res.ok) setDevices(await res.json())
            else setDevices([])
        } catch { setDevices([]) }
        finally { setLoading(false) }
    }, [token, subId])

    useEffect(() => {
        fetchDevices()
        const interval = setInterval(fetchDevices, 30000)
        return () => clearInterval(interval)
    }, [fetchDevices])

    const kickDevice = async (deviceId: number) => {
        if (!token || !subId) return
        setKickingId(deviceId)
        try {
            const res = await fetch(`/api/client/subscription/${subId}/devices/${deviceId}`, {
                method: 'DELETE',
                headers: { Authorization: `Bearer ${token}` },
            })
            if (res.ok) {
                setDevices(prev => prev.filter(d => d.id !== deviceId))
            }
        } catch { /* ignore */ }
        finally { setKickingId(null) }
    }

    const deviceLimit = (sub?.device_limit ?? 0) > 0 ? sub!.device_limit : null
    const planName = sub?.plan_name || 'Подписка'

    if (!token || !subId) {
        return (
            <div className="page devices-page">
                <header className="page-header">
                    <button className="back-button" onClick={() => navigate('/')}>{'<'}</button>
                    <h2>Устройства</h2>
                </header>
                <div className="devices-empty">
                    <div className="empty-icon">🔌</div>
                    <p>Подписка не найдена</p>
                </div>
            </div>
        )
    }

    return (
        <div className="page devices-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')}>{'<'}</button>
                <h2>Устройства</h2>
                <span className="badge badge-success">{planName}</span>
            </header>

            <div className="devices-summary">
                <span>Активные устройства</span>
                <span className="count">
                    {devices.length} / {deviceLimit ?? '∞'}
                </span>
            </div>

            {loading ? (
                <div className="loading">Загрузка...</div>
            ) : devices.length === 0 ? (
                <div className="devices-empty">
                    <div className="empty-icon">🔌</div>
                    <p>Нет активных устройств</p>
                </div>
            ) : (
                <div className="devices-list">
                    {devices.map(device => (
                        <div key={device.id} className={`device-card${device.is_current ? ' current' : ''}`}>
                            <div className="device-icon">{deviceIcon(device.device_name)}</div>
                            <div className="device-info">
                                <div className="device-name">{device.device_name || 'Неизвестное устройство'}</div>
                                <div className="device-meta">
                                    <span>IP: {device.last_ip}</span>
                                    <span>{timeAgo(device.last_seen_at)}</span>
                                </div>
                            </div>
                            <div className="device-actions">
                                {device.is_current ? (
                                    <span className="badge-current">текущее</span>
                                ) : (
                                    <button
                                        className="btn-kick"
                                        onClick={() => kickDevice(device.id)}
                                        disabled={kickingId === device.id}
                                    >
                                        {kickingId === device.id ? '...' : 'Отключить'}
                                    </button>
                                )}
                            </div>
                        </div>
                    ))}
                </div>
            )}

            <p className="devices-note">
                Показаны устройства, активные за последние 15 минут. Обновление каждые 30 сек.
            </p>
        </div>
    )
}

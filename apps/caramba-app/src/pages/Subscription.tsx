import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { QRCodeSVG } from 'qrcode.react'
import { useAuth, UserSubscription } from '../context/AuthContext'
import './Subscription.css'

function formatTraffic(gb: number): string {
    if (gb >= 1024) return `${(gb / 1024).toFixed(1)} TB`
    return `${gb} GB`
}

function formatDateTime(value?: string | null): string {
    if (!value) return '--'
    const ts = new Date(value)
    if (Number.isNaN(ts.getTime())) return value
    return ts.toLocaleString()
}

function withClient(url: string, client: string): string {
    const separator = url.includes('?') ? '&' : '?'
    return `${url}${separator}client=${encodeURIComponent(client)}`
}

export default function Subscription() {
    const { subscriptions, isLoading, refreshData, token, error } = useAuth()
    const navigate = useNavigate()
    const [expandedId, setExpandedId] = useState<number | null>(null)
    const [copied, setCopied] = useState<number | null>(null)
    const [copiedVless, setCopiedVless] = useState<number | null>(null)
    const [activatingId, setActivatingId] = useState<number | null>(null)
    const [giftingId, setGiftingId] = useState<number | null>(null)
    const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null)

    const handleCopy = (sub: UserSubscription) => {
        navigator.clipboard.writeText(sub.subscription_url)
        setCopied(sub.id)
        setTimeout(() => setCopied(null), 2000)
    }

    const handleCopyVless = (sub: UserSubscription) => {
        if (!sub.primary_vless_link) return
        navigator.clipboard.writeText(sub.primary_vless_link)
        setCopiedVless(sub.id)
        setTimeout(() => setCopiedVless(null), 2000)
    }

    const openExternal = (url: string) => {
        window.location.href = url
    }

    const handleActivate = async (subId: number) => {
        if (!token) return

        setActivatingId(subId)
        setMessage(null)
        try {
            const res = await fetch(`/api/client/subscription/${subId}/activate`, {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                },
            })

            if (res.ok) {
                const data = await res.json()
                setMessage({
                    type: 'success',
                    text: data?.message || 'Подписка успешно активирована.',
                })
                await refreshData()
                setExpandedId(subId)
            } else {
                const err = await res.text()
                setMessage({ type: 'error', text: err || 'Не удалось активировать подписку.' })
            }
        } catch {
            setMessage({ type: 'error', text: 'Сетевая ошибка при активации подписки.' })
        } finally {
            setActivatingId(null)
        }
    }

    const handleConvertToGift = async (subId: number) => {
        if (!token) return

        setGiftingId(subId)
        setMessage(null)
        try {
            const res = await fetch(`/api/client/subscription/${subId}/gift`, {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                },
            })

            if (res.ok) {
                const data = await res.json()
                const code = data?.code ? ` ${data.code}` : ''
                if (data?.code) {
                    navigator.clipboard.writeText(data.code).catch(() => undefined)
                }
                setMessage({
                    type: 'success',
                    text: `Подарочный код создан.${code ? ` Скопировано: ${code}` : ''}`,
                })
                await refreshData()
            } else {
                const err = await res.text()
                setMessage({ type: 'error', text: err || 'Не удалось создать подарочный код.' })
            }
        } catch {
            setMessage({ type: 'error', text: 'Сетевая ошибка при создании подарочного кода.' })
        } finally {
            setGiftingId(null)
        }
    }

    const toggleExpand = (id: number) => {
        setExpandedId(expandedId === id ? null : id)
    }

    if (isLoading) return <div className="page"><div className="loading">Загрузка подписок...</div></div>

    if (!token) {
        return (
            <div className="page sub-page">
                <header className="page-header">
                    <button className="back-button" onClick={() => navigate('/')}>{'<'}</button>
                    <h2>Мои сервисы</h2>
                </header>
                <div className="empty-state">
                    <div className="empty-icon">AU</div>
                    <h3>Требуется авторизация</h3>
                    <p>{error || 'Откройте Mini App из Telegram-бота, чтобы загрузить подписки.'}</p>
                </div>
            </div>
        )
    }

    const sorted = [...subscriptions].sort((a, b) => {
        const order: Record<string, number> = { active: 0, pending: 1, expired: 2 }
        const diff = (order[a.status] ?? 3) - (order[b.status] ?? 3)
        if (diff !== 0) return diff
        return new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
    })

    return (
        <div className="page sub-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')}>{'<'}</button>
                <h2>Мои сервисы</h2>
                {subscriptions.length > 0 && (
                    <span className="badge badge-success">{subscriptions.filter((s) => s.status === 'active').length} активных</span>
                )}
            </header>

            <button className="btn-secondary" onClick={() => navigate('/support/connect')}>
                Как подключиться: каталог приложений
            </button>

            {message && (
                <div className={`purchase-msg ${message.type}`}>
                    {message.text}
                </div>
            )}

            {sorted.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-icon">SV</div>
                    <h3>Подписок пока нет</h3>
                    <p>Активируйте тариф, чтобы получить доступ к VPN-серверам.</p>
                    <button className="btn-primary" onClick={() => navigate('/plans')}>
                        Открыть тарифы
                    </button>
                </div>
            ) : (
                <div className="subs-list">
                    {sorted.map((sub, index) => (
                        <div key={sub.id} className={`sub-card glass-card ${sub.status}`}>
                            <div className="sub-header" onClick={() => toggleExpand(sub.id)}>
                                <div className="sub-header-left">
                                    <span className="sub-number">#{index + 1}</span>
                                    <div className="sub-plan-info">
                                        <span className="sub-plan-name">{sub.plan_name}</span>
                                        <span className={`badge badge-${sub.status === 'active' ? 'success' : sub.status === 'pending' ? 'warning' : 'error'}`}>
                                            {sub.status === 'active' ? 'Активна' : sub.status === 'pending' ? 'Ожидает' : 'Истекла'}
                                        </span>
                                    </div>
                                </div>
                                <span className="expand-arrow">{expandedId === sub.id ? '^' : 'v'}</span>
                            </div>

                            <div className="sub-traffic">
                                <div className="traffic-bar-row">
                                    <span>Трафик</span>
                                    <span>{sub.used_traffic_gb} GB / {sub.traffic_limit_gb > 0 ? formatTraffic(sub.traffic_limit_gb) : '∞'}</span>
                                </div>
                                {sub.traffic_limit_gb > 0 && (
                                    <div className="progress-bar-mini">
                                        <div
                                            className="progress-fill-mini"
                                            style={{ width: `${Math.min(100, (parseFloat(sub.used_traffic_gb) / sub.traffic_limit_gb) * 100)}%` }}
                                        />
                                    </div>
                                )}
                            </div>

                            <div className="sub-meta-row">
                                {sub.status === 'active' ? (
                                    <>
                                        <span>{sub.days_left > 0 ? `${sub.days_left} дн. осталось` : sub.duration_days === 0 ? 'Без срока действия' : 'Скоро истечет'}</span>
                                        <span className="sub-date">{sub.duration_days > 0 ? new Date(sub.expires_at).toLocaleDateString() : 'Трафик-план'}</span>
                                    </>
                                ) : sub.status === 'pending' ? (
                                    <span>{sub.duration_days > 0 ? `${sub.duration_days} дней (запуск после активации)` : 'Трафик-план'}</span>
                                ) : (
                                    <span>Истекла</span>
                                )}
                            </div>

                            {sub.note && (
                                <div className="sub-note">
                                    {sub.note}
                                </div>
                            )}

                            <div className="sub-extra-row">
                                <span>
                                    Устройства: {sub.active_devices ?? 0}/{(sub.device_limit ?? 0) > 0 ? sub.device_limit : '∞'}
                                </span>
                                {sub.last_node_name && (
                                    <span>
                                        Последний узел: {sub.last_node_flag ? `${sub.last_node_flag} ` : ''}{sub.last_node_name}
                                        {sub.last_node_id ? ` (#${sub.last_node_id})` : ''}
                                    </span>
                                )}
                            </div>
                            <div className="sub-extra-row">
                                <span>Последний pull конфига: {formatDateTime(sub.last_sub_access)}</span>
                            </div>

                            {sub.status === 'active' && (
                                <div className="sub-actions">
                                    <button
                                        className="btn-text"
                                        onClick={(e) => { e.stopPropagation(); navigate(`/servers/${sub.id}`) }}
                                    >
                                        Оптимизация узла
                                    </button>
                                    <button
                                        className="btn-text"
                                        onClick={(e) => { e.stopPropagation(); navigate(`/servers/${sub.id}?magic=1`) }}
                                    >
                                        Магическая оптимизация
                                    </button>
                                </div>
                            )}
                            {sub.status === 'pending' && (
                                <div className="sub-actions">
                                    <button
                                        className="btn-text"
                                        onClick={(e) => {
                                            e.stopPropagation()
                                            handleActivate(sub.id)
                                        }}
                                        disabled={activatingId !== null || giftingId !== null}
                                    >
                                        {activatingId === sub.id ? 'Активация...' : 'Активировать'}
                                    </button>
                                    <button
                                        className="btn-text"
                                        onClick={(e) => {
                                            e.stopPropagation()
                                            handleConvertToGift(sub.id)
                                        }}
                                        disabled={activatingId !== null || giftingId !== null}
                                    >
                                        {giftingId === sub.id ? 'Создание кода...' : 'Сделать подарочный код'}
                                    </button>
                                </div>
                            )}

                            {expandedId === sub.id && sub.status === 'active' && (
                                <div className="sub-expanded">
                                    <div className="qr-wrapper">
                                        <QRCodeSVG
                                            value={sub.subscription_url}
                                            size={160}
                                            bgColor="#ffffff"
                                            fgColor="#0D0D1A"
                                            level="M"
                                            includeMargin
                                        />
                                    </div>
                                    <p className="qr-hint">Сканируйте в приложении VPN</p>

                                    <div className="link-row">
                                        <input type="text" readOnly value={sub.subscription_url} onClick={(e) => e.currentTarget.select()} />
                                        <button
                                            className={`btn-secondary copy-btn ${copied === sub.id ? 'copied' : ''}`}
                                            onClick={() => handleCopy(sub)}
                                        >
                                            {copied === sub.id ? 'Готово' : 'Копировать'}
                                        </button>
                                    </div>

                                    {sub.primary_vless_link && (
                                        <div className="link-row">
                                            <input type="text" readOnly value={sub.primary_vless_link ?? ''} onClick={(e) => e.currentTarget.select()} />
                                            <button
                                                className={`btn-secondary copy-btn ${copiedVless === sub.id ? 'copied' : ''}`}
                                                onClick={() => handleCopyVless(sub)}
                                            >
                                                {copiedVless === sub.id ? 'Готово' : 'VLESS'}
                                            </button>
                                        </div>
                                    )}

                                    <div className="app-links-grid">
                                        <button
                                            className="btn-text btn-app"
                                            onClick={() => openExternal(withClient(sub.subscription_url, 'singbox'))}
                                        >
                                            Sing-box
                                        </button>
                                        <button
                                            className="btn-text btn-app"
                                            onClick={() => openExternal(`hiddify://import/${encodeURIComponent(withClient(sub.subscription_url, 'hiddify'))}`)}
                                        >
                                            Hiddify
                                        </button>
                                        <button
                                            className="btn-text btn-app"
                                            onClick={() => openExternal(`happ://add?url=${encodeURIComponent(withClient(sub.subscription_url, 'happ'))}`)}
                                        >
                                            Happ
                                        </button>
                                        {sub.primary_vless_link && (
                                            <button
                                                className="btn-text btn-app"
                                                onClick={() => openExternal(sub.primary_vless_link!)}
                                            >
                                                Открыть VLESS
                                            </button>
                                        )}
                                    </div>

                                    {sub.is_trial && (
                                        <div className="badge badge-warning trial-badge">Пробный период</div>
                                    )}
                                </div>
                            )}
                        </div>
                    ))}
                </div>
            )}
        </div>
    )
}

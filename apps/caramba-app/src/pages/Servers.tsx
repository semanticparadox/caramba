import { useState, useEffect } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useAuth } from '../context/AuthContext'
import type { SingboxConnectionVariant } from '../context/AuthContext'
import { QRCodeSVG } from 'qrcode.react'
import { copyText } from '../lib/copyActions'
import { buildConfigUrl } from '../lib/subscriptionUrl'
import './Servers.css'

interface ServerInfo {
    id: number
    name: string
    country_code: string
    flag: string
    latency?: number
    status: string
    distance_km?: number
    available_variant_ids?: string[]
    recommended_variant_id?: string | null
    active_connections?: number
    max_users?: number
    is_full?: boolean
}

export default function Servers() {
    const navigate = useNavigate()
    const { subId: subIdParam } = useParams<{ subId?: string }>()
    const { token, subscriptions } = useAuth()
    const activeSub = subIdParam
        ? subscriptions.find(s => s.id === Number(subIdParam)) || subscriptions.find(s => s.status === 'active')
        : subscriptions.find(s => s.status === 'active')
    const [servers, setServers] = useState<ServerInfo[]>([])
    const [loading, setLoading] = useState(true)
    const [selectedServer, setSelectedServer] = useState<ServerInfo | null>(null)
    const [countryFilter, setCountryFilter] = useState<string | null>(null)
    const [clientType, setClientType] = useState('singbox')
    const [selectedVariant, setSelectedVariant] = useState<string>('')
    const [configUrl, setConfigUrl] = useState('')
    const [copied, setCopied] = useState(false)

    const singboxVariants = activeSub?.singbox_variants ?? []
    const availableVariants = selectedServer
        ? singboxVariants.filter(variant => selectedServer.available_variant_ids?.includes(variant.id))
        : singboxVariants
    const prioritizedVariants = [...availableVariants].sort((a, b) => {
        const recommendedId = selectedServer?.recommended_variant_id
        const aRecommended = a.id === recommendedId
        const bRecommended = b.id === recommendedId
        if (aRecommended !== bRecommended) return Number(bRecommended) - Number(aRecommended)
        if (a.relay !== b.relay) return Number(a.relay) - Number(b.relay)
        return a.label.localeCompare(b.label)
    })

    const trackVariantEvent = async (serverId: number, variantId: string, event: string, client = 'singbox') => {
        if (!token || !activeSub || !variantId) return
        try {
            await fetch(`/api/client/subscription/${activeSub.id}/variant-event`, {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    node_id: serverId,
                    variant_id: variantId,
                    event,
                    client,
                }),
            })
        } catch (error) {
            console.error('Variant telemetry failed', error)
        }
    }

    useEffect(() => {
        if (!token) return;
        const fetchData = async () => {
            try {
                const query = activeSub ? `?sub_id=${activeSub.id}` : ''
                const res = await fetch(`/api/client/servers${query}`, {
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (res.ok) setServers(await res.json());
            } catch (e) { console.error(e); }
            finally { setLoading(false); }
        };
        fetchData();
    }, [activeSub?.id, token]);

    const handleGetConfig = (server: ServerInfo) => {
        if (!activeSub) return;
        setSelectedServer(server);
        const serverVariants = singboxVariants.filter(variant => server.available_variant_ids?.includes(variant.id))
        const recommendedVariant = serverVariants.find(variant => variant.id === server.recommended_variant_id)
        const defaultVariant = clientType === 'singbox'
            ? recommendedVariant?.id ?? serverVariants[0]?.id ?? ''
            : '';
        setSelectedVariant(defaultVariant);
        updateConfigUrl(server.id, clientType, defaultVariant);
        if (clientType === 'singbox' && defaultVariant) {
            void trackVariantEvent(server.id, defaultVariant, 'config_opened')
        }
    }

    const updateConfigUrl = (nodeId: number, type: string, variantId?: string) => {
        if (!activeSub) return;
        setConfigUrl(buildConfigUrl(activeSub, { client: type, nodeId, variantId }));
    }

    const handleClientChange = (type: string) => {
        setClientType(type);
        const hasCurrentVariant = availableVariants.some(variant => variant.id === selectedVariant)
        const nextVariant = type === 'singbox'
            ? ((hasCurrentVariant ? selectedVariant : '') || prioritizedVariants[0]?.id || '')
            : '';
        setSelectedVariant(nextVariant);
        if (selectedServer) updateConfigUrl(selectedServer.id, type, nextVariant);
    }

    const handleVariantChange = (variant: SingboxConnectionVariant) => {
        setSelectedVariant(variant.id);
        if (selectedServer) updateConfigUrl(selectedServer.id, 'singbox', variant.id);
        if (selectedServer) {
            void trackVariantEvent(selectedServer.id, variant.id, 'variant_selected')
        }
    }

    const handleCopy = async () => {
        await copyText(configUrl);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
        if (selectedServer && clientType === 'singbox' && selectedVariant) {
            void trackVariantEvent(selectedServer.id, selectedVariant, 'config_copied')
        }
    }

    const handleConnectionFeedback = (event: 'connection_succeeded' | 'connection_failed') => {
        if (selectedServer && clientType === 'singbox' && selectedVariant) {
            void trackVariantEvent(selectedServer.id, selectedVariant, event)
        }
    }

    if (loading) return <div className="page"><div className="loading">Загружаем серверы...</div></div>

    return (
        <div className="page servers-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate(-1)}>←</button>
                <h2>Настройка маршрута</h2>
                <span className="badge badge-success">{servers.filter(s => s.status === 'online').length} онлайн</span>
            </header>

            <p className="servers-context-note">
                Обычный импорт остается основным путем. Этот экран нужен только для дополнительной настройки, если хотите улучшить совместимость или проверить другой маршрут.
            </p>

            {(() => {
                const countries = [...new Set(servers.map(s => s.country_code))];
                return countries.length > 1 ? (
                    <div className="country-quick-picker">
                        <button className={`country-chip ${!countryFilter ? 'active' : ''}`} onClick={() => setCountryFilter(null)}>Все</button>
                        {countries.map(cc => {
                            const s = servers.find(sv => sv.country_code === cc);
                            return (
                                <button key={cc} className={`country-chip ${countryFilter === cc ? 'active' : ''}`} onClick={() => setCountryFilter(cc)}>
                                    {s?.flag} {cc}
                                </button>
                            );
                        })}
                    </div>
                ) : null;
            })()}

            <div className="servers-list">
                {servers
                    .filter(s => !countryFilter || s.country_code === countryFilter)
                    .map((server, i) => {
                    const capacity = server.max_users && server.max_users > 0
                        ? `${server.active_connections ?? 0}/${server.max_users}`
                        : `${server.active_connections ?? 0}/∞`;
                    const loadPct = server.max_users && server.max_users > 0
                        ? ((server.active_connections ?? 0) / server.max_users) * 100
                        : 0;
                    const loadColor = loadPct > 90 ? 'error' : loadPct > 70 ? 'warning' : 'success';

                    return (
                    <div key={server.id} className={`server-item glass-card ${i === 0 && !countryFilter ? 'best' : ''} ${server.is_full ? 'full' : ''}`}>
                        <div className="server-row">
                            <span className="server-flag">{server.flag}</span>
                            <div className="server-info">
                                <span className="server-name">{server.name}</span>
                                <span className="server-meta">
                                    {server.country_code}
                                    {server.distance_km !== undefined && ` · ${server.distance_km} km`}
                                </span>
                            </div>
                            <div className="server-right">
                                <span className={`badge badge-${loadColor}`}>{capacity}</span>
                                {!!server.available_variant_ids?.length && (
                                    <span className="badge badge-success">{server.available_variant_ids.length} вар.</span>
                                )}
                                {i === 0 && !countryFilter && <span className="badge badge-warning">Лучший</span>}
                                {server.is_full && <span className="badge badge-error">Заполнен</span>}
                            </div>
                        </div>
                        <button
                            className="btn-secondary"
                            onClick={() => handleGetConfig(server)}
                            disabled={server.is_full}
                        >
                            {server.is_full ? 'Сервер заполнен' : 'Открыть настройку'}
                        </button>
                    </div>
                    );
                })}
            </div>

            {selectedServer && (
                <div className="modal-overlay" onClick={() => setSelectedServer(null)}>
                    <div className="modal-content" onClick={e => e.stopPropagation()}>
                        <h3>{selectedServer.flag} {selectedServer.name}</h3>
                        <p className="connection-modal-note">
                            По умолчанию достаточно импорта с рекомендованными настройками. Блок ниже нужен только для ручной подстройки.
                        </p>
                        <div className="client-tabs">
                            {['singbox', 'v2ray', 'clash'].map(type => (
                                <button
                                    key={type}
                                    className={`tab ${clientType === type ? 'active' : ''}`}
                                    onClick={() => handleClientChange(type)}
                                >
                                    {type === 'singbox' ? 'Sing-box' : type === 'v2ray' ? 'V2Ray' : 'Clash'}
                                </button>
                            ))}
                        </div>
                        {clientType === 'singbox' && prioritizedVariants.length > 0 && (
                            <>
                                <div className="variant-section-intro">
                                    <strong>Дополнительная настройка маршрута</strong>
                                    <span>Выбирайте другой вариант только если хотите повысить совместимость или скорость на конкретной сети.</span>
                                </div>
                                <div className="variant-list">
                                {prioritizedVariants.map(variant => (
                                    <button
                                        key={variant.id}
                                        className={`variant-card ${selectedVariant === variant.id ? 'active' : ''}`}
                                        onClick={() => handleVariantChange(variant)}
                                    >
                                        <span className="variant-title-row">
                                            <span className="variant-title">{variant.label}</span>
                                            <span className="variant-badges">
                                                {variant.id === selectedServer.recommended_variant_id && (
                                                    <span className="variant-chip recommended">Рекомендуем</span>
                                                )}
                                                <span className={`variant-chip ${variant.family}`}>
                                                    {variant.family === 'grpc'
                                                        ? 'gRPC'
                                                        : variant.family === 'hysteria2'
                                                            ? 'Hysteria2'
                                                            : 'VLESS'}
                                                </span>
                                            </span>
                                        </span>
                                        <span className="variant-meta-row">
                                            <span>{variant.transport || 'стандартный транспорт'}</span>
                                            <span>{variant.relay ? 'через relay' : 'прямой маршрут'}</span>
                                        </span>
                                        <span className="variant-summary">{variant.summary}</span>
                                        {variant.id === selectedServer.recommended_variant_id && (
                                            <span className="variant-recommendation-note">Лучший стартовый вариант для большинства пользователей на этом сервере.</span>
                                        )}
                                    </button>
                                ))}
                                </div>
                            </>
                        )}
                        {clientType === 'singbox' && prioritizedVariants.length === 0 && (
                            <div className="variant-empty-state">
                                Для этого сервера сейчас нет дополнительных вариантов настройки.
                            </div>
                        )}
                        <div className="qr-wrapper">
                            <QRCodeSVG value={configUrl} size={160} bgColor="#fff" fgColor="#0D0D1A" />
                        </div>
                        <div className="config-url-label">Ссылка для импорта</div>
                        <div className="config-url-row">
                            <input type="text" readOnly value={configUrl} onClick={e => e.currentTarget.select()} />
                            <button className="btn-secondary" onClick={handleCopy}>
                                {copied ? 'Скопировано' : 'Скопировать'}
                            </button>
                        </div>
                        {clientType === 'singbox' && selectedVariant && (
                            <div className="variant-feedback-row">
                                <button
                                    className="btn-secondary variant-feedback success"
                                    onClick={() => handleConnectionFeedback('connection_succeeded')}
                                >
                                    Работает хорошо
                                </button>
                                <button
                                    className="btn-secondary variant-feedback danger"
                                    onClick={() => handleConnectionFeedback('connection_failed')}
                                >
                                    Попробовать другой
                                </button>
                            </div>
                        )}
                        <button className="btn-secondary close-btn" onClick={() => setSelectedServer(null)}>
                            Закрыть
                        </button>
                    </div>
                </div>
            )}
        </div>
    )
}

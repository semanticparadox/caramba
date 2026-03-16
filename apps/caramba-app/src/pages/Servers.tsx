import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../context/AuthContext'
import type { SingboxConnectionVariant } from '../context/AuthContext'
import { QRCodeSVG } from 'qrcode.react'
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
}

export default function Servers() {
    const navigate = useNavigate()
    const { token, subscriptions } = useAuth()
    const activeSub = subscriptions.find(s => s.status === 'active')
    const [servers, setServers] = useState<ServerInfo[]>([])
    const [loading, setLoading] = useState(true)
    const [selectedServer, setSelectedServer] = useState<ServerInfo | null>(null)
    const [clientType, setClientType] = useState('singbox')
    const [selectedVariant, setSelectedVariant] = useState<string>('')
    const [configUrl, setConfigUrl] = useState('')
    const [copied, setCopied] = useState(false)

    const singboxVariants = activeSub?.singbox_variants ?? []
    const availableVariants = selectedServer
        ? singboxVariants.filter(variant => selectedServer.available_variant_ids?.includes(variant.id))
        : singboxVariants

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
        let base = activeSub.subscription_url;
        const params = new URLSearchParams();
        params.set('client', type);
        params.set('node_id', String(nodeId));
        if (type === 'singbox' && variantId) {
            params.set('variant', variantId);
        }
        const sep = base.includes('?') ? '&' : '?';
        setConfigUrl(`${base}${sep}${params.toString()}`);
    }

    const handleClientChange = (type: string) => {
        setClientType(type);
        const hasCurrentVariant = availableVariants.some(variant => variant.id === selectedVariant)
        const nextVariant = type === 'singbox'
            ? ((hasCurrentVariant ? selectedVariant : '') || availableVariants[0]?.id || '')
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

    const handleCopy = () => {
        navigator.clipboard.writeText(configUrl);
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

    if (loading) return <div className="page"><div className="loading">Loading servers...</div></div>;

    return (
        <div className="page servers-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate(-1)}>←</button>
                <h2>Servers</h2>
                <span className="badge badge-success">{servers.filter(s => s.status === 'online').length} online</span>
            </header>

            <div className="servers-list">
                {servers.map((server, i) => (
                    <div key={server.id} className={`server-item glass-card ${i === 0 ? 'best' : ''}`}>
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
                                <span className={`status-indicator ${server.status}`}>
                                    {server.status === 'online' ? '●' : '○'}
                                </span>
                                {!!server.available_variant_ids?.length && (
                                    <span className="badge badge-success">{server.available_variant_ids.length} paths</span>
                                )}
                                {i === 0 && <span className="badge badge-warning">⭐ Best</span>}
                            </div>
                        </div>
                        <button className="btn-secondary" onClick={() => handleGetConfig(server)}>
                            🔗 Get Config
                        </button>
                    </div>
                ))}
            </div>

            {selectedServer && (
                <div className="modal-overlay" onClick={() => setSelectedServer(null)}>
                    <div className="modal-content" onClick={e => e.stopPropagation()}>
                        <h3>{selectedServer.flag} {selectedServer.name}</h3>
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
                        {clientType === 'singbox' && availableVariants.length > 0 && (
                            <div className="variant-list">
                                {availableVariants.map(variant => (
                                    <button
                                        key={variant.id}
                                        className={`variant-card ${selectedVariant === variant.id ? 'active' : ''}`}
                                        onClick={() => handleVariantChange(variant)}
                                    >
                                        <span className="variant-title-row">
                                            <span className="variant-title">{variant.label}</span>
                                            <span className="variant-badges">
                                                {variant.id === selectedServer.recommended_variant_id && (
                                                    <span className="variant-chip recommended">Best</span>
                                                )}
                                                <span className={`variant-chip ${variant.family}`}>
                                                    {variant.family === 'grpc' ? 'gRPC' : 'VLESS'}
                                                </span>
                                            </span>
                                        </span>
                                        <span className="variant-meta-row">
                                            <span>{variant.transport}</span>
                                            <span>{variant.relay ? 'relay' : 'direct'}</span>
                                        </span>
                                        <span className="variant-summary">{variant.summary}</span>
                                    </button>
                                ))}
                            </div>
                        )}
                        {clientType === 'singbox' && availableVariants.length === 0 && (
                            <div className="variant-empty-state">
                                No sing-box variants available for this server yet.
                            </div>
                        )}
                        <div className="qr-wrapper">
                            <QRCodeSVG value={configUrl} size={160} bgColor="#fff" fgColor="#0D0D1A" />
                        </div>
                        <div className="config-url-row">
                            <input type="text" readOnly value={configUrl} onClick={e => e.currentTarget.select()} />
                            <button className="btn-secondary" onClick={handleCopy}>
                                {copied ? '✓' : '📋'}
                            </button>
                        </div>
                        {clientType === 'singbox' && selectedVariant && (
                            <div className="variant-feedback-row">
                                <button
                                    className="btn-secondary variant-feedback success"
                                    onClick={() => handleConnectionFeedback('connection_succeeded')}
                                >
                                    Works
                                </button>
                                <button
                                    className="btn-secondary variant-feedback danger"
                                    onClick={() => handleConnectionFeedback('connection_failed')}
                                >
                                    Didn't work
                                </button>
                            </div>
                        )}
                        <button className="btn-secondary close-btn" onClick={() => setSelectedServer(null)}>
                            Close
                        </button>
                    </div>
                </div>
            )}
        </div>
    )
}

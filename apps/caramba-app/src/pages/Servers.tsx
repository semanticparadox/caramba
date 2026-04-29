import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
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

// Оценка координат пользователя по таймзоне (без запроса разрешений)
function estimateCoords(): { lat: number; lon: number } | null {
    try {
        const tz = Intl.DateTimeFormat().resolvedOptions().timeZone
        const tzCoords: Record<string, [number, number]> = {
            // Americas
            'America/New_York': [40.7, -74.0],
            'America/Chicago': [41.8, -87.6],
            'America/Denver': [39.7, -104.9],
            'America/Los_Angeles': [34.0, -118.2],
            'America/Toronto': [43.6, -79.3],
            'America/Vancouver': [49.2, -123.1],
            // Western Europe
            'Europe/London': [51.5, -0.1],
            'Europe/Paris': [48.8, 2.3],
            'Europe/Berlin': [52.5, 13.4],
            'Europe/Amsterdam': [52.3, 4.9],
            'Europe/Zurich': [47.3, 8.5],
            'Europe/Vienna': [48.2, 16.3],
            'Europe/Rome': [41.9, 12.5],
            'Europe/Madrid': [40.4, -3.7],
            // Central/Eastern Europe
            'Europe/Warsaw': [52.2, 21.0],
            'Europe/Prague': [50.0, 14.4],
            'Europe/Budapest': [47.5, 19.0],
            'Europe/Bucharest': [44.4, 26.1],
            // Nordics & Baltics
            'Europe/Helsinki': [60.2, 24.9],
            'Europe/Stockholm': [59.3, 18.0],
            'Europe/Riga': [56.9, 24.1],
            'Europe/Vilnius': [54.6, 25.2],
            'Europe/Tallinn': [59.4, 24.7],
            // Russia (all timezones)
            'Europe/Moscow': [55.7, 37.6],
            'Europe/Samara': [53.2, 50.1],
            'Europe/Kaliningrad': [54.7, 20.5],
            'Asia/Yekaterinburg': [56.8, 60.6],
            'Asia/Omsk': [54.9, 73.3],
            'Asia/Novosibirsk': [55.0, 82.9],
            'Asia/Krasnoyarsk': [56.0, 92.8],
            'Asia/Irkutsk': [52.3, 104.3],
            'Asia/Yakutsk': [62.0, 129.7],
            'Asia/Vladivostok': [43.1, 131.9],
            'Asia/Magadan': [59.5, 150.8],
            'Asia/Kamchatka': [53.0, 158.6],
            // CIS & neighbors
            'Europe/Minsk': [53.9, 27.5],
            'Europe/Kiev': [50.4, 30.5],
            'Europe/Kyiv': [50.4, 30.5],
            'Europe/Chisinau': [47.0, 28.8],
            'Asia/Almaty': [43.2, 76.9],
            'Asia/Tashkent': [41.3, 69.2],
            'Asia/Tbilisi': [41.7, 44.8],
            'Asia/Baku': [40.4, 49.8],
            'Asia/Yerevan': [40.1, 44.5],
            'Asia/Bishkek': [42.8, 74.5],
            'Asia/Dushanbe': [38.5, 68.7],
            'Asia/Ashgabat': [37.9, 58.3],
            // Turkey & Middle East
            'Europe/Istanbul': [41.0, 28.9],
            'Asia/Dubai': [25.2, 55.2],
            'Asia/Tehran': [35.6, 51.3],
            'Asia/Jerusalem': [31.7, 35.2],
            // Asia Pacific
            'Asia/Tokyo': [35.6, 139.7],
            'Asia/Shanghai': [31.2, 121.4],
            'Asia/Hong_Kong': [22.3, 114.1],
            'Asia/Singapore': [1.3, 103.8],
            'Asia/Seoul': [37.5, 126.9],
            'Asia/Bangkok': [13.7, 100.5],
            'Asia/Kolkata': [28.6, 77.2],
            'Australia/Sydney': [-33.8, 151.2],
        }
        const match = tzCoords[tz]
        if (match) return { lat: match[0], lon: match[1] }
    } catch { /* ignore */ }
    return null
}

export default function Servers() {
    const { t } = useTranslation()
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
    const [pinningNodeId, setPinningNodeId] = useState<number | null>(null)
    const [pinnedNodeId, setPinnedNodeId] = useState<number | null>(activeSub?.last_node_id ?? null)
    const [pinMessage, setPinMessage] = useState<string | null>(null)
    const [clientType, setClientType] = useState('singbox')
    const [selectedVariant, setSelectedVariant] = useState<string>('')
    const [configUrl, setConfigUrl] = useState('')
    const [copied, setCopied] = useState(false)
    const [relayCountries, setRelayCountries] = useState<{code: string, flag: string, name: string}[]>([])
    const [selectedRelay, setSelectedRelay] = useState<string>(() => {
        try { return localStorage.getItem(`relay_${activeSub?.id}`) || 'auto' } catch { return 'auto' }
    })

    const handleRelayChange = (code: string) => {
        setSelectedRelay(code)
        try { if (activeSub) localStorage.setItem(`relay_${activeSub.id}`, code) } catch { /* ignore */ }
    }

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
        } catch {
            // Телеметрия не критична — тихая ошибка не ломает UX
        }
    }

    useEffect(() => {
        if (!token) return;
        const controller = new AbortController()
        const fetchData = async () => {
            try {
                const query = activeSub ? `?sub_id=${activeSub.id}` : ''
                const coords = estimateCoords()
                const coordsParam = coords ? `&lat=${coords.lat}&lon=${coords.lon}` : ''
                const res = await fetch(`/api/client/servers${query}${coordsParam}`, {
                    headers: { 'Authorization': `Bearer ${token}` },
                    signal: controller.signal,
                });
                if (res.ok) setServers(await res.json());
            } catch (e: unknown) {
                if (e instanceof Error && e.name === 'AbortError') return;
                // Тихая ошибка — пользователь увидит пустой список серверов
            } finally {
                setLoading(false);
            }
        };
        const fetchRelays = async () => {
            try {
                const res = await fetch('/api/client/relay-countries', {
                    headers: { Authorization: `Bearer ${token}` },
                    signal: controller.signal,
                })
                if (res.ok) setRelayCountries(await res.json())
            } catch { /* ignore */ }
        };
        void fetchData();
        void fetchRelays();
        return () => { controller.abort(); };
    }, [activeSub?.id, token]);

    const handlePinServer = async (nodeId: number | null) => {
        if (!activeSub || !token) return
        setPinningNodeId(nodeId)
        setPinMessage(null)
        try {
            const res = await fetch(`/api/client/subscription/${activeSub.id}/server`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
                body: JSON.stringify({ node_id: nodeId }),
            })
            if (res.ok) {
                setPinnedNodeId(nodeId)
                setPinMessage(nodeId ? t('servers.serverSelected') : t('servers.autoEnabled'))
            } else {
                setPinMessage(t('servers.switchError'))
            }
        } catch { setPinMessage(t('servers.networkError')) }
        finally { setPinningNodeId(null) }
    }

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
        setConfigUrl(buildConfigUrl(activeSub, { client: type, nodeId, variantId, relayCountry: selectedRelay }));
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

    if (loading) return <div className="page"><div className="loading">{t('servers.loadingServers')}</div></div>

    return (
        <div className="page servers-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate(-1)}>←</button>
                <h2>{t('servers.title')}</h2>
                <span className="badge badge-success">
                    {t('servers.onlineCount', { count: servers.filter(s => s.status === 'online').length })}
                </span>
            </header>

            <p className="servers-context-note">
                {t('servers.contextNote')}
            </p>

            {(() => {
                const countries = [...new Set(servers.map(s => s.country_code))];
                return countries.length > 1 ? (
                    <div className="country-quick-picker">
                        <button className={`country-chip ${!countryFilter ? 'active' : ''}`} onClick={() => setCountryFilter(null)}>
                            {t('servers.filterAll')}
                        </button>
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

            {relayCountries.length > 0 && (
                <div className="relay-picker glass-card" style={{ padding: 'var(--space-sm) var(--space-md)' }}>
                    <p style={{ fontSize: 13, color: 'var(--text-secondary)', marginBottom: 6 }}>
                        {t('servers.relayTitle')}
                    </p>
                    <div className="country-quick-picker">
                        <button className={`country-chip ${selectedRelay === 'none' ? 'active' : ''}`} onClick={() => handleRelayChange('none')}>
                            {t('servers.relayNone')}
                        </button>
                        {relayCountries.map(rc => (
                            <button key={rc.code} className={`country-chip ${selectedRelay === rc.code ? 'active' : ''}`} onClick={() => handleRelayChange(rc.code)}>
                                {rc.flag} {rc.name}
                            </button>
                        ))}
                    </div>
                </div>
            )}

            {pinMessage && <div className="home-banner success">{pinMessage}</div>}

            {pinnedNodeId && (
                <button className="btn-ghost" style={{ width: '100%', marginBottom: 8 }} onClick={() => handlePinServer(null)}>
                    {t('servers.autoSelect')}
                </button>
            )}

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
                                    <span className="badge badge-success">
                                        {t('servers.variants', { count: server.available_variant_ids.length })}
                                    </span>
                                )}
                                {i === 0 && !countryFilter && <span className="badge badge-warning">{t('servers.best')}</span>}
                                {server.is_full && <span className="badge badge-error">{t('servers.full')}</span>}
                            </div>
                        </div>
                        <div className="server-actions">
                            <button
                                className={`btn-primary ${pinnedNodeId === server.id ? 'pinned' : ''}`}
                                onClick={() => handlePinServer(server.id)}
                                disabled={server.is_full || pinningNodeId !== null}
                            >
                                {pinningNodeId === server.id
                                    ? '...'
                                    : pinnedNodeId === server.id
                                        ? t('servers.selected')
                                        : server.is_full
                                            ? t('servers.full')
                                            : t('servers.connect')}
                            </button>
                            <button
                                className="btn-secondary"
                                onClick={() => handleGetConfig(server)}
                                disabled={server.is_full}
                            >
                                {t('servers.configure')}
                            </button>
                        </div>
                    </div>
                    );
                })}
            </div>

            {selectedServer && (
                <div className="modal-overlay" onClick={() => setSelectedServer(null)}>
                    <div className="modal-content" onClick={e => e.stopPropagation()}>
                        <h3>{selectedServer.flag} {selectedServer.name}</h3>
                        <p className="connection-modal-note">
                            {t('servers.modalNote')}
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
                                    <strong>{t('servers.variantSectionTitle')}</strong>
                                    <span>{t('servers.variantSectionDesc')}</span>
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
                                                    <span className="variant-chip recommended">{t('servers.recommended')}</span>
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
                                            <span>{variant.transport || t('servers.standardTransport')}</span>
                                            <span>{variant.relay ? t('servers.viaRelay') : t('servers.directRoute')}</span>
                                        </span>
                                        <span className="variant-summary">{variant.summary}</span>
                                        {variant.id === selectedServer.recommended_variant_id && (
                                            <span className="variant-recommendation-note">{t('servers.bestVariantNote')}</span>
                                        )}
                                    </button>
                                ))}
                                </div>
                            </>
                        )}
                        {clientType === 'singbox' && prioritizedVariants.length === 0 && (
                            <div className="variant-empty-state">
                                {t('servers.noVariants')}
                            </div>
                        )}
                        <div className="qr-wrapper">
                            <QRCodeSVG value={configUrl} size={160} bgColor="#fff" fgColor="#0D0D1A" />
                        </div>
                        <div className="config-url-label">{t('servers.importUrlLabel')}</div>
                        <div className="config-url-row">
                            <input type="text" readOnly value={configUrl} onClick={e => e.currentTarget.select()} />
                            <button className="btn-secondary" onClick={handleCopy}>
                                {copied ? t('common.copied') : t('common.copy')}
                            </button>
                        </div>
                        {clientType === 'singbox' && selectedVariant && (
                            <div className="variant-feedback-row">
                                <button
                                    className="btn-secondary variant-feedback success"
                                    onClick={() => handleConnectionFeedback('connection_succeeded')}
                                >
                                    {t('servers.working')}
                                </button>
                                <button
                                    className="btn-secondary variant-feedback danger"
                                    onClick={() => handleConnectionFeedback('connection_failed')}
                                >
                                    {t('servers.tryOther')}
                                </button>
                            </div>
                        )}
                        <button className="btn-secondary close-btn" onClick={() => setSelectedServer(null)}>
                            {t('common.close')}
                        </button>
                    </div>
                </div>
            )}
        </div>
    )
}

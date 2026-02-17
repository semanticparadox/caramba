import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../context/AuthContext'
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
}

export default function Servers() {
    const navigate = useNavigate()
    const { token, subscriptions } = useAuth()
    const activeSub = subscriptions.find(s => s.status === 'active')
    const [servers, setServers] = useState<ServerInfo[]>([])
    const [loading, setLoading] = useState(true)
    const [selectedServer, setSelectedServer] = useState<ServerInfo | null>(null)
    const [clientType, setClientType] = useState('singbox')
    const [configUrl, setConfigUrl] = useState('')
    const [copied, setCopied] = useState(false)

    useEffect(() => {
        if (!token) return;
        const fetchData = async () => {
            try {
                const res = await fetch('/api/client/servers', {
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (res.ok) setServers(await res.json());
            } catch (e) { console.error(e); }
            finally { setLoading(false); }
        };
        fetchData();
    }, [token]);

    const handleGetConfig = (server: ServerInfo) => {
        if (!activeSub) return;
        setSelectedServer(server);
        updateConfigUrl(server.id, clientType);
    }

    const updateConfigUrl = (nodeId: number, type: string) => {
        if (!activeSub) return;
        let base = activeSub.subscription_url;
        const sep = base.includes('?') ? '&' : '?';
        setConfigUrl(`${base}${sep}client=${type}&node_id=${nodeId}`);
    }

    const handleClientChange = (type: string) => {
        setClientType(type);
        if (selectedServer) updateConfigUrl(selectedServer.id, type);
    }

    const handleCopy = () => {
        navigator.clipboard.writeText(configUrl);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
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
                        <div className="qr-wrapper">
                            <QRCodeSVG value={configUrl} size={160} bgColor="#fff" fgColor="#0D0D1A" />
                        </div>
                        <div className="config-url-row">
                            <input type="text" readOnly value={configUrl} onClick={e => e.currentTarget.select()} />
                            <button className="btn-secondary" onClick={handleCopy}>
                                {copied ? '✓' : '📋'}
                            </button>
                        </div>
                        <button className="btn-secondary close-btn" onClick={() => setSelectedServer(null)}>
                            Close
                        </button>
                    </div>
                </div>
            )}
        </div>
    )
}

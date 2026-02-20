import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { QRCodeSVG } from 'qrcode.react';
import { useAuth, UserSubscription } from '../context/AuthContext';
import './Subscription.css';

function formatTraffic(gb: number): string {
    if (gb >= 1024) return `${(gb / 1024).toFixed(1)} TB`;
    return `${gb} GB`;
}

function formatDateTime(value?: string | null): string {
    if (!value) return '—';
    const ts = new Date(value);
    if (Number.isNaN(ts.getTime())) return value;
    return ts.toLocaleString();
}

export default function Subscription() {
    const { subscriptions, isLoading, refreshData, token } = useAuth();
    const navigate = useNavigate();
    const [expandedId, setExpandedId] = useState<number | null>(null);
    const [copied, setCopied] = useState<number | null>(null);
    const [copiedVless, setCopiedVless] = useState<number | null>(null);
    const [activatingId, setActivatingId] = useState<number | null>(null);
    const [giftingId, setGiftingId] = useState<number | null>(null);
    const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);

    const handleCopy = (sub: UserSubscription) => {
        navigator.clipboard.writeText(sub.subscription_url);
        setCopied(sub.id);
        setTimeout(() => setCopied(null), 2000);
    };

    const handleCopyVless = (sub: UserSubscription) => {
        if (!sub.primary_vless_link) return;
        navigator.clipboard.writeText(sub.primary_vless_link);
        setCopiedVless(sub.id);
        setTimeout(() => setCopiedVless(null), 2000);
    };

    const handleActivate = async (subId: number) => {
        if (!token) return;

        setActivatingId(subId);
        setMessage(null);
        try {
            const res = await fetch(`/api/client/subscription/${subId}/activate`, {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                },
            });

            if (res.ok) {
                const data = await res.json();
                setMessage({
                    type: 'success',
                    text: data?.message || 'Subscription activated successfully.',
                });
                await refreshData();
                setExpandedId(subId);
            } else {
                const err = await res.text();
                setMessage({ type: 'error', text: err || 'Failed to activate subscription.' });
            }
        } catch {
            setMessage({ type: 'error', text: 'Network error while activating subscription.' });
        } finally {
            setActivatingId(null);
        }
    };

    const handleConvertToGift = async (subId: number) => {
        if (!token) return;

        setGiftingId(subId);
        setMessage(null);
        try {
            const res = await fetch(`/api/client/subscription/${subId}/gift`, {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                },
            });

            if (res.ok) {
                const data = await res.json();
                const code = data?.code ? ` ${data.code}` : '';
                if (data?.code) {
                    navigator.clipboard.writeText(data.code).catch(() => undefined);
                }
                setMessage({
                    type: 'success',
                    text: `Gift code created.${code ? ` Copied:${code}` : ''}`,
                });
                await refreshData();
            } else {
                const err = await res.text();
                setMessage({ type: 'error', text: err || 'Failed to convert subscription to gift code.' });
            }
        } catch {
            setMessage({ type: 'error', text: 'Network error while converting to gift code.' });
        } finally {
            setGiftingId(null);
        }
    };

    const toggleExpand = (id: number) => {
        setExpandedId(expandedId === id ? null : id);
    };

    if (isLoading) return <div className="page"><div className="loading">Loading subscriptions...</div></div>;

    // Sort: active first, then pending, then by created_at desc
    const sorted = [...subscriptions].sort((a, b) => {
        const order: Record<string, number> = { active: 0, pending: 1, expired: 2 };
        const diff = (order[a.status] ?? 3) - (order[b.status] ?? 3);
        if (diff !== 0) return diff;
        return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
    });

    return (
        <div className="page sub-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')}>←</button>
                <h2>My Services</h2>
                {subscriptions.length > 0 && (
                    <span className="badge badge-success">{subscriptions.filter(s => s.status === 'active').length} active</span>
                )}
            </header>

            {message && (
                <div className={`purchase-msg ${message.type}`}>
                    {message.text}
                </div>
            )}

            {sorted.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-icon">🔒</div>
                    <h3>No Subscriptions</h3>
                    <p>Start a subscription to access premium VPN servers.</p>
                    <button className="btn-primary" onClick={() => navigate('/plans')}>
                        🛒 Buy Subscription
                    </button>
                </div>
            ) : (
                <div className="subs-list">
                    {sorted.map((sub, index) => (
                        <div key={sub.id} className={`sub-card glass-card ${sub.status}`}>
                            {/* Header Row */}
                            <div className="sub-header" onClick={() => toggleExpand(sub.id)}>
                                <div className="sub-header-left">
                                    <span className="sub-number">#{index + 1}</span>
                                    <div className="sub-plan-info">
                                        <span className="sub-plan-name">{sub.plan_name}</span>
                                        <span className={`badge badge-${sub.status === 'active' ? 'success' : sub.status === 'pending' ? 'warning' : 'error'}`}>
                                            {sub.status === 'active' ? '✅' : sub.status === 'pending' ? '⏳' : '⛔'} {sub.status}
                                        </span>
                                    </div>
                                </div>
                                <span className="expand-arrow">{expandedId === sub.id ? '▲' : '▼'}</span>
                            </div>

                            {/* Traffic Bar */}
                            <div className="sub-traffic">
                                <div className="traffic-bar-row">
                                    <span>📊 Traffic</span>
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

                            {/* Expiry */}
                            <div className="sub-meta-row">
                                {sub.status === 'active' ? (
                                    <>
                                        <span>⏳ {sub.days_left > 0 ? `${sub.days_left} days left` : sub.duration_days === 0 ? 'No expiration' : 'Expiring soon'}</span>
                                        <span className="sub-date">{sub.duration_days > 0 ? new Date(sub.expires_at).toLocaleDateString() : 'Traffic Plan'}</span>
                                    </>
                                ) : sub.status === 'pending' ? (
                                    <span>⏱ {sub.duration_days > 0 ? `${sub.duration_days} days (starts on activation)` : 'Traffic Plan'}</span>
                                ) : (
                                    <span>Expired</span>
                                )}
                            </div>

                            {/* Note */}
                            {sub.note && (
                                <div className="sub-note">
                                    📝 {sub.note}
                                </div>
                            )}

                            <div className="sub-extra-row">
                                <span>
                                    📱 Devices: {sub.active_devices ?? 0}/{(sub.device_limit ?? 0) > 0 ? sub.device_limit : '∞'}
                                </span>
                                {sub.last_node_name && (
                                    <span>
                                        🌍 Last node: {sub.last_node_flag ? `${sub.last_node_flag} ` : ''}{sub.last_node_name}
                                        {sub.last_node_id ? ` (#${sub.last_node_id})` : ''}
                                    </span>
                                )}
                            </div>
                            <div className="sub-extra-row">
                                <span>🕒 Last config pull: {formatDateTime(sub.last_sub_access)}</span>
                            </div>

                            {/* Actions Row */}
                            {sub.status === 'active' && (
                                <div className="sub-actions">
                                    <button
                                        className="btn-text"
                                        onClick={(e) => { e.stopPropagation(); navigate(`/servers/${sub.id}`); }}
                                    >
                                        ⚙️ Optimize Connection
                                    </button>
                                </div>
                            )}
                            {sub.status === 'pending' && (
                                <div className="sub-actions">
                                    <button
                                        className="btn-text"
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            handleActivate(sub.id);
                                        }}
                                        disabled={activatingId !== null || giftingId !== null}
                                    >
                                        {activatingId === sub.id ? '⏳ Activating...' : '✅ Activate Now'}
                                    </button>
                                    <button
                                        className="btn-text"
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            handleConvertToGift(sub.id);
                                        }}
                                        disabled={activatingId !== null || giftingId !== null}
                                    >
                                        {giftingId === sub.id ? '⏳ Creating code...' : '🎁 Make Gift Code'}
                                    </button>
                                </div>
                            )}

                            {/* Expanded: QR + Link */}
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
                                    <p className="qr-hint">Scan with your VPN app</p>

                                    <div className="link-row">
                                        <input type="text" readOnly value={sub.subscription_url} onClick={e => e.currentTarget.select()} />
                                        <button
                                            className={`btn-secondary copy-btn ${copied === sub.id ? 'copied' : ''}`}
                                            onClick={() => handleCopy(sub)}
                                        >
                                            {copied === sub.id ? '✓' : '📋'}
                                        </button>
                                    </div>

                                    {sub.primary_vless_link && (
                                        <div className="link-row">
                                            <input type="text" readOnly value={sub.primary_vless_link} onClick={e => e.currentTarget.select()} />
                                            <button
                                                className={`btn-secondary copy-btn ${copiedVless === sub.id ? 'copied' : ''}`}
                                                onClick={() => handleCopyVless(sub)}
                                            >
                                                {copiedVless === sub.id ? '✓' : 'VLESS'}
                                            </button>
                                        </div>
                                    )}

                                    {sub.is_trial && (
                                        <div className="badge badge-warning trial-badge">Free Trial</div>
                                    )}
                                </div>
                            )}
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
}

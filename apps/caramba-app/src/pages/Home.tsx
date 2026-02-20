import { useNavigate } from 'react-router-dom'
import { useAuth } from '../context/AuthContext'
import './Home.css'

function formatBytes(bytes: number, decimals = 2): string {
    if (!bytes || bytes === 0) return '0 B';
    const k = 1024;
    const dm = decimals < 0 ? 0 : decimals;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + sizes[i];
}

const MENU_ITEMS = [
    { path: '/plans', icon: '🛍', title: 'Buy Subscription', subtitle: 'Browse plans & purchase', gradient: 'linear-gradient(135deg, #F59E0B 0%, #EF4444 100%)' },
    { path: '/subscription', icon: '🔐', title: 'My Services', subtitle: 'Subscriptions & config', gradient: 'var(--accent-gradient)' },
    { path: '/promo', icon: '🎫', title: 'Promo Center', subtitle: 'Redeem promo & gift codes', gradient: 'linear-gradient(135deg, #8B5CF6 0%, #D946EF 100%)' },
    { path: '/referral', icon: '🎁', title: 'Referral', subtitle: 'Invite & Earn', gradient: 'linear-gradient(135deg, #F59E0B 0%, #10B981 100%)' },
    { path: '/support', icon: '💬', title: 'Support', subtitle: 'Help & FAQ', gradient: 'linear-gradient(135deg, #6366F1 0%, #EC4899 100%)' },
]

export default function Home() {
    const navigate = useNavigate()
    const { userStats: stats, isLoading, user, subscriptions } = useAuth()

    const activeSubscriptions = subscriptions.filter((s) => s.status === 'active')
    const totalUsedFromSubs = activeSubscriptions.reduce((acc, sub) => acc + (sub.used_traffic_bytes || 0), 0)
    const totalLimitFromSubs = activeSubscriptions.reduce((acc, sub) => {
        const limitBytes = Math.max(0, sub.traffic_limit_gb || 0) * 1024 * 1024 * 1024
        return acc + limitBytes
    }, 0)

    const effectiveUsed = totalLimitFromSubs > 0 ? totalUsedFromSubs : (stats?.traffic_used || 0)
    const effectiveLimit = totalLimitFromSubs > 0 ? totalLimitFromSubs : (stats?.traffic_limit || 0)
    const effectiveDaysLeft = activeSubscriptions.length > 0
        ? Math.min(...activeSubscriptions.map((s) => Math.max(0, s.days_left || 0)))
        : (stats?.days_left ?? null)

    const percentage = effectiveLimit > 0
        ? Math.min(100, Math.round((effectiveUsed / effectiveLimit) * 100))
        : 0

    // SVG circular progress
    const radius = 54;
    const circumference = 2 * Math.PI * radius;
    const strokeOffset = circumference - (percentage / 100) * circumference;

    const subscriptionsPreview = activeSubscriptions
        .slice()
        .sort((a, b) => (a.days_left || 0) - (b.days_left || 0))
        .slice(0, 3)

    return (
        <div className="page home-page">
            {/* Header */}
            <div className="home-header">
                <div className="logo-section">
                    <span className="logo-icon">🚀</span>
                    <h1 className="gradient-text">EXA-ROBOT</h1>
                </div>
                {user && <p className="user-greeting">Welcome, {user.username || 'User'}</p>}
            </div>

            {/* Traffic Ring */}
            <div className="traffic-card glass-card">
                <div className="traffic-ring-container">
                    <svg className="traffic-ring" viewBox="0 0 120 120">
                        <circle className="ring-bg" cx="60" cy="60" r={radius} />
                        <circle
                            className="ring-progress"
                            cx="60" cy="60" r={radius}
                            strokeDasharray={circumference}
                            strokeDashoffset={strokeOffset}
                        />
                    </svg>
                    <div className="ring-label">
                        <span className="ring-percent">{isLoading ? '...' : `${percentage}%`}</span>
                        <span className="ring-text">Data Usage</span>
                    </div>
                </div>
                <div className="traffic-meta">
                    <div className="traffic-stat">
                        <span className="stat-label">Used</span>
                        <span className="stat-value">{isLoading ? '...' : formatBytes(effectiveUsed)}</span>
                    </div>
                    <div className="traffic-divider" />
                    <div className="traffic-stat">
                        <span className="stat-label">Limit</span>
                        <span className="stat-value">{isLoading ? '...' : formatBytes(effectiveLimit)}</span>
                    </div>
                    <div className="traffic-divider" />
                    <div className="traffic-stat">
                        <span className="stat-label">Days left</span>
                        <span className="stat-value">{isLoading ? '...' : (effectiveDaysLeft ?? '—')}</span>
                    </div>
                </div>
            </div>

            <div className="subs-overview glass-card">
                <div className="subs-overview-header">
                    <h3>My Active Subscriptions</h3>
                    <span className="subs-counter">{activeSubscriptions.length}</span>
                </div>
                {subscriptionsPreview.length === 0 ? (
                    <p className="subs-empty">
                        No active subscriptions yet. Open Buy Subscription to get started.
                    </p>
                ) : (
                    <div className="subs-list">
                        {subscriptionsPreview.map((sub) => (
                            <button
                                key={sub.id}
                                className="subs-item"
                                onClick={() => navigate('/subscription')}
                            >
                                <div className="subs-item-title">{sub.plan_name}</div>
                                <div className="subs-item-meta">
                                    <span>{sub.used_traffic_gb} GB / {sub.traffic_limit_gb || '∞'} GB</span>
                                    <span>{sub.days_left}d left</span>
                                </div>
                            </button>
                        ))}
                    </div>
                )}
                {activeSubscriptions.length > subscriptionsPreview.length && (
                    <button className="subs-view-all" onClick={() => navigate('/subscription')}>
                        View all subscriptions
                    </button>
                )}
            </div>

            {/* Quick Actions Grid */}
            <div className="quick-actions">
                {MENU_ITEMS.map((item) => (
                    <button
                        key={item.path}
                        className="action-card glass-card"
                        onClick={() => navigate(item.path)}
                    >
                        <div className="action-icon-wrap" style={{ background: item.gradient }}>
                            <span className="action-icon">{item.icon}</span>
                        </div>
                        <span className="action-title">{item.title}</span>
                        <span className="action-subtitle">{item.subtitle}</span>
                    </button>
                ))}
            </div>

            {/* Status */}
            <div className="status-bar">
                <div className="status-dot" />
                <span>Connected</span>
            </div>
        </div>
    )
}

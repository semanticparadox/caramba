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
    { path: '/subscription', icon: '🔐', title: 'My Services', subtitle: 'Subscriptions & config', gradient: 'var(--accent-gradient)' },
    { path: '/servers', icon: '🌐', title: 'Servers', subtitle: 'Choose location', gradient: 'var(--accent-gradient-green)' },
    { path: '/store', icon: '📦', title: 'Store', subtitle: 'Digital products', gradient: 'linear-gradient(135deg, #8B5CF6 0%, #D946EF 100%)' },
    { path: '/statistics', icon: '📊', title: 'Statistics', subtitle: 'Usage charts', gradient: 'var(--accent-gradient-pink)' },
    { path: '/billing', icon: '💎', title: 'Billing', subtitle: 'Balance & Top up', gradient: 'var(--accent-gradient-warm)' },
    { path: '/referral', icon: '🎁', title: 'Referral', subtitle: 'Invite & Earn', gradient: 'linear-gradient(135deg, #F59E0B 0%, #10B981 100%)' },
    { path: '/support', icon: '💬', title: 'Support', subtitle: 'Help & FAQ', gradient: 'linear-gradient(135deg, #6366F1 0%, #EC4899 100%)' },
]

export default function Home() {
    const navigate = useNavigate()
    const { userStats: stats, isLoading, user } = useAuth()

    const percentage = stats && stats.traffic_limit > 0
        ? Math.min(100, Math.round((stats.traffic_used / stats.traffic_limit) * 100))
        : 0;

    // SVG circular progress
    const radius = 54;
    const circumference = 2 * Math.PI * radius;
    const strokeOffset = circumference - (percentage / 100) * circumference;

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
                        <span className="stat-value">{stats ? formatBytes(stats.traffic_used) : '—'}</span>
                    </div>
                    <div className="traffic-divider" />
                    <div className="traffic-stat">
                        <span className="stat-label">Limit</span>
                        <span className="stat-value">{stats ? formatBytes(stats.traffic_limit) : '—'}</span>
                    </div>
                    <div className="traffic-divider" />
                    <div className="traffic-stat">
                        <span className="stat-label">Days left</span>
                        <span className="stat-value">{stats?.days_left ?? '—'}</span>
                    </div>
                </div>
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

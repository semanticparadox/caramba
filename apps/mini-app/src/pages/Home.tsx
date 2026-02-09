import { useNavigate } from 'react-router-dom'
import { useAuth } from '../context/AuthContext'
import './Home.css'

function formatBytes(bytes: number, decimals = 2) {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const dm = decimals < 0 ? 0 : decimals;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + sizes[i];
}

export default function Home() {
    const navigate = useNavigate()
    const { userStats: stats, isLoading } = useAuth()

    if (isLoading) {
        return <div className="loading">Loading...</div>
    }

    const percentage = stats && stats.traffic_limit > 0
        ? Math.min(100, Math.round((stats.traffic_used / stats.traffic_limit) * 100))
        : 0;

    return (
        <div className="page home-page">
            <header className="header">
                <h1>🚀 EXA-ROBOT</h1>
                {stats && <div className="plan-badge">{stats.plan_name}</div>}
            </header>

            <div className="traffic-widget">
                <div className="traffic-header">
                    <span>Traffic Usage</span>
                    <span className="traffic-percentage">{isLoading ? '...' : `${percentage}%`}</span>
                </div>
                <div className="progress-bar">
                    <div className="progress-fill" style={{ width: `${percentage}%` }}></div>
                </div>
                <div className="traffic-details">
                    <span>
                        {stats ? `${formatBytes(stats.traffic_used)} / ${formatBytes(stats.traffic_limit)}` : 'Loading...'}
                    </span>
                    <span>
                        {stats ? `${stats.days_left} days left` : '...'}
                    </span>
                </div>
            </div>

            <div className="quick-actions">
                <button
                    className="action-card"
                    onClick={() => navigate('/subscription')}
                >
                    <div className="icon">📱</div>
                    <div className="title">Subscription</div>
                    <div className="subtitle">View links & QR codes</div>
                </button>

                <button
                    className="action-card"
                    onClick={() => navigate('/servers')}
                >
                    <div className="icon">🌍</div>
                    <div className="title">Servers</div>
                    <div className="subtitle">Choose best server</div>
                </button>

                <button
                    className="action-card"
                    onClick={() => navigate('/statistics')}
                >
                    <div className="icon">📊</div>
                    <div className="title">Statistics</div>
                    <div className="subtitle">View detailed charts</div>
                </button>

                <button
                    className="action-card"
                    onClick={() => navigate('/billing')}
                >
                    <div className="icon">💳</div>
                    <div className="title">Billing</div>
                    <div className="subtitle">Balance & History</div>
                </button>

                <button
                    className="action-card"
                    onClick={() => navigate('/referral')}
                >
                    <div className="icon">🎁</div>
                    <div className="title">Referral</div>
                    <div className="subtitle">Invite & Earn</div>
                </button>
                <button
                    className="action-card"
                    onClick={() => navigate('/support')}
                >
                    <div className="icon">❓</div>
                    <div className="title">Support</div>
                    <div className="subtitle">FAQ & Chat</div>
                </button>
            </div>

            <div className="status-indicator">
                <div className="status-dot active"></div>
                <span>Connected</span>
            </div>
        </div>
    )
}

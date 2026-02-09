import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { QRCodeSVG } from 'qrcode.react';
import { useAuth } from '../context/AuthContext';
import './Subscription.css';

export default function Subscription() {
    const { subscription, isLoading, userStats: stats } = useAuth();
    const navigate = useNavigate();
    const [copied, setCopied] = useState(false);

    const handleCopy = () => {
        if (subscription?.subscription_url) {
            navigator.clipboard.writeText(subscription.subscription_url);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        }
    };

    if (isLoading) return <div className="page"><div className="loading">Loading subscription...</div></div>;

    if (!subscription) return (
        <div className="page sub-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')}>←</button>
                <h2>Subscription</h2>
            </header>
            <div className="empty-state">
                <div className="empty-icon">🔒</div>
                <h3>No Active Subscription</h3>
                <p>Start a subscription to access premium VPN servers.</p>
                <button className="btn-primary" onClick={() => window.open('https://t.me/exarobot_bot', '_blank')}>
                    🛒 Buy Subscription
                </button>
            </div>
        </div>
    );

    return (
        <div className="page sub-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')}>←</button>
                <h2>Subscription</h2>
                {stats && <span className="badge badge-success">{stats.days_left}d left</span>}
            </header>

            <div className="qr-card glass-card">
                <h3>Your Access Key</h3>
                <div className="qr-wrapper">
                    <QRCodeSVG
                        value={subscription.subscription_url}
                        size={180}
                        bgColor={"#ffffff"}
                        fgColor={"#0D0D1A"}
                        level={"M"}
                        includeMargin={true}
                    />
                </div>
                <p className="qr-hint">Scan with your VPN app</p>
            </div>

            <div className="link-card glass-card">
                <h3>Subscription Link</h3>
                <div className="link-input-wrap">
                    <input
                        type="text"
                        readOnly
                        value={subscription.subscription_url}
                        onClick={(e) => e.currentTarget.select()}
                    />
                </div>
                <button
                    className={`btn-primary ${copied ? 'copied' : ''}`}
                    onClick={handleCopy}
                >
                    {copied ? '✓ Copied!' : '📋 Copy Link'}
                </button>
            </div>
        </div>
    );
}

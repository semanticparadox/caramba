import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { QRCodeSVG } from 'qrcode.react'; // Using SVG for better quality
import { useAuth } from '../context/AuthContext';
import './Subscription.css';

export default function Subscription() {
    const { subscription, isLoading } = useAuth();
    const navigate = useNavigate();
    const [copied, setCopied] = useState(false);

    const handleCopy = () => {
        if (subscription?.subscription_url) {
            navigator.clipboard.writeText(subscription.subscription_url);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        }
    };

    if (isLoading) return <div className="page subscription-page"><div className="loading">Loading subscription...</div></div>;

    if (!subscription) return (
        <div className="page subscription-page error">
            <div className="header">
                <button className="back-button" onClick={() => navigate('/')}>Back</button>
            </div>
            <div className="content">
                <p>No active subscription found.</p>
                <div className="empty-state-action">
                    <button onClick={() => window.open('https://t.me/exarobot_bot', '_blank')}>Buy Subscription</button>
                </div>
            </div>
        </div>
    );

    return (
        <div className="page subscription-page">
            <div className="header">
                <h1>Subscription</h1>
                <button className="back-button" onClick={() => navigate('/')}>Back</button>
            </div>

            <div className="card qr-card">
                <h3>Your Access Key</h3>
                <div className="qr-wrapper">
                    <QRCodeSVG
                        value={subscription.subscription_url}
                        size={200}
                        bgColor={"#ffffff"}
                        fgColor={"#000000"}
                        level={"M"}
                        includeMargin={true}
                    />
                </div>
                <p className="instruction">Scan this QR code with your VPN client</p>
            </div>

            <div className="card link-card">
                <h3>Subscription Link</h3>
                <div className="link-box">
                    <input
                        type="text"
                        readOnly
                        value={subscription.subscription_url}
                        onClick={(e) => e.currentTarget.select()}
                    />
                </div>
                <button
                    className={`copy-button ${copied ? 'copied' : ''}`}
                    onClick={handleCopy}
                >
                    {copied ? 'Copied!' : 'Copy Link'}
                </button>
            </div>

            <style>{`
                .subscription-page {
                    padding: 20px;
                    color: white;
                    min-height: 100vh;
                    background: var(--bg-color, #1a1a1a);
                }
                .header {
                    display: flex;
                    justify-content: space-between;
                    align-items: center;
                    margin-bottom: 20px;
                }
                .back-button {
                    background: rgba(255,255,255,0.1);
                    border: none;
                    color: white;
                    padding: 8px 16px;
                    border-radius: 8px;
                    cursor: pointer;
                }
                .card {
                    background: rgba(255,255,255,0.05);
                    border-radius: 16px;
                    padding: 20px;
                    margin-bottom: 20px;
                    text-align: center;
                }
                .qr-wrapper {
                    background: white;
                    padding: 10px;
                    border-radius: 10px;
                    display: inline-block;
                    margin: 20px 0;
                }
                .instruction {
                    color: #aaa;
                    font-size: 14px;
                }
                .link-box input {
                    width: 100%;
                    background: rgba(0,0,0,0.2);
                    border: 1px solid rgba(255,255,255,0.1);
                    color: #fff;
                    padding: 10px;
                    border-radius: 8px;
                    text-align: center;
                    font-family: monospace;
                    margin-bottom: 10px;
                }
                .copy-button {
                    width: 100%;
                    padding: 12px;
                    border-radius: 10px;
                    border: none;
                    background: #4ade80;
                    color: #004d20;
                    font-weight: bold;
                    cursor: pointer;
                    transition: all 0.2s;
                }
                .copy-button.copied {
                    background: #22c55e;
                    color: white;
                }
                .empty-state-action button {
                    background: #3b82f6;
                    color: white;
                    border: none;
                    padding: 10px 20px;
                    border-radius: 8px;
                    font-weight: bold;
                    margin-top: 10px;
                    cursor: pointer;
                }
            `}</style>
        </div>
    );
}

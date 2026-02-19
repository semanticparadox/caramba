import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '../context/AuthContext';
import './Promo.css';

type Banner = { type: 'success' | 'error'; text: string } | null;

export default function Promo() {
    const navigate = useNavigate();
    const { token, refreshData } = useAuth();
    const [promoCode, setPromoCode] = useState('');
    const [referrerCode, setReferrerCode] = useState('');
    const [redeeming, setRedeeming] = useState(false);
    const [linking, setLinking] = useState(false);
    const [banner, setBanner] = useState<Banner>(null);

    const headers = {
        Authorization: `Bearer ${token}`,
        'Content-Type': 'application/json',
    };

    const redeemPromo = async () => {
        const code = promoCode.trim();
        if (!code || !token) return;

        setRedeeming(true);
        setBanner(null);
        try {
            const res = await fetch('/api/client/promo/redeem', {
                method: 'POST',
                headers,
                body: JSON.stringify({ code }),
            });

            if (res.ok) {
                const data = await res.json();
                setBanner({
                    type: 'success',
                    text: data?.message || 'Code activated successfully.',
                });
                setPromoCode('');
                await refreshData();
            } else {
                const err = await res.text();
                setBanner({ type: 'error', text: err || 'Failed to redeem code.' });
            }
        } catch {
            setBanner({ type: 'error', text: 'Network error while redeeming code.' });
        } finally {
            setRedeeming(false);
        }
    };

    const linkReferrer = async () => {
        const code = referrerCode.trim();
        if (!code || !token) return;

        setLinking(true);
        setBanner(null);
        try {
            const res = await fetch('/api/client/user/referrer', {
                method: 'POST',
                headers,
                body: JSON.stringify({ code }),
            });

            if (res.ok) {
                const data = await res.json();
                setBanner({
                    type: 'success',
                    text: data?.message || 'Referrer linked successfully.',
                });
                setReferrerCode('');
            } else {
                const err = await res.text();
                setBanner({ type: 'error', text: err || 'Failed to link referrer.' });
            }
        } catch {
            setBanner({ type: 'error', text: 'Network error while linking referrer.' });
        } finally {
            setLinking(false);
        }
    };

    return (
        <div className="page promo-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')}>←</button>
                <h2>Promo Center</h2>
            </header>

            {banner && (
                <div className={`promo-banner ${banner.type}`}>
                    {banner.text}
                </div>
            )}

            <section className="promo-card glass-card">
                <h3>🎫 Redeem Promo or Gift Code</h3>
                <p>Activate bonuses, trial, or gifted subscriptions.</p>
                <div className="promo-input-row">
                    <input
                        type="text"
                        placeholder="Enter code"
                        value={promoCode}
                        onChange={(e) => setPromoCode(e.target.value.toUpperCase())}
                    />
                    <button
                        className="btn-primary"
                        onClick={redeemPromo}
                        disabled={redeeming || promoCode.trim().length === 0}
                    >
                        {redeeming ? 'Activating...' : 'Activate'}
                    </button>
                </div>
            </section>

            <section className="promo-card glass-card">
                <h3>👥 Link Referrer</h3>
                <p>Enter inviter code once to join referral program.</p>
                <div className="promo-input-row">
                    <input
                        type="text"
                        placeholder="Referral code"
                        value={referrerCode}
                        onChange={(e) => setReferrerCode(e.target.value)}
                    />
                    <button
                        className="btn-secondary"
                        onClick={linkReferrer}
                        disabled={linking || referrerCode.trim().length === 0}
                    >
                        {linking ? 'Saving...' : 'Link'}
                    </button>
                </div>
            </section>
        </div>
    );
}

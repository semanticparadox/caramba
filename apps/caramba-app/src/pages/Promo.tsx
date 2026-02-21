import { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '../context/AuthContext';
import './Promo.css';

type Banner = { type: 'success' | 'error'; text: string } | null;

interface GiftCode {
    id: number;
    code: string;
    plan_name?: string | null;
    duration_days?: number | null;
    status: string;
    created_at: string;
    redeemed_at?: string | null;
    can_revoke?: boolean;
}

export default function Promo() {
    const navigate = useNavigate();
    const { token, refreshData, subscriptions, error } = useAuth();
    const [promoCode, setPromoCode] = useState('');
    const [referrerCode, setReferrerCode] = useState('');
    const [giftCodes, setGiftCodes] = useState<GiftCode[]>([]);
    const [loadingCodes, setLoadingCodes] = useState(true);
    const [copyingCodeId, setCopyingCodeId] = useState<number | null>(null);
    const [revokingCodeId, setRevokingCodeId] = useState<number | null>(null);
    const [convertingSubId, setConvertingSubId] = useState<number | null>(null);
    const [redeeming, setRedeeming] = useState(false);
    const [linking, setLinking] = useState(false);
    const [banner, setBanner] = useState<Banner>(null);

    const headers = {
        Authorization: `Bearer ${token}`,
        'Content-Type': 'application/json',
    };

    const pendingSubs = useMemo(
        () => subscriptions.filter((s) => s.status === 'pending'),
        [subscriptions],
    );

    const loadGiftCodes = async () => {
        if (!token) return;
        setLoadingCodes(true);
        try {
            const res = await fetch('/api/client/promo/my-codes', {
                headers: { Authorization: `Bearer ${token}` },
            });
            if (res.ok) {
                const data = await res.json();
                setGiftCodes(Array.isArray(data) ? data : []);
            } else {
                setGiftCodes([]);
            }
        } catch {
            setGiftCodes([]);
        } finally {
            setLoadingCodes(false);
        }
    };

    useEffect(() => {
        if (!token) {
            setLoadingCodes(false);
            setBanner({
                type: 'error',
                text: error || 'Authorization required. Reopen Mini App from bot.',
            });
            return;
        }
        loadGiftCodes();
    }, [token, error]);

    const goBack = () => {
        if (window.history.length > 1) {
            navigate(-1);
        } else {
            navigate('/');
        }
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
                await loadGiftCodes();
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

    const convertPendingToGift = async (subId: number) => {
        if (!token) return;
        setConvertingSubId(subId);
        setBanner(null);
        try {
            const res = await fetch(`/api/client/subscription/${subId}/gift`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}` },
            });
            if (res.ok) {
                const data = await res.json();
                if (data?.code) {
                    navigator.clipboard.writeText(data.code).catch(() => undefined);
                }
                setBanner({
                    type: 'success',
                    text: data?.code
                        ? `Gift code created: ${data.code} (copied).`
                        : (data?.message || 'Gift code created.'),
                });
                await refreshData();
                await loadGiftCodes();
            } else {
                const err = await res.text();
                setBanner({ type: 'error', text: err || 'Failed to create gift code.' });
            }
        } catch {
            setBanner({ type: 'error', text: 'Network error while creating gift code.' });
        } finally {
            setConvertingSubId(null);
        }
    };

    const revokeGiftCode = async (giftId: number) => {
        if (!token) return;
        setRevokingCodeId(giftId);
        setBanner(null);
        try {
            const res = await fetch(`/api/client/promo/my-codes/${giftId}`, {
                method: 'DELETE',
                headers: { Authorization: `Bearer ${token}` },
            });
            if (res.ok) {
                setBanner({ type: 'success', text: 'Gift code revoked.' });
                await loadGiftCodes();
            } else {
                const err = await res.text();
                setBanner({ type: 'error', text: err || 'Failed to revoke gift code.' });
            }
        } catch {
            setBanner({ type: 'error', text: 'Network error while revoking gift code.' });
        } finally {
            setRevokingCodeId(null);
        }
    };

    const copyGiftCode = (gift: GiftCode) => {
        navigator.clipboard.writeText(gift.code);
        setCopyingCodeId(gift.id);
        setTimeout(() => setCopyingCodeId(null), 1500);
    };

    return (
        <div className="page promo-page">
            <header className="page-header">
                <button className="back-button" onClick={goBack}>←</button>
                <h2>Promo Center</h2>
            </header>

            {banner && (
                <div className={`promo-banner ${banner.type}`}>
                    {banner.text}
                </div>
            )}

            <section className="promo-card glass-card">
                <h3>🎁 Pending Subscriptions → Gift Codes</h3>
                <p>Convert a pending purchase into a reusable gift code before activation.</p>
                {pendingSubs.length === 0 ? (
                    <div className="promo-empty">No pending subscriptions available for conversion.</div>
                ) : (
                    <div className="promo-list">
                        {pendingSubs.map((sub) => (
                            <div key={sub.id} className="promo-list-item">
                                <div>
                                    <div className="promo-list-title">{sub.plan_name}</div>
                                    <div className="promo-list-meta">
                                        #{sub.id} • {sub.duration_days > 0 ? `${sub.duration_days} days` : 'Traffic plan'}
                                    </div>
                                </div>
                                <button
                                    className="btn-secondary"
                                    onClick={() => convertPendingToGift(sub.id)}
                                    disabled={convertingSubId !== null}
                                >
                                    {convertingSubId === sub.id ? 'Creating...' : 'Make Code'}
                                </button>
                            </div>
                        ))}
                    </div>
                )}
            </section>

            <section className="promo-card glass-card">
                <h3>🗂 My Gift Codes</h3>
                <p>Manage gift codes you created and share them with others.</p>
                {loadingCodes ? (
                    <div className="promo-empty">Loading your gift codes...</div>
                ) : giftCodes.length === 0 ? (
                    <div className="promo-empty">No gift codes yet.</div>
                ) : (
                    <div className="promo-list">
                        {giftCodes.map((gift) => (
                            <div key={gift.id} className="promo-list-item promo-code-item">
                                <div>
                                    <div className="promo-list-title code-text">{gift.code}</div>
                                    <div className="promo-list-meta">
                                        {(gift.plan_name || 'Plan')} • {gift.duration_days || 0} days • {gift.status}
                                    </div>
                                </div>
                                <div className="promo-actions">
                                    <button
                                        className="btn-secondary"
                                        onClick={() => copyGiftCode(gift)}
                                    >
                                        {copyingCodeId === gift.id ? 'Copied' : 'Copy'}
                                    </button>
                                    {gift.can_revoke && (
                                        <button
                                            className="btn-secondary btn-danger"
                                            onClick={() => revokeGiftCode(gift.id)}
                                            disabled={revokingCodeId !== null}
                                        >
                                            {revokingCodeId === gift.id ? 'Revoking...' : 'Revoke'}
                                        </button>
                                    )}
                                </div>
                            </div>
                        ))}
                    </div>
                )}
            </section>

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

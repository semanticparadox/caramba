import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '../context/AuthContext';
import './Plans.css';

interface PlanDuration {
    id: number;
    duration_days: number;
    price: number;
    price_cents: number;
}

interface Plan {
    id: number;
    name: string;
    description: string | null;
    traffic_limit_gb: number;
    device_limit: number;
    durations: PlanDuration[];
}

interface PaymentProvider {
    id: string;
    label: string;
}

export default function Plans() {
    const navigate = useNavigate();
    const { token, refreshData, user, error } = useAuth();
    const [plans, setPlans] = useState<Plan[]>([]);
    const [providers, setProviders] = useState<PaymentProvider[]>([]);
    const [loading, setLoading] = useState(true);
    const [purchasing, setPurchasing] = useState<number | null>(null);
    const [selectedDuration, setSelectedDuration] = useState<PlanDuration | null>(null);
    const [showPayModal, setShowPayModal] = useState(false);
    const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);

    const headers = { Authorization: `Bearer ${token}` };

    useEffect(() => {
        if (!token) {
            setLoading(false);
            setMessage({
                type: 'error',
                text: error || 'Authorization required. Reopen Mini App from bot.',
            });
            return;
        }

        const controller = new AbortController();
        const timeout = setTimeout(() => controller.abort(), 12000);

        (async () => {
            try {
                // Fetch plans
                const plansRes = await fetch('/api/client/plans', {
                    headers,
                    signal: controller.signal,
                });
                if (plansRes.ok) {
                    const data = await plansRes.json();
                    setPlans(Array.isArray(data) ? data : []);
                }

                // Fetch providers
                const providersRes = await fetch('/api/client/payment/providers', {
                    headers,
                    signal: controller.signal,
                });
                if (providersRes.ok) {
                    const data = await providersRes.json();
                    setProviders(data.providers || []);
                }

                setMessage(null);
            } catch (e: any) {
                console.error(e);
                setMessage({
                    type: 'error',
                    text: e?.name === 'AbortError' ? 'Loading data timed out. Try again.' : (e?.message || 'Failed to load plans.'),
                });
            } finally {
                clearTimeout(timeout);
                setLoading(false);
            }
        })();

        return () => {
            clearTimeout(timeout);
            controller.abort();
        };
    }, [token]);

    const handleSelectDuration = (duration: PlanDuration) => {
        setSelectedDuration(duration);
        setShowPayModal(true);
    };

    const handlePurchase = async (providerId: string) => {
        if (!selectedDuration) return;

        setPurchasing(selectedDuration.id);
        setMessage(null);
        setShowPayModal(false);

        try {
            const res = await fetch('/api/client/payment/invoice', {
                method: 'POST',
                headers: { ...headers, 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    duration_id: selectedDuration.id,
                    provider: providerId
                }),
            });

            if (res.ok) {
                const data = await res.json();
                if (data.invoice_url) {
                    // For manual, we might show a message. For others, redirect.
                    if (providerId === 'manual') {
                        setMessage({ type: 'success', text: `Please upload your receipt to: ${data.invoice_url}` });
                    } else {
                        // Redirect to payment provider
                        window.location.href = data.invoice_url;
                    }
                }
                await refreshData();
            } else {
                const err = await res.text();
                setMessage({ type: 'error', text: err || 'Failed to generate invoice' });
            }
        } catch (e) {
            setMessage({ type: 'error', text: 'Network error' });
        } finally {
            setPurchasing(null);
            setSelectedDuration(null);
        }
    };

    const formatPrice = (priceCents: number) => {
        const major = Math.floor(priceCents / 100);
        const minor = priceCents % 100;
        return `$${major}.${minor.toString().padStart(2, '0')}`;
    };

    const formatDuration = (days: number) => {
        if (days === 0) return 'Traffic Only';
        if (days === 30) return '1 Month';
        if (days === 60) return '2 Months';
        if (days === 90) return '3 Months';
        if (days === 180) return '6 Months';
        if (days === 365) return '1 Year';
        return `${days} days`;
    };

    if (loading) return <div className="page"><div className="loading">Loading plans...</div></div>;

    return (
        <div className="page plans-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/')}>←</button>
                <h2>🛍 Buy Subscription</h2>
            </header>

            {/* Balance indicator */}
            <div className="balance-strip glass-card">
                <span>💰 Your Balance</span>
                <span className="balance-val">
                    ${((user?.balance || 0)).toFixed(2)}
                </span>
            </div>

            {message && (
                <div className={`purchase-msg ${message.type}`}>
                    {message.text}
                </div>
            )}

            {plans.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-icon">📋</div>
                    <h3>No Plans Available</h3>
                    <p>Check back later for available subscription plans.</p>
                </div>
            ) : (
                <div className="plans-list">
                    {plans.map(plan => (
                        <div key={plan.id} className="plan-card glass-card">
                            <div className="plan-header">
                                <h3 className="plan-name">{plan.name}</h3>
                                <div className="plan-badges">
                                    <span className="plan-badge">📊 {plan.traffic_limit_gb > 0 ? `${plan.traffic_limit_gb} GB` : '∞'}</span>
                                    <span className="plan-badge">📱 {plan.device_limit > 0 ? `${plan.device_limit} devices` : '∞'}</span>
                                </div>
                            </div>

                            {plan.description && (
                                <p className="plan-desc">{plan.description}</p>
                            )}

                            <div className="duration-grid">
                                {plan.durations.map(dur => (
                                    <button
                                        key={dur.id}
                                        className={`duration-btn ${purchasing === dur.id ? 'purchasing' : ''}`}
                                        onClick={() => handleSelectDuration(dur)}
                                        disabled={purchasing !== null}
                                    >
                                        <span className="dur-label">
                                            {dur.duration_days === 0 ? '🚀 Traffic' : formatDuration(dur.duration_days)}
                                        </span>
                                        <span className="dur-price">{formatPrice(dur.price_cents)}</span>
                                        {purchasing === dur.id && <span className="dur-spinner">⏳</span>}
                                    </button>
                                ))}
                            </div>
                        </div>
                    ))}
                </div>
            )}

            {/* Payment Provider Modal */}
            {showPayModal && (
                <div className="modal-overlay" onClick={() => setShowPayModal(false)}>
                    <div className="modal-content glass-card" onClick={e => e.stopPropagation()}>
                        <h3>💳 Select Payment Method</h3>
                        <p>Choose how you want to pay for this plan:</p>
                        <div className="provider-list">
                            {providers.map(p => (
                                <button
                                    key={p.id}
                                    className="provider-btn"
                                    onClick={() => handlePurchase(p.id)}
                                >
                                    {p.label}
                                </button>
                            ))}
                        </div>
                        <button className="btn-cancel" onClick={() => setShowPayModal(false)}>Cancel</button>
                    </div>
                </div>
            )}
        </div>
    );
}

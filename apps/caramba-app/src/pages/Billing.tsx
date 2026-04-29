import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useAuth } from '../context/AuthContext'
import { useNavigate } from 'react-router-dom'
import './Billing.css'

interface Payment {
    id: number;
    amount: number;
    method: string;
    status: string;
    created_at: number;
}

export default function Billing() {
    const { t } = useTranslation()
    const { user, userStats: stats, token, error } = useAuth()
    const navigate = useNavigate()
    const [payments, setPayments] = useState<Payment[]>([])
    const [loading, setLoading] = useState(true)

    // Метки статусов платежей через i18n
    const statusLabel = (status: string): string => {
        const key = status.toLowerCase()
        const map: Record<string, string> = {
            completed: t('billing.statusCompleted'),
            pending: t('billing.statusPending'),
            failed: t('billing.statusFailed'),
            refunded: t('billing.statusRefunded'),
        }
        return map[key] || status
    }

    useEffect(() => {
        if (!token) {
            setLoading(false)
            return
        }
        const controller = new AbortController()
        const fetchPayments = async () => {
            try {
                const res = await fetch('/api/client/user/payments', {
                    headers: { 'Authorization': `Bearer ${token}` },
                    signal: controller.signal,
                })
                if (res.ok) setPayments(await res.json())
            } catch (e: unknown) {
                // AbortError при размонтировании — игнорируем
                if (e instanceof Error && e.name !== 'AbortError') {
                    setLoading(false)
                }
                return
            }
            setLoading(false)
        }
        void fetchPayments()
        return () => { controller.abort() }
    }, [token])

    const goBack = () => {
        if (window.history.length > 1) {
            navigate(-1)
        } else {
            navigate('/')
        }
    }

    const formatDate = (ts: number) => new Date(ts * 1000).toLocaleDateString()
    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat('ru-RU', { style: 'currency', currency: 'USD' }).format(amount)

    const balance = user?.balance ?? stats?.balance ?? 0

    return (
        <div className="page billing-page">
            <header className="page-header">
                <button className="back-button" onClick={goBack}>←</button>
                <h2>{t('billing.title')}</h2>
            </header>

            {!token && (
                <div className="empty-state">
                    <div className="empty-icon">AU</div>
                    <h3>{t('billing.authRequired')}</h3>
                    <p>{error || t('billing.authNote')}</p>
                </div>
            )}

            {token && <div className="balance-hero glass-card">
                <span className="balance-label">{t('billing.currentBalance')}</span>
                <span className="balance-amount gradient-text">
                    {formatCurrency(balance)}
                </span>
            </div>}

            {token && <div className="transactions-section">
                <h3>{t('billing.historyTitle')}</h3>
                {loading ? (
                    <div className="loading">{t('billing.loading')}</div>
                ) : payments.length > 0 ? (
                    <div className="transactions-list">
                        {payments.map(p => (
                            <div key={p.id} className="transaction-item glass-card">
                                <div className="tx-left">
                                    <span className="tx-method">{p.method}</span>
                                    <span className="tx-date">{formatDate(p.created_at)}</span>
                                </div>
                                <div className="tx-right">
                                    <span className={`tx-amount ${p.amount > 0 ? 'positive' : 'negative'}`}>
                                        {p.amount > 0 ? '+' : ''}{formatCurrency(p.amount)}
                                    </span>
                                    <span className={`badge badge-${p.status.toLowerCase() === 'completed' ? 'success' : p.status.toLowerCase() === 'pending' ? 'warning' : 'error'}`}>
                                        {statusLabel(p.status)}
                                    </span>
                                </div>
                            </div>
                        ))}
                    </div>
                ) : (
                    <div className="empty-state">
                        <div className="empty-icon">TX</div>
                        <p>{t('billing.noPayments')}</p>
                    </div>
                )}
            </div>}
        </div>
    )
}

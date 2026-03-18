import { useEffect, useState } from 'react'
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

const statusLabels: Record<string, string> = {
    completed: 'Завершен',
    pending: 'Ожидает',
    failed: 'Ошибка',
    refunded: 'Возврат',
}

export default function Billing() {
    const { user, userStats: stats, token, error } = useAuth()
    const navigate = useNavigate()
    const [payments, setPayments] = useState<Payment[]>([])
    const [loading, setLoading] = useState(true)

    useEffect(() => {
        if (!token) {
            setLoading(false)
            return
        }
        const fetchPayments = async () => {
            try {
                const res = await fetch('/api/client/user/payments', {
                    headers: { 'Authorization': `Bearer ${token}` }
                })
                if (res.ok) setPayments(await res.json())
            } catch (e) { console.error(e); }
            finally { setLoading(false); }
        }
        fetchPayments()
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
                <h2>Баланс и платежи</h2>
            </header>

            {!token && (
                <div className="empty-state">
                    <div className="empty-icon">AU</div>
                    <h3>Требуется авторизация</h3>
                    <p>{error || 'Откройте Mini App из Telegram-бота.'}</p>
                </div>
            )}

            {token && <div className="balance-hero glass-card">
                <span className="balance-label">Текущий баланс</span>
                <span className="balance-amount gradient-text">
                    {formatCurrency(balance)}
                </span>
            </div>}

            {token && <div className="transactions-section">
                <h3>История платежей</h3>
                {loading ? (
                    <div className="loading">Загрузка истории...</div>
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
                                        {statusLabels[p.status.toLowerCase()] || p.status}
                                    </span>
                                </div>
                            </div>
                        ))}
                    </div>
                ) : (
                    <div className="empty-state">
                        <div className="empty-icon">TX</div>
                        <p>Платежей пока нет</p>
                    </div>
                )}
            </div>}
        </div>
    )
}

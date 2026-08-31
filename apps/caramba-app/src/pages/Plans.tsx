import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import PurchaseFlow from '../components/PurchaseFlow'
import { useAuth } from '../context/AuthContext'
import './Plans.css'

/**
 * Страница тарифов — тонкая обёртка над единым потоком покупки
 * (components/PurchaseFlow). Вся логика каталога/оплаты живёт там.
 */
export default function Plans() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { refreshData, token, error } = useAuth()

    // Возврат в Mini App после внешней оплаты — обновляем данные.
    useEffect(() => {
        const handleFocus = () => {
            void refreshData()
        }
        window.addEventListener('focus', handleFocus)
        return () => window.removeEventListener('focus', handleFocus)
    }, [refreshData])

    return (
        <div className="page plans-page">
            <header className="plans-hero">
                <span className="plans-hero-mascot" aria-hidden="true">🤖</span>
                <h1>{t('plans.title')}</h1>
                <p>{t('plans.subtitle')}</p>
            </header>

            {!token && (
                <div className="purchase-banner error">
                    {error || t('home.authError')}
                </div>
            )}

            <PurchaseFlow />

            <button className="btn-ghost plans-billing-link" onClick={() => navigate('/billing')}>
                {t('plans.billingLink')}
            </button>
        </div>
    )
}

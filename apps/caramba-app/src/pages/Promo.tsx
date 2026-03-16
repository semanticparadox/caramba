import { useEffect, useState } from 'react'
import { useAuth } from '../context/AuthContext'
import { copyText } from '../lib/copyActions'
import './Promo.css'

type Banner = { type: 'success' | 'error'; text: string } | null

interface ReferralEntry {
    id: number
    username?: string | null
    full_name?: string | null
    joined_at: string
    total_earned_cents: number
}

interface ReferralStats {
    referral_code: string
    referred_count: number
    referral_link: string
    total_earned_cents: number
    total_earned_usd: number
    referrals: ReferralEntry[]
}

export default function Promo() {
    const { token, refreshData, error } = useAuth()

    const [promoCode, setPromoCode] = useState('')
    const [referrerCode, setReferrerCode] = useState('')
    const [redeeming, setRedeeming] = useState(false)
    const [linking, setLinking] = useState(false)
    const [loadingReferrals, setLoadingReferrals] = useState(true)
    const [copiedCode, setCopiedCode] = useState(false)
    const [copiedLink, setCopiedLink] = useState(false)
    const [banner, setBanner] = useState<Banner>(null)
    const [referralStats, setReferralStats] = useState<ReferralStats | null>(null)

    const loadReferrals = async () => {
        if (!token) {
            setLoadingReferrals(false)
            return
        }

        setLoadingReferrals(true)
        try {
            const res = await fetch('/api/client/user/referrals', {
                headers: { Authorization: `Bearer ${token}` },
            })
            if (res.ok) {
                setReferralStats(await res.json())
            } else {
                setReferralStats(null)
            }
        } catch {
            setReferralStats(null)
        } finally {
            setLoadingReferrals(false)
        }
    }

    useEffect(() => {
        if (!token) {
            setBanner({
                type: 'error',
                text: error || 'Требуется авторизация. Откройте Mini App из бота.',
            })
            setLoadingReferrals(false)
            return
        }

        void loadReferrals()
    }, [token, error])

    const redeemPromo = async () => {
        const code = promoCode.trim().toUpperCase()
        if (!code || !token) return

        setRedeeming(true)
        setBanner(null)
        try {
            const res = await fetch('/api/client/promo/redeem', {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ code }),
            })

            if (res.ok) {
                const data = await res.json()
                setBanner({
                    type: 'success',
                    text: data?.message || 'Код успешно активирован.',
                })
                setPromoCode('')
                await refreshData()
                await loadReferrals()
            } else {
                const errText = await res.text()
                setBanner({ type: 'error', text: errText || 'Не удалось активировать код.' })
            }
        } catch {
            setBanner({ type: 'error', text: 'Сетевая ошибка при активации кода.' })
        } finally {
            setRedeeming(false)
        }
    }

    const linkReferrer = async () => {
        const code = referrerCode.trim()
        if (!code || !token) return

        setLinking(true)
        setBanner(null)
        try {
            const res = await fetch('/api/client/user/referrer', {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ code }),
            })

            if (res.ok) {
                const data = await res.json()
                setBanner({
                    type: 'success',
                    text: data?.message || 'Реферер успешно привязан.',
                })
                setReferrerCode('')
                await loadReferrals()
            } else {
                const errText = await res.text()
                setBanner({ type: 'error', text: errText || 'Не удалось привязать реферера.' })
            }
        } catch {
            setBanner({ type: 'error', text: 'Сетевая ошибка при привязке реферера.' })
        } finally {
            setLinking(false)
        }
    }

    const handleCopyCode = async () => {
        if (!referralStats?.referral_code) return
        await copyText(referralStats.referral_code)
        setCopiedCode(true)
        setTimeout(() => setCopiedCode(false), 1500)
    }

    const handleCopyLink = async () => {
        if (!referralStats?.referral_link) return
        await copyText(referralStats.referral_link)
        setCopiedLink(true)
        setTimeout(() => setCopiedLink(false), 1500)
    }

    return (
        <div className="page promo-page">
            <header className="page-header promo-header">
                <h2>Промо и рефералы</h2>
                <button className="btn-ghost promo-refresh" onClick={() => void loadReferrals()}>
                    Обновить
                </button>
            </header>

            {banner && <div className={`promo-banner ${banner.type}`}>{banner.text}</div>}

            <section className="promo-card glass-card">
                <h3>Промокод</h3>
                <p>Введите код, чтобы активировать бонус или подписку.</p>
                <div className="promo-input-row">
                    <input
                        type="text"
                        placeholder="Введите код"
                        value={promoCode}
                        onChange={(e) => setPromoCode(e.target.value.toUpperCase())}
                    />
                    <button
                        className="btn-primary"
                        onClick={redeemPromo}
                        disabled={redeeming || promoCode.trim().length === 0}
                    >
                        {redeeming ? 'Активация...' : 'Активировать'}
                    </button>
                </div>
            </section>

            <section className="promo-card glass-card">
                <h3>Реферальная программа</h3>
                <p>Делитесь ссылкой, приглашайте друзей и получайте бонусы.</p>

                {loadingReferrals ? (
                    <div className="promo-empty">Загружаем данные по рефералам...</div>
                ) : referralStats ? (
                    <>
                        <div className="referral-stats-grid">
                            <div className="ref-stat-box">
                                <span>Приглашено</span>
                                <strong>{referralStats.referred_count}</strong>
                            </div>
                            <div className="ref-stat-box">
                                <span>Заработано</span>
                                <strong>${(referralStats.total_earned_usd || 0).toFixed(2)}</strong>
                            </div>
                        </div>

                        <div className="promo-field-block">
                            <label>Ваш код</label>
                            <div className="promo-input-row">
                                <input type="text" value={referralStats.referral_code || '—'} readOnly />
                                <button className="btn-secondary" onClick={handleCopyCode}>
                                    {copiedCode ? 'Скопировано' : 'Копировать'}
                                </button>
                            </div>
                        </div>

                        <div className="promo-field-block">
                            <label>Ваша ссылка</label>
                            <div className="promo-input-row">
                                <input type="text" value={referralStats.referral_link || '—'} readOnly />
                                <button className="btn-secondary" onClick={handleCopyLink}>
                                    {copiedLink ? 'Скопировано' : 'Копировать'}
                                </button>
                            </div>
                        </div>
                    </>
                ) : (
                    <div className="promo-empty">Не удалось загрузить реферальную статистику.</div>
                )}
            </section>

            <section className="promo-card glass-card">
                <h3>Привязать реферера</h3>
                <p>Введите код пригласившего пользователя один раз.</p>
                <div className="promo-input-row">
                    <input
                        type="text"
                        placeholder="Реферальный код"
                        value={referrerCode}
                        onChange={(e) => setReferrerCode(e.target.value)}
                    />
                    <button
                        className="btn-secondary"
                        onClick={linkReferrer}
                        disabled={linking || referrerCode.trim().length === 0}
                    >
                        {linking ? 'Сохранение...' : 'Привязать'}
                    </button>
                </div>
            </section>

            {referralStats && referralStats.referrals.length > 0 && (
                <section className="promo-card glass-card">
                    <h3>Ваши приглашенные</h3>
                    <div className="promo-list">
                        {referralStats.referrals.map((item) => (
                            <div key={item.id} className="promo-list-item">
                                <div>
                                    <div className="promo-list-title">
                                        {item.full_name || item.username || `Пользователь #${item.id}`}
                                    </div>
                                    <div className="promo-list-meta">
                                        Присоединился {new Date(item.joined_at).toLocaleDateString()}
                                    </div>
                                </div>
                                <div className="promo-list-title">
                                    ${(item.total_earned_cents / 100).toFixed(2)}
                                </div>
                            </div>
                        ))}
                    </div>
                </section>
            )}
        </div>
    )
}

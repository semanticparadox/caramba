import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
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
    bonus_percent: number
    referrer_signup_bonus_cents: number
    referred_signup_bonus_cents: number
    referrals: ReferralEntry[]
}

export default function Promo() {
    const { t } = useTranslation()
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

    const loadReferrals = useCallback(async () => {
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
    }, [token])

    useEffect(() => {
        if (!token) {
            setBanner({
                type: 'error',
                text: error || t('promo.authError'),
            })
            setLoadingReferrals(false)
            return
        }

        void loadReferrals()
    // error/t не включены намеренно — banner для auth-ошибки нужен только при отсутствии token
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [token, loadReferrals])

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
                    text: data?.message || t('promo.promoActivated'),
                })
                setPromoCode('')
                await refreshData()
                await loadReferrals()
            } else {
                const errText = await res.text()
                setBanner({ type: 'error', text: errText || t('promo.promoError') })
            }
        } catch {
            setBanner({ type: 'error', text: t('promo.networkPromoError') })
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
                    text: data?.message || t('promo.referrerLinked'),
                })
                setReferrerCode('')
                await loadReferrals()
            } else {
                const errText = await res.text()
                setBanner({ type: 'error', text: errText || t('promo.referrerError') })
            }
        } catch {
            setBanner({ type: 'error', text: t('promo.networkReferrerError') })
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
                <h2>{t('promo.title')}</h2>
                <button className="btn-ghost promo-refresh" onClick={() => void loadReferrals()}>
                    {t('common.refresh')}
                </button>
            </header>

            {banner && <div className={`promo-banner ${banner.type}`}>{banner.text}</div>}

            <section className="promo-card glass-card">
                <h3>{t('promo.promoTitle')}</h3>
                <p>{t('promo.promoDesc')}</p>
                <div className="promo-input-row">
                    <input
                        type="text"
                        placeholder={t('promo.promoPlaceholder')}
                        value={promoCode}
                        onChange={(e) => setPromoCode(e.target.value.toUpperCase())}
                        onKeyDown={(e) => { if (e.key === 'Enter' && promoCode.trim()) redeemPromo() }}
                    />
                    <button
                        className="btn-primary"
                        onClick={redeemPromo}
                        disabled={redeeming || promoCode.trim().length === 0}
                    >
                        {redeeming ? t('promo.activating') : t('promo.activate')}
                    </button>
                </div>
            </section>

            <section className="promo-card glass-card">
                <h3>{t('promo.referralTitle')}</h3>
                <p>
                    {t('promo.referralDescBefore')}{' '}
                    <strong>{referralStats?.bonus_percent ?? 10}%</strong>
                    {' '}{t('promo.referralDescAfter')}
                </p>

                {/* Блок с бонусами за регистрацию — показываем только если заданы */}
                {referralStats && referralStats.referred_signup_bonus_cents > 0 && (
                    <div className="promo-signup-bonus-hint">
                        {t('promo.friendSignupBonus', {
                            amount: (referralStats.referred_signup_bonus_cents / 100).toFixed(2),
                        })}
                    </div>
                )}
                {referralStats && referralStats.referrer_signup_bonus_cents > 0 && (
                    <div className="promo-signup-bonus-hint">
                        {t('promo.referrerSignupBonus', {
                            amount: (referralStats.referrer_signup_bonus_cents / 100).toFixed(2),
                        })}
                    </div>
                )}

                {loadingReferrals ? (
                    <div className="promo-empty">{t('promo.loadingReferrals')}</div>
                ) : referralStats ? (
                    <>
                        <div className="referral-stats-grid">
                            <div className="ref-stat-box">
                                <span>{t('promo.invited')}</span>
                                <strong>{referralStats.referred_count}</strong>
                            </div>
                            <div className="ref-stat-box">
                                <span>{t('promo.earned')}</span>
                                <strong>${(referralStats.total_earned_usd || 0).toFixed(2)}</strong>
                            </div>
                        </div>

                        <div className="promo-field-block">
                            <label>{t('promo.yourCode')}</label>
                            <div className="promo-input-row">
                                <input type="text" value={referralStats.referral_code || '—'} readOnly />
                                <button className="btn-secondary" onClick={handleCopyCode}>
                                    {copiedCode ? t('common.copied') : t('promo.copy')}
                                </button>
                            </div>
                        </div>

                        <div className="promo-field-block">
                            <label>{t('promo.yourLink')}</label>
                            <div className="promo-input-row">
                                <input type="text" value={referralStats.referral_link || '—'} readOnly />
                                <button className="btn-secondary" onClick={handleCopyLink}>
                                    {copiedLink ? t('common.copied') : t('promo.copy')}
                                </button>
                            </div>
                        </div>
                    </>
                ) : (
                    <div className="promo-empty">{t('promo.referralLoadError')}</div>
                )}
            </section>

            <section className="promo-card glass-card">
                <h3>{t('promo.referrerTitle')}</h3>
                <p>{t('promo.referrerDesc')}</p>
                <div className="promo-input-row">
                    <input
                        type="text"
                        placeholder={t('promo.referrerPlaceholder')}
                        value={referrerCode}
                        onChange={(e) => setReferrerCode(e.target.value)}
                        onKeyDown={(e) => { if (e.key === 'Enter' && referrerCode.trim()) linkReferrer() }}
                    />
                    <button
                        className="btn-secondary"
                        onClick={linkReferrer}
                        disabled={linking || referrerCode.trim().length === 0}
                    >
                        {linking ? t('promo.saving') : t('promo.link')}
                    </button>
                </div>
            </section>

            {referralStats && referralStats.referrals.length > 0 && (
                <section className="promo-card glass-card">
                    <h3>{t('promo.invitedTitle')}</h3>
                    <div className="promo-list">
                        {referralStats.referrals.map((item) => (
                            <div key={item.id} className="promo-list-item">
                                <div>
                                    <div className="promo-list-title">
                                        {item.full_name || item.username || t('promo.user', { id: item.id })}
                                    </div>
                                    <div className="promo-list-meta">
                                        {t('promo.joinedAt', { date: new Date(item.joined_at).toLocaleDateString() })}
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

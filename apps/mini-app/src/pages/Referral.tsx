import { useEffect, useState } from 'react'
import { useAuth } from '../context/AuthContext'
import { useNavigate } from 'react-router-dom'
import './Referral.css'

interface ReferralStats {
    referral_code: string;
    referred_count: number;
    referral_link: string;
}

export default function Referral() {
    const { token } = useAuth()
    const navigate = useNavigate()
    const [stats, setStats] = useState<ReferralStats | null>(null)
    const [loading, setLoading] = useState(true)
    const [copied, setCopied] = useState(false)

    useEffect(() => {
        if (!token) return
        const fetchStats = async () => {
            try {
                const res = await fetch('/api/client/user/referrals', {
                    headers: { 'Authorization': `Bearer ${token}` }
                })
                if (res.ok) setStats(await res.json())
            } catch (e) { console.error(e); }
            finally { setLoading(false); }
        }
        fetchStats()
    }, [token])

    const copyLink = () => {
        if (stats) {
            navigator.clipboard.writeText(stats.referral_link)
            setCopied(true)
            setTimeout(() => setCopied(false), 2000)
        }
    }

    if (loading) return <div className="page"><div className="loading">Loading...</div></div>

    return (
        <div className="page referral-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate(-1)}>←</button>
                <h2>Refer & Earn</h2>
            </header>

            <div className="referral-hero glass-card">
                <div className="hero-icon">🎁</div>
                <h3>Invite Friends</h3>
                <p className="hero-desc">
                    Share your link and earn bonuses when friends subscribe!
                </p>
                <div className="referral-stats">
                    <div className="ref-stat">
                        <span className="ref-stat-value gradient-text">{stats?.referred_count || 0}</span>
                        <span className="ref-stat-label">Friends Invited</span>
                    </div>
                </div>
            </div>

            <div className="invite-card glass-card">
                <h4>Your Invite Link</h4>
                <input type="text" readOnly value={stats?.referral_link || 'No referral link'} />
                <button className={`btn-primary ${copied ? 'copied' : ''}`} onClick={copyLink}>
                    {copied ? '✓ Copied!' : '📋 Copy Link'}
                </button>
            </div>
        </div>
    )
}

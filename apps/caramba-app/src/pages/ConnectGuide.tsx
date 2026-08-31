import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import Icon from '../components/Icon'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../context/AuthContext'
import { PLATFORM_DIRECTORY, PlatformKey } from '../data/appDirectory'
import { copyText } from '../lib/copyActions'
import { triggerSelectionHaptic } from '../lib/telegram'
import './ConnectGuide.css'

const PLATFORM_ORDER: PlatformKey[] = ['android', 'ios', 'windows', 'macos', 'linux', 'tv']

export default function ConnectGuide() {
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { subscriptions } = useAuth()
    const [activePlatform, setActivePlatform] = useState<PlatformKey>('android')
    const [copiedLink, setCopiedLink] = useState(false)
    const [copiedSetup, setCopiedSetup] = useState(false)

    const activeSubscription = subscriptions.find((sub) => sub.status === 'active')
    const directory = useMemo(
        () => PLATFORM_DIRECTORY.find((platform) => platform.id === activePlatform) || PLATFORM_DIRECTORY[0],
        [activePlatform],
    )

    // Метки уровня проверенности клиента
    const confidenceLabel: Record<'high' | 'medium-high' | 'medium', string> = {
        high: t('connectGuide.confidenceHigh'),
        'medium-high': t('connectGuide.confidenceMediumHigh'),
        medium: t('connectGuide.confidenceMedium'),
    }

    const copySubscriptionLink = async () => {
        if (!activeSubscription?.subscription_url) return
        await copyText(activeSubscription.subscription_url)
        setCopiedLink(true)
        setTimeout(() => setCopiedLink(false), 1600)
    }

    const copySetupSnippet = async () => {
        await copyText(directory.quickSetup)
        setCopiedSetup(true)
        setTimeout(() => setCopiedSetup(false), 1600)
    }

    return (
        <div className="page connect-guide-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/support')}><Icon name="chevron-left" /></button>
                <h2>{t('connectGuide.title')}</h2>
            </header>

            <section className="connect-hero glass-card">
                <div className="connect-hero-copy">
                    <h3>{t('connectGuide.heroTitle')}</h3>
                    <p>{t('connectGuide.heroDesc')}</p>
                </div>

                <div className="connect-hero-actions">
                    <button
                        className="btn-primary"
                        onClick={() => void copySubscriptionLink()}
                        disabled={!activeSubscription?.subscription_url}
                    >
                        {copiedLink ? t('connectGuide.linkCopied') : t('connectGuide.copyLink')}
                    </button>
                    <button className="btn-secondary" onClick={() => navigate('/')}>
                        {t('connectGuide.openCenter')}
                    </button>
                </div>

                {activeSubscription?.subscription_url && (
                    <p className="connect-auto-note">
                        {t('connectGuide.alreadyActiveNote')}
                    </p>
                )}

                {!activeSubscription?.subscription_url && (
                    <p className="connect-warning">
                        {t('connectGuide.noActiveWarning')}
                    </p>
                )}
            </section>

            <section className="platform-select glass-card">
                <div className="platform-select-head">
                    <h3>{t('connectGuide.chooseDevice')}</h3>
                    <span>{t('connectGuide.availableClients')}</span>
                </div>

                <div className="platform-chip-grid">
                    {PLATFORM_ORDER.map((platformId) => {
                        const platform = PLATFORM_DIRECTORY.find((item) => item.id === platformId)
                        if (!platform) return null
                        return (
                            <button
                                key={platform.id}
                                className={`platform-chip ${activePlatform === platform.id ? 'active' : ''}`}
                                onClick={() => {
                                    triggerSelectionHaptic()
                                    setActivePlatform(platform.id)
                                }}
                            >
                                {platform.label}
                            </button>
                        )
                    })}
                </div>

                <div className="setup-strip">
                    <p>{directory.quickSetup}</p>
                    <button className="btn-ghost" onClick={() => void copySetupSnippet()}>
                        {copiedSetup ? t('connectGuide.setupCopied') : t('connectGuide.copySetup')}
                    </button>
                </div>
            </section>

            <section className="app-directory-grid">
                {directory.entries.map((entry) => (
                    <article key={entry.id} className="app-card glass-card">
                        <div className="app-card-head">
                            <div>
                                <h4>{entry.name}</h4>
                                <p>{entry.description}</p>
                            </div>
                            <div className="app-meta">
                                {(entry.badgeKey || entry.badgeText) && (
                                    <span className="app-badge">
                                        {entry.badgeKey ? t(entry.badgeKey) : entry.badgeText}
                                    </span>
                                )}
                                <span className="confidence-tag">{confidenceLabel[entry.confidence]}</span>
                            </div>
                        </div>

                        <div className="app-card-actions">
                            <button className="btn-secondary" onClick={() => window.open(entry.officialUrl, '_blank', 'noopener,noreferrer')}>
                                {t('connectGuide.downloadApp', { name: entry.name })}
                            </button>
                            {entry.fallbackUrl && (
                                <button
                                    className="btn-ghost"
                                    onClick={() => window.open(entry.fallbackUrl, '_blank', 'noopener,noreferrer')}
                                >
                                    {t('connectGuide.releases')}
                                </button>
                            )}
                        </div>
                    </article>
                ))}
            </section>
        </div>
    )
}

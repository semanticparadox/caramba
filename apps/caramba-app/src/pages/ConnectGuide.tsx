import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../context/AuthContext'
import { PLATFORM_DIRECTORY, PlatformKey } from '../data/appDirectory'
import { copyText } from '../lib/copyActions'
import { triggerSelectionHaptic } from '../lib/telegram'
import './ConnectGuide.css'

const PLATFORM_ORDER: PlatformKey[] = ['android', 'ios', 'windows', 'macos', 'linux', 'tv']

const CONFIDENCE_LABEL: Record<'high' | 'medium-high' | 'medium', string> = {
    high: 'Проверено',
    'medium-high': 'Проверено с оговорками',
    medium: 'Нужна дополнительная проверка',
}

export default function ConnectGuide() {
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
                <button className="back-button" onClick={() => navigate('/support')}>{'<'}</button>
                <h2>Как подключиться</h2>
            </header>

            <section className="connect-hero glass-card">
                <div className="connect-hero-copy">
                    <h3>Пошаговый вход в VPN-клиент</h3>
                    <p>
                        Выберите платформу, установите рекомендуемое приложение и импортируйте ссылку подписки одним действием.
                    </p>
                </div>

                <div className="connect-hero-actions">
                    <button
                        className="btn-primary"
                        onClick={() => void copySubscriptionLink()}
                        disabled={!activeSubscription?.subscription_url}
                    >
                        {copiedLink ? 'Ссылка скопирована' : 'Скопировать ссылку подписки'}
                    </button>
                    <button className="btn-secondary" onClick={() => navigate('/subscription')}>
                        Открыть мои сервисы
                    </button>
                </div>

                {!activeSubscription?.subscription_url && (
                    <p className="connect-warning">
                        Активная подписка не найдена. Откройте раздел тарифов и активируйте доступ.
                    </p>
                )}
            </section>

            <section className="platform-select glass-card">
                <div className="platform-select-head">
                    <h3>Платформа</h3>
                    <span>Проверенный каталог приложений 2026</span>
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
                        {copiedSetup ? 'Скопировано' : 'Скопировать шаги'}
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
                                {entry.badge && <span className="app-badge">{entry.badge}</span>}
                                <span className="confidence-tag">{CONFIDENCE_LABEL[entry.confidence]}</span>
                            </div>
                        </div>

                        <div className="app-card-actions">
                            <button className="btn-secondary" onClick={() => window.open(entry.officialUrl, '_blank', 'noopener,noreferrer')}>
                                Скачать
                            </button>
                            {entry.fallbackUrl && (
                                <button
                                    className="btn-ghost"
                                    onClick={() => window.open(entry.fallbackUrl, '_blank', 'noopener,noreferrer')}
                                >
                                    Гид
                                </button>
                            )}
                        </div>
                    </article>
                ))}
            </section>
        </div>
    )
}

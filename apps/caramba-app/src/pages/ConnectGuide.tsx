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
                <h2>Подключение</h2>
            </header>

            <section className="connect-hero glass-card">
                <div className="connect-hero-copy">
                    <h3>Подключение к VPN</h3>
                    <p>
                        Скопируйте ссылку подписки, откройте любой совместимый клиент и выполните импорт. Рекомендуем Hiddify — после импорта маршруты подтянутся автоматически.
                    </p>
                </div>

                <div className="connect-hero-actions">
                    <button
                        className="btn-primary"
                        onClick={() => void copySubscriptionLink()}
                        disabled={!activeSubscription?.subscription_url}
                    >
                        {copiedLink ? 'Ссылка скопирована' : 'Скопировать ссылку для импорта'}
                    </button>
                    <button className="btn-secondary" onClick={() => navigate('/')}>
                        Открыть центр
                    </button>
                </div>

                {activeSubscription?.subscription_url && (
                    <p className="connect-auto-note">
                        Если у вас уже есть активная подписка, достаточно одного импорта - ручной выбор маршрутов обычно не нужен.
                    </p>
                )}

                {!activeSubscription?.subscription_url && (
                    <p className="connect-warning">
                        Пока нет активной подписки. Откройте тарифы, активируйте доступ и вернитесь сюда для быстрого импорта.
                    </p>
                )}
            </section>

            <section className="platform-select glass-card">
                <div className="platform-select-head">
                    <h3>Выберите устройство</h3>
                    <span>Доступные клиенты</span>
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
                        {copiedSetup ? 'Скопировано' : 'Скопировать памятку'}
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
                                Скачать {entry.name}
                            </button>
                            {entry.fallbackUrl && (
                                <button
                                    className="btn-ghost"
                                    onClick={() => window.open(entry.fallbackUrl, '_blank', 'noopener,noreferrer')}
                                >
                                    Релизы
                                </button>
                            )}
                        </div>
                    </article>
                ))}
            </section>
        </div>
    )
}

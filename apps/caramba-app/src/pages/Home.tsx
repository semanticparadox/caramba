import { useNavigate } from 'react-router-dom'
import { useAuth } from '../context/AuthContext'
import { useAppLock } from '../context/AppLockContext'
import { formatBytes, getUsageSnapshot } from '../lib/subscriptionMetrics'
import './Home.css'

const CONTROL_ITEMS = [
    { path: '/support/connect', title: 'Каталог приложений', subtitle: 'Подберите клиент и импортируйте ссылку' },
    { path: '/promo', title: 'Промокоды', subtitle: 'Активируйте код и проверьте доступ' },
    { path: '/referral', title: 'Приглашения', subtitle: 'Поделитесь доступом с друзьями' },
    { path: '/support', title: 'Поддержка', subtitle: 'Ответы, диагностика и связь с командой' },
]

export default function Home() {
    const navigate = useNavigate()
    const { userStats: stats, isLoading, user, subscriptions, refreshData } = useAuth()
    const { isPinEnabled, lockNow } = useAppLock()

    const usage = getUsageSnapshot(stats, subscriptions)
    const activeSubscriptions = usage.activeSubscriptions

    const primarySubscription = activeSubscriptions[0]
    const connectTarget = primarySubscription ? `/subscription?sub=${primarySubscription.id}&connect=1` : '/plans'
    const connectLabel = primarySubscription ? 'Импортировать и подключиться' : 'Выбрать тариф и подключиться'
    const hasAccess = activeSubscriptions.length > 0

    const radius = 44
    const circumference = 2 * Math.PI * radius
    const strokeOffset = circumference - (usage.percent / 100) * circumference

    const subscriptionsPreview = activeSubscriptions
        .slice()
        .sort((a, b) => (a.days_left || 0) - (b.days_left || 0))
        .slice(0, 3)

    const isSimpleMode = stats?.simple_mode_enabled === true

    const filteredControlItems = CONTROL_ITEMS.filter((item) => {
        if (!isSimpleMode) return true
        return item.path !== '/plans' && item.path !== '/subscription' && item.path !== '/store'
    })

    const totalDownload = stats?.total_download || 0
    const totalUpload = stats?.total_upload || 0

    return (
        <div className="page home-page">
            <section className="quick-connect-hero glass-card">
                <div className="hero-top-row">
                    <span className="hero-state">
                        <span className={`state-dot ${hasAccess ? 'is-online' : 'is-idle'}`} />
                        {hasAccess ? 'Готово к подключению' : 'Нет активного доступа'}
                    </span>
                    {isSimpleMode && <span className="hero-mode">Упрощенный режим</span>}
                </div>

                <div className="hero-copy">
                    <p className="hero-kicker">Быстрый старт</p>
                    <h1>{user?.username || 'Оператор'}</h1>
                    <p>
                        Импортируйте подписку одним действием. Маршруты загрузятся автоматически, а тонкая настройка останется доступной позже.
                    </p>
                </div>

                <div className="hero-actions">
                    <button className="btn-primary hero-connect" onClick={() => navigate(connectTarget)}>
                        {connectLabel}
                    </button>
                    <p className="hero-connect-note">Основной путь: открыть подписку, импортировать ссылку и сразу проверить соединение.</p>
                    <div className="hero-secondary-row">
                        <button className="btn-ghost" onClick={() => navigate('/support/connect')}>
                            Гид по приложениям
                        </button>
                        <button className="btn-ghost" onClick={() => void refreshData()}>
                            Обновить
                        </button>
                        <button className="btn-ghost" onClick={() => navigate('/subscription')}>
                            Мои сервисы
                        </button>
                    </div>
                </div>

                <div className="hero-links-row">
                    <button className="hero-link" onClick={() => navigate('/support')}>
                        Нужна помощь
                    </button>
                    <button className="hero-link" onClick={() => navigate('/store')}>
                        Магазин
                    </button>
                    {isPinEnabled ? (
                        <button className="hero-link" onClick={lockNow}>
                            Заблокировать Mini App
                        </button>
                    ) : (
                        <button className="hero-link" onClick={() => navigate('/support')}>
                            Включить PIN
                        </button>
                    )}
                </div>
            </section>

            <section className="control-panel glass-card">
                <div className="panel-header">
                    <h3>Ваши маршруты</h3>
                    <span>Откройте подписку и начните импорт с нужного тарифа</span>
                </div>

                <div className="subs-preview-grid">
                    {subscriptionsPreview.length === 0 ? (
                        <div className="empty-state control-empty">
                            <div className="empty-icon">CC</div>
                            <h3>Пока нет активных подписок</h3>
                            <p>Откройте тарифы, чтобы получить ссылку для импорта и подключиться в пару касаний.</p>
                        </div>
                    ) : (
                        subscriptionsPreview.map((sub) => (
                            <button
                                key={sub.id}
                                className="sub-preview-row"
                                onClick={() => navigate(`/subscription?sub=${sub.id}&connect=1`)}
                            >
                                <span className="sub-preview-name">{sub.plan_name}</span>
                                <span className="sub-preview-meta">
                                    {sub.used_traffic_gb} GB / {sub.traffic_limit_gb || '∞'} GB
                                </span>
                                <span className="sub-preview-meta">{sub.days_left} дн. осталось</span>
                            </button>
                        ))
                    )}
                </div>

                <div className="panel-header panel-header-secondary">
                    <h3>Дополнительно</h3>
                    <span>Промо, приглашения и справка</span>
                </div>

                <div className="control-grid">
                    {filteredControlItems.map((item) => (
                        <button
                            key={item.path}
                            className="control-card"
                            onClick={() => navigate(item.path)}
                        >
                            <span className="control-title">{item.title}</span>
                            <span className="control-subtitle">{item.subtitle}</span>
                        </button>
                    ))}
                </div>
            </section>

            <section className="home-bento-grid">
                <article className="bento-card bento-traffic glass-card">
                    <div className="bento-head">
                        <h3>Трафик</h3>
                        <span>{usage.percent}%</span>
                    </div>
                    <div className="traffic-ring-wrap">
                        <svg className="traffic-ring" viewBox="0 0 100 100">
                            <circle className="ring-bg" cx="50" cy="50" r={radius} />
                            <circle
                                className="ring-progress"
                                cx="50"
                                cy="50"
                                r={radius}
                                strokeDasharray={circumference}
                                strokeDashoffset={strokeOffset}
                            />
                        </svg>
                        <div className="ring-text-wrap">
                            <span className="ring-percent">{isLoading ? '...' : `${usage.percent}%`}</span>
                            <span className="ring-text">использовано</span>
                        </div>
                    </div>
                    <div className="traffic-values">
                        <span>{isLoading ? '...' : `${usage.usedGbText} GB`}</span>
                        <span>{isLoading ? '...' : usage.limitLabel}</span>
                    </div>
                    {!hasAccess ? (
                        <div className="traffic-empty-note">
                            <p>После активации подписки здесь появится расход по трафику.</p>
                            <button className="btn-ghost" onClick={() => navigate('/plans')}>Открыть тарифы</button>
                        </div>
                    ) : usage.percent === 0 ? (
                        <p className="traffic-empty-hint">Первое подключение создаст статистику автоматически.</p>
                    ) : null}
                </article>

                <article className="bento-card glass-card">
                    <div className="bento-head">
                        <h3>Срок действия</h3>
                        <span>{activeSubscriptions.length} активных</span>
                    </div>
                    <div className="metric-grid">
                        <div>
                            <p>Дней осталось</p>
                            <strong>{isLoading ? '...' : (usage.daysLeft ?? 'Н/Д')}</strong>
                        </div>
                        <div>
                            <p>Тарифов</p>
                            <strong>{subscriptions.length}</strong>
                        </div>
                    </div>
                </article>

                <article className="bento-card glass-card">
                    <div className="bento-head">
                        <h3>Передача</h3>
                        <span>За все время</span>
                    </div>
                    <div className="metric-grid">
                        <div>
                            <p>Входящий</p>
                            <strong>{formatBytes(totalDownload)}</strong>
                        </div>
                        <div>
                            <p>Исходящий</p>
                            <strong>{formatBytes(totalUpload)}</strong>
                        </div>
                    </div>
                </article>

                <article className="bento-card glass-card">
                    <div className="bento-head">
                        <h3>Баланс</h3>
                        <span>Доступно</span>
                    </div>
                    <div className="balance-value">${(user?.balance || stats?.balance || 0).toFixed(2)}</div>
                    <button className="btn-ghost" onClick={() => navigate('/plans')}>
                        Перейти к оплате
                    </button>
                </article>
            </section>
        </div>
    )
}

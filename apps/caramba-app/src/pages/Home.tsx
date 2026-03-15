import { useNavigate } from 'react-router-dom'
import { useAuth } from '../context/AuthContext'
import { useAppLock } from '../context/AppLockContext'
import './Home.css'

function formatBytes(bytes: number, decimals = 2): string {
    if (!bytes || bytes === 0) return '0 B'
    const k = 1024
    const dm = decimals < 0 ? 0 : decimals
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + sizes[i]
}

const CONTROL_ITEMS = [
    { path: '/subscription', title: 'Мои сервисы', subtitle: 'Конфиги, ссылки и состояние активации', tag: 'СЕРВИС' },
    { path: '/plans', title: 'Тарифы', subtitle: 'Покупка, продление и баланс', tag: 'ОПЛАТА' },
    { path: '/store', title: 'Магазин', subtitle: 'Дополнения и разовые покупки', tag: 'ШОП' },
    { path: '/promo', title: 'Промо', subtitle: 'Активация и управление кодами', tag: 'ПРОМО' },
    { path: '/referral', title: 'Рефералы', subtitle: 'Приглашения и трекинг наград', tag: 'РОСТ' },
    { path: '/support', title: 'Поддержка', subtitle: 'Диагностика и справочный центр', tag: 'ПОМОЩЬ' },
]

export default function Home() {
    const navigate = useNavigate()
    const { userStats: stats, isLoading, user, subscriptions, refreshData } = useAuth()
    const { isPinEnabled, lockNow } = useAppLock()

    const activeSubscriptions = subscriptions.filter((s) => s.status === 'active')
    const totalUsedFromSubs = activeSubscriptions.reduce((acc, sub) => acc + (sub.used_traffic_bytes || 0), 0)
    const totalLimitFromSubs = activeSubscriptions.reduce((acc, sub) => {
        const limitBytes = Math.max(0, sub.traffic_limit_gb || 0) * 1024 * 1024 * 1024
        return acc + limitBytes
    }, 0)

    const effectiveUsed = totalLimitFromSubs > 0 ? totalUsedFromSubs : (stats?.traffic_used || 0)
    const effectiveLimit = totalLimitFromSubs > 0 ? totalLimitFromSubs : (stats?.traffic_limit || 0)
    const effectiveDaysLeft = activeSubscriptions.length > 0
        ? Math.min(...activeSubscriptions.map((s) => Math.max(0, s.days_left || 0)))
        : (stats?.days_left ?? null)

    const percentage = effectiveLimit > 0
        ? Math.min(100, Math.round((effectiveUsed / effectiveLimit) * 100))
        : 0

    const primarySubscription = activeSubscriptions[0]
    const connectTarget = primarySubscription ? `/servers/${primarySubscription.id}` : '/plans'
    const connectLabel = primarySubscription ? 'Быстрое подключение' : 'Открыть доступ'
    const hasAccess = activeSubscriptions.length > 0

    const radius = 44
    const circumference = 2 * Math.PI * radius
    const strokeOffset = circumference - (percentage / 100) * circumference

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
                    <p className="hero-kicker">Центр быстрого подключения</p>
                    <h1>{(user?.username || 'Оператор').toUpperCase()}</h1>
                    <p>
                        Сначала подключение, затем диагностика и коммерческие действия в едином центре управления.
                    </p>
                </div>

                <div className="hero-actions">
                    <button className="btn-primary hero-connect" onClick={() => navigate(connectTarget)}>
                        {connectLabel}
                    </button>
                    <div className="hero-secondary-row">
                        <button className="btn-secondary" onClick={() => navigate('/subscription')}>
                            Сервисы
                        </button>
                        <button className="btn-secondary" onClick={() => void refreshData()}>
                            Обновить
                        </button>
                        <button className="btn-secondary" onClick={() => navigate('/plans')}>
                            Продлить
                        </button>
                    </div>
                </div>

                <div className="hero-meta-row">
                    <button className="hero-chip" onClick={() => navigate('/support')}>
                        {isPinEnabled ? 'PIN включен' : 'PIN выключен'}
                    </button>
                    {isPinEnabled && (
                        <button className="hero-chip hero-chip-warn" onClick={lockNow}>
                            Заблокировать
                        </button>
                    )}
                    <button className="hero-chip" onClick={() => navigate('/store')}>
                        Открыть магазин
                    </button>
                </div>

                <div className="hero-haptic-mark" aria-hidden="true">
                    haptic-ready
                </div>
            </section>

            <section className="home-bento-grid">
                <article className="bento-card bento-traffic glass-card">
                    <div className="bento-head">
                        <h3>Трафик</h3>
                        <span>{percentage}%</span>
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
                            <span className="ring-percent">{isLoading ? '...' : `${percentage}%`}</span>
                            <span className="ring-text">использовано</span>
                        </div>
                    </div>
                    <div className="traffic-values">
                        <span>{isLoading ? '...' : formatBytes(effectiveUsed)}</span>
                        <span>{isLoading ? '...' : formatBytes(effectiveLimit)}</span>
                    </div>
                </article>

                <article className="bento-card glass-card">
                    <div className="bento-head">
                        <h3>Срок действия</h3>
                        <span>{activeSubscriptions.length} активных</span>
                    </div>
                    <div className="metric-grid">
                        <div>
                            <p>Дней осталось</p>
                            <strong>{isLoading ? '...' : (effectiveDaysLeft ?? 'Н/Д')}</strong>
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

            <section className="control-panel glass-card">
                <div className="panel-header">
                    <h3>Панель действий</h3>
                    <span>Операционные и коммерческие быстрые сценарии</span>
                </div>

                <div className="control-grid">
                    {filteredControlItems.map((item) => (
                        <button
                            key={item.path}
                            className="control-card"
                            onClick={() => navigate(item.path)}
                        >
                            <span className="control-tag">{item.tag}</span>
                            <span className="control-title">{item.title}</span>
                            <span className="control-subtitle">{item.subtitle}</span>
                        </button>
                    ))}
                </div>

                <div className="subs-preview-grid">
                    {subscriptionsPreview.length === 0 ? (
                        <div className="empty-state control-empty">
                            <div className="empty-icon">CC</div>
                            <h3>Нет активных подписок</h3>
                            <p>Откройте тарифы, чтобы создать первый маршрут подключения.</p>
                        </div>
                    ) : (
                        subscriptionsPreview.map((sub) => (
                            <button
                                key={sub.id}
                                className="sub-preview-row"
                                onClick={() => navigate('/subscription')}
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
            </section>
        </div>
    )
}

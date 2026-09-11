import { Suspense, lazy, useEffect } from 'react'
import { BrowserRouter, Navigate, Route, Routes, useLocation, useNavigate, useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { getStartRoute } from '../lib/telegram'
import { AuthProvider, useAuth } from '../context/AuthContext'
import { carambaIconUrl, isCarambaBrand } from './BrandMark'
import { AppLockProvider } from '../context/AppLockContext'
import { NotificationProvider } from '../context/NotificationContext'
import AppLockGate from '../components/AppLockGate'
import TabBar from './TabBar'
import { applyTelegramTheme } from './theme'
import { ToastProvider } from './lib/useToast'

const Connect = lazy(() => import('./pages/Connect'))
const Servers = lazy(() => import('./pages/Servers'))
const Profile = lazy(() => import('./pages/Profile'))
const Devices = lazy(() => import('./pages/Devices'))
const Pay = lazy(() => import('./pages/Pay'))
const Guide = lazy(() => import('./pages/Guide'))
const Notifications = lazy(() => import('./pages/Notifications'))
const Support = lazy(() => import('./pages/Support'))
const SupportTicket = lazy(() => import('./pages/SupportTicket'))

/** Старый адрес тикета `/tickets/:id` живёт как редирект на `/support/:id`:
 *  так ссылки из бота и старых уведомлений открывают тот же экран. */
function TicketRedirect() {
    const { id } = useParams()
    return <Navigate to={id ? `/support/${id}` : '/support'} replace />
}

/** Учесть `?startapp=` один раз за сессию — с `replace`, чтобы системная
 *  кнопка «назад» вела наружу, а не на главный экран, о котором не просили. */
function useStartParamRedirect() {
    const navigate = useNavigate()
    const location = useLocation()
    useEffect(() => {
        const route = getStartRoute()
        if (route && route !== location.pathname) navigate(route, { replace: true })
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
}

/** Старые маршруты живут как редиректы: ссылки из бота и уведомлений не ломаются. */
const LEGACY: Record<string, string> = {
    '/subscription': '/',
    '/plans': '/pay',
    '/store': '/pay',
    '/billing': '/profile',
    '/promo': '/profile?promo=1',
    '/referral': '/profile?promo=1',
    '/statistics': '/',
    '/tickets': '/support',
    '/support/connect': '/guide',
    '/notifications/preferences': '/profile',
}

function Shell() {
    const { t } = useTranslation()
    const { userStats } = useAuth()
    useEffect(() => {
        document.title = userStats?.brand_name?.trim() || 'Caramba'
        for (const id of ['brand-favicon', 'brand-touch-icon']) {
            const icon = document.getElementById(id)
            if (isCarambaBrand(userStats?.brand_name)) icon?.setAttribute('href', carambaIconUrl)
            else icon?.removeAttribute('href')
        }
    }, [userStats?.brand_name])
    useStartParamRedirect()
    useEffect(() => applyTelegramTheme(), [])

    return (
        <div className="exa-app">
            <Suspense fallback={<div className="exa-loading">{t('exa.common.loading')}</div>}>
                <Routes>
                    <Route path="/" element={<Connect />} />
                    <Route path="/servers" element={<Servers />} />
                    <Route path="/servers/:subId" element={<Servers />} />
                    <Route path="/profile" element={<Profile />} />
                    <Route path="/devices" element={<Devices />} />
                    <Route path="/pay" element={<Pay />} />
                    <Route path="/guide" element={<Guide />} />
                    <Route path="/notifications" element={<Notifications />} />
                    <Route path="/support" element={<Support />} />
                    <Route path="/support/:id" element={<SupportTicket />} />
                    {Object.entries(LEGACY).map(([from, to]) => (
                        <Route key={from} path={from} element={<Navigate to={to} replace />} />
                    ))}
                    <Route path="/tickets/:id" element={<TicketRedirect />} />
                    <Route path="*" element={<Navigate to="/" replace />} />
                </Routes>
            </Suspense>
            <TabBar />
        </div>
    )
}

export default function ExaApp() {
    return (
        <AuthProvider>
            <AppLockProvider>
                <BrowserRouter basename="/app">
                    <NotificationProvider>
                        <ToastProvider>
                            <AppLockGate />
                            <Shell />
                        </ToastProvider>
                    </NotificationProvider>
                </BrowserRouter>
            </AppLockProvider>
        </AuthProvider>
    )
}

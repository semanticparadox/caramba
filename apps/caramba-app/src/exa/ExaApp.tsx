import { Suspense, lazy, useEffect } from 'react'
import { BrowserRouter, Navigate, Route, Routes, useLocation, useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { getStartRoute } from '../lib/telegram'
import { AuthProvider } from '../context/AuthContext'
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
    '/support': '/profile',
    '/tickets': '/profile',
    '/support/connect': '/guide',
    '/notifications/preferences': '/profile',
}

function Shell() {
    const { t } = useTranslation()
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
                    {Object.entries(LEGACY).map(([from, to]) => (
                        <Route key={from} path={from} element={<Navigate to={to} replace />} />
                    ))}
                    <Route path="/tickets/*" element={<Navigate to="/profile" replace />} />
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

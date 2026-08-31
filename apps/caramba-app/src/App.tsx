import { BrowserRouter as Router, Routes, Route, NavLink, Navigate, useLocation } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { AuthProvider } from './context/AuthContext'
import { AppLockProvider } from './context/AppLockContext'
import { NotificationProvider } from './context/NotificationContext'
import { lazy, Suspense } from 'react'
import Home from './pages/Home'
import Promo from './pages/Promo'
import Support from './pages/Support'
import ConnectGuide from './pages/ConnectGuide'
import AppLockGate from './components/AppLockGate'
import { hapticSelect } from './lib/haptics'

const Plans = lazy(() => import('./pages/Plans'))
const Servers = lazy(() => import('./pages/Servers'))
const Subscription = lazy(() => import('./pages/Subscription'))
const Devices = lazy(() => import('./pages/Devices'))
const Billing = lazy(() => import('./pages/Billing'))
const Notifications = lazy(() => import('./pages/Notifications'))
const NotificationPreferences = lazy(() => import('./pages/NotificationPreferences'))
const Tickets = lazy(() => import('./pages/Tickets'))
const TicketNew = lazy(() => import('./pages/TicketNew'))
const TicketDetail = lazy(() => import('./pages/TicketDetail'))
import './App.css'

/** Иконки табов — лёгкий inline-SVG, без библиотек. */
const TAB_ICONS = {
    home: (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M4 11.5 12 4l8 7.5" />
            <path d="M6 10.5V20h12v-9.5" />
        </svg>
    ),
    plans: (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M13 2 5 13.5h6L11 22l8-11.5h-6L13 2Z" />
        </svg>
    ),
    promo: (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <rect x="4" y="8" width="16" height="5" rx="1.5" />
            <path d="M6 13v7h12v-7M12 8v12" />
            <path d="M12 8c-4 0-4.5-5-1.5-5C12.5 3 12 8 12 8Zm0 0c4 0 4.5-5 1.5-5C11.5 3 12 8 12 8Z" />
        </svg>
    ),
    support: (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="9" />
            <circle cx="12" cy="12" r="3.6" />
            <path d="M12 3v5.4M12 15.6V21M3 12h5.4M15.6 12H21" />
        </svg>
    ),
} as const

type TabDef = {
    to: string
    icon: keyof typeof TAB_ICONS
    labelKey: string
    /** Считать таб активным и для этих префиксов маршрутов. */
    also?: string[]
    end?: boolean
}

const TABS: TabDef[] = [
    {
        to: '/',
        icon: 'home',
        labelKey: 'nav.center',
        end: true,
        also: ['/subscription', '/servers', '/devices', '/statistics', '/notifications'],
    },
    { to: '/plans', icon: 'plans', labelKey: 'nav.plans', also: ['/store', '/billing'] },
    { to: '/promo', icon: 'promo', labelKey: 'nav.promo', also: ['/referral'] },
    { to: '/support', icon: 'support', labelKey: 'nav.support', also: ['/tickets'] },
]

function BottomTabBar() {
    const { t } = useTranslation()
    const location = useLocation()

    return (
        <nav className="tab-bar" aria-label={t('nav.center')}>
            {TABS.map((tab) => (
                <NavLink
                    key={tab.to}
                    to={tab.to}
                    end={tab.end}
                    onClick={() => hapticSelect()}
                    className={({ isActive }) => {
                        const alsoActive = tab.also?.some((prefix) => location.pathname.startsWith(prefix)) ?? false
                        return `tab-link${isActive || alsoActive ? ' active' : ''}`
                    }}
                >
                    <span className="tab-icon">{TAB_ICONS[tab.icon]}</span>
                    <span className="tab-label">{t(tab.labelKey)}</span>
                </NavLink>
            ))}
        </nav>
    )
}

function AppShell() {
    const { t } = useTranslation()

    return (
        <div className="app-container app-shell">
            <div className="app-mesh" />
            <div className="app-noise" />
            <BottomTabBar />
            <Suspense fallback={<div className="loading">{t('app.loading')}</div>}>
                <Routes>
                    <Route path="/" element={<Home />} />
                    <Route path="/subscription" element={<Subscription />} />
                    <Route path="/servers/:subId" element={<Servers />} />
                    <Route path="/servers" element={<Servers />} />
                    <Route path="/devices" element={<Devices />} />
                    <Route path="/billing" element={<Billing />} />
                    <Route path="/plans" element={<Plans />} />
                    <Route path="/store" element={<Navigate to="/plans" replace />} />
                    <Route path="/statistics" element={<Navigate to="/" replace />} />
                    <Route path="/referral" element={<Navigate to="/promo" replace />} />
                    <Route path="/promo" element={<Promo />} />
                    <Route path="/support" element={<Support />} />
                    <Route path="/support/connect" element={<ConnectGuide />} />
                    <Route path="/notifications" element={<Notifications />} />
                    <Route path="/notifications/preferences" element={<NotificationPreferences />} />
                    <Route path="/tickets" element={<Tickets />} />
                    <Route path="/tickets/new" element={<TicketNew />} />
                    <Route path="/tickets/:id" element={<TicketDetail />} />
                </Routes>
            </Suspense>
            <AppLockGate />
        </div>
    )
}

function App() {
    return (
        <AuthProvider>
            <AppLockProvider>
                <Router basename="/app">
                    <NotificationProvider>
                        <AppShell />
                    </NotificationProvider>
                </Router>
            </AppLockProvider>
        </AuthProvider>
    )
}

export default App

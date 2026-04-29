import { BrowserRouter as Router, Routes, Route, NavLink, Navigate, useLocation } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { AuthProvider } from './context/AuthContext'
import { AppLockProvider } from './context/AppLockContext'
import { lazy, Suspense } from 'react'
import Home from './pages/Home'
import Promo from './pages/Promo'
import Support from './pages/Support'
import ConnectGuide from './pages/ConnectGuide'
import AppLockGate from './components/AppLockGate'

const Servers = lazy(() => import('./pages/Servers'))
const Subscription = lazy(() => import('./pages/Subscription'))
const Devices = lazy(() => import('./pages/Devices'))
const Billing = lazy(() => import('./pages/Billing'))
import './App.css'

function BottomCommandNav() {
    const { t } = useTranslation()
    const location = useLocation()
    const isCenter = location.pathname === '/'
        || location.pathname.startsWith('/subscription')
        || location.pathname.startsWith('/servers')
        || location.pathname.startsWith('/devices')
        || location.pathname.startsWith('/plans')
        || location.pathname.startsWith('/store')
        || location.pathname.startsWith('/billing')
        || location.pathname.startsWith('/statistics')
    const isPromo = location.pathname.startsWith('/promo')
    const isSupport = location.pathname.startsWith('/support')

    return (
        <nav className="bottom-command-nav" aria-label={t('nav.center')}>
            <NavLink to="/" className={`rail-link${isCenter ? ' active' : ''}`}>{t('nav.center')}</NavLink>
            <NavLink to="/promo" className={`rail-link${isPromo ? ' active' : ''}`}>{t('nav.promo')}</NavLink>
            <NavLink to="/support" className={`rail-link${isSupport ? ' active' : ''}`}>{t('nav.support')}</NavLink>
        </nav>
    )
}

function AppShell() {
    const { t } = useTranslation()

    return (
        <div className="app-container app-shell">
            <div className="app-mesh" />
            <div className="app-noise" />
            <BottomCommandNav />
            <Suspense fallback={<div className="loading">{t('app.loading')}</div>}>
                <Routes>
                    <Route path="/" element={<Home />} />
                    <Route path="/subscription" element={<Subscription />} />
                    <Route path="/servers/:subId" element={<Servers />} />
                    <Route path="/servers" element={<Servers />} />
                    <Route path="/devices" element={<Devices />} />
                    <Route path="/billing" element={<Billing />} />
                    <Route path="/store" element={<Navigate to="/" replace />} />
                    <Route path="/plans" element={<Navigate to="/" replace />} />
                    <Route path="/statistics" element={<Navigate to="/" replace />} />
                    <Route path="/referral" element={<Navigate to="/promo" replace />} />
                    <Route path="/promo" element={<Promo />} />
                    <Route path="/support" element={<Support />} />
                    <Route path="/support/connect" element={<ConnectGuide />} />
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
                    <AppShell />
                </Router>
            </AppLockProvider>
        </AuthProvider>
    )
}

export default App

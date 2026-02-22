import { BrowserRouter as Router, Routes, Route } from 'react-router-dom'
import { AuthProvider } from './context/AuthContext'
import { AppLockProvider } from './context/AppLockContext'
import Home from './pages/Home'
import Subscription from './pages/Subscription'
import Servers from './pages/Servers'
import Store from './pages/Store'
import Plans from './pages/Plans'
import ServerSelector from './pages/ServerSelector'
import Statistics from './pages/Statistics'
import Billing from './pages/Billing'
import Referral from './pages/Referral'
import Promo from './pages/Promo'
import Support from './pages/Support'
import AppLockGate from './components/AppLockGate'
import './App.css'

function App() {
    return (
        <AuthProvider>
            <AppLockProvider>
                <Router basename="/app">
                    <div className="app-container">
                        <Routes>
                            <Route path="/" element={<Home />} />
                            <Route path="/subscription" element={<Subscription />} />
                            <Route path="/servers" element={<Servers />} />
                            <Route path="/store" element={<Store />} />
                            <Route path="/plans" element={<Plans />} />
                            <Route path="/servers/:subId" element={<ServerSelector />} />
                            <Route path="/statistics" element={<Statistics />} />
                            <Route path="/billing" element={<Billing />} />
                            <Route path="/referral" element={<Referral />} />
                            <Route path="/promo" element={<Promo />} />
                            <Route path="/support" element={<Support />} />
                        </Routes>
                        <AppLockGate />
                    </div>
                </Router>
            </AppLockProvider>
        </AuthProvider>
    )
}

export default App

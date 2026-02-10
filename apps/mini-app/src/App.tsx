import { BrowserRouter as Router, Routes, Route } from 'react-router-dom'
import { AuthProvider } from './context/AuthContext'
import Home from './pages/Home'
import Subscription from './pages/Subscription'
import Servers from './pages/Servers'
import Store from './pages/Store'
import Statistics from './pages/Statistics'
import Billing from './pages/Billing'
import Referral from './pages/Referral'
import Support from './pages/Support'
import './App.css'

function App() {
    return (
        <AuthProvider>
            <Router basename="/app">
                <div className="app-container">
                    <Routes>
                        <Route path="/" element={<Home />} />
                        <Route path="/subscription" element={<Subscription />} />
                        <Route path="/servers" element={<Servers />} />
                        <Route path="/store" element={<Store />} />
                        <Route path="/statistics" element={<Statistics />} />
                        <Route path="/billing" element={<Billing />} />
                        <Route path="/referral" element={<Referral />} />
                        <Route path="/support" element={<Support />} />
                    </Routes>
                </div>
            </Router>
        </AuthProvider>
    )
}

export default App

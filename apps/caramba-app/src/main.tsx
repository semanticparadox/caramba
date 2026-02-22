import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App.tsx'
import './index.css'

const tg = (window as any).Telegram?.WebApp;
const isEnvDev = import.meta.env.DEV;

if (!tg?.initData && !isEnvDev) {
    ReactDOM.createRoot(document.getElementById('root')!).render(
        <div className="flex flex-col items-center justify-center min-h-screen bg-slate-950 text-white p-4">
            <div className="text-center p-8 bg-slate-900 rounded-2xl border border-white/5 shadow-2xl max-w-sm w-full">
                <div className="text-5xl mb-4">🛡️</div>
                <h2 className="text-xl font-bold mb-2 text-slate-100">Access Denied</h2>
                <p className="text-slate-400 text-sm">Please open this application directly through Telegram.</p>
            </div>
        </div>
    );
} else {
    ReactDOM.createRoot(document.getElementById('root')!).render(
        <React.StrictMode>
            <App />
        </React.StrictMode>,
    )
}

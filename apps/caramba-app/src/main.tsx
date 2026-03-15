import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App.tsx'
import './i18n'
import './index.css'
import { runTelegramReady } from './lib/telegram'

const tg = (window as any).Telegram?.WebApp;
const isEnvDev = import.meta.env.DEV;

if (!tg?.initData && !isEnvDev) {
    ReactDOM.createRoot(document.getElementById('root')!).render(
        <div className="tg-denied-screen">
            <div className="tg-denied-card glass-card">
                <div className="tg-denied-emblem" aria-hidden="true">TG</div>
                <h2>Откройте через Telegram</h2>
                <p>Этот интерфейс доступен только при запуске из Telegram Mini App.</p>
            </div>
        </div>,
    )
} else {
    runTelegramReady()
    ReactDOM.createRoot(document.getElementById('root')!).render(
        <React.StrictMode>
            <App />
        </React.StrictMode>,
    )
}

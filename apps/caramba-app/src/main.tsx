import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App.tsx'
import './i18n'
// Самохостинг шрифтов (@fontsource): без обращений к Google Fonts — РФ-аудитория.
import '@fontsource-variable/unbounded/index.css'
import '@fontsource/golos-text/400.css'
import '@fontsource/golos-text/500.css'
import '@fontsource/golos-text/600.css'
import '@fontsource/golos-text/700.css'
import '@fontsource/golos-text/800.css'
import './index.css'
import { LANGUAGE_STORAGE_KEY, runTelegramReady } from './lib/telegram'

const tg = (window as any).Telegram?.WebApp;
const isEnvDev = import.meta.env.DEV;

// Заглушка рисуется до монтирования React и до загрузки словарей i18n,
// поэтому язык читаем сам — из того же ключа, куда пишет LanguageSwitch.
// Неизвестное/отсутствующее значение — русский, как и везде.
const deniedCopy = (() => {
    let stored: string | null = null
    try {
        stored = localStorage.getItem(LANGUAGE_STORAGE_KEY)
    } catch {
        // приватный режим — просто останемся на русском
    }
    return stored === 'en'
        ? {
            title: 'Open via Telegram',
            hint: 'This interface only works when launched from the Telegram Mini App.',
        }
        : {
            title: 'Откройте через Telegram',
            hint: 'Этот интерфейс доступен только при запуске из Telegram Mini App.',
        }
})()

if (!tg?.initData && !isEnvDev) {
    ReactDOM.createRoot(document.getElementById('root')!).render(
        <div className="tg-denied-screen">
            <div className="tg-denied-card glass-card">
                <div className="tg-denied-emblem" aria-hidden="true">🤖</div>
                <h2>{deniedCopy.title}</h2>
                <p>{deniedCopy.hint}</p>
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

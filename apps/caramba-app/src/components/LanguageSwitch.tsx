import { useTranslation } from 'react-i18next'
import { useAuth } from '../context/AuthContext'
import { apiUrl } from '../config'
import { LANGUAGE_STORAGE_KEY, triggerSelectionHaptic } from '../lib/telegram'
import './LanguageSwitch.css'

const LANGUAGES = [
    { code: 'ru', label: 'Русский', short: 'RU' },
    { code: 'en', label: 'English', short: 'EN' },
] as const

export default function LanguageSwitch() {
    const { t, i18n } = useTranslation()
    const { token } = useAuth()
    const current = i18n.resolvedLanguage === 'en' ? 'en' : 'ru'

    const select = (code: string) => {
        if (code === current) return
        triggerSelectionHaptic()

        // Локально переключаемся сразу — интерфейс не должен ждать сеть.
        void i18n.changeLanguage(code)
        try {
            localStorage.setItem(LANGUAGE_STORAGE_KEY, code)
        } catch {
            // choice just won't survive a restart — not worth surfacing
        }

        // И сообщаем серверу: users.language_code — единственный источник
        // правды для языка уведомлений бота. Без этого переключатель менял бы
        // только приложение, а DM'ки продолжали бы приходить на старом языке.
        // Сбой запроса намеренно не откатывает локальный выбор и ничего не
        // показывает — язык приложения уже переключился, а сервер догонит при
        // следующем переключении.
        if (!token) return
        void fetch(apiUrl('/api/client/user/language'), {
            method: 'PUT',
            headers: {
                Authorization: `Bearer ${token}`,
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({ language: code }),
        }).catch(() => {
            // сеть недоступна — локальный выбор остаётся в силе
        })
    }

    return (
        <section className="lang-switch glass-card">
            <span className="lang-switch-title">{t('support.language')}</span>
            <div className="lang-switch-options" role="group" aria-label={t('support.language')}>
                {LANGUAGES.map((lang) => (
                    <button
                        key={lang.code}
                        type="button"
                        className={`lang-option${current === lang.code ? ' is-active' : ''}`}
                        aria-pressed={current === lang.code}
                        onClick={() => select(lang.code)}
                    >
                        <span className="lang-short">{lang.short}</span>
                        <span className="lang-label">{lang.label}</span>
                    </button>
                ))}
            </div>
        </section>
    )
}

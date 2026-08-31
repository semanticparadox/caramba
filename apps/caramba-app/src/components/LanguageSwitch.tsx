import { useTranslation } from 'react-i18next'
import { LANGUAGE_STORAGE_KEY, triggerSelectionHaptic } from '../lib/telegram'
import './LanguageSwitch.css'

const LANGUAGES = [
    { code: 'ru', label: 'Русский', short: 'RU' },
    { code: 'en', label: 'English', short: 'EN' },
] as const

export default function LanguageSwitch() {
    const { t, i18n } = useTranslation()
    const current = i18n.resolvedLanguage === 'en' ? 'en' : 'ru'

    const select = (code: string) => {
        if (code === current) return
        triggerSelectionHaptic()
        void i18n.changeLanguage(code)
        try {
            localStorage.setItem(LANGUAGE_STORAGE_KEY, code)
        } catch {
            // choice just won't survive a restart — not worth surfacing
        }
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

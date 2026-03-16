import i18n from 'i18next'
import Backend from 'i18next-http-backend'
import { initReactI18next } from 'react-i18next'
import { getTelegramLanguage } from './lib/telegram'

const baseUrl = import.meta.env.BASE_URL || '/'
const normalizedBase = baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`

void i18n
    .use(Backend)
    .use(initReactI18next)
    .init({
        lng: getTelegramLanguage(),
        fallbackLng: 'ru',
        supportedLngs: ['ru', 'en'],
        ns: ['common'],
        defaultNS: 'common',
        interpolation: {
            escapeValue: false,
        },
        react: {
            useSuspense: false,
        },
        backend: {
            loadPath: `${normalizedBase}locales/{{lng}}.json`,
        },
    })

export default i18n

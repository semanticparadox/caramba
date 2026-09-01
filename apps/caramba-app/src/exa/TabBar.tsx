import { NavLink, useLocation } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { ExaIcon, type ExaIconName } from './icons'
import { hapticSelect } from '../lib/haptics'

type Tab = {
    to: string
    icon: ExaIconName
    labelKey: string
    /** Считать вкладку активной и на этих префиксах. */
    also: string[]
}

/** Ровно три вкладки. «Назад» — системная кнопка Telegram, поэтому у корней
 *  вкладок нет своей стрелки назад. */
const TABS: Tab[] = [
    { to: '/', icon: 'connect', labelKey: 'exa.nav.connect', also: ['/pay', '/qr'] },
    { to: '/servers', icon: 'server', labelKey: 'exa.nav.servers', also: [] },
    {
        to: '/profile',
        icon: 'profile',
        labelKey: 'exa.nav.profile',
        also: ['/devices', '/payments', '/promo', '/notifications'],
    },
]

export default function TabBar() {
    const { t } = useTranslation()
    const { pathname } = useLocation()
    return (
        <nav className="exa-tabbar" aria-label={t('exa.nav.aria')}>
            {TABS.map((tab) => (
                <NavLink
                    key={tab.to}
                    to={tab.to}
                    end={tab.to === '/'}
                    onClick={() => hapticSelect()}
                    className={({ isActive }) => {
                        const also = tab.also.some((p) => pathname.startsWith(p))
                        return `exa-tab${isActive || also ? ' is-active' : ''}`
                    }}
                >
                    <ExaIcon name={tab.icon} />
                    <span>{t(tab.labelKey)}</span>
                </NavLink>
            ))}
        </nav>
    )
}

import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import WebApp from '@twa-dev/sdk'
import { useAuth } from '../../context/AuthContext'
import { PLATFORM_DIRECTORY, type PlatformKey } from '../../data/appDirectory'
import { copyText } from '../../lib/copyActions'
import { hapticSuccess, hapticTap } from '../../lib/haptics'
import { ExaIcon } from '../icons'
import { Button, Pill, ScreenHeader } from '../ui'
import { pickPrimary, subscriptionUrl } from '../lib/subscription'
import { useGuides, type GuidePlatform } from '../lib/useGuides'
import { useToast } from '../lib/useToast'

const PLATFORMS: { id: PlatformKey; labelKey: string; guide: GuidePlatform }[] = [
    { id: 'ios', labelKey: 'exa.guide.ios', guide: 'ios' },
    { id: 'android', labelKey: 'exa.guide.android', guide: 'android' },
    { id: 'windows', labelKey: 'exa.guide.windows', guide: 'windows' },
    { id: 'macos', labelKey: 'exa.guide.macos', guide: 'macos' },
    { id: 'linux', labelKey: 'exa.guide.linux', guide: 'linux' },
    { id: 'tv', labelKey: 'exa.guide.tv', guide: 'tv' },
]

/** Гайд по подключению: платформа → приложения с загрузкой и подробной
 *  инструкцией на Telegraph. Ссылка подписки — одной кнопкой сверху. */
export default function Guide() {
    const { t } = useTranslation()
    const toast = useToast()
    const { token, subscriptions } = useAuth()
    const guides = useGuides(token)
    const sub = useMemo(() => pickPrimary(subscriptions), [subscriptions])
    const [platform, setPlatform] = useState<PlatformKey>(() => {
        const p = (WebApp.platform || '').toLowerCase()
        if (p.includes('ios')) return 'ios'
        if (p.includes('android')) return 'android'
        if (p.includes('mac')) return 'macos'
        if (p.includes('win') || p.includes('tdesktop')) return 'windows'
        return 'android'
    })
    const dir = PLATFORM_DIRECTORY.find((d) => d.id === platform) ?? PLATFORM_DIRECTORY[0]
    const guideUrl = guides[PLATFORMS.find((p) => p.id === platform)?.guide ?? 'index'] ?? guides.index

    const copy = async () => {
        if (!sub) return
        if (await copyText(subscriptionUrl(sub))) {
            hapticSuccess()
            toast(t('exa.common.copied'))
        }
    }
    const open = (url: string) => {
        hapticTap()
        WebApp.openLink(url)
    }

    return (
        <div className="exa-screen">
            <ScreenHeader title={t('exa.guide.title')} />

            <section className="exa-card">
                <p className="exa-card__note">{t('exa.guide.intro')}</p>
                <Button size="md" icon={<ExaIcon name="copy" size={20} />} disabled={!sub} onClick={() => void copy()}>
                    {t('exa.home.copyLink')}
                </Button>
                {!sub ? <p className="exa-muted">{t('exa.guide.noSubscription')}</p> : null}
            </section>

            <div className="exa-platforms">
                {PLATFORMS.map((p) => (
                    <button
                        key={p.id}
                        type="button"
                        className={`exa-platform${platform === p.id ? ' is-on' : ''}`}
                        onClick={() => {
                            hapticTap()
                            setPlatform(p.id)
                        }}
                    >
                        {t(p.labelKey)}
                    </button>
                ))}
            </div>

            {guideUrl ? (
                <Button variant="secondary" size="md" icon={<ExaIcon name="external" size={20} />} onClick={() => open(guideUrl)}>
                    {t('exa.guide.openGuide')}
                </Button>
            ) : null}

            <section className="exa-card exa-card--list">
                {dir.entries.map((entry) => (
                    <div key={entry.id} className="exa-row" style={{ minHeight: 64 }}>
                        <span className="exa-client-mark">{entry.name.slice(0, 2)}</span>
                        <span className="exa-row__body">
                            <span className="exa-row__title">
                                <span>{entry.name}</span>
                                {entry.badgeKey ? <Pill tone="accent">{t('exa.common.recommended')}</Pill> : entry.badgeText ? <Pill>{entry.badgeText}</Pill> : null}
                            </span>
                            <span className="exa-row__meta" style={{ fontSize: 12, display: 'block', whiteSpace: 'normal', lineHeight: 1.35 }}>
                                {entry.description}
                            </span>
                        </span>
                        <Button variant="secondary" size="sm" block={false} onClick={() => open(entry.officialUrl)}>
                            {t('exa.guide.download')}
                        </Button>
                    </div>
                ))}
            </section>

            {guides.router ? (
                <button type="button" className="exa-card exa-linkrow" onClick={() => open(guides.router!)}>
                    <span className="exa-linkrow__icon">
                        <ExaIcon name="router" size={22} />
                    </span>
                    <span className="exa-linkrow__title">{t('exa.guide.router')}</span>
                    <ExaIcon name="chevron" size={20} className="exa-linkrow__chevron" />
                </button>
            ) : null}
        </div>
    )
}

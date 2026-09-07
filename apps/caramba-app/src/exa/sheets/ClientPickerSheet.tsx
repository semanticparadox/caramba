import { useTranslation } from 'react-i18next'
import WebApp from '@twa-dev/sdk'
import type { UserSubscription } from '../../context/AuthContext'
import { copyText } from '../../lib/copyActions'
import { hapticSuccess } from '../../lib/haptics'
import { Button, Pill, Sheet } from '../ui'
import { subscriptionUrl } from '../lib/subscription'
import { useToast } from '../lib/useToast'

type Client = {
    id: string
    name: string
    mark: string
    platformsKey: string
    /** deep-link импорта; без него — копируем ссылку и подсказываем. */
    scheme?: (url: string) => string
    recommended?: boolean
}

/** Сторонние клиенты с автоимпортом подписки. Своё приложение (Caramba
 *  Connect) рендерится отдельной строкой выше этого списка, потому что у
 *  него не импорт подписки, а вход по ссылке `caramba://connect`.
 *  Hiddify сам определяет формат по User-Agent, поэтому ссылка передаётся
 *  чистой, без ?client=. */
const CLIENTS: Client[] = [
    {
        id: 'hiddify',
        name: 'Hiddify',
        mark: 'H',
        platformsKey: 'exa.clients.hiddify',
        scheme: (u) => `hiddify://import/${encodeURIComponent(u)}`,
    },
    { id: 'happ', name: 'Happ', mark: 'Ha', platformsKey: 'exa.clients.happ', scheme: (u) => `happ://import/${encodeURIComponent(u)}` },
    { id: 'v2raytun', name: 'v2rayTun', mark: 'V', platformsKey: 'exa.clients.v2raytun', scheme: (u) => `v2raytun://import/${encodeURIComponent(u)}` },
    { id: 'singbox', name: 'sing-box', mark: 'S', platformsKey: 'exa.clients.singbox', scheme: (u) => `sing-box://import-remote-profile?url=${encodeURIComponent(u)}#EXA` },
]

export default function ClientPickerSheet({
    open,
    sub,
    onClose,
    onGuide,
    onCaramba,
}: {
    open: boolean
    sub: UserSubscription | null
    onClose: () => void
    onGuide: () => void
    onCaramba: () => void
}) {
    const { t } = useTranslation()
    const toast = useToast()

    const openIn = async (client: Client) => {
        if (!sub) return
        const url = subscriptionUrl(sub)
        if (client.scheme) {
            const w = window.open(client.scheme(url), '_blank', 'noopener,noreferrer')
            if (w) {
                onClose()
                return
            }
        }
        // Схема не открылась (нет приложения или WebView запретил) — кладём ссылку в буфер.
        if (await copyText(url)) {
            hapticSuccess()
            toast(t('exa.home.copiedOpenManually', { app: client.name }))
        }
        onClose()
    }

    return (
        <Sheet open={open} title={t('exa.home.openInApp')} subtitle={t('exa.home.openInAppHint')} onClose={onClose}>
            <div className="exa-card exa-card--list">
                <button
                    type="button"
                    className="exa-row is-tappable"
                    onClick={() => {
                        onClose()
                        onCaramba()
                    }}
                >
                    <span className="exa-client-mark">C</span>
                    <span className="exa-row__body">
                        <span className="exa-row__title">
                            <span>Caramba Connect</span>
                        </span>
                        <span className="exa-row__meta">{t('exa.caramba.pickerMeta')}</span>
                    </span>
                    <Pill tone="accent">{t('exa.caramba.pill')}</Pill>
                </button>
                {CLIENTS.map((c) => (
                    <button key={c.id} type="button" className="exa-row is-tappable" onClick={() => void openIn(c)}>
                        <span className="exa-client-mark">{c.mark}</span>
                        <span className="exa-row__body">
                            <span className="exa-row__title">
                                <span>{c.name}</span>
                            </span>
                            <span className="exa-row__meta">{t(c.platformsKey)}</span>
                        </span>
                        {c.recommended ? <Pill tone="accent">{t('exa.common.recommended')}</Pill> : null}
                    </button>
                ))}
            </div>
            <Button
                variant="ghost"
                size="md"
                onClick={() => {
                    onClose()
                    if (WebApp.platform) onGuide()
                }}
            >
                {t('exa.home.noAppGuide')}
            </Button>
        </Sheet>
    )
}

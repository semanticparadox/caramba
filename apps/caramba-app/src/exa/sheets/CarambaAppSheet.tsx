import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import WebApp from '@twa-dev/sdk'
import { useAuth } from '../../context/AuthContext'
import { copyText } from '../../lib/copyActions'
import { hapticError, hapticSuccess } from '../../lib/haptics'
import { Button, Pill, SectionLabel, Sheet } from '../ui'
import { ExaIcon } from '../icons'
import { useToast } from '../lib/useToast'
import { APP_PLATFORMS, useAppDownloads } from '../lib/useAppDownloads'
import { openInCarambaApp, requestConnectLink, type ConnectLink } from '../lib/connectLink'

/** Приглашение в своё приложение: вход по одноразовой ссылке и список сборок.
 *  Узнать из WebView, установлено ли приложение, нельзя — поэтому по тапу
 *  делаем всё сразу: копируем ссылку, пробуем открыть схему и оставляем
 *  ссылку на экране, чтобы её можно было вставить руками. */
export default function CarambaAppSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
    const { t } = useTranslation()
    const toast = useToast()
    const { token, userStats } = useAuth()
    const downloads = useAppDownloads(token)
    const [busy, setBusy] = useState(false)
    const [link, setLink] = useState<ConnectLink | null>(null)

    // Ссылка живёт 30 минут и сгорает после входа — держать её на экране
    // до следующего открытия шторки нечестно.
    useEffect(() => {
        if (!open) setLink(null)
    }, [open])

    const openApp = async () => {
        if (!token) return
        setBusy(true)
        const l = await requestConnectLink(token)
        if (!l) {
            hapticError()
            toast(t('exa.caramba.linkFailed'))
        } else {
            setLink(l)
            if (await copyText(l.link)) {
                hapticSuccess()
                toast(t('exa.caramba.copiedHint'))
            }
            openInCarambaApp(l.link)
        }
        setBusy(false)
    }

    const copy = async () => {
        if (!link) return
        if (await copyText(link.link)) {
            hapticSuccess()
            toast(t('exa.common.copied'))
        }
    }

    // Linux показываем только со ссылкой: обещать «Скоро» там, куда мы
    // не собираемся, — врать.
    const platforms = APP_PLATFORMS.filter((p) => p !== 'linux' || !!downloads.linux)

    // Домен панели могут заблокировать — APK можно получить напрямую от бота
    // по file_id, в обход домена вообще. Кнопка есть, как только известен
    // username бота: сам бот решает, готов ли файл (иначе ответит "не загружен").
    const telegramApkUrl = userStats?.bot_username ? `https://t.me/${userStats.bot_username}?start=apk` : null

    return (
        <Sheet open={open} title={t('exa.caramba.title')} subtitle={t('exa.caramba.subtitle')} onClose={onClose}>
            <div className="exa-stack" style={{ gap: 6 }}>
                <SectionLabel>{t('exa.caramba.signInSection')}</SectionLabel>
                <div className="exa-card">
                    <Button icon={<ExaIcon name="connect" size={22} />} disabled={busy} onClick={() => void openApp()}>
                        {busy ? t('exa.caramba.preparing') : t('exa.caramba.open')}
                    </Button>
                    {link ? (
                        <>
                            <p className="exa-card__note">{t('exa.caramba.linkReady')}</p>
                            <div className="exa-codebox">{link.link}</div>
                            <Button
                                variant="secondary"
                                size="sm"
                                icon={<ExaIcon name="copy" size={18} />}
                                onClick={() => void copy()}
                            >
                                {t('exa.caramba.copy')}
                            </Button>
                            <p className="exa-card__note">{t('exa.caramba.linkNote')}</p>
                        </>
                    ) : null}
                </div>
            </div>

            <div className="exa-stack" style={{ gap: 6 }}>
                <SectionLabel>{t('exa.caramba.downloadSection')}</SectionLabel>
                <div className="exa-card exa-card--list">
                    {platforms.map((p) => {
                        const url = downloads[p]
                        const primaryAction = url ? (
                            <Button variant="secondary" size="sm" block={false} onClick={() => WebApp.openLink(url)}>
                                {t('exa.caramba.download')}
                            </Button>
                        ) : (
                            <Pill>{t('exa.caramba.soon')}</Pill>
                        )
                        return (
                            <div key={p} className="exa-row">
                                <span className="exa-row__body">
                                    <span className="exa-row__title">
                                        <span>{t(`exa.caramba.${p}`)}</span>
                                    </span>
                                    {p === 'android' ? (
                                        <span className="exa-row__meta">{t('exa.caramba.androidMeta')}</span>
                                    ) : null}
                                </span>
                                {p === 'android' && telegramApkUrl ? (
                                    // Вторая кнопка рядом с обычной: файл из Telegram не зависит
                                    // от домена панели и не требует настроенной ссылки скачивания.
                                    <div style={{ display: 'flex', gap: 6 }}>
                                        {primaryAction}
                                        <Button
                                            variant="secondary"
                                            size="sm"
                                            block={false}
                                            onClick={() => WebApp.openTelegramLink(telegramApkUrl)}
                                        >
                                            {t('exa.caramba.getInTelegram')}
                                        </Button>
                                    </div>
                                ) : (
                                    primaryAction
                                )}
                            </div>
                        )
                    })}
                </div>
            </div>
        </Sheet>
    )
}

import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { copyText } from '../../lib/copyActions'
import { hapticSuccess } from '../../lib/haptics'
import { ExaIcon } from '../icons'
import { Button, IconButton, SectionLabel, Segmented, Sheet } from '../ui'
import { countryName } from '../lib/countries'
import { linksForNode, type InboundLink, type LinkKind } from '../lib/links'
import type { ExaServer } from '../lib/useServers'
import { useToast } from '../lib/useToast'
import QrSheet from './QrSheet'

/** «Ссылка для роутера»: тип подключения × маршрут (прямой / через релей),
 *  результат — готовая ссылка одного инбаунда. Для тех, кто настраивает
 *  роутер или десктоп руками; глубина — один уровень. */
export default function RouterLinkSheet({
    open,
    server,
    relayServer,
    links,
    loading,
    onClose,
}: {
    open: boolean
    server: ExaServer | null
    /** Релей-узел (RU), если есть — даёт переключатель маршрута. */
    relayServer: ExaServer | null
    links: InboundLink[]
    loading: boolean
    onClose: () => void
}) {
    const { t, i18n } = useTranslation()
    const toast = useToast()
    const [route, setRoute] = useState<'direct' | 'relay'>('direct')
    const [kind, setKind] = useState<LinkKind | null>(null)
    const [qr, setQr] = useState(false)

    const targetNode = route === 'relay' && relayServer ? relayServer : server
    const options = useMemo(() => (targetNode ? linksForNode(links, targetNode.id) : []), [links, targetNode])

    useEffect(() => {
        if (!open) {
            setRoute('direct')
            setKind(null)
            return
        }
        if (options.length && (kind === null || !options.some((o) => o.kind === kind))) setKind(options[0].kind)
    }, [open, options, kind])

    const current = options.find((o) => o.kind === kind) ?? options[0] ?? null

    const copy = async () => {
        if (!current) return
        if (await copyText(current.url)) {
            hapticSuccess()
            toast(t('exa.common.copied'))
        }
    }

    if (!server) return null
    return (
        <>
            <Sheet
                open={open && !qr}
                title={t('exa.servers.routerLink')}
                subtitle={`${countryName(server.country_code, i18n.language)}, ${server.name}`}
                onClose={onClose}
            >
                <div className="exa-stack" style={{ gap: 6 }}>
                    <SectionLabel>{t('exa.servers.connectionType')}</SectionLabel>
                    {loading && options.length === 0 ? <div className="exa-loading">{t('exa.common.loading')}</div> : null}
                    {!loading && options.length === 0 ? <p className="exa-muted">{t('exa.servers.noLinks')}</p> : null}
                    {options.length > 0 ? (
                        <div className="exa-card exa-card--list">
                            {options.map((o) => (
                                <label key={o.kind} className="exa-row is-tappable" style={{ minHeight: 52, padding: '12px 14px' }} onClick={() => setKind(o.kind)}>
                                    <span className={`exa-radio${o.kind === kind ? ' is-on' : ''}`} />
                                    <span className="exa-row__body" style={{ gap: 1 }}>
                                        <span className="exa-row__title">
                                            <span>{t(`exa.servers.kind.${o.kind}.name`)}</span>
                                        </span>
                                        <span className="exa-row__meta" style={{ fontSize: 12 }}>
                                            {t(`exa.servers.kind.${o.kind}.hint`)}
                                        </span>
                                    </span>
                                </label>
                            ))}
                        </div>
                    ) : null}
                </div>

                {relayServer ? (
                    <div className="exa-stack" style={{ gap: 6 }}>
                        <SectionLabel>{t('exa.servers.route')}</SectionLabel>
                        <Segmented
                            value={route}
                            onChange={setRoute}
                            options={[
                                { value: 'direct', label: t('exa.servers.direct') },
                                { value: 'relay', label: t('exa.servers.viaRelay', { code: relayServer.country_code.toUpperCase() }) },
                            ]}
                        />
                    </div>
                ) : null}

                {current ? (
                    <div className="exa-stack" style={{ gap: 8 }}>
                        <div className="exa-code">{current.url}</div>
                        <div style={{ display: 'grid', gridTemplateColumns: '1fr auto', gap: 8 }}>
                            <Button size="md" icon={<ExaIcon name="copy" size={20} />} onClick={() => void copy()}>
                                {t('exa.common.copy')}
                            </Button>
                            <IconButton label="QR" className="is-lg" onClick={() => setQr(true)}>
                                <ExaIcon name="qr" size={22} />
                            </IconButton>
                        </div>
                    </div>
                ) : null}
            </Sheet>
            {current ? (
                <QrSheet
                    open={open && qr}
                    value={current.url}
                    title={t(`exa.servers.kind.${current.kind}.name`)}
                    subtitle={`${countryName(targetNode?.country_code ?? server.country_code, i18n.language)}, ${targetNode?.name ?? server.name}`}
                    onClose={() => setQr(false)}
                />
            ) : null}
        </>
    )
}

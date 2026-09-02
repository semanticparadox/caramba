import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useAuth } from '../../context/AuthContext'
import { hapticError, hapticSuccess, hapticTap } from '../../lib/haptics'
import { ExaIcon } from '../icons'
import { Button, CountryChip, IconButton, Pill, ScreenHeader } from '../ui'
import { countryName } from '../lib/countries'
import { useInboundLinks } from '../lib/links'
import { availability, serverSpeedMbps } from '../lib/serverLabel'
import { pickPrimary } from '../lib/subscription'
import { groupByCountry, loadOf, selectServer, useServers, type ExaServer } from '../lib/useServers'
import RouterLinkSheet from '../sheets/RouterLinkSheet'

/** «Серверы»: страны → узлы со скоростью канала и нагрузкой. Тап —
 *  переключение с подтверждением прямо в строке. У каждого узла — «прямая
 *  ссылка» на конкретный инбаунд для роутера или ручной настройки.
 *  Пинг не показываем: из WebView Telegram его честно не измерить, а задержка
 *  панель→узел человеку ничего не говорит. */
export default function Servers() {
    const { t, i18n } = useTranslation()
    const { token, subscriptions, refreshData } = useAuth()
    const sub = useMemo(() => pickPrimary(subscriptions), [subscriptions])
    const { servers, loading } = useServers(token, sub?.id ?? null)
    const { links, loading: linksLoading } = useInboundLinks(token, sub?.id ?? null)

    const [pending, setPending] = useState<ExaServer | null>(null)
    const [busy, setBusy] = useState(false)
    const [routerFor, setRouterFor] = useState<ExaServer | null>(null)

    const groups = useMemo(() => groupByCountry(servers), [servers])
    const online = servers.filter((s) => availability(s) !== 'offline').length
    const currentId = sub?.last_node_id ?? null
    const relayServer = servers.find((s) => s.country_code.toUpperCase() === 'RU') ?? null
    const locale = i18n.language

    const confirm = async () => {
        if (!token || !sub || !pending || busy) return
        setBusy(true)
        const ok = await selectServer(token, sub.id, pending.id)
        setBusy(false)
        if (ok) {
            hapticSuccess()
            setPending(null)
            void refreshData()
        } else {
            hapticError()
        }
    }

    return (
        <div className="exa-screen">
            <ScreenHeader title={t('exa.servers.title')} aside={servers.length ? t('exa.servers.online', { count: online }) : undefined} />
            {loading && servers.length === 0 ? <div className="exa-loading">{t('exa.common.loading')}</div> : null}
            {!loading && servers.length === 0 ? <p className="exa-muted exa-center">{t('exa.servers.empty')}</p> : null}

            {groups.map((g) => (
                <div key={g.code} className="exa-stack" style={{ gap: 8 }}>
                    <div className="exa-group-head">
                        <CountryChip code={g.code} />
                        <span>{countryName(g.code, locale)}</span>
                    </div>
                    <section className={`exa-card exa-card--list${g.servers.some((s) => s.id === pending?.id) ? ' is-selected' : ''}`}>
                        {g.servers.map((s, idx) => {
                            const load = loadOf(s)
                            const avail = availability(s)
                            const selected = s.id === currentId
                            const asking = pending?.id === s.id
                            const speed = serverSpeedMbps(s)
                            // Несколько узлов в одной стране различаем порядковым номером,
                            // а не техническим «Node #7».
                            const title = g.servers.length > 1 ? `${countryName(g.code, locale)} ${idx + 1}` : countryName(g.code, locale)
                            return (
                                <div key={s.id}>
                                    <div
                                        className={`exa-row${sub && !selected && avail !== 'offline' ? ' is-tappable' : ''}`}
                                        role="button"
                                        onClick={() => {
                                            if (!sub || selected || busy || avail === 'offline') return
                                            hapticTap()
                                            setPending(asking ? null : s)
                                        }}
                                    >
                                        <div className="exa-row__body">
                                            <div className="exa-row__title">
                                                <span>{title}</span>
                                                {selected ? <Pill tone="accent">{t('exa.servers.selected')}</Pill> : null}
                                                {avail === 'full' ? <Pill tone="warning">{t('exa.servers.full')}</Pill> : null}
                                                {avail === 'offline' ? <Pill tone="danger">{t('exa.servers.offline')}</Pill> : null}
                                            </div>
                                            <div className="exa-row__meta">
                                                {speed ? <span>{t('exa.servers.speed', { mbps: speed })}</span> : null}
                                                <span className={`exa-load${load.level === 'high' ? ' is-high' : ''}`}>
                                                    <span style={{ width: `${Math.max(8, Math.round(load.ratio * 100))}%` }} />
                                                </span>
                                                <span>{t(`exa.servers.load.${load.level}`)}</span>
                                            </div>
                                        </div>
                                        <IconButton
                                            label={t('exa.servers.routerLink')}
                                            onClick={(e) => {
                                                e.stopPropagation()
                                                setRouterFor(s)
                                            }}
                                        >
                                            <ExaIcon name="router" size={22} />
                                        </IconButton>
                                    </div>
                                    {asking ? (
                                        <div className="exa-inline-confirm">
                                            <span>{t('exa.servers.switchTo', { name: title })}</span>
                                            <Button size="sm" block={false} disabled={busy} onClick={() => void confirm()}>
                                                {t('exa.common.yes')}
                                            </Button>
                                            <Button variant="secondary" size="sm" block={false} onClick={() => setPending(null)}>
                                                {t('exa.common.no')}
                                            </Button>
                                        </div>
                                    ) : null}
                                </div>
                            )
                        })}
                    </section>
                </div>
            ))}

            <RouterLinkSheet
                open={routerFor !== null}
                server={routerFor}
                relayServer={routerFor && relayServer && relayServer.id !== routerFor.id ? relayServer : null}
                links={links}
                loading={linksLoading}
                onClose={() => setRouterFor(null)}
            />
        </div>
    )
}

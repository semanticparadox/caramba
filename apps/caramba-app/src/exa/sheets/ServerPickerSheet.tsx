import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { hapticSuccess, hapticError } from '../../lib/haptics'
import { CountryChip, Pill, Sheet } from '../ui'
import { countryName } from '../lib/countries'
import { loadOf, selectServer, useServers, type ExaServer } from '../lib/useServers'
import { availability, serverSpeedMbps } from '../lib/serverLabel'

/** Быстрая смена сервера с главного экрана: одна страница, тап — переключение. */
export default function ServerPickerSheet({
    open,
    token,
    subId,
    currentNodeId,
    onClose,
    onChanged,
}: {
    open: boolean
    token: string | null
    subId: number | null
    currentNodeId: number | null
    onClose: () => void
    onChanged: () => void
}) {
    const { t, i18n } = useTranslation()
    const { servers, loading } = useServers(open ? token : null)
    const [busy, setBusy] = useState<number | null>(null)

    const pick = async (s: ExaServer) => {
        if (!token || !subId || busy) return
        if (s.id === currentNodeId) {
            onClose()
            return
        }
        setBusy(s.id)
        const ok = await selectServer(token, subId, s.id)
        setBusy(null)
        if (ok) {
            hapticSuccess()
            onChanged()
            onClose()
        } else {
            hapticError()
        }
    }

    return (
        <Sheet
            open={open}
            title={t('exa.home.changeServer')}
            aside={servers.length ? t('exa.servers.online', { count: servers.filter((s) => availability(s) !== 'offline').length }) : undefined}
            onClose={onClose}
        >
            {loading && servers.length === 0 ? <div className="exa-loading">{t('exa.common.loading')}</div> : null}
            <div className="exa-card exa-card--list">
                {servers.map((s) => {
                    const load = loadOf(s)
                    const selected = s.id === currentNodeId
                    const avail = availability(s)
                    const speed = serverSpeedMbps(s)
                    return (
                        <button
                            key={s.id}
                            type="button"
                            className="exa-row is-tappable"
                            disabled={busy !== null || avail === 'offline'}
                            onClick={() => void pick(s)}
                        >
                            <CountryChip code={s.country_code} />
                            <span className="exa-row__body">
                                <span className="exa-row__title">
                                    <span>{countryName(s.country_code, i18n.language)}</span>
                                    {selected ? <Pill tone="accent">{t('exa.servers.selected')}</Pill> : null}
                                    {avail === 'full' ? <Pill tone="warning">{t('exa.servers.full')}</Pill> : null}
                                    {avail === 'offline' ? <Pill tone="danger">{t('exa.servers.offline')}</Pill> : null}
                                </span>
                                <span className="exa-row__meta">
                                    {speed ? <span>{t('exa.servers.speed', { mbps: speed })}</span> : null}
                                    <span className={`exa-load${load.level === 'high' ? ' is-high' : ''}`}>
                                        <span style={{ width: `${Math.max(8, Math.round(load.ratio * 100))}%` }} />
                                    </span>
                                    <span>{t(`exa.servers.load.${load.level}`)}</span>
                                </span>
                            </span>
                        </button>
                    )
                })}
            </div>
        </Sheet>
    )
}

import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useAuth } from '../../context/AuthContext'
import { apiUrl } from '../../config'
import { copyText } from '../../lib/copyActions'
import { hapticError, hapticSuccess, hapticTap } from '../../lib/haptics'
import { subscriptionLimitBytes } from '../../lib/subscriptionMetrics'
import ExaMark from '../ExaMark'
import Mascot from '../Mascot'
import { ExaIcon } from '../icons'
import { Button, Card, CountryChip, IconButton, Pill } from '../ui'
import { deriveState, formatDate, formatGb, pickPrimary, subscriptionUrl } from '../lib/subscription'
import { countryName } from '../lib/countries'
import { useServers } from '../lib/useServers'
import { availability, serverSpeedMbps } from '../lib/serverLabel'
import { useToast } from '../lib/useToast'
import ClientPickerSheet from '../sheets/ClientPickerSheet'
import QrSheet from '../sheets/QrSheet'
import ServerPickerSheet from '../sheets/ServerPickerSheet'

/** «Подключение» — вся работа на одном экране: статус, оплата, сервер,
 *  трафик, устройства, ссылка. Четыре состояния из макета. */
export default function Connect() {
    const { t, i18n } = useTranslation()
    const navigate = useNavigate()
    const toast = useToast()
    const { token, subscriptions, userStats, refreshData, isLoading } = useAuth()
    const sub = useMemo(() => pickPrimary(subscriptions), [subscriptions])
    const state = deriveState(sub)
    const locale = i18n.language

    const { servers } = useServers(token, sub?.id ?? null)
    const currentServer = sub?.last_node_id ? servers.find((s) => s.id === sub.last_node_id) ?? null : null

    const [sheet, setSheet] = useState<'server' | 'client' | 'qr' | null>(null)
    const [activating, setActivating] = useState(false)

    const copyLink = async () => {
        if (!sub) return
        if (await copyText(subscriptionUrl(sub))) {
            hapticSuccess()
            toast(t('exa.common.copied'))
        }
    }

    const activate = async () => {
        if (!token || !sub || activating) return
        hapticTap()
        setActivating(true)
        try {
            const res = await fetch(apiUrl(`/api/client/subscription/${sub.id}/activate`), {
                method: 'POST',
                headers: { Authorization: `Bearer ${token}` },
            })
            if (res.ok) {
                hapticSuccess()
                await refreshData()
            } else {
                hapticError()
                toast(t('exa.common.error'))
            }
        } catch {
            hapticError()
            toast(t('exa.common.error'))
        } finally {
            setActivating(false)
        }
    }

    if (isLoading && !sub) {
        return (
            <div className="exa-screen">
                <StatusHeader lit={false} title={t('exa.home.stateOff')} sub={t('exa.common.loading')} tone="off" onBell={() => navigate('/notifications')} />
            </div>
        )
    }

    // ---------- нет подписки ----------
    if (!sub || state === 'none') {
        return (
            <div className="exa-screen">
                <StatusHeader lit={false} title={t('exa.home.stateOff')} sub={t('exa.home.noSubscription')} tone="off" />
                <div className="exa-empty">
                    <Mascot state="sitting" />
                    <div>
                        <div className="exa-empty__title">{t('exa.home.noSubscription')}</div>
                        <div className="exa-empty__text">{t('exa.home.noSubscriptionText')}</div>
                    </div>
                    <div className="exa-empty__actions">
                        <Button onClick={() => navigate('/pay')}>{t('exa.home.choosePlan')}</Button>
                        <Button variant="ghost" size="md" onClick={() => navigate('/profile?promo=1')}>
                            {t('exa.home.enterPromo')}
                        </Button>
                    </div>
                </div>
            </div>
        )
    }

    const limitBytes = subscriptionLimitBytes(sub)
    const usedBytes = sub.used_traffic_bytes || 0
    const unlimited = limitBytes <= 0
    const days = Math.max(0, sub.days_left || 0)
    const deviceLimit = sub.device_limit ?? 0
    const activeDevices = sub.active_devices ?? 0
    const isProtected = state === 'protected'
    const muted = state === 'expired'

    const statusTitle =
        state === 'protected'
            ? t('exa.home.stateProtected')
            : state === 'expired'
              ? t('exa.home.stateExpired')
              : state === 'exhausted'
                ? t('exa.home.stateExhausted')
                : t('exa.home.stateOff')
    const statusTone = state === 'protected' ? 'protected' : state === 'expired' ? 'danger' : state === 'exhausted' ? 'warning' : 'off'
    const statusSub =
        state === 'expired'
            ? `${sub.plan_name} · ${formatDate(sub.expires_at, locale)}`
            : state === 'pending'
              ? t('exa.home.pendingActivation')
              : `${sub.plan_name} · ${t('exa.common.days', { count: days })}`

    const serverLine = currentServer
        ? countryName(currentServer.country_code, locale)
        : sub.last_node_flag
          ? countryName(sub.last_node_flag, locale)
          : t('exa.home.serverAuto')
    const serverSpeed = currentServer ? serverSpeedMbps(currentServer) : null
    const serverAvail = currentServer ? availability(currentServer) : 'ok'

    return (
        <div className="exa-screen">
            <StatusHeader lit={isProtected} title={statusTitle} sub={statusSub} tone={statusTone} onBell={() => navigate('/notifications')} />

            {/* Трафик исчерпан — карточка лимита выше тарифа, как в макете */}
            {state === 'exhausted' ? (
                <Card>
                    <div className="exa-card__head">
                        <span className="exa-card__hint">{t('exa.home.trafficMonth')}</span>
                        <Pill tone="warning">{t('exa.home.limitReached')}</Pill>
                    </div>
                    <div className="exa-big">
                        <span className="exa-big__num is-md is-warning">{formatGb(usedBytes, locale)}</span>
                        <span className="exa-big__unit is-sm">{t('exa.home.ofGb', { gb: formatGb(limitBytes, locale) })}</span>
                    </div>
                    <div className="exa-bar is-warning">
                        <span style={{ width: '100%' }} />
                    </div>
                    <p className="exa-card__note">{t('exa.home.exhaustedNote', { date: formatDate(sub.expires_at, locale) })}</p>
                    <Button onClick={() => navigate('/pay?upgrade=1')}>{t('exa.home.upgradeGold')}</Button>
                </Card>
            ) : null}

            {/* Тариф */}
            <Card className="exa-plan-card">
                <div className="exa-card__head">
                    <span className="exa-display">{sub.plan_name}</span>
                    <span className={`exa-card__hint${state === 'expired' ? ' is-danger' : ''}`}>
                        {state === 'expired'
                            ? t('exa.home.expiredOn', { date: formatDate(sub.expires_at, locale) })
                            : t('exa.home.until', { date: formatDate(sub.expires_at, locale) })}
                    </span>
                </div>
                <div className="exa-big">
                    <span className={`exa-big__num${state === 'exhausted' ? ' is-lg' : ''}${state === 'expired' ? ' is-dim' : ''}`}>{days}</span>
                    <span className="exa-big__unit">{t('exa.common.daysWord', { count: days })}</span>
                </div>
                {state === 'expired' ? <p className="exa-card__note">{t('exa.home.expiredNote')}</p> : null}
                {state === 'pending' ? (
                    <Button onClick={() => void activate()} disabled={activating}>
                        {activating ? t('exa.home.activating') : t('exa.home.activate')}
                    </Button>
                ) : state === 'exhausted' ? (
                    <Button variant="secondary" size="md" onClick={() => navigate('/pay')}>
                        {t('exa.home.renew')}
                    </Button>
                ) : (
                    <Button onClick={() => navigate('/pay')}>{state === 'expired' ? t('exa.home.pay') : t('exa.home.renew')}</Button>
                )}
            </Card>

            {/* Подключение — главное действие, сразу под тарифом */}
            {isProtected || state === 'exhausted' ? (
                <div className="exa-connect">
                    <Button icon={<ExaIcon name="connect" size={22} />} onClick={() => setSheet('client')}>
                        {t('exa.home.connect')}
                    </Button>
                    <IconButton label={t('exa.home.copyLink')} className="is-lg" onClick={() => void copyLink()}>
                        <ExaIcon name="copy" size={22} />
                    </IconButton>
                    <IconButton label="QR" className="is-lg" onClick={() => setSheet('qr')}>
                        <ExaIcon name="qr" size={22} />
                    </IconButton>
                </div>
            ) : null}

            {/* Сервер */}
            <Card className="exa-card--row" muted={muted || state === 'exhausted'}>
                <CountryChip code={currentServer?.country_code ?? sub.last_node_flag ?? '··'} />
                <div className="exa-row__body">
                    <div className="exa-row__title">
                        <span>{serverLine}</span>
                    </div>
                    <div className="exa-row__meta">
                        {state === 'expired'
                            ? t('exa.home.serverOff')
                            : state === 'exhausted'
                              ? t('exa.home.serverPaused')
                              : serverAvail === 'offline'
                                ? t('exa.servers.offline')
                                : serverSpeed
                                  ? `${t('exa.servers.speed', { mbps: serverSpeed })} · ${t('exa.home.direct')}`
                                  : t('exa.home.direct')}
                    </div>
                </div>
                {isProtected ? (
                    <Button variant="secondary" size="sm" block={false} onClick={() => setSheet('server')}>
                        {t('exa.home.change')}
                    </Button>
                ) : null}
            </Card>

            {/* Трафик */}
            {state !== 'exhausted' ? (
                <Card muted={muted}>
                    <div className="exa-card__head">
                        <span className="exa-card__hint">{t('exa.home.trafficMonth')}</span>
                        {!muted ? (
                            unlimited ? <Pill>{t('exa.common.unlimited')}</Pill> : <Pill>{t('exa.home.ofGb', { gb: formatGb(limitBytes, locale) })}</Pill>
                        ) : null}
                    </div>
                    {muted ? (
                        <div className="exa-big__num is-md is-dim">—</div>
                    ) : (
                        <>
                            <div className="exa-big">
                                <span className="exa-big__num is-md">{formatGb(usedBytes, locale)}</span>
                                <span className="exa-big__unit is-sm">{t('exa.common.gb')}</span>
                            </div>
                            {!unlimited ? (
                                <div className={`exa-bar${usedBytes / limitBytes > 0.85 ? ' is-warning' : ''}`}>
                                    <span style={{ width: `${Math.min(100, Math.round((usedBytes / limitBytes) * 100))}%` }} />
                                </div>
                            ) : null}
                            {userStats ? (
                                <div className="exa-traffic-split">
                                    <span>
                                        <ExaIcon name="down" size={14} weight={2.5} />
                                        {formatGb(userStats.total_download || 0, locale)} {t('exa.common.gb')}
                                    </span>
                                    <span>
                                        <ExaIcon name="up" size={14} weight={2.5} />
                                        {formatGb(userStats.total_upload || 0, locale)} {t('exa.common.gb')}
                                    </span>
                                </div>
                            ) : null}
                        </>
                    )}
                </Card>
            ) : null}

            {/* Устройства */}
            <Card as="button" className="exa-card--row" muted={muted} onClick={() => navigate('/devices')}>
                <div className="exa-row__body">
                    <div className="exa-card__hint">{t('exa.home.devices')}</div>
                    <div className="exa-row__title">
                        <span>{deviceLimit > 0 ? t('exa.home.devicesOf', { n: activeDevices, of: deviceLimit }) : String(activeDevices)}</span>
                    </div>
                </div>
                {!muted && deviceLimit > 0 ? (
                    <div className="exa-device-slots">
                        {Array.from({ length: Math.min(deviceLimit, 4) }).map((_, i) => (
                            <span key={i} className={`exa-device-slot${i < activeDevices ? '' : ' is-empty'}`}>
                                {i < activeDevices ? <ExaIcon name={i === 0 ? 'phone' : 'laptop'} size={18} /> : null}
                            </span>
                        ))}
                    </div>
                ) : null}
                {!muted ? <ExaIcon name="chevron" size={20} className="exa-linkrow__chevron" /> : null}
            </Card>

            <ServerPickerSheet
                open={sheet === 'server'}
                token={token}
                subId={sub.id}
                currentNodeId={sub.last_node_id ?? null}
                onClose={() => setSheet(null)}
                onChanged={() => void refreshData()}
            />
            <ClientPickerSheet open={sheet === 'client'} sub={sub} onClose={() => setSheet(null)} onGuide={() => navigate('/guide')} />
            <QrSheet
                open={sheet === 'qr'}
                value={subscriptionUrl(sub)}
                title={t('exa.home.qrTitle')}
                subtitle={t('exa.home.qrHint')}
                onClose={() => setSheet(null)}
            />
        </div>
    )
}

function StatusHeader({
    lit,
    title,
    sub,
    tone,
    onBell,
}: {
    lit: boolean
    title: string
    sub: string
    tone: 'protected' | 'danger' | 'warning' | 'off'
    onBell?: () => void
}) {
    const { t } = useTranslation()
    return (
        <header className="exa-status">
            <ExaMark size={36} lit={lit} />
            <div className="exa-status__text">
                <div className={`exa-status__title is-${tone}`}>{title}</div>
                <div className="exa-status__sub">{sub}</div>
            </div>
            {onBell ? (
                <IconButton label={t('exa.home.notifications')} className="is-ghost" onClick={onBell}>
                    <ExaIcon name="bell" />
                </IconButton>
            ) : null}
        </header>
    )
}

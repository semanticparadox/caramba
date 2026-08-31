import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import BotPaymentPanel from './BotPaymentPanel'
import DrawerModal from './DrawerModal'
import ProviderPicker from './ProviderPicker'
import { apiUrl } from '../config'
import { useAuth } from '../context/AuthContext'
import { copyText } from '../lib/copyActions'
import { hapticError, hapticSuccess, hapticTap } from '../lib/haptics'
import { mapProviderCards } from '../lib/paymentProviders'
import { formatPrice, useDurationFormatter, type PlanDuration } from '../lib/planFormat'
import { usePlansCatalog } from '../lib/usePlansCatalog'
import { usePurchase } from '../lib/usePurchase'
import './PurchaseFlow.css'

type PurchaseBanner = { type: 'success' | 'error'; text: string } | null

/**
 * ЕДИНЫЙ поток покупки подписки: тариф → срок → drawer способов оплаты
 * (+ переключатель «в подарок») → BotPaymentPanel-состояние.
 * Раньше этот JSX жил параллельно в Home.tsx, Plans.tsx и Store.tsx —
 * теперь одна реализация, подключаемая на любой странице.
 */
export default function PurchaseFlow() {
    const { t } = useTranslation()
    const { user, userStats: stats, refreshData, token } = useAuth()
    const formatDuration = useDurationFormatter()

    const { plans, providers, loading: catalogLoading, retry } = usePlansCatalog({ token })

    const [showPayModal, setShowPayModal] = useState(false)
    const [selectedDuration, setSelectedDuration] = useState<PlanDuration | null>(null)
    const [banner, setBanner] = useState<PurchaseBanner>(null)
    const [buyAsGift, setBuyAsGift] = useState(false)
    const [giftCode, setGiftCode] = useState<string | null>(null)
    const [copiedGiftCode, setCopiedGiftCode] = useState(false)
    // Локальный busy-флаг для gift-покупки (через /plans/purchase, не через hook).
    const [giftPurchasingDurationId, setGiftPurchasingDurationId] = useState<number | null>(null)

    const { purchasing: invoicePurchasingId, purchasingProvider, purchase, botPayment, clearBotPayment } = usePurchase({
        token,
        onRefresh: refreshData,
    })

    // Кнопки выбора срока блокируются, пока идёт любая покупка (счёт или подарок).
    const purchasingDurationId = invoicePurchasingId ?? giftPurchasingDurationId

    const providerCards = mapProviderCards(providers, t('home.defaultProviderDesc'))
    const balance = user?.balance || stats?.balance || 0

    const handleSelectDuration = (duration: PlanDuration) => {
        hapticTap()
        setSelectedDuration(duration)
        setBuyAsGift(false)
        setGiftCode(null)
        setCopiedGiftCode(false)
        setShowPayModal(true)
    }

    const closePayModal = () => {
        setShowPayModal(false)
        setBuyAsGift(false)
    }

    // Покупка подарочного кода — всегда через balance (баланс списывается напрямую,
    // отдельный эндпоинт /plans/purchase). Не проходит через usePurchase.
    const handleGiftPurchase = async (durationId: number) => {
        if (!token) return
        setGiftPurchasingDurationId(durationId)
        setBanner(null)
        setShowPayModal(false)
        try {
            const res = await fetch(apiUrl('/api/client/plans/purchase'), {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    duration_id: durationId,
                    as_gift: true,
                }),
            })

            if (res.ok) {
                const data = await res.json()
                if (data.type === 'gift' && data.gift_code) {
                    setGiftCode(data.gift_code)
                    setBanner({ type: 'success', text: t('home.giftCodeCreated') })
                    hapticSuccess()
                }
                await refreshData()
            } else {
                const errText = await res.text()
                setBanner({ type: 'error', text: errText || t('home.invoiceError') })
                hapticError()
            }
        } catch {
            setBanner({ type: 'error', text: t('home.networkInvoiceError') })
            hapticError()
        } finally {
            setGiftPurchasingDurationId(null)
            setSelectedDuration(null)
        }
    }

    const handlePurchase = async (providerId: string) => {
        if (!selectedDuration) return
        hapticTap()

        const pickedDuration = selectedDuration

        // Подарок: оплата только балансом — отдельный поток.
        if (buyAsGift && providerId === 'balance') {
            await handleGiftPurchase(pickedDuration.id)
            return
        }

        setBanner(null)
        const result = await purchase({ durationId: pickedDuration.id, provider: providerId })

        switch (result.outcome) {
            case 'success':
                setBanner({ type: 'success', text: t(result.messageKey, result.messageParams) })
                hapticSuccess()
                closePayModal()
                setSelectedDuration(null)
                break
            case 'manual':
                setBanner({
                    type: 'success',
                    text: t('home.paymentCreated', { url: result.invoiceUrl }),
                })
                closePayModal()
                setSelectedDuration(null)
                break
            case 'error':
                setBanner({
                    type: 'error',
                    text: result.message || (result.messageKey ? t(result.messageKey) : t('home.invoiceError')),
                })
                hapticError()
                // Модал остаётся открытым — пользователь может выбрать другой способ.
                break
            case 'redirect':
                // UI передан Stars SDK.
                closePayModal()
                setSelectedDuration(null)
                break
            case 'bot_link':
                // Ссылка на оплату отправлена в чат бота — панель BotPaymentPanel
                // (рендерится по botPayment) показывает кнопки и ждёт completed.
                closePayModal()
                setSelectedDuration(null)
                break
        }
    }

    const handleCopyGiftCode = async () => {
        if (!giftCode) return
        await copyText(giftCode)
        hapticSuccess()
        setCopiedGiftCode(true)
        setTimeout(() => setCopiedGiftCode(false), 1600)
    }

    return (
        <div className="purchase-flow">
            {banner && <div className={`purchase-banner ${banner.type}`}>{banner.text}</div>}

            {/* Ссылка на оплату ушла в чат бота — панель со статусом и кнопками */}
            {botPayment && (
                <BotPaymentPanel
                    payment={botPayment}
                    botUsername={stats?.bot_username}
                    onClose={clearBotPayment}
                />
            )}

            <div className="balance-inline">
                <span>{t('home.balance')}</span>
                <strong>${balance.toFixed(2)}</strong>
            </div>

            {catalogLoading ? (
                <div className="empty-state control-empty">
                    <div className="empty-icon">⚡</div>
                    <h3>{t('home.loadingPlans')}</h3>
                    <p>{t('home.loadingPlansDesc')}</p>
                </div>
            ) : plans.length === 0 ? (
                <div className="empty-state control-empty">
                    <div className="empty-icon">🤖</div>
                    <h3>{t('home.noPlans')}</h3>
                    <p>{t('home.noPlansDesc')}</p>
                    <button className="btn-secondary retry-btn" onClick={retry}>
                        {t('home.retryLoad')}
                    </button>
                </div>
            ) : (
                <div className="plans-grid">
                    {plans.map((plan) => (
                        <article key={plan.id} className="plan-card">
                            <div className="plan-card-head">
                                <h4>{plan.name}</h4>
                                <p>{plan.description || t('home.defaultPlanDesc')}</p>
                            </div>
                            <div className="plan-card-meta">
                                <span>{plan.traffic_limit_gb > 0 ? `${plan.traffic_limit_gb} GB` : t('home.unlimited')}</span>
                                <span>{plan.device_limit > 0 ? t('home.deviceLimit', { count: plan.device_limit }) : t('home.noDeviceLimit')}</span>
                            </div>
                            <div className="duration-grid">
                                {plan.durations.map((dur) => (
                                    <button
                                        key={dur.id}
                                        className="duration-btn"
                                        onClick={() => handleSelectDuration(dur)}
                                        disabled={purchasingDurationId !== null}
                                    >
                                        <span className="dur-label">{formatDuration(dur.duration_days)}</span>
                                        <span className="dur-price">{formatPrice(dur.price_cents)}</span>
                                        {purchasingDurationId === dur.id && <span className="dur-spinner">...</span>}
                                    </button>
                                ))}
                                {plan.durations.length === 0 && (
                                    <div className="plan-empty-note">{t('home.noDurations')}</div>
                                )}
                            </div>
                        </article>
                    ))}
                </div>
            )}

            {/* Показываем подарочный код после покупки */}
            {giftCode && (
                <section className="gift-code-result glass-card">
                    <h3>{t('home.giftCodeTitle')}</h3>
                    <p>{t('home.giftCodeInstructions')}</p>
                    <div className="gift-code-box">
                        <code className="gift-code-text">{giftCode}</code>
                        <button className="btn-primary" onClick={() => void handleCopyGiftCode()}>
                            {copiedGiftCode ? t('common.copied') : t('common.copy')}
                        </button>
                    </div>
                    <button className="btn-ghost" onClick={() => setGiftCode(null)}>
                        {t('common.close')}
                    </button>
                </section>
            )}

            <DrawerModal
                open={showPayModal}
                onClose={closePayModal}
                title={t('home.selectPayment')}
                subtitle={selectedDuration ? `${formatDuration(selectedDuration.duration_days)} • ${formatPrice(selectedDuration.price_cents)}` : undefined}
                closeLabel={t('common.close')}
                footer={<button className="btn-ghost" onClick={closePayModal}>{t('common.cancel')}</button>}
            >
                {/* Переключатель «Купить в подарок» */}
                <label className="gift-toggle-row">
                    <span className="gift-toggle-label">{t('home.buyAsGift')}</span>
                    <input
                        type="checkbox"
                        className="gift-toggle-checkbox"
                        checked={buyAsGift}
                        onChange={(e) => {
                            hapticTap()
                            setBuyAsGift(e.target.checked)
                        }}
                    />
                </label>
                {buyAsGift && <p className="gift-toggle-hint">{t('home.buyAsGiftHint')}</p>}

                {(() => {
                    if (providers.length === 0) {
                        return (
                            <div className="empty-state drawer-empty">
                                <div className="empty-icon">💳</div>
                                <h3>{t('home.noProviders')}</h3>
                                <p>{t('home.noProvidersDesc')}</p>
                            </div>
                        )
                    }
                    // При покупке как подарок доступна только оплата балансом.
                    const visibleCards = buyAsGift
                        ? providerCards.filter((p) => p.id === 'balance')
                        : providerCards
                    if (buyAsGift && visibleCards.length === 0) {
                        return <p className="gift-toggle-hint">{t('home.giftNeedsBalance')}</p>
                    }
                    return (
                        <ProviderPicker
                            cards={visibleCards}
                            onSelect={handlePurchase}
                            busyProviderId={purchasingProvider}
                            descriptionOverride={buyAsGift ? t('home.giftBalanceNote') : undefined}
                        />
                    )
                })()}
            </DrawerModal>
        </div>
    )
}

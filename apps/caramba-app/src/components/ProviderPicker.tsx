import { useTranslation } from 'react-i18next'
import Icon from './Icon'
import {
    groupProviderCards,
    type PaymentProviderCard,
    type PaymentProviderGroup,
} from '../lib/paymentProviders'
import './ProviderPicker.css'

type ProviderPickerProps = {
    cards: PaymentProviderCard[]
    onSelect: (providerId: string) => void
    /** Способ оплаты, по которому сейчас идёт запрос — блокирует все кнопки. */
    busyProviderId?: string | null
    /** Перебивает описание карточки (используется в gift-режиме на Home). */
    descriptionOverride?: string
    /** Показывать ли цену провайдера (Plans использует override-aware цены). */
    showPrice?: boolean
}

const GROUP_LABEL_KEY: Record<PaymentProviderGroup, string> = {
    stars: 'payment.groupStars',
    crypto: 'payment.groupCrypto',
    cards: 'payment.groupCards',
    other: 'payment.groupOther',
}

function formatProviderPrice(amount?: number, currency?: string): string | null {
    if (amount == null || !currency) return null
    const major = amount / 100
    const c = currency.toUpperCase()
    if (c === 'USD') return `$${major.toFixed(2)}`
    if (c === 'RUB') return `${major.toFixed(2)} ₽`
    if (c === 'EUR') return `€${major.toFixed(2)}`
    return `${major.toFixed(2)} ${c}`
}

/**
 * Сгруппированный список способов оплаты (Stars / Crypto / Cards / Other)
 * с бейджем «Рекомендуем» для RU-safe методов. Используется в drawer выбора
 * оплаты на страницах Plans и Home — единый источник правды для UI выбора.
 */
export default function ProviderPicker({
    cards,
    onSelect,
    busyProviderId,
    descriptionOverride,
    showPrice = false,
}: ProviderPickerProps) {
    const { t } = useTranslation()
    const sections = groupProviderCards(cards)

    return (
        <div className="provider-groups">
            {sections.map((section) => (
                <section key={section.group} className="provider-group">
                    <h4 className="provider-group-title">{t(GROUP_LABEL_KEY[section.group])}</h4>
                    <div className="provider-list provider-card-list">
                        {section.cards.map((card) => {
                            const busy = busyProviderId === card.id
                            const price = showPrice
                                ? formatProviderPrice(card.amount, card.currency)
                                : null
                            return (
                                <button
                                    key={card.id}
                                    type="button"
                                    className={`provider-btn provider-card ${card.accent}`}
                                    onClick={() => onSelect(card.id)}
                                    disabled={busyProviderId != null}
                                    aria-busy={busy}
                                >
                                    <span className="provider-card-copy">
                                        <strong>
                                            {card.titleKey ? t(card.titleKey) : card.titleFallback}
                                        </strong>
                                        <small>
                                            {descriptionOverride ??
                                                (card.descriptionKey
                                                    ? t(card.descriptionKey)
                                                    : card.descriptionFallback)}
                                        </small>
                                    </span>
                                    <span className="provider-card-meta">
                                        {price && <span className="provider-price">{price}</span>}
                                        {card.recommended && (
                                            <span className="provider-pill provider-pill-recommended">
                                                {t('payment.recommended')}
                                            </span>
                                        )}
                                        {card.badgeKey && (
                                            <span className="provider-pill">{t(card.badgeKey)}</span>
                                        )}
                                        <span className="provider-arrow">
                                            <Icon name="chevron-right" size={14} />
                                        </span>
                                    </span>
                                </button>
                            )
                        })}
                    </div>
                </section>
            ))}
        </div>
    )
}

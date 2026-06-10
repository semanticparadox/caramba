import {
  PAYMENT_PROVIDER_META,
  PAYMENT_GROUP_ORDER,
  inferProviderGroup,
  type PaymentProviderGroup,
} from '../data/paymentProviders'

export type { PaymentProviderGroup } from '../data/paymentProviders'

export type PaymentProviderApiItem = {
  id: string
  label: string
  /** Effective price in minor units (cents/kopecks) for the selected target, if known. */
  amount?: number
  /** ISO currency (or "USD" fallback) for `amount`, if known. */
  currency?: string
}

export type PaymentProviderCard = PaymentProviderApiItem & {
  title: string
  description: string
  badge?: string
  accent: 'violet' | 'lime' | 'cyan' | 'amber' | 'rose'
  /** Секция для группировки в drawer выбора оплаты. */
  group: PaymentProviderGroup
  /** RU-safe способ (Stars / крипта) — помечается бейджем «Рекомендуем». */
  recommended?: boolean
}

export type PaymentProviderGroupCards = {
  group: PaymentProviderGroup
  cards: PaymentProviderCard[]
}

// fallbackDescription передаётся снаружи чтобы избежать hardcoded строк в lib-слое
export const mapProviderCards = (
  providers: PaymentProviderApiItem[],
  fallbackDescription = '',
): PaymentProviderCard[] =>
  providers.map((provider) => {
    const meta = PAYMENT_PROVIDER_META[provider.id]

    if (!meta) {
      // Неизвестный провайдер (добавлен на бэкенде раньше, чем в метаданные):
      // подбираем группу эвристикой по id, оставляем нейтральный accent.
      return {
        ...provider,
        title: provider.label,
        description: fallbackDescription || provider.label,
        accent: 'violet' as const,
        group: inferProviderGroup(provider.id),
      }
    }

    return {
      ...provider,
      title: meta.title,
      description: meta.description,
      badge: meta.badge,
      accent: meta.accent,
      group: meta.group,
      recommended: meta.recommended,
    }
  })

/**
 * Группирует карточки провайдеров по секциям (Stars / Crypto / Cards / Other),
 * сохраняя относительный порядок внутри секции и общий порядок секций
 * (PAYMENT_GROUP_ORDER). Пустые секции отбрасываются.
 */
export const groupProviderCards = (
  cards: PaymentProviderCard[],
): PaymentProviderGroupCards[] => {
  const buckets = new Map<PaymentProviderGroup, PaymentProviderCard[]>()
  for (const card of cards) {
    const list = buckets.get(card.group)
    if (list) list.push(card)
    else buckets.set(card.group, [card])
  }

  return PAYMENT_GROUP_ORDER
    .map((group) => ({ group, cards: buckets.get(group) ?? [] }))
    .filter((section) => section.cards.length > 0)
}

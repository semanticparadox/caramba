import { PAYMENT_PROVIDER_META } from '../data/paymentProviders'

export type PaymentProviderApiItem = {
  id: string
  label: string
}

export type PaymentProviderCard = PaymentProviderApiItem & {
  title: string
  description: string
  badge?: string
  accent: 'violet' | 'lime' | 'cyan' | 'amber' | 'rose'
}

export const mapProviderCards = (
  providers: PaymentProviderApiItem[],
): PaymentProviderCard[] =>
  providers.map((provider) => {
    const meta = PAYMENT_PROVIDER_META[provider.id]

    if (!meta) {
      return {
        ...provider,
        title: provider.label,
        description: 'Доступный способ оплаты для вашей подписки.',
        accent: 'violet',
      }
    }

    return {
      ...provider,
      title: meta.title,
      description: meta.description,
      badge: meta.badge,
      accent: meta.accent,
    }
  })

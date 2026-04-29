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

// fallbackDescription передаётся снаружи чтобы избежать hardcoded строк в lib-слое
export const mapProviderCards = (
  providers: PaymentProviderApiItem[],
  fallbackDescription = '',
): PaymentProviderCard[] =>
  providers.map((provider) => {
    const meta = PAYMENT_PROVIDER_META[provider.id]

    if (!meta) {
      return {
        ...provider,
        title: provider.label,
        description: fallbackDescription || provider.label,
        accent: 'violet' as const,
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

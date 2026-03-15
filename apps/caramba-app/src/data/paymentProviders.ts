export type PaymentProviderMeta = {
  id: string
  title: string
  description: string
  badge?: string
  accent: 'violet' | 'lime' | 'cyan' | 'amber' | 'rose'
}

export const PAYMENT_PROVIDER_META: Record<string, PaymentProviderMeta> = {
  balance: {
    id: 'balance',
    title: 'Баланс аккаунта',
    description: 'Мгновенная оплата без внешнего редиректа.',
    badge: 'Быстро',
    accent: 'lime',
  },
  manual: {
    id: 'manual',
    title: 'Карта / ручная оплата',
    description: 'Подходит для ручного подтверждения и нестандартных кейсов.',
    badge: 'Manual',
    accent: 'amber',
  },
  stars: {
    id: 'stars',
    title: 'Telegram Stars',
    description: 'Нативный сценарий оплаты внутри Telegram.',
    badge: 'Telegram',
    accent: 'violet',
  },
  cryptobot: {
    id: 'cryptobot',
    title: 'CryptoBot',
    description: 'Быстрый крипто-чекаут через Telegram ecosystem.',
    badge: 'Crypto',
    accent: 'cyan',
  },
  nowpayments: {
    id: 'nowpayments',
    title: 'NowPayments',
    description: 'Широкий выбор криптоактивов и внешняя платежная страница.',
    accent: 'violet',
  },
  cryptomus: {
    id: 'cryptomus',
    title: 'Cryptomus',
    description: 'Крипто-эквайринг с внешним checkout flow.',
    accent: 'cyan',
  },
  lava: {
    id: 'lava',
    title: 'Lava',
    description: 'Альтернативный checkout для внешней оплаты.',
    accent: 'amber',
  },
  aaio: {
    id: 'aaio',
    title: 'AAIO',
    description: 'Мульти-методный checkout с редиректом во внешний платежный flow.',
    accent: 'rose',
  },
}

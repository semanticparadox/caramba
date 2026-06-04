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
  stripe: {
    id: 'stripe',
    title: 'Stripe',
    description: 'Международный приём карт через внешнюю платёжную страницу.',
    badge: 'Cards',
    accent: 'violet',
  },
  wata: {
    id: 'wata',
    title: 'WATA',
    description: 'Приём карт в рублях, расчёты в РФ-эквайринге.',
    badge: 'RUB',
    accent: 'rose',
  },
  crystalpay: {
    id: 'crystalpay',
    title: 'CrystalPay',
    description: 'РФ-агрегатор: карты и СБП в рублях.',
    badge: 'RUB',
    accent: 'cyan',
  },
  tribute: {
    id: 'tribute',
    title: 'Tribute',
    description: 'Оплата картой РФ в рублях, вывод средств в крипте.',
    badge: 'RUB → Crypto',
    accent: 'lime',
  },
  btcpay: {
    id: 'btcpay',
    title: 'BTCPay Server',
    description: 'Самостоятельный приём биткоина и альткоинов без посредников.',
    badge: 'Self-hosted',
    accent: 'amber',
  },
  oxapay: {
    id: 'oxapay',
    title: 'OxaPay',
    description: 'Крипто-эквайринг с поддержкой множества монет.',
    badge: 'Crypto',
    accent: 'cyan',
  },
  coinbase_commerce: {
    id: 'coinbase_commerce',
    title: 'Coinbase Commerce',
    description: 'Приём криптовалют через Coinbase Commerce.',
    badge: 'Crypto',
    accent: 'violet',
  },
  plisio: {
    id: 'plisio',
    title: 'Plisio',
    description: 'Крипто-чекаут с поддержкой популярных монет.',
    badge: 'Crypto',
    accent: 'cyan',
  },
}

// Группа способа оплаты — используется для секционирования в drawer выбора оплаты.
// 'stars' — Telegram Stars, 'crypto' — криптовалюта, 'cards' — карты/фиат,
// 'other' — остальное (баланс, ручная оплата и т.п.).
export type PaymentProviderGroup = 'stars' | 'crypto' | 'cards' | 'other'

export type PaymentProviderMeta = {
  id: string
  title: string
  description: string
  badge?: string
  accent: 'violet' | 'lime' | 'cyan' | 'amber' | 'rose'
  /** Секция, в которую попадает способ оплаты в сгруппированном списке. */
  group: PaymentProviderGroup
  /**
   * RU-safe способы (Stars + крипта) — помечаются бейджем «Рекомендуем».
   * Это методы, которые надёжно работают для пользователей из РФ без риска
   * блокировки карты/эквайринга.
   */
  recommended?: boolean
}

export const PAYMENT_PROVIDER_META: Record<string, PaymentProviderMeta> = {
  balance: {
    id: 'balance',
    title: 'Баланс аккаунта',
    description: 'Мгновенная оплата без внешнего редиректа.',
    badge: 'Быстро',
    accent: 'lime',
    group: 'other',
  },
  manual: {
    id: 'manual',
    title: 'Карта / ручная оплата',
    description: 'Подходит для ручного подтверждения и нестандартных кейсов.',
    badge: 'Manual',
    accent: 'amber',
    group: 'cards',
  },
  stars: {
    id: 'stars',
    title: 'Telegram Stars',
    description: 'Нативный сценарий оплаты внутри Telegram.',
    badge: 'Telegram',
    accent: 'violet',
    group: 'stars',
    recommended: true,
  },
  cryptobot: {
    id: 'cryptobot',
    title: 'CryptoBot',
    description: 'Быстрый крипто-чекаут через Telegram ecosystem.',
    badge: 'Crypto',
    accent: 'cyan',
    group: 'crypto',
    recommended: true,
  },
  nowpayments: {
    id: 'nowpayments',
    title: 'NowPayments',
    description: 'Широкий выбор криптоактивов и внешняя платежная страница.',
    accent: 'violet',
    group: 'crypto',
    recommended: true,
  },
  cryptomus: {
    id: 'cryptomus',
    title: 'Cryptomus',
    description: 'Крипто-эквайринг с внешним checkout flow.',
    accent: 'cyan',
    group: 'crypto',
    recommended: true,
  },
  lava: {
    id: 'lava',
    title: 'Lava',
    description: 'Альтернативный checkout для внешней оплаты.',
    accent: 'amber',
    group: 'cards',
  },
  aaio: {
    id: 'aaio',
    title: 'AAIO',
    description: 'Мульти-методный checkout с редиректом во внешний платежный flow.',
    accent: 'rose',
    group: 'cards',
  },
  stripe: {
    id: 'stripe',
    title: 'Stripe',
    description: 'Международный приём карт через внешнюю платёжную страницу.',
    badge: 'Cards',
    accent: 'violet',
    group: 'cards',
  },
  wata: {
    id: 'wata',
    title: 'WATA',
    description: 'Приём карт в рублях, расчёты в РФ-эквайринге.',
    badge: 'RUB',
    accent: 'rose',
    group: 'cards',
  },
  crystalpay: {
    id: 'crystalpay',
    title: 'CrystalPay',
    description: 'РФ-агрегатор: карты и СБП в рублях.',
    badge: 'RUB',
    accent: 'cyan',
    group: 'cards',
  },
  tribute: {
    id: 'tribute',
    title: 'Tribute',
    description: 'Оплата картой РФ в рублях, вывод средств в крипте.',
    badge: 'RUB → Crypto',
    accent: 'lime',
    group: 'cards',
  },
  btcpay: {
    id: 'btcpay',
    title: 'BTCPay Server',
    description: 'Самостоятельный приём биткоина и альткоинов без посредников.',
    badge: 'Self-hosted',
    accent: 'amber',
    group: 'crypto',
    recommended: true,
  },
  oxapay: {
    id: 'oxapay',
    title: 'OxaPay',
    description: 'Крипто-эквайринг с поддержкой множества монет.',
    badge: 'Crypto',
    accent: 'cyan',
    group: 'crypto',
    recommended: true,
  },
  coinbase_commerce: {
    id: 'coinbase_commerce',
    title: 'Coinbase Commerce',
    description: 'Приём криптовалют через Coinbase Commerce.',
    badge: 'Crypto',
    accent: 'violet',
    group: 'crypto',
    recommended: true,
  },
  plisio: {
    id: 'plisio',
    title: 'Plisio',
    description: 'Крипто-чекаут с поддержкой популярных монет.',
    badge: 'Crypto',
    accent: 'cyan',
    group: 'crypto',
    recommended: true,
  },
}

// Порядок секций в drawer: Stars → Crypto → Cards → Other.
// Используется как fallback порядок, если ключ не задан явно.
export const PAYMENT_GROUP_ORDER: PaymentProviderGroup[] = ['stars', 'crypto', 'cards', 'other']

// Эвристика группировки для провайдеров, которых нет в PAYMENT_PROVIDER_META
// (например, новый провайдер добавлен на бэкенде раньше, чем сюда).
// Подбираем группу по подстрокам в id — безопасный fallback, не падает на unknown.
export function inferProviderGroup(id: string): PaymentProviderGroup {
  const lower = id.toLowerCase()
  if (lower.includes('star')) return 'stars'
  if (
    lower.includes('crypto') ||
    lower.includes('btc') ||
    lower.includes('coin') ||
    lower.includes('nowpayments') ||
    lower.includes('cryptomus') ||
    lower.includes('plisio') ||
    lower.includes('oxapay')
  ) {
    return 'crypto'
  }
  if (
    lower.includes('card') ||
    lower.includes('stripe') ||
    lower.includes('wata') ||
    lower.includes('crystal') ||
    lower.includes('tribute') ||
    lower.includes('lava') ||
    lower.includes('aaio') ||
    lower.includes('manual')
  ) {
    return 'cards'
  }
  return 'other'
}

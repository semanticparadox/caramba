// Группа способа оплаты — используется для секционирования в drawer выбора оплаты.
// 'stars' — Telegram Stars, 'crypto' — криптовалюта, 'cards' — карты/фиат,
// 'other' — остальное (баланс, ручная оплата и т.п.).
export type PaymentProviderGroup = 'stars' | 'crypto' | 'cards' | 'other'

export type PaymentProviderMeta = {
  id: string
  /**
   * i18n-ключи, а не готовый текст: тексты живут в public/locales/*.json,
   * иначе английские бейджи («Crypto», «Cards», «Self-hosted») и русские
   * описания показывались бы всем независимо от выбранного языка.
   */
  titleKey: string
  descriptionKey: string
  badgeKey?: string
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
    titleKey: 'payment.providers.balance.title',
    descriptionKey: 'payment.providers.balance.description',
    badgeKey: 'payment.providers.balance.badge',
    accent: 'lime',
    group: 'other',
  },
  manual: {
    id: 'manual',
    titleKey: 'payment.providers.manual.title',
    descriptionKey: 'payment.providers.manual.description',
    badgeKey: 'payment.providers.manual.badge',
    accent: 'amber',
    group: 'cards',
  },
  stars: {
    id: 'stars',
    titleKey: 'payment.providers.stars.title',
    descriptionKey: 'payment.providers.stars.description',
    badgeKey: 'payment.providers.stars.badge',
    accent: 'violet',
    group: 'stars',
    recommended: true,
  },
  cryptobot: {
    id: 'cryptobot',
    titleKey: 'payment.providers.cryptobot.title',
    descriptionKey: 'payment.providers.cryptobot.description',
    badgeKey: 'payment.providers.cryptobot.badge',
    accent: 'cyan',
    group: 'crypto',
    recommended: true,
  },
  nowpayments: {
    id: 'nowpayments',
    titleKey: 'payment.providers.nowpayments.title',
    descriptionKey: 'payment.providers.nowpayments.description',
    accent: 'violet',
    group: 'crypto',
    recommended: true,
  },
  cryptomus: {
    id: 'cryptomus',
    titleKey: 'payment.providers.cryptomus.title',
    descriptionKey: 'payment.providers.cryptomus.description',
    accent: 'cyan',
    group: 'crypto',
    recommended: true,
  },
  lava: {
    id: 'lava',
    titleKey: 'payment.providers.lava.title',
    descriptionKey: 'payment.providers.lava.description',
    accent: 'amber',
    group: 'cards',
  },
  aaio: {
    id: 'aaio',
    titleKey: 'payment.providers.aaio.title',
    descriptionKey: 'payment.providers.aaio.description',
    accent: 'rose',
    group: 'cards',
  },
  stripe: {
    id: 'stripe',
    titleKey: 'payment.providers.stripe.title',
    descriptionKey: 'payment.providers.stripe.description',
    badgeKey: 'payment.providers.stripe.badge',
    accent: 'violet',
    group: 'cards',
  },
  wata: {
    id: 'wata',
    titleKey: 'payment.providers.wata.title',
    descriptionKey: 'payment.providers.wata.description',
    badgeKey: 'payment.providers.wata.badge',
    accent: 'rose',
    group: 'cards',
  },
  crystalpay: {
    id: 'crystalpay',
    titleKey: 'payment.providers.crystalpay.title',
    descriptionKey: 'payment.providers.crystalpay.description',
    badgeKey: 'payment.providers.crystalpay.badge',
    accent: 'cyan',
    group: 'cards',
  },
  tribute: {
    id: 'tribute',
    titleKey: 'payment.providers.tribute.title',
    descriptionKey: 'payment.providers.tribute.description',
    badgeKey: 'payment.providers.tribute.badge',
    accent: 'lime',
    group: 'cards',
  },
  btcpay: {
    id: 'btcpay',
    titleKey: 'payment.providers.btcpay.title',
    descriptionKey: 'payment.providers.btcpay.description',
    badgeKey: 'payment.providers.btcpay.badge',
    accent: 'amber',
    group: 'crypto',
    recommended: true,
  },
  oxapay: {
    id: 'oxapay',
    titleKey: 'payment.providers.oxapay.title',
    descriptionKey: 'payment.providers.oxapay.description',
    badgeKey: 'payment.providers.oxapay.badge',
    accent: 'cyan',
    group: 'crypto',
    recommended: true,
  },
  coinbase_commerce: {
    id: 'coinbase_commerce',
    titleKey: 'payment.providers.coinbase_commerce.title',
    descriptionKey: 'payment.providers.coinbase_commerce.description',
    badgeKey: 'payment.providers.coinbase_commerce.badge',
    accent: 'violet',
    group: 'crypto',
    recommended: true,
  },
  plisio: {
    id: 'plisio',
    titleKey: 'payment.providers.plisio.title',
    descriptionKey: 'payment.providers.plisio.description',
    badgeKey: 'payment.providers.plisio.badge',
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

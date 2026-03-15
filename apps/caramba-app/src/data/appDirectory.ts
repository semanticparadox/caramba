export type PlatformKey = 'ios' | 'android' | 'windows' | 'macos' | 'linux' | 'tv'

export type AppDirectoryEntry = {
  id: string
  name: string
  badge?: string
  description: string
  officialUrl: string
  fallbackUrl?: string
  confidence: 'high' | 'medium-high' | 'medium'
}

export type PlatformDirectory = {
  id: PlatformKey
  label: string
  quickSetup: string
  entries: AppDirectoryEntry[]
}

export const PLATFORM_DIRECTORY: PlatformDirectory[] = [
  {
    id: 'ios',
    label: 'iOS',
    quickSetup: 'Скопируйте ссылку -> откройте приложение -> Import from Clipboard/URL -> Connect',
    entries: [
      {
        id: 'hiddify-ios',
        name: 'Hiddify',
        badge: 'Рекомендуем',
        description: 'Бесплатный универсальный клиент, простой старт.',
        officialUrl: 'https://hiddify.com',
        fallbackUrl: 'https://github.com/hiddify/hiddify-app/releases',
        confidence: 'high',
      },
      {
        id: 'sing-box-vt-ios',
        name: 'sing-box VT',
        badge: 'Official',
        description: 'Минималистичный официальный клиент для Apple платформ.',
        officialUrl: 'https://apps.apple.com/us/app/sing-box-vt/id6673731168',
        confidence: 'medium-high',
      },
      {
        id: 'shadowrocket-ios',
        name: 'Shadowrocket',
        badge: 'Paid',
        description: 'Платный pro-choice клиент для тонкой настройки.',
        officialUrl: 'https://apps.apple.com/us/app/shadowrocket/id932747118',
        confidence: 'high',
      },
      {
        id: 'happ-ios',
        name: 'Happ',
        description: 'Современный быстрый UI с хорошим UX импорта.',
        officialUrl: 'https://apps.apple.com/us/app/happ-proxy-utility/id6504287215',
        fallbackUrl: 'https://www.happ.su/main',
        confidence: 'high',
      },
      {
        id: 'stash-ios',
        name: 'Stash',
        description: 'Надежный rule-based вариант для продвинутых маршрутов.',
        officialUrl: 'https://apps.apple.com/us/app/stash-rule-based-proxy/id1596063349',
        fallbackUrl: 'https://stash.ws',
        confidence: 'high',
      },
      {
        id: 'streisand-ios',
        name: 'Streisand',
        description: 'Надежная альтернатива с удобным mobile-first UX.',
        officialUrl: 'https://apps.apple.com/us/app/streisand/id6450534064',
        fallbackUrl: 'https://streisand.pages.dev',
        confidence: 'high',
      },
    ],
  },
  {
    id: 'android',
    label: 'Android',
    quickSetup: 'Скопируйте ссылку -> откройте приложение -> Import from Clipboard/URL -> Connect',
    entries: [
      {
        id: 'hiddify-android',
        name: 'Hiddify',
        badge: 'Лучший старт',
        description: 'Лучший all-in-one вариант для большинства пользователей.',
        officialUrl: 'https://hiddify.com',
        fallbackUrl: 'https://github.com/hiddify/hiddify-app/releases',
        confidence: 'high',
      },
      {
        id: 'sing-box-android',
        name: 'sing-box',
        badge: 'Official',
        description: 'Официальный Android-клиент экосистемы sing-box.',
        officialUrl: 'https://play.google.com/store/apps/details?id=io.nekohasekai.sfa',
        fallbackUrl: 'https://github.com/SagerNet/sing-box/releases',
        confidence: 'high',
      },
      {
        id: 'nekobox-android',
        name: 'NekoBox',
        description: 'Мощный клиент с широким набором функций.',
        officialUrl: 'https://github.com/MatsuriDayo/NekoBoxForAndroid/releases',
        confidence: 'high',
      },
      {
        id: 'v2rayng-android',
        name: 'v2rayNG',
        description: 'Стабильная классика с большим комьюнити.',
        officialUrl: 'https://github.com/2dust/v2rayNG/releases',
        confidence: 'high',
      },
    ],
  },
  {
    id: 'macos',
    label: 'macOS',
    quickSetup: 'Скопируйте ссылку -> откройте приложение -> Import from Clipboard/URL -> Connect',
    entries: [
      {
        id: 'koala-clash-macos',
        name: 'Koala Clash',
        badge: 'Рекомендуем',
        description: 'Любимец пользователей с современным GUI.',
        officialUrl: 'https://github.com/coolcoala/koala-clash/releases',
        confidence: 'high',
      },
      {
        id: 'hiddify-macos',
        name: 'Hiddify',
        description: 'Кроссплатформенная консистентность между устройствами.',
        officialUrl: 'https://hiddify.com',
        fallbackUrl: 'https://github.com/hiddify/hiddify-app/releases',
        confidence: 'high',
      },
      {
        id: 'sing-box-vt-macos',
        name: 'sing-box VT',
        description: 'Нативный Apple Silicon клиент от экосистемы sing-box.',
        officialUrl: 'https://apps.apple.com/us/app/sing-box-vt/id6673731168',
        confidence: 'medium-high',
      },
      {
        id: 'stash-macos',
        name: 'Stash',
        description: 'Продвинутый routing и rule-based конфигурации.',
        officialUrl: 'https://stash.ws',
        fallbackUrl: 'https://apps.apple.com/us/app/stash-rule-based-proxy/id1596063349',
        confidence: 'high',
      },
    ],
  },
  {
    id: 'windows',
    label: 'Windows',
    quickSetup: 'Скопируйте ссылку -> откройте приложение -> Import from Clipboard/URL -> Connect',
    entries: [
      {
        id: 'hiddify-windows',
        name: 'Hiddify',
        badge: 'Рекомендуем',
        description: 'Простой и мощный стартовый клиент для Windows.',
        officialUrl: 'https://hiddify.com',
        fallbackUrl: 'https://github.com/hiddify/hiddify-app/releases',
        confidence: 'high',
      },
      {
        id: 'v2rayn-windows',
        name: 'v2rayN',
        description: 'Функционально насыщенный клиент с гибкой настройкой.',
        officialUrl: 'https://github.com/2dust/v2rayN/releases',
        confidence: 'high',
      },
      {
        id: 'nekoray-windows',
        name: 'NekoRay',
        description: 'Чистый desktop UI и приятный UX импорта.',
        officialUrl: 'https://github.com/MatsuriDayo/nekoray/releases',
        confidence: 'medium',
      },
    ],
  },
  {
    id: 'linux',
    label: 'Linux',
    quickSetup: 'Скопируйте ссылку -> откройте приложение -> Import from Clipboard/URL -> Connect',
    entries: [
      {
        id: 'hiddify-linux',
        name: 'Hiddify',
        badge: 'Рекомендуем',
        description: 'AppImage/Flatpak-first опыт с быстрым импортом.',
        officialUrl: 'https://hiddify.com',
        fallbackUrl: 'https://github.com/hiddify/hiddify-app/releases',
        confidence: 'high',
      },
      {
        id: 'nekoray-linux',
        name: 'NekoRay',
        description: 'GUI-базированный вариант для desktop Linux.',
        officialUrl: 'https://github.com/MatsuriDayo/nekoray/releases',
        confidence: 'medium',
      },
      {
        id: 'v2raya-linux',
        name: 'v2rayA',
        description: 'Популярный Linux GUI/control plane для маршрутизации.',
        officialUrl: 'https://v2raya.org/en/',
        fallbackUrl: 'https://github.com/v2rayA/v2rayA/releases',
        confidence: 'high',
      },
    ],
  },
  {
    id: 'tv',
    label: 'TV',
    quickSetup: 'Скопируйте ссылку -> откройте приложение -> Import from Clipboard/URL -> Connect',
    entries: [
      {
        id: 'sing-box-vt-tv',
        name: 'sing-box VT',
        badge: 'Apple TV',
        description: 'Нативный клиент для Apple TV 17+.',
        officialUrl: 'https://apps.apple.com/us/app/sing-box-vt/id6673731168',
        confidence: 'medium-high',
      },
      {
        id: 'hiddify-tv',
        name: 'Hiddify',
        badge: 'Android TV',
        description: 'Оптимизированный путь для Android TV устройств.',
        officialUrl: 'https://hiddify.com',
        fallbackUrl: 'https://github.com/hiddify/hiddify-app/releases',
        confidence: 'high',
      },
      {
        id: 'v2rayng-tv',
        name: 'v2rayNG',
        description: 'Стандартный вариант для Android TV boxes.',
        officialUrl: 'https://github.com/2dust/v2rayNG/releases',
        confidence: 'high',
      },
    ],
  },
]

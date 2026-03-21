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

const HIDDIFY_SITE = 'https://hiddify.com'
const HIDDIFY_RELEASES = 'https://github.com/hiddify/hiddify-app/releases'

export const PLATFORM_DIRECTORY: PlatformDirectory[] = [
  {
    id: 'ios',
    label: 'iOS',
    quickSetup: 'Hiddify: Add Profile -> Import from Clipboard -> Connect. Streisand/Shadowrocket: скопируйте ссылку подписки и вставьте в настройки.',
    entries: [
      {
        id: 'hiddify-ios',
        name: 'Hiddify',
        badge: 'Рекомендуем',
        description: 'Универсальный клиент, автоматический импорт. Бесплатно.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
      {
        id: 'happ-ios',
        name: 'Happ',
        description: 'Минималистичный клиент на базе sing-box. Бесплатно.',
        officialUrl: 'https://apps.apple.com/app/happ-proxy-utility/id6504287215',
        confidence: 'high',
      },
      {
        id: 'streisand-ios',
        name: 'Streisand',
        description: 'Альтернативный клиент с поддержкой VLESS/Trojan. Бесплатно.',
        officialUrl: 'https://apps.apple.com/app/streisand/id6450534064',
        confidence: 'medium-high',
      },
      {
        id: 'shadowrocket-ios',
        name: 'Shadowrocket',
        badge: '$2.99',
        description: 'Популярный платный клиент. Поддерживает все протоколы.',
        officialUrl: 'https://apps.apple.com/app/shadowrocket/id932747118',
        confidence: 'high',
      },
      {
        id: 'singbox-ios',
        name: 'Sing-box',
        description: 'Ядро sing-box в нативной обёртке. Для продвинутых.',
        officialUrl: 'https://apps.apple.com/app/sing-box/id6451272673',
        fallbackUrl: 'https://github.com/SagerNet/sing-box/releases',
        confidence: 'medium-high',
      },
    ],
  },
  {
    id: 'android',
    label: 'Android',
    quickSetup: 'Hiddify: New Profile -> Import from Clipboard/URL -> Connect. V2rayNG: скопируйте ссылку, приложение подхватит автоматически.',
    entries: [
      {
        id: 'hiddify-android',
        name: 'Hiddify',
        badge: 'Рекомендуем',
        description: 'Универсальный клиент с автоимпортом. Бесплатно.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
      {
        id: 'happ-android',
        name: 'Happ',
        description: 'Минималистичный клиент на базе sing-box. Бесплатно.',
        officialUrl: 'https://play.google.com/store/apps/details?id=com.happ.proxy',
        confidence: 'high',
      },
      {
        id: 'v2rayng-android',
        name: 'V2rayNG',
        description: 'Легковесный клиент для VLESS/VMess/Trojan.',
        officialUrl: 'https://play.google.com/store/apps/details?id=com.v2ray.ang',
        fallbackUrl: 'https://github.com/2dust/v2rayNG/releases',
        confidence: 'high',
      },
      {
        id: 'nekobox-android',
        name: 'NekoBox',
        description: 'Продвинутый клиент на базе sing-box. Гибкая настройка.',
        officialUrl: 'https://github.com/MatsuriDayo/NekoBoxForAndroid/releases',
        confidence: 'medium-high',
      },
      {
        id: 'singbox-android',
        name: 'Sing-box',
        description: 'Официальный клиент sing-box.',
        officialUrl: 'https://play.google.com/store/apps/details?id=io.nekohasekai.sfa',
        fallbackUrl: 'https://github.com/SagerNet/sing-box/releases',
        confidence: 'medium-high',
      },
    ],
  },
  {
    id: 'windows',
    label: 'Windows',
    quickSetup: 'Hiddify: Add Profile -> Import from URL/Clipboard -> Connect. NekoRay: Preferences -> добавьте ссылку подписки.',
    entries: [
      {
        id: 'hiddify-windows',
        name: 'Hiddify',
        badge: 'Рекомендуем',
        description: 'Универсальный GUI-клиент для Windows.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
      {
        id: 'nekoray-windows',
        name: 'NekoRay',
        description: 'GUI на базе sing-box/Xray. Продвинутая маршрутизация.',
        officialUrl: 'https://github.com/MatsuriDayo/nekoray/releases',
        confidence: 'medium-high',
      },
      {
        id: 'v2rayn-windows',
        name: 'V2rayN',
        description: 'Популярный Windows-клиент для V2Ray/Xray.',
        officialUrl: 'https://github.com/2dust/v2rayN/releases',
        confidence: 'high',
      },
    ],
  },
  {
    id: 'macos',
    label: 'macOS',
    quickSetup: 'Hiddify: Add Profile -> Import from URL/Clipboard -> Connect.',
    entries: [
      {
        id: 'hiddify-macos',
        name: 'Hiddify',
        badge: 'Рекомендуем',
        description: 'Универсальный GUI-клиент для macOS.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
      {
        id: 'singbox-macos',
        name: 'Sing-box (GUI)',
        description: 'Нативное приложение sing-box для macOS.',
        officialUrl: 'https://apps.apple.com/app/sing-box/id6451272673',
        fallbackUrl: 'https://github.com/SagerNet/sing-box/releases',
        confidence: 'medium-high',
      },
      {
        id: 'nekoray-macos',
        name: 'NekoRay',
        description: 'GUI на базе sing-box. Гибкая настройка маршрутов.',
        officialUrl: 'https://github.com/MatsuriDayo/nekoray/releases',
        confidence: 'medium',
      },
    ],
  },
  {
    id: 'linux',
    label: 'Linux',
    quickSetup: 'Hiddify: Add Profile -> Import from URL/Clipboard -> Connect. CLI: sing-box run -c config.json.',
    entries: [
      {
        id: 'hiddify-linux',
        name: 'Hiddify',
        badge: 'Рекомендуем',
        description: 'GUI-клиент для Linux (AppImage/deb).',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
      {
        id: 'nekoray-linux',
        name: 'NekoRay',
        description: 'GUI-клиент на базе sing-box для Linux.',
        officialUrl: 'https://github.com/MatsuriDayo/nekoray/releases',
        confidence: 'medium-high',
      },
      {
        id: 'singbox-cli-linux',
        name: 'Sing-box CLI',
        description: 'Консольная утилита. Для серверов и продвинутых пользователей.',
        officialUrl: 'https://github.com/SagerNet/sing-box/releases',
        confidence: 'medium',
      },
    ],
  },
  {
    id: 'tv',
    label: 'TV',
    quickSetup: 'Откройте Hiddify на TV -> Add Profile -> Import from URL/Clipboard -> Connect.',
    entries: [
      {
        id: 'hiddify-tv',
        name: 'Hiddify',
        badge: 'Рекомендуем',
        description: 'Клиент для Android TV и TV Box.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
    ],
  },
]

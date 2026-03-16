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
    quickSetup: 'Откройте Hiddify -> Add/Import Profile -> Paste from Clipboard -> Connect',
    entries: [
      {
        id: 'hiddify-ios',
        name: 'Hiddify',
        badge: 'Официально',
        description: 'Рекомендуемый клиент для iPhone и iPad.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
    ],
  },
  {
    id: 'android',
    label: 'Android',
    quickSetup: 'Откройте Hiddify -> New Profile -> Import from Clipboard/URL -> Connect',
    entries: [
      {
        id: 'hiddify-android',
        name: 'Hiddify',
        badge: 'Официально',
        description: 'Рекомендуемый клиент для Android-смартфонов и планшетов.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
    ],
  },
  {
    id: 'windows',
    label: 'Windows',
    quickSetup: 'Откройте Hiddify -> Add Profile -> Import from URL/Clipboard -> Connect',
    entries: [
      {
        id: 'hiddify-windows',
        name: 'Hiddify',
        badge: 'Официально',
        description: 'Рекомендуемый клиент для Windows.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
    ],
  },
  {
    id: 'macos',
    label: 'macOS',
    quickSetup: 'Откройте Hiddify -> Add Profile -> Import from URL/Clipboard -> Connect',
    entries: [
      {
        id: 'hiddify-macos',
        name: 'Hiddify',
        badge: 'Официально',
        description: 'Рекомендуемый клиент для macOS.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
    ],
  },
  {
    id: 'linux',
    label: 'Linux',
    quickSetup: 'Откройте Hiddify -> Add Profile -> Import from URL/Clipboard -> Connect',
    entries: [
      {
        id: 'hiddify-linux',
        name: 'Hiddify',
        badge: 'Официально',
        description: 'Рекомендуемый клиент для Linux.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
    ],
  },
  {
    id: 'tv',
    label: 'TV',
    quickSetup: 'Откройте Hiddify на TV -> Add Profile -> Import from URL/Clipboard -> Connect',
    entries: [
      {
        id: 'hiddify-tv',
        name: 'Hiddify',
        badge: 'Официально',
        description: 'Рекомендуемый клиент для Android TV/TV box.',
        officialUrl: HIDDIFY_SITE,
        fallbackUrl: HIDDIFY_RELEASES,
        confidence: 'high',
      },
    ],
  },
]

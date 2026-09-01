/** Иконки EXA: 24 px, штрих 2 px, характер «детали машины». Набор из
 *  identity-листа плюс служебные (шеврон, карандаш, стрелки, галочка).
 *  Эмодзи в интерфейсе не используются нигде. */
import type { SVGProps } from 'react'

const PATHS = {
    connect: 'M12 3v7M7.5 6.5a7 7 0 1 0 9 0M10 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0',
    server: 'M5 4h14a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM5 13h14a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2zM7 7.5h.01M7 16.5h.01',
    profile: 'M11 3h2a3 3 0 0 1 3 3v2a3 3 0 0 1-3 3h-2a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3zM4 21v-3a5 5 0 0 1 5-5h6a5 5 0 0 1 5 5v3',
    copy: 'M9 9h9a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2zM5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1',
    qr: 'M3 3h6v6H3zM15 3h6v6h-6zM3 15h6v6H3zM15 15h2v2h-2zM19 15h2v2h-2zM15 19h2v2h-2zM19 19h2v2h-2zM6 6h.01M18 6h.01M6 18h.01',
    router: 'M4 13h16a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2zM7 17h.01M11 17h.01M17 13V7M14 6.5a4.2 4.2 0 0 1 6 0M12 3.5a7 7 0 0 1 10 0',
    phone: 'M8 2h8a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2zM12 18h.01',
    laptop: 'M5 4h14a2 2 0 0 1 2 2v9H3V6a2 2 0 0 1 2-2zM2 19h20',
    tv: 'M4 4h16a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM8 21h8M12 17v4',
    traffic: 'M7 20V5M4 8l3-3 3 3M17 4v15M14 16l3 3 3-3',
    devices: 'M3 6h11a2 2 0 0 1 2 2v1M3 6v10a2 2 0 0 0 2 2h6M16 11h4a1.5 1.5 0 0 1 1.5 1.5v7A1.5 1.5 0 0 1 20 21h-4a1.5 1.5 0 0 1-1.5-1.5v-7A1.5 1.5 0 0 1 16 11zM18 18h.01',
    shield: 'M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6l7-3zM12 9v5',
    pay: 'M4 5h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2zM2 10h20M6 15h4',
    stars: 'M12 3l2.2 5.8L20 11l-5.8 2.2L12 19l-2.2-5.8L4 11l5.8-2.2L12 3z',
    crypto: 'M12 2.5l8 4.5v10l-8 4.5-8-4.5V7l8-4.5zM9.5 8h4a2 2 0 0 1 0 4h-4zM9.5 12h4.5a2 2 0 0 1 0 4H9.5zM9.5 8v8',
    promo: 'M3 12V5a2 2 0 0 1 2-2h7l9 9-9 9-9-9zM7.5 7.5h.01',
    gift: 'M4 10h16v9a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-9zM3 7h18v3H3zM12 7v14M12 7c-3 0-4.5-1.5-4.5-3S10 2 12 7zM12 7c3 0 4.5-1.5 4.5-3S14 2 12 7z',
    bell: 'M6 16v-5a6 6 0 0 1 12 0v5l2 2H4l2-2zM10 21h4',
    language: 'M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM3 12h18M12 3c-3 3-3 15 0 18M12 3c3 3 3 15 0 18',
    support: 'M4 13v-1a8 8 0 0 1 16 0v1M4 13h3v5H4a1 1 0 0 1-1-1v-3a1 1 0 0 1 1-1zM17 13h3a1 1 0 0 1 1 1v3a1 1 0 0 1-1 1h-3v-5zM15 21h-3',
    switch: 'M4 7h13M14 4l3 3-3 3M20 17H7M10 14l-3 3 3 3',
    ping: 'M3 12h4l3-7 4 14 3-7h4',
    chevron: 'M9 6l6 6-6 6',
    back: 'M15 6l-6 6 6 6',
    pencil: 'M4 20h4l11-11a2 2 0 0 0 0-3l-1-1a2 2 0 0 0-3 0L4 16v4z',
    down: 'M12 4v15M8 15l4 4 4-4',
    up: 'M12 20V5M8 9l4-4 4 4',
    check: 'M5 12l5 5L20 7',
    close: 'M6 6l12 12M18 6L6 18',
    external: 'M14 4h6v6M20 4l-9 9M19 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h5',
} as const

export type ExaIconName = keyof typeof PATHS

type ExaIconProps = SVGProps<SVGSVGElement> & {
    name: ExaIconName
    size?: number
    /** Толщина штриха; 2.5 для мелких стрелок трафика. */
    weight?: number
}

export function ExaIcon({ name, size = 24, weight = 2, ...rest }: ExaIconProps) {
    return (
        <svg
            width={size}
            height={size}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={weight}
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
            {...rest}
        >
            <path d={PATHS[name]} />
        </svg>
    )
}

/** Иконка устройства по его имени/клиенту — то же, что в шторке устройств. */
export function deviceIconName(label: string): ExaIconName {
    const s = label.toLowerCase()
    if (/(mac|windows|linux|pc|desktop|laptop|book)/.test(s)) return 'laptop'
    if (/(tv|android tv|apple tv|box)/.test(s)) return 'tv'
    if (/(router|keenetic|mikrotik|openwrt|gl\.inet)/.test(s)) return 'router'
    return 'phone'
}

/** Заполненная звезда Telegram Stars — единственная заливная иконка набора. */
export function StarFilled({ size = 16, color = 'currentColor' }: { size?: number; color?: string }) {
    return (
        <svg width={size} height={size} viewBox="0 0 24 24" fill={color} aria-hidden="true">
            <path d={PATHS.stars} />
        </svg>
    )
}

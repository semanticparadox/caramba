import { apiUrl } from '../../config'

export const CONNECT_LINK_PREFIX = 'caramba://connect?d='

export type ConnectLink = { link: string; expiresInSeconds: number }

/** Одноразовая ссылка входа в Caramba Connect. Каждый вызов гасит предыдущую,
 *  поэтому дёргать его стоит только по явному тапу. Любая осечка — null:
 *  пользователю показываем один понятный тост, а не код ошибки. */
export async function requestConnectLink(token: string): Promise<ConnectLink | null> {
    try {
        const r = await fetch(apiUrl('/api/client/app/connect-link'), {
            method: 'POST',
            headers: { Authorization: `Bearer ${token}` },
        })
        if (!r.ok) return null
        const d = await r.json()
        const link = d && typeof d.link === 'string' ? d.link : ''
        // Чужая схема или мусор в ответе — не наш вход, открывать такое нельзя.
        if (!link.startsWith(CONNECT_LINK_PREFIX)) return null
        return { link, expiresInSeconds: Number(d.expires_in_seconds) || 1800 }
    } catch {
        return null
    }
}

/** Попытка передать ссылку установленному приложению. Установлено ли оно,
 *  WebView не сообщает — успех здесь ничего не доказывает, поэтому вызывающий
 *  всё равно копирует ссылку и показывает её на экране. */
export function openInCarambaApp(link: string): boolean {
    try {
        const w = window.open(link, '_blank', 'noopener,noreferrer')
        return !!w
    } catch {
        return false
    }
}

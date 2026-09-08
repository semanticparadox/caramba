import { useEffect, useState } from 'react'
import { apiUrl } from '../../config'

export type AppPlatform = 'android' | 'ios' | 'windows' | 'macos' | 'linux'

/** Порядок показа в списке скачивания — от самой массовой платформы к редкой. */
export const APP_PLATFORMS: AppPlatform[] = ['android', 'ios', 'windows', 'macos', 'linux']

/** Адреса сборок Caramba Connect — из настроек панели, чтобы владелец добавлял
 *  ссылку без релиза мини-аппа. Всё, что не https, отбрасываем: такую ссылку
 *  Telegram всё равно не откроет, а пустая настройка честнее показывает «Скоро». */
export function useAppDownloads(token: string | null): Partial<Record<AppPlatform, string>> {
    const [downloads, setDownloads] = useState<Partial<Record<AppPlatform, string>>>({})
    useEffect(() => {
        if (!token) return
        void fetch(apiUrl('/api/client/app/downloads'), { headers: { Authorization: `Bearer ${token}` } })
            .then((r) => (r.ok ? r.json() : {}))
            .then((d) => {
                if (!d || typeof d !== 'object') return
                const next: Partial<Record<AppPlatform, string>> = {}
                for (const platform of APP_PLATFORMS) {
                    const url = (d as Record<string, unknown>)[platform]
                    if (typeof url === 'string' && url.startsWith('https://')) next[platform] = url
                }
                setDownloads(next)
            })
            .catch(() => {})
    }, [token])
    return downloads
}

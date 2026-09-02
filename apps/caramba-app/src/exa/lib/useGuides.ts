import { useEffect, useState } from 'react'
import { apiUrl } from '../../config'

export type GuidePlatform = 'index' | 'ios' | 'android' | 'windows' | 'macos' | 'linux' | 'tv' | 'router'

/** Адреса инструкций (Telegraph) — из настроек панели, чтобы менять их без релиза. */
export function useGuides(token: string | null) {
    const [guides, setGuides] = useState<Partial<Record<GuidePlatform, string>>>({})
    useEffect(() => {
        if (!token) return
        void fetch(apiUrl('/api/client/guides'), { headers: { Authorization: `Bearer ${token}` } })
            .then((r) => (r.ok ? r.json() : {}))
            .then((d) => setGuides(d && typeof d === 'object' ? d : {}))
            .catch(() => {})
    }, [token])
    return guides
}

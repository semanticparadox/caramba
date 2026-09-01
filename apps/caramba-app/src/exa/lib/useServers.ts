import { useCallback, useEffect, useState } from 'react'
import { apiUrl } from '../../config'

export interface ExaServer {
    id: number
    name: string
    country_code: string
    latency?: number
    status: string
    available_variant_ids?: string[]
    recommended_variant_id?: string | null
    active_connections?: number
    max_users?: number
    is_full?: boolean
}

export type LoadLevel = 'low' | 'medium' | 'high'

/** Нагрузка узла — доля занятых слотов; без лимита считаем по числу сессий. */
export function loadOf(server: ExaServer): { level: LoadLevel; ratio: number } {
    const used = server.active_connections ?? 0
    const cap = server.max_users && server.max_users > 0 ? server.max_users : 100
    const ratio = Math.min(1, used / cap)
    const level: LoadLevel = server.is_full || ratio >= 0.8 ? 'high' : ratio >= 0.45 ? 'medium' : 'low'
    return { level, ratio }
}

/** Список серверов панели; порядок — по стране, затем по пингу. */
export function useServers(token: string | null, subId?: number | null) {
    const [servers, setServers] = useState<ExaServer[]>([])
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const load = useCallback(async () => {
        if (!token) return
        setLoading(true)
        setError(null)
        try {
            // sub_id нужен панели, чтобы отметить рекомендуемый вариант подключения.
            const query = subId ? `?sub_id=${subId}` : ''
            const res = await fetch(apiUrl(`/api/client/servers${query}`), { headers: { Authorization: `Bearer ${token}` } })
            if (!res.ok) throw new Error(String(res.status))
            const data = await res.json()
            const list: ExaServer[] = Array.isArray(data) ? data : (data?.servers ?? [])
            list.sort(
                (a, b) =>
                    a.country_code.localeCompare(b.country_code) || (a.latency ?? 9999) - (b.latency ?? 9999),
            )
            setServers(list)
        } catch (e) {
            setError(e instanceof Error ? e.message : 'error')
        } finally {
            setLoading(false)
        }
    }, [token, subId])

    useEffect(() => {
        void load()
    }, [load])

    return { servers, loading, error, reload: load }
}

/** Закрепить сервер за подпиской. */
export async function selectServer(token: string, subId: number, nodeId: number): Promise<boolean> {
    const res = await fetch(apiUrl(`/api/client/subscription/${subId}/server`), {
        method: 'POST',
        headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
        body: JSON.stringify({ node_id: nodeId }),
    })
    return res.ok
}

/** Страны в порядке появления в списке серверов. */
export function groupByCountry(servers: ExaServer[]): { code: string; servers: ExaServer[] }[] {
    const map = new Map<string, ExaServer[]>()
    for (const s of servers) {
        const code = (s.country_code || '??').toUpperCase()
        if (!map.has(code)) map.set(code, [])
        map.get(code)!.push(s)
    }
    return [...map.entries()].map(([code, list]) => ({ code, servers: list }))
}

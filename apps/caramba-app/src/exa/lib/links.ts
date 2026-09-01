import { useCallback, useEffect, useState } from 'react'
import { apiUrl } from '../../config'

/** Тип подключения для роутера — то, что человек выбирает в шторке. */
export type LinkKind = 'reality' | 'ws' | 'grpc' | 'httpupgrade' | 'tcp' | 'hysteria2' | 'tuic' | 'trojan' | 'other'

export interface InboundLink {
    url: string
    nodeId: number | null
    kind: LinkKind
    /** Человеческая метка из панели: «VLESS-REALITY», «Hysteria2-udp»… */
    remark: string
}

/** Порядок в шторке: сначала самое устойчивое к блокировкам. */
export const KIND_ORDER: LinkKind[] = ['reality', 'ws', 'grpc', 'httpupgrade', 'hysteria2', 'tuic', 'trojan', 'tcp', 'other']

/** Разбор одной ссылки: схема и query дают тип, фрагмент — узел
 *  (панель пишет его как `{имя}-{node_id} {протокол}-{транспорт}`). */
export function parseLink(raw: string): InboundLink {
    let scheme = ''
    let query = ''
    let fragment = ''
    try {
        const u = new URL(raw)
        scheme = u.protocol.replace(':', '').toLowerCase()
        query = u.search.toLowerCase()
        fragment = decodeURIComponent(u.hash.replace(/^#/, ''))
    } catch {
        const m = /^([a-z0-9+.-]+):/i.exec(raw)
        scheme = m ? m[1].toLowerCase() : ''
        const h = raw.indexOf('#')
        fragment = h >= 0 ? decodeURIComponent(raw.slice(h + 1)) : ''
        const q = raw.indexOf('?')
        query = q >= 0 ? raw.slice(q, h >= 0 ? h : undefined).toLowerCase() : ''
    }

    let kind: LinkKind = 'other'
    if (scheme === 'hysteria2' || scheme === 'hy2') kind = 'hysteria2'
    else if (scheme === 'tuic') kind = 'tuic'
    else if (scheme === 'trojan') kind = 'trojan'
    else if (scheme === 'vless' || scheme === 'vmess') {
        if (/security=reality/.test(query)) kind = 'reality'
        else if (/type=ws/.test(query)) kind = 'ws'
        else if (/type=grpc/.test(query)) kind = 'grpc'
        else if (/type=httpupgrade/.test(query)) kind = 'httpupgrade'
        else kind = 'tcp'
    }

    const m = /^(.*)-(\d+)\s+(.+)$/.exec(fragment)
    return {
        url: raw,
        nodeId: m ? Number(m[2]) : null,
        kind,
        remark: m ? m[3] : fragment,
    }
}

export function useInboundLinks(token: string | null, subId: number | null) {
    const [links, setLinks] = useState<InboundLink[]>([])
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const load = useCallback(async () => {
        if (!token || !subId) return
        setLoading(true)
        setError(null)
        try {
            const res = await fetch(apiUrl(`/api/client/subscription/${subId}/links`), {
                headers: { Authorization: `Bearer ${token}` },
            })
            if (!res.ok) throw new Error(String(res.status))
            const data = await res.json()
            const raw: string[] = Array.isArray(data?.links) ? data.links : []
            setLinks(raw.map(parseLink))
        } catch (e) {
            setError(e instanceof Error ? e.message : 'error')
        } finally {
            setLoading(false)
        }
    }, [token, subId])

    useEffect(() => {
        void load()
    }, [load])

    return { links, loading, error, reload: load }
}

/** Ссылки конкретного узла, по одной на тип, в брендовом порядке. */
export function linksForNode(links: InboundLink[], nodeId: number): InboundLink[] {
    const byKind = new Map<LinkKind, InboundLink>()
    for (const l of links) {
        if (l.nodeId !== nodeId) continue
        if (!byKind.has(l.kind)) byKind.set(l.kind, l)
    }
    return KIND_ORDER.filter((k) => byKind.has(k)).map((k) => byKind.get(k)!)
}

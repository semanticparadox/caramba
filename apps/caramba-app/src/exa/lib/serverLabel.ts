import type { ExaServer } from './useServers'

/** Панель нарочно не раскрывает клиенту имена узлов и отдаёт «Node #7 (900 Mbps)».
 *  Пользователю это ничего не говорит, поэтому имя не показываем вовсе:
 *  заголовок — страна, а скорость канала вытаскиваем из строки. */
export function serverSpeedMbps(server: ExaServer): number | null {
    const m = /\((\d+)\s*Mbps\)/i.exec(server.name || '')
    return m ? Number(m[1]) : null
}

/** «Недоступен» — только реально выключенный узел. Панель отдаёт ещё
 *  `fast`, `busy` и `full`; первые два — оттенки нагрузки, а не отказ. */
export type Availability = 'ok' | 'full' | 'offline'

export function availability(server: ExaServer): Availability {
    const s = (server.status || '').toLowerCase()
    if (server.is_full || s === 'full') return 'full'
    if (['offline', 'inactive', 'disabled', 'maintenance', 'down', 'error'].includes(s)) return 'offline'
    return 'ok'
}

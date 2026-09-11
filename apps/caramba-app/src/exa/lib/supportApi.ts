/** Тикеты поддержки в мини-аппе: контракт `/api/client/tickets*`
 *  (`apps/caramba-panel/src/api/client.rs`). Панель отдаёт сырые модели БД
 *  (`libs/caramba-db/src/models/tickets.rs`), поэтому поля здесь называются
 *  как в базе, а не как в Flutter-контракте `/api/v2/app/tickets`. */

import { apiUrl } from '../../config'

export type TicketStatus = 'open' | 'in_progress' | 'awaiting_user' | 'resolved' | 'closed'

/** Категории из `ALLOWED_TICKET_CATEGORIES` панели: список обязан совпадать
 *  один в один, неизвестное значение панель не примет. */
export const TICKET_CATEGORIES = ['general', 'billing', 'connection', 'device', 'technical', 'feature_request', 'other'] as const
export type TicketCategory = (typeof TICKET_CATEGORIES)[number]

export interface TicketSummary {
    id: number
    category: string
    subject: string
    status: TicketStatus
    created_at: string
    updated_at: string
    last_message_preview: string | null
    /** Ответы поддержки новее последнего открытия переписки владельцем. */
    unread_for_user: number
}

export interface TicketMessage {
    id: number
    ticket_id: number
    sender_role: 'user' | 'admin' | 'system'
    body: string
    created_at: string
}

export interface TicketDetail {
    ticket: {
        id: number
        category: string
        subject: string
        status: TicketStatus
        created_at: string
        updated_at: string
    }
    messages: TicketMessage[]
}

/** В тикет можно писать, пока он не решён и не закрыт. */
export const ticketIsOpen = (status: TicketStatus) => status !== 'resolved' && status !== 'closed'

const auth = (token: string) => ({ Authorization: `Bearer ${token}` })

export async function fetchTickets(token: string, signal?: AbortSignal): Promise<TicketSummary[]> {
    const res = await fetch(apiUrl('/api/client/tickets'), { headers: auth(token), signal })
    if (!res.ok) throw new Error(`tickets ${res.status}`)
    const data = await res.json()
    return Array.isArray(data) ? (data as TicketSummary[]) : []
}

/** GET одного тикета заодно отмечает его прочитанным на панели
 *  (`tickets_service::get_ticket` для владельца). */
export async function fetchTicket(token: string, id: number, signal?: AbortSignal): Promise<TicketDetail | null> {
    const res = await fetch(apiUrl(`/api/client/tickets/${id}`), { headers: auth(token), signal })
    if (res.status === 404 || res.status === 403) return null
    if (!res.ok) throw new Error(`ticket ${res.status}`)
    return (await res.json()) as TicketDetail
}

export async function createTicket(
    token: string,
    input: { category: TicketCategory; subject: string; body: string },
): Promise<{ id: number } | null> {
    const res = await fetch(apiUrl('/api/client/tickets'), {
        method: 'POST',
        headers: { ...auth(token), 'Content-Type': 'application/json' },
        body: JSON.stringify(input),
    })
    if (!res.ok) return null
    const data = await res.json().catch(() => null)
    return data && typeof data.id === 'number' ? { id: data.id } : null
}

/** Ответ в тикет. 422 означает, что тикет уже закрыт: отдаём `closed`, чтобы
 *  экран перечитал статус, а не показал общую ошибку. */
export async function replyTicket(token: string, id: number, body: string): Promise<'ok' | 'closed' | 'error'> {
    const res = await fetch(apiUrl(`/api/client/tickets/${id}/messages`), {
        method: 'POST',
        headers: { ...auth(token), 'Content-Type': 'application/json' },
        body: JSON.stringify({ body }),
    })
    if (res.ok) return 'ok'
    if (res.status === 422) return 'closed'
    return 'error'
}

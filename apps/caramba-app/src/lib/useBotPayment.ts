import { useEffect, useRef, useState } from 'react'
import { apiUrl } from '../config'

/** Интервал поллинга статуса платёжной сессии. */
const POLL_INTERVAL_MS = 5000
/** Максимум попыток: 120 × 5с = 10 минут, дальше переходим в 'timeout'. */
const POLL_MAX_ATTEMPTS = 120

/**
 * 'failed' | 'expired' — терминальный неуспех от сервера;
 * 'timeout' — 10 минут без подтверждения: поллинг остановлен, оплата могла
 * пройти позже (итог придёт в чат бота).
 */
export type BotPaymentStatus = 'pending' | 'completed' | 'failed' | 'expired' | 'timeout'

export type BotPaymentState = {
  /** Абсолютный URL внешнего чекаута — для кнопки «Оплатить в браузере». */
  invoiceUrl: string
  /** id платёжной сессии для поллинга статуса (может отсутствовать у старых ответов). */
  sessionId: string | null
  status: BotPaymentStatus
}

type UseBotPaymentOptions = {
  token: string | null
  /** Вызывается один раз, когда сессия стала completed — обновить данные пользователя. */
  onRefresh: () => Promise<void> | void
}

/**
 * Состояние «ссылка на оплату отправлена в чат бота» + поллинг
 * GET /api/client/payment/session/{id} каждые 5 секунд (до 10 минут),
 * пока платёж pending. Когда сервер отвечает completed — переключает
 * статус (UI показывает success-баннер) и дёргает onRefresh.
 *
 * Используется через usePurchase (планы) и напрямую в Store (заказы).
 */
export function useBotPayment({ token, onRefresh }: UseBotPaymentOptions) {
  const [botPayment, setBotPayment] = useState<BotPaymentState | null>(null)

  // Рефы, чтобы интервал не пересоздавался из-за нестабильных ссылок
  // на token/onRefresh между рендерами.
  const tokenRef = useRef(token)
  tokenRef.current = token
  const onRefreshRef = useRef(onRefresh)
  onRefreshRef.current = onRefresh

  const startBotPayment = (invoiceUrl: string, sessionId: string | null) => {
    setBotPayment({ invoiceUrl, sessionId, status: 'pending' })
  }

  const clearBotPayment = () => setBotPayment(null)

  const sessionId = botPayment?.sessionId ?? null
  const isPending = botPayment?.status === 'pending'

  useEffect(() => {
    if (!sessionId || !isPending) return

    let attempts = 0
    let cancelled = false

    // Единая точка выхода из pending: остановить поллинг И показать
    // пользователю терминальное состояние — иначе панель вечно врала бы
    // «статус обновится автоматически», хотя обновлять его уже некому.
    const finish = (timer: number, status: BotPaymentStatus) => {
      window.clearInterval(timer)
      setBotPayment((prev) => (prev && prev.sessionId === sessionId ? { ...prev, status } : prev))
    }

    const timer = window.setInterval(async () => {
      attempts += 1
      if (attempts > POLL_MAX_ATTEMPTS) {
        finish(timer, 'timeout')
        return
      }
      const authToken = tokenRef.current
      if (!authToken) return
      try {
        const res = await fetch(apiUrl(`/api/client/payment/session/${sessionId}`), {
          headers: { Authorization: `Bearer ${authToken}` },
        })
        if (!res.ok || cancelled) return
        const data = await res.json()
        if (cancelled) return
        if (data.status === 'completed') {
          finish(timer, 'completed')
          void onRefreshRef.current()
        } else if (data.status === 'failed' || data.status === 'expired') {
          // Терминальный неуспех — поллить дальше бессмысленно. Кнопки
          // «Открыть чат» / «Оплатить в браузере» остаются доступны.
          finish(timer, data.status)
        }
      } catch {
        // Сетевая ошибка — молча пробуем в следующем тике.
      }
    }, POLL_INTERVAL_MS)

    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [sessionId, isPending])

  return { botPayment, startBotPayment, clearBotPayment }
}

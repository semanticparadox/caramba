import WebApp from '@twa-dev/sdk'
import { useState } from 'react'
import { apiUrl } from '../config'
import { useBotPayment } from './useBotPayment'

/**
 * Результат попытки покупки, который вызывающий код использует для
 * показа баннера/сообщения. `outcome` различает завершённые сценарии
 * (success / error) и редиректы (где UI уже передан внешней странице/SDK).
 */
export type PurchaseResult =
  | { outcome: 'success'; messageKey: string; messageParams?: Record<string, unknown> }
  | { outcome: 'error'; message?: string; messageKey?: string }
  // Платёж продолжается во внешнем потоке (Stars invoice внутри Telegram).
  // UI не должен показывать success — итог придёт через refreshData/webhook.
  | { outcome: 'redirect' }
  // Ручная оплата: создан счёт, нужно показать ссылку на загрузку чека.
  | { outcome: 'manual'; invoiceUrl: string }
  // Внешний http(s)-чекаут: сервер продублировал ссылку в чат бота.
  // UI показывает панель «ссылка отправлена в чат» (см. botPayment в хуке),
  // статус сессии поллится до completed. Никаких window.location.href.
  | { outcome: 'bot_link'; invoiceUrl: string; sessionId: string | null }

export type PurchaseParams = {
  durationId: number
  provider: string
}

type UsePurchaseOptions = {
  token: string | null
  /** Вызывается после успешного завершения, чтобы обновить данные пользователя. */
  onRefresh: () => Promise<void> | void
}

/**
 * Централизованная логика создания счёта / запуска оплаты.
 * Раньше дублировалась в Plans.tsx и Home.tsx — теперь единый источник правды.
 *
 * Возвращает `purchasingDurationId` (для блокировки кнопок выбранного срока)
 * и `purchase()` — асинхронную функцию, которая выполняет запрос и возвращает
 * структурированный результат. Сообщения НЕ хардкодятся: вызывающий код сам
 * переводит messageKey через i18n.
 */
export function usePurchase({ token, onRefresh }: UsePurchaseOptions) {
  const [purchasingDurationId, setPurchasingDurationId] = useState<number | null>(null)
  // Способ оплаты, по которому сейчас идёт запрос — для busy-состояния кнопки.
  const [purchasingProvider, setPurchasingProvider] = useState<string | null>(null)
  // Состояние «ссылка на оплату отправлена в чат бота» + поллинг статуса сессии.
  const { botPayment, startBotPayment, clearBotPayment } = useBotPayment({ token, onRefresh })

  const isPurchasing = purchasingDurationId !== null

  const purchase = async ({ durationId, provider }: PurchaseParams): Promise<PurchaseResult> => {
    if (!token) {
      return { outcome: 'error', messageKey: 'home.authError' }
    }
    // Защита от двойного клика: если запрос уже в полёте — игнорируем.
    if (purchasingDurationId !== null) {
      return { outcome: 'redirect' }
    }

    setPurchasingDurationId(durationId)
    setPurchasingProvider(provider)

    try {
      const res = await fetch(apiUrl('/api/client/payment/invoice'), {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${token}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          duration_id: durationId,
          provider,
        }),
      })

      if (!res.ok) {
        const errText = await res.text().catch(() => '')
        return { outcome: 'error', message: errText || undefined, messageKey: 'home.invoiceError' }
      }

      const data = await res.json()
      const invoiceUrl = String(data.invoice_url ?? '')

      // Нет invoice_url — оплата завершена сервером сразу (например, balance).
      if (!data.invoice_url) {
        await onRefresh()
        return { outcome: 'success', messageKey: 'home.paymentSuccess' }
      }

      // Ручная оплата — показываем ссылку на загрузку чека, поток завершён.
      if (provider === 'manual') {
        await onRefresh()
        return { outcome: 'manual', invoiceUrl: data.invoice_url }
      }

      // Telegram Stars — нативный invoice внутри Telegram.
      const isStars =
        provider === 'telegram_stars' || provider === 'stars' || invoiceUrl.includes('t.me/invoice')

      if (isStars) {
        WebApp.openInvoice(data.invoice_url, (status) => {
          if (status) {
            void onRefresh()
          }
        })
        return { outcome: 'redirect' }
      }

      // Balance (сервер отвечает сентинелом invoice_url: "SUCCESS") и любой
      // другой не-http payload мгновенного провайдера: покупка уже завершена
      // на сервере — показываем успех, НИКОГДА не уходим в location.href
      // (раньше приложение редиректило на буквальную строку "SUCCESS").
      const isBalance = provider === 'balance' || data.provider === 'balance'
      if (isBalance || !/^https?:\/\//i.test(invoiceUrl)) {
        await onRefresh()
        return { outcome: 'success', messageKey: 'home.paymentSuccess' }
      }

      // Внешний http(s)-чекаут: сервер уже отправил ссылку в чат бота
      // (delivered_via: "bot"). Показываем in-app панель и поллим статус
      // сессии до completed — вместо прежнего window.location.href.
      startBotPayment(invoiceUrl, data.session_id ? String(data.session_id) : null)
      return { outcome: 'bot_link', invoiceUrl, sessionId: data.session_id ?? null }
    } catch {
      return { outcome: 'error', messageKey: 'home.networkInvoiceError' }
    } finally {
      // Для redirect-сценариев компонент всё равно уходит со страницы,
      // но сброс безопасен и нужен для Stars/manual (страница остаётся).
      setPurchasingDurationId(null)
      setPurchasingProvider(null)
    }
  }

  return {
    purchasing: purchasingDurationId,
    purchasingProvider,
    isPurchasing,
    purchase,
    botPayment,
    clearBotPayment,
  }
}

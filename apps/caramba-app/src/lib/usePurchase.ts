import { useState } from 'react'
import WebApp from '@twa-dev/sdk'
import { apiUrl } from '../config'

/**
 * Результат попытки покупки, который вызывающий код использует для
 * показа баннера/сообщения. `outcome` различает завершённые сценарии
 * (success / error) и редиректы (где UI уже передан внешней странице/SDK).
 */
export type PurchaseResult =
  | { outcome: 'success'; messageKey: string; messageParams?: Record<string, unknown> }
  | { outcome: 'error'; message?: string; messageKey?: string }
  // Платёж продолжается во внешнем потоке (Stars invoice / редирект на checkout).
  // UI не должен показывать success — итог придёт через refreshData/webhook.
  | { outcome: 'redirect' }
  // Ручная оплата: создан счёт, нужно показать ссылку на загрузку чека.
  | { outcome: 'manual'; invoiceUrl: string }

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
        provider === 'telegram_stars' ||
        provider === 'stars' ||
        String(data.invoice_url).includes('t.me/invoice')

      if (isStars) {
        WebApp.openInvoice(data.invoice_url, (status) => {
          if (status) {
            void onRefresh()
          }
        })
        return { outcome: 'redirect' }
      }

      // Внешний checkout — редирект на платёжную страницу провайдера.
      window.location.href = data.invoice_url
      return { outcome: 'redirect' }
    } catch {
      return { outcome: 'error', messageKey: 'home.networkInvoiceError' }
    } finally {
      // Для redirect-сценариев компонент всё равно уходит со страницы,
      // но сброс безопасен и нужен для Stars/manual (страница остаётся).
      setPurchasingDurationId(null)
      setPurchasingProvider(null)
    }
  }

  return { purchasing: purchasingDurationId, purchasingProvider, isPurchasing, purchase }
}

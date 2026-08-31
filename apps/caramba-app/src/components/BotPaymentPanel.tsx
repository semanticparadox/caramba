import WebApp from '@twa-dev/sdk'
import { useTranslation } from 'react-i18next'
import type { BotPaymentState } from '../lib/useBotPayment'
import './BotPaymentPanel.css'

type BotPaymentPanelProps = {
  payment: BotPaymentState
  /** Username бота (без @) из /user/stats — для кнопки «Открыть чат». */
  botUsername?: string | null
  /** Закрыть панель (пользователь ознакомился / успех подтверждён). */
  onClose: () => void
}

/** Открывает внешний URL через Telegram SDK, вне Telegram — window.open. */
const openExternalLink = (url: string) => {
  try {
    WebApp.openLink(url)
  } catch {
    window.open(url, '_blank', 'noopener')
  }
}

/** Открывает чат бота через Telegram SDK, вне Telegram — window.open. */
const openBotChat = (botUsername: string) => {
  const url = `https://t.me/${botUsername}`
  try {
    WebApp.openTelegramLink(url)
  } catch {
    window.open(url, '_blank', 'noopener')
  }
}

/**
 * In-app состояние платежа, ссылка на который отправлена в чат бота:
 * pending — подсказка + кнопки «Открыть чат» / «Оплатить в браузере»,
 * completed — success-баннер (данные уже обновлены поллингом useBotPayment).
 */
export default function BotPaymentPanel({ payment, botUsername, onClose }: BotPaymentPanelProps) {
  const { t } = useTranslation()

  if (payment.status === 'completed') {
    return (
      <div className="bot-payment glass-card completed" role="status" aria-live="polite">
        <p className="bot-payment-text">{t('payment.confirmed')}</p>
        <div className="bot-payment-actions">
          <button type="button" className="bot-payment-btn" onClick={onClose}>
            {t('common.close')}
          </button>
        </div>
      </div>
    )
  }

  // Поллинг завершился без успеха: 'failed'/'expired' — сессия мертва,
  // 'timeout' — 10 минут без подтверждения (итог, если оплата всё же пройдёт,
  // придёт в чат бота). Панель обязана сказать это явно, а не вечно обещать
  // «статус обновится автоматически».
  const isTerminalFailure = payment.status === 'failed' || payment.status === 'expired'
  const isTimeout = payment.status === 'timeout'

  return (
    <div className="bot-payment glass-card" role="status" aria-live="polite">
      <p className="bot-payment-text">
        {isTerminalFailure ? t('payment.sessionFailed') : t('payment.botLinkSent')}
      </p>
      <p className="bot-payment-hint">
        {isTerminalFailure
          ? t('payment.sessionFailedHint')
          : isTimeout
            ? t('payment.pollTimeout')
            : t('payment.waitingConfirmation')}
      </p>
      <div className="bot-payment-actions">
        {botUsername && (
          <button
            type="button"
            className="bot-payment-btn primary"
            onClick={() => openBotChat(botUsername)}
          >
            {t('payment.openChat')}
          </button>
        )}
        {!isTerminalFailure && (
          <button
            type="button"
            className="bot-payment-btn"
            onClick={() => openExternalLink(payment.invoiceUrl)}
          >
            {t('payment.payInBrowser')}
          </button>
        )}
        <button type="button" className="bot-payment-btn ghost" onClick={onClose}>
          {t('common.close')}
        </button>
      </div>
    </div>
  )
}

type CopySubscriptionButtonProps = {
  onCopy: () => void
  copied: boolean
}

export default function CopySubscriptionButton({
  onCopy,
  copied,
}: CopySubscriptionButtonProps) {
  return (
    <button type="button" className="btn-secondary copy-subscription-btn" onClick={onCopy}>
      {copied ? 'Ссылка скопирована' : 'Скопировать ссылку подписки'}
    </button>
  )
}

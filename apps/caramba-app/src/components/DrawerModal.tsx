import { ReactNode, useEffect, useId, useRef } from 'react'
import Icon from './Icon'
import './DrawerModal.css'

type DrawerModalProps = {
    open: boolean
    title: string
    subtitle?: string
    onClose: () => void
    children: ReactNode
    footer?: ReactNode
    /** Локализованный aria-label для кнопки закрытия (по умолчанию — без подписи). */
    closeLabel?: string
}

export default function DrawerModal({
    open,
    title,
    subtitle,
    onClose,
    children,
    footer,
    closeLabel,
}: DrawerModalProps) {
    const panelRef = useRef<HTMLDivElement>(null)
    const titleId = useId()
    const subtitleId = useId()

    // Закрытие по Escape + перевод фокуса на панель при открытии (a11y).
    useEffect(() => {
        if (!open) return

        const handleKeyDown = (e: KeyboardEvent) => {
            if (e.key === 'Escape') {
                e.stopPropagation()
                onClose()
            }
        }
        document.addEventListener('keydown', handleKeyDown)

        // Фокусируем панель, чтобы скринридер озвучил заголовок диалога
        // и клавиатура (Tab/Escape) работала внутри модалки.
        const focusTimer = window.setTimeout(() => {
            panelRef.current?.focus()
        }, 0)

        return () => {
            document.removeEventListener('keydown', handleKeyDown)
            window.clearTimeout(focusTimer)
        }
    }, [open, onClose])

    if (!open) return null

    return (
        <div className="drawer-overlay" onClick={onClose}>
            <div
                ref={panelRef}
                className="drawer-panel modal-drawer"
                role="dialog"
                aria-modal="true"
                aria-labelledby={titleId}
                aria-describedby={subtitle ? subtitleId : undefined}
                tabIndex={-1}
                onClick={(e) => e.stopPropagation()}
            >
                <header className="modal-drawer-head">
                    <div>
                        <h3 id={titleId}>{title}</h3>
                        {subtitle && <p id={subtitleId}>{subtitle}</p>}
                    </div>
                    <button
                        type="button"
                        className="modal-close-btn"
                        onClick={onClose}
                        aria-label={closeLabel || title}
                    >
                        <Icon name="close" size={14} />
                    </button>
                </header>
                <div className="modal-drawer-body">{children}</div>
                {footer && <div className="modal-drawer-foot">{footer}</div>}
            </div>
        </div>
    )
}

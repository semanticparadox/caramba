import { ReactNode } from 'react'
import './DrawerModal.css'

type DrawerModalProps = {
    open: boolean
    title: string
    subtitle?: string
    onClose: () => void
    children: ReactNode
    footer?: ReactNode
}

export default function DrawerModal({
    open,
    title,
    subtitle,
    onClose,
    children,
    footer,
}: DrawerModalProps) {
    if (!open) return null

    return (
        <div className="drawer-overlay" onClick={onClose}>
            <div className="drawer-panel modal-drawer" onClick={(e) => e.stopPropagation()}>
                <header className="modal-drawer-head">
                    <div>
                        <h3>{title}</h3>
                        {subtitle && <p>{subtitle}</p>}
                    </div>
                    <button className="modal-close-btn" onClick={onClose} aria-label="Закрыть">X</button>
                </header>
                <div className="modal-drawer-body">{children}</div>
                {footer && <div className="modal-drawer-foot">{footer}</div>}
            </div>
        </div>
    )
}

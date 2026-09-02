/** Примитивы EXA: карточка, кнопка, чип страны, метка, шторка.
 *  Всё — на токенах из exa-tokens.css; стили в exa.css. */
import { useEffect, type ButtonHTMLAttributes, type ReactNode } from 'react'
import { hapticTap } from '../lib/haptics'
import { countryFlag } from '../lib/planFormat'
import { ExaIcon } from './icons'

export function Card({
    children,
    className = '',
    muted = false,
    onClick,
    as: Tag = 'section',
}: {
    children: ReactNode
    className?: string
    /** Приглушённая карточка — для состояний «истекла», «приостановлено». */
    muted?: boolean
    onClick?: () => void
    as?: 'section' | 'div' | 'button'
}) {
    const cls = `exa-card${muted ? ' is-muted' : ''}${onClick ? ' is-tappable' : ''} ${className}`.trim()
    if (Tag === 'button') {
        return (
            <button type="button" className={cls} onClick={onClick}>
                {children}
            </button>
        )
    }
    return (
        <Tag className={cls} onClick={onClick}>
            {children}
        </Tag>
    )
}

/** children объявлены явно: react-i18next расширяет тип детей у DOM-элементов,
 *  и без этого TypeScript не сводит два определения. */
type ButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> & {
    children?: ReactNode
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger'
    size?: 'lg' | 'md' | 'sm'
    icon?: ReactNode
    block?: boolean
}

export function Button({
    variant = 'primary',
    size = 'lg',
    icon,
    block = true,
    className = '',
    children,
    onClick,
    ...rest
}: ButtonProps) {
    return (
        <button
            type="button"
            className={`exa-btn exa-btn--${variant} exa-btn--${size}${block ? ' is-block' : ''} ${className}`.trim()}
            onClick={(e) => {
                hapticTap()
                onClick?.(e)
            }}
            {...rest}
        >
            {icon}
            {children}
        </button>
    )
}

/** Квадратная кнопка-иконка 44×44 (роутер, QR, копирование). */
export function IconButton({
    label,
    children,
    className = '',
    onClick,
    ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; children: ReactNode }) {
    return (
        <button
            type="button"
            aria-label={label}
            className={`exa-iconbtn ${className}`.trim()}
            onClick={(e) => {
                hapticTap()
                onClick?.(e)
            }}
            {...rest}
        >
            {children}
        </button>
    )
}

/** Чип страны: флаг в стальной рамке. Флаг рисует система (Regional Indicator
 *  Symbols), поэтому в Telegram он выглядит нативно; код — только в aria. */
export function CountryChip({ code, small = false }: { code: string; small?: boolean }) {
    const cc = (code || '').toUpperCase().slice(0, 2)
    const flag = /^[A-Z]{2}$/.test(cc) ? countryFlag(cc) : cc
    return (
        <span className={`exa-chip${small ? ' is-small' : ''}`} role="img" aria-label={cc}>
            {flag}
        </span>
    )
}

export function Pill({
    tone = 'neutral',
    children,
}: {
    tone?: 'neutral' | 'accent' | 'warning' | 'danger'
    children: ReactNode
}) {
    return <span className={`exa-pill exa-pill--${tone}`}>{children}</span>
}

export function SectionLabel({ children }: { children: ReactNode }) {
    return <div className="exa-label">{children}</div>
}

/** Заголовок экрана-вкладки: крупный Tektur слева, вторичный текст справа. */
export function ScreenHeader({ title, aside }: { title: string; aside?: ReactNode }) {
    return (
        <header className="exa-screen-header">
            <h1>{title}</h1>
            {aside ? <span>{aside}</span> : null}
        </header>
    )
}

/** Строка списка с шевроном — ссылка на подэкран. */
export function LinkRow({
    icon,
    title,
    aside,
    onClick,
}: {
    icon?: ReactNode
    title: ReactNode
    aside?: ReactNode
    onClick: () => void
}) {
    return (
        <button
            type="button"
            className="exa-card exa-linkrow"
            onClick={() => {
                hapticTap()
                onClick()
            }}
        >
            {icon ? <span className="exa-linkrow__icon">{icon}</span> : null}
            <span className="exa-linkrow__title">{title}</span>
            {aside ? <span className="exa-linkrow__aside">{aside}</span> : null}
            <ExaIcon name="chevron" size={20} className="exa-linkrow__chevron" />
        </button>
    )
}

/** Нижняя шторка: ручка, заголовок, подзаголовок, тело. Выезжает за 180 мс,
 *  закрывается по скриму. Ничего не подпрыгивает. */
export function Sheet({
    open,
    title,
    subtitle,
    aside,
    onClose,
    children,
}: {
    open: boolean
    title: string
    subtitle?: ReactNode
    aside?: ReactNode
    onClose: () => void
    children: ReactNode
}) {
    useEffect(() => {
        if (!open) return
        const onKey = (e: KeyboardEvent) => {
            if (e.key === 'Escape') onClose()
        }
        document.addEventListener('keydown', onKey)
        return () => document.removeEventListener('keydown', onKey)
    }, [open, onClose])

    if (!open) return null
    return (
        <div className="exa-sheet-root" role="dialog" aria-modal="true" aria-label={title}>
            <div className="exa-scrim" onClick={onClose} />
            <div className="exa-sheet">
                <div className="exa-sheet__handle" />
                <header className="exa-sheet__header">
                    <div>
                        <div className="exa-sheet__title">{title}</div>
                        {subtitle ? <div className="exa-sheet__subtitle">{subtitle}</div> : null}
                    </div>
                    {aside ? <span className="exa-sheet__aside">{aside}</span> : null}
                </header>
                {children}
            </div>
        </div>
    )
}

/** Сегментный переключатель на два положения (Прямой / Через релей, RU / EN). */
export function Segmented<T extends string>({
    value,
    options,
    onChange,
    compact = false,
}: {
    value: T
    options: { value: T; label: string }[]
    onChange: (v: T) => void
    compact?: boolean
}) {
    return (
        <div className={`exa-segmented${compact ? ' is-compact' : ''}`} role="tablist">
            {options.map((o) => (
                <button
                    key={o.value}
                    type="button"
                    role="tab"
                    aria-selected={o.value === value}
                    className={o.value === value ? 'is-on' : ''}
                    onClick={() => {
                        if (o.value !== value) {
                            hapticTap()
                            onChange(o.value)
                        }
                    }}
                >
                    {o.label}
                </button>
            ))}
        </div>
    )
}

export function Toggle({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
    return (
        <button
            type="button"
            role="switch"
            aria-checked={on}
            aria-label={label}
            className={`exa-toggle${on ? ' is-on' : ''}`}
            onClick={() => {
                hapticTap()
                onChange(!on)
            }}
        >
            <span />
        </button>
    )
}

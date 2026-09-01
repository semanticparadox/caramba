/** Логомарк EXA: голова робота — капсула, тёмный визор, два объектива, антенна.
 *  Единственный «живой» элемент бренда — тёплый свет в глазах. Он горит, когда
 *  пользователь защищён, и гаснет во всех остальных состояниях. Цвета берутся из
 *  токенов, поэтому знак сам подстраивается под светлую тему Telegram. */
type ExaMarkProps = {
    size?: number
    lit?: boolean
    className?: string
}

export default function ExaMark({ size = 36, lit = true, className }: ExaMarkProps) {
    const eye = lit ? 'var(--exa-ember)' : 'var(--exa-mark-eye-off)'
    return (
        <svg
            width={size}
            height={size}
            viewBox="0 0 64 64"
            fill="none"
            role="img"
            aria-label="EXA"
            className={className}
            style={{ display: 'block', flexShrink: 0 }}
        >
            <rect x="29" y="4" width="6" height="12" rx="3" fill="var(--exa-mark-plate)" />
            <rect x="6" y="14" width="52" height="44" rx="16" fill="var(--exa-mark-head)" />
            <rect x="14" y="26" width="36" height="20" rx="10" fill="var(--exa-mark-visor)" />
            <circle
                cx="24"
                cy="36"
                r="9"
                fill={eye}
                opacity={lit ? 0.22 : 0}
                style={{ transition: `opacity var(--exa-glow-in)` }}
            />
            <circle
                cx="40"
                cy="36"
                r="9"
                fill={eye}
                opacity={lit ? 0.22 : 0}
                style={{ transition: `opacity var(--exa-glow-in)` }}
            />
            <circle cx="24" cy="36" r="4.5" fill={eye} />
            <circle cx="40" cy="36" r="4.5" fill={eye} />
        </svg>
    )
}

/** Маскот — стальной гигант. Ровно три состояния, как в identity-листе:
 *  `standing` — онбординг, глаза горят; `sitting` — пустое состояние, глаза
 *  погашены; `shield` — успех оплаты, глаза ярче и рука прикрывает щит.
 *  Плоский вектор, три тона стали и эмбер; никаких градиентов. */
type MascotState = 'standing' | 'sitting' | 'shield'

type MascotProps = {
    state: MascotState
    width?: number
}

const HEAD = 'var(--exa-mark-head)'
const PLATE = 'var(--exa-mark-plate)'
const VISOR = 'var(--exa-mark-visor)'
const EMBER = 'var(--exa-ember)'
const EYE_OFF = 'var(--exa-mark-eye-off)'

export default function Mascot({ state, width = 180 }: MascotProps) {
    const height = Math.round((width * 240) / 200)
    if (state === 'sitting') {
        return (
            <svg width={width} height={height} viewBox="0 0 200 240" fill="none" aria-hidden="true">
                <g transform="translate(0,40)">
                    <g transform="rotate(-7 100 60)">
                        <rect x="96" y="6" width="8" height="16" rx="4" fill={PLATE} />
                        <rect x="50" y="20" width="100" height="76" rx="28" fill={HEAD} />
                        <rect x="66" y="44" width="68" height="30" rx="15" fill={VISOR} />
                        <circle cx="86" cy="61" r="6" fill={EYE_OFF} />
                        <circle cx="114" cy="61" r="6" fill={EYE_OFF} />
                    </g>
                    <rect x="90" y="94" width="20" height="10" fill={PLATE} />
                    <rect x="44" y="104" width="112" height="76" rx="26" fill={HEAD} />
                    <rect x="74" y="122" width="52" height="28" rx="10" fill={PLATE} />
                    <circle cx="44" cy="126" r="15" fill={PLATE} />
                    <circle cx="156" cy="126" r="15" fill={PLATE} />
                    <rect x="22" y="128" width="24" height="56" rx="12" fill={HEAD} />
                    <rect x="154" y="128" width="24" height="56" rx="12" fill={HEAD} />
                    <rect x="30" y="168" width="60" height="30" rx="12" fill={PLATE} />
                    <rect x="110" y="168" width="60" height="30" rx="12" fill={PLATE} />
                    <rect x="10" y="170" width="28" height="28" rx="10" fill={HEAD} />
                    <rect x="162" y="170" width="28" height="28" rx="10" fill={HEAD} />
                </g>
            </svg>
        )
    }

    const bright = state === 'shield'
    return (
        <svg width={width} height={height} viewBox="0 0 200 240" fill="none" aria-hidden="true">
            <rect x="96" y="6" width="8" height="16" rx="4" fill={PLATE} />
            <rect x="50" y="20" width="100" height="76" rx="28" fill={HEAD} />
            <rect x="66" y="44" width="68" height="30" rx="15" fill={VISOR} />
            <circle cx="86" cy="59" r={bright ? 18 : 14} fill={EMBER} opacity={bright ? 0.3 : 0.22} />
            <circle cx="114" cy="59" r={bright ? 18 : 14} fill={EMBER} opacity={bright ? 0.3 : 0.22} />
            <circle cx="86" cy="59" r={bright ? 8 : 7} fill={EMBER} />
            <circle cx="114" cy="59" r={bright ? 8 : 7} fill={EMBER} />
            <rect x="90" y="96" width="20" height="10" fill={PLATE} />
            <rect x="40" y="106" width="120" height="90" rx="26" fill={HEAD} />
            {bright ? (
                <>
                    <circle cx="40" cy="128" r="16" fill={PLATE} />
                    <circle cx="160" cy="128" r="16" fill={PLATE} />
                    <rect x="160" y="130" width="24" height="64" rx="12" fill={HEAD} />
                    <rect x="16" y="130" width="24" height="36" rx="12" fill={HEAD} />
                    <rect x="28" y="150" width="60" height="22" rx="11" fill={HEAD} transform="rotate(-20 28 150)" />
                    <path d="M100 118l30 10v20c0 16-12 28-30 34-18-6-30-18-30-34v-20l30-10z" fill={PLATE} />
                    <path d="M100 128l20 6.5v14c0 11-8 19-20 23-12-4-20-12-20-23v-14l20-6.5z" fill={VISOR} />
                    <path d="M100 138v18" stroke={EMBER} strokeWidth="4" strokeLinecap="round" />
                </>
            ) : (
                <>
                    <rect x="72" y="126" width="56" height="32" rx="10" fill={PLATE} />
                    <rect x="92" y="138" width="16" height="8" rx="4" fill={VISOR} />
                    <circle cx="40" cy="128" r="16" fill={PLATE} />
                    <circle cx="160" cy="128" r="16" fill={PLATE} />
                    <rect x="16" y="130" width="24" height="64" rx="12" fill={HEAD} />
                    <rect x="160" y="130" width="24" height="64" rx="12" fill={HEAD} />
                </>
            )}
            <rect x="58" y="196" width="32" height="34" rx="10" fill={PLATE} />
            <rect x="110" y="196" width="32" height="34" rx="10" fill={PLATE} />
            <rect x="54" y="222" width="40" height="12" rx="6" fill={HEAD} />
            <rect x="106" y="222" width="40" height="12" rx="6" fill={HEAD} />
        </svg>
    )
}

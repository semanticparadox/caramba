import { useAuth } from '../context/AuthContext'
import ExaMark from './ExaMark'

export const carambaIconUrl = `${import.meta.env.BASE_URL}brand/caramba-icon.png`

export function isCarambaBrand(name?: string) {
    return !name?.trim() || /^caramba(?: connect)?$/i.test(name.trim())
}

/** The product mark is the default; custom/EXA instances keep their robot identity. */
export default function BrandMark({ size = 36, lit = true }: { size?: number; lit?: boolean }) {
    const { userStats } = useAuth()
    if (!isCarambaBrand(userStats?.brand_name)) return <ExaMark size={size} lit={lit} />
    return (
        <img src={carambaIconUrl} width={size} height={size} alt="Caramba"
            style={{ display: 'block', flexShrink: 0, borderRadius: size / 4 }} />
    )
}

import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { apiUrl } from '../config'
import { useAuth } from '../context/AuthContext'
import './ServerSelector.css'

interface Node {
    id: number
    country_code: string | null
    flag: string
    status: string
    distance_km: number | null
    name: string
}

interface RecommendedResponse {
    strategy?: string
    country_filter?: string | null
    nodes?: Array<{ id: number; name: string }>
}

type RoutingStrategy = 'balanced' | 'fastest' | 'stable'

const STRATEGY_OPTIONS: Array<{ value: RoutingStrategy; label: string }> = [
    { value: 'balanced', label: 'Баланс' },
    { value: 'fastest', label: 'Скорость' },
    { value: 'stable', label: 'Стабильность' },
]

function normalizeCountryParam(raw: string | null): string {
    const normalized = (raw ?? '').trim().toUpperCase()
    if (!normalized || normalized === 'ANY' || normalized === 'AUTO') return 'ANY'
    return /^[A-Z]{2}$/.test(normalized) ? normalized : 'ANY'
}

function normalizeStrategyParam(raw: string | null): RoutingStrategy {
    const normalized = (raw ?? '').trim().toLowerCase()
    if (normalized === 'fastest' || normalized === 'latency') return 'fastest'
    if (normalized === 'stable' || normalized === 'lowload' || normalized === 'load') return 'stable'
    return 'balanced'
}

export default function ServerSelector() {
    const { subId } = useParams()
    const navigate = useNavigate()
    const { token } = useAuth()
    const [searchParams] = useSearchParams()
    const [nodes, setNodes] = useState<Node[]>([])
    const [loading, setLoading] = useState(true)
    const [pinning, setPinning] = useState<number | null>(null)
    const [optimizing, setOptimizing] = useState(false)
    const [selectedCountry, setSelectedCountry] = useState<string>(() => normalizeCountryParam(searchParams.get('country')))
    const [selectedStrategy, setSelectedStrategy] = useState<RoutingStrategy>(() => normalizeStrategyParam(searchParams.get('strategy')))
    const [recommendedNodeId, setRecommendedNodeId] = useState<number | null>(null)
    const [msg, setMsg] = useState<{ type: 'success' | 'error'; text: string } | null>(null)
    const hasAutoRunMagic = useRef(false)

    useEffect(() => {
        if (!token) return
        fetch(apiUrl('/api/client/nodes'), {
            headers: { Authorization: `Bearer ${token}` },
        })
            .then((r) => r.json())
            .then((data) => {
                setNodes(Array.isArray(data) ? data : [])
                setLoading(false)
            })
            .catch((err) => {
                console.error(err)
                setLoading(false)
            })
    }, [token])

    const countryOptions = useMemo(() => {
        const options = new Set<string>()
        nodes.forEach((node) => {
            if (!node.country_code) return
            const normalized = node.country_code.trim().toUpperCase()
            if (/^[A-Z]{2}$/.test(normalized)) {
                options.add(normalized)
            }
        })
        return ['ANY', ...Array.from(options).sort()]
    }, [nodes])

    useEffect(() => {
        if (selectedCountry === 'ANY') return
        if (!countryOptions.includes(selectedCountry)) {
            setSelectedCountry('ANY')
        }
    }, [selectedCountry, countryOptions])

    const handlePin = async (nodeId: number, source: 'manual' | 'magic' = 'manual') => {
        if (!subId) return
        setPinning(nodeId)
        setMsg(null)

        try {
            const res = await fetch(apiUrl(`/api/client/subscription/${subId}/server`), {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ node_id: nodeId }),
            })

            if (res.ok) {
                setMsg({
                    type: 'success',
                    text: source === 'magic'
                        ? 'Автоподбор завершен. Маршрут обновляется, это дополнительная настройка.'
                        : 'Сервер сохранен. Подключение обновляется...',
                })
                setTimeout(() => {
                    navigate(`/subscription?sub=${subId}&connect=1${source === 'magic' ? '&optimized=1' : ''}`)
                }, 1400)
            } else {
                setMsg({ type: 'error', text: 'Не удалось закрепить сервер' })
            }
        } catch {
            setMsg({ type: 'error', text: 'Сетевая ошибка' })
        } finally {
            setPinning(null)
        }
    }

    const runMagicOptimize = async () => {
        if (!token || !subId || optimizing) return
        setOptimizing(true)
        setMsg(null)
        setRecommendedNodeId(null)

        try {
            const params = new URLSearchParams()
            params.set('strategy', selectedStrategy)
            if (selectedCountry !== 'ANY') {
                params.set('country', selectedCountry)
            }

            const query = params.toString()
            const res = await fetch(apiUrl(`/api/v2/client/recommended${query ? `?${query}` : ''}`), {
                headers: { Authorization: `Bearer ${token}` },
            })
            if (!res.ok) {
                setMsg({ type: 'error', text: 'Не удалось получить рекомендованные узлы' })
                return
            }

            const data = await res.json() as RecommendedResponse
            const topNodeId = data.nodes?.[0]?.id
            setRecommendedNodeId(topNodeId ?? null)
            if (!topNodeId) {
                setMsg({
                    type: 'error',
                    text: selectedCountry === 'ANY'
                        ? 'Рекомендации недоступны для текущей локации'
                        : `Нет доступных exit-узлов для ${selectedCountry}`,
                })
                return
            }

            await handlePin(topNodeId, 'magic')
        } catch {
            setMsg({ type: 'error', text: 'Сетевая ошибка во время магической оптимизации' })
        } finally {
            setOptimizing(false)
        }
    }

    useEffect(() => {
        if (hasAutoRunMagic.current) return
        if (searchParams.get('magic') !== '1') return
        if (loading) return
        hasAutoRunMagic.current = true
        void runMagicOptimize()
    }, [searchParams, loading])

    const sortedNodes = [...nodes].sort((a, b) => {
        const aOnline = a.status === 'online'
        const bOnline = b.status === 'online'
        if (aOnline !== bOnline) return Number(bOnline) - Number(aOnline)

        const aDistance = a.distance_km ?? Number.MAX_SAFE_INTEGER
        const bDistance = b.distance_km ?? Number.MAX_SAFE_INTEGER
        if (aDistance !== bDistance) return aDistance - bDistance

        return a.name.localeCompare(b.name)
    })

    const fallbackRecommendedNodeId = sortedNodes.find((node) => node.status === 'online')?.id ?? sortedNodes[0]?.id ?? null
    const activeRecommendedNodeId = recommendedNodeId ?? fallbackRecommendedNodeId

    if (loading) return <div className="page"><div className="loading">Проверка сети...</div></div>

    return (
        <div className="page server-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/subscription')}>{'<'}</button>
                <h2>Выбор сервера</h2>
                <span className="subtitle">Обычное подключение уже работает. Этот экран нужен только для дополнительной настройки.</span>
            </header>

            <section className="magic-optimize-card glass-card">
                <div className="magic-optimize-head">
                    <div>
                        <h3>Автоподбор маршрута</h3>
                        <p>Если хотите, система подберет сервер автоматически по задержке и доступности.</p>
                    </div>
                    <span className="optimize-optional-pill">Опционально</span>
                </div>
                <div className="optimize-controls">
                    <label className="optimize-control">
                        <span>Страна выхода</span>
                        <select
                            value={selectedCountry}
                            onChange={(event) => {
                                setSelectedCountry(event.target.value)
                                setRecommendedNodeId(null)
                            }}
                            disabled={optimizing || pinning !== null}
                        >
                            {countryOptions.map((country) => (
                                <option key={country} value={country}>
                                    {country === 'ANY' ? 'Авто (любая)' : country}
                                </option>
                            ))}
                        </select>
                    </label>
                    <label className="optimize-control">
                        <span>Стратегия</span>
                        <select
                            value={selectedStrategy}
                            onChange={(event) => {
                                setSelectedStrategy(normalizeStrategyParam(event.target.value))
                                setRecommendedNodeId(null)
                            }}
                            disabled={optimizing || pinning !== null}
                        >
                            {STRATEGY_OPTIONS.map((strategy) => (
                                <option key={strategy.value} value={strategy.value}>
                                    {strategy.label}
                                </option>
                            ))}
                        </select>
                    </label>
                </div>
                <button className="btn-secondary magic-optimize-cta" disabled={optimizing || pinning !== null} onClick={() => void runMagicOptimize()}>
                    {optimizing ? 'Подбираем...' : 'Подобрать автоматически'}
                </button>
            </section>

            {msg && <div className={`msg-banner ${msg.type}`}>{msg.text}</div>}

            <section className="manual-select-head glass-card">
                <h3>Ручной выбор</h3>
                <p>Нажмите сервер, если хотите закрепить конкретную точку подключения.</p>
            </section>

            <div className="server-list">
                {sortedNodes.map((node) => {
                    const isRecommended = node.id === activeRecommendedNodeId
                    return (
                    <div key={node.id} className={`server-card glass-card ${isRecommended ? 'recommended' : ''}`} onClick={() => void handlePin(node.id)}>
                        <div className="server-info">
                            <span className="server-flag">{node.flag}</span>
                            <div className="server-details">
                                <span className="server-name-row">
                                    <span className="server-name">{node.name}</span>
                                    {isRecommended && <span className="server-recommendation">Рекомендуем</span>}
                                </span>
                                {node.country_code && <span className="server-country">{node.country_code}</span>}
                            </div>
                        </div>

                        <div className="server-meta">
                            <span className={`server-status ${node.status === 'online' ? 'online' : 'offline'}`}>
                                {node.status === 'online' ? 'Онлайн' : 'Проверка'}
                            </span>
                            {node.distance_km !== null && (
                                <span className={`server-dist ${node.distance_km < 1000 ? 'good' : 'ok'}`}>
                                    {node.distance_km} km
                                </span>
                            )}
                            <button className="btn-select" disabled={pinning !== null || optimizing}>
                                {pinning === node.id ? 'Сохраняем...' : 'Использовать'}
                            </button>
                        </div>
                    </div>
                )})}

                {sortedNodes.length === 0 && (
                    <div className="empty-state">Доступные серверы пока не найдены</div>
                )}
            </div>

            <div className="info-box">
                <p>Если ничего не менять, подключение остается в стандартном режиме. Ручной выбор и автоподбор нужны только для точечной настройки.</p>
            </div>
        </div>
    )
}

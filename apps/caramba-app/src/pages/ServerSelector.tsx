import { useEffect, useRef, useState } from 'react'
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
    nodes?: Array<{ id: number; name: string }>
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
                        ? 'Магическая оптимизация завершена. Маршрут обновляется...'
                        : 'Узел закреплен. Конфигурация обновляется...',
                })
                setTimeout(() => {
                    navigate('/subscription')
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

        try {
            const res = await fetch(apiUrl('/api/v2/client/recommended'), {
                headers: { Authorization: `Bearer ${token}` },
            })
            if (!res.ok) {
                setMsg({ type: 'error', text: 'Не удалось получить рекомендованные узлы' })
                return
            }

            const data = await res.json() as RecommendedResponse
            const topNodeId = data.nodes?.[0]?.id
            if (!topNodeId) {
                setMsg({ type: 'error', text: 'Рекомендации недоступны для текущей локации' })
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

    if (loading) return <div className="page"><div className="loading">Проверка сети...</div></div>

    return (
        <div className="page server-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/subscription')}>{'<'}</button>
                <h2>Оптимизация соединения</h2>
                <span className="subtitle">Выберите узел вручную или запустите автооптимизацию</span>
            </header>

            <section className="magic-optimize-card glass-card">
                <div>
                    <h3>Магическая оптимизация</h3>
                    <p>Система выберет лучший узел по расстоянию, задержке и текущей нагрузке.</p>
                </div>
                <button className="btn-primary" disabled={optimizing || pinning !== null} onClick={() => void runMagicOptimize()}>
                    {optimizing ? 'Оптимизируем...' : 'Запустить магическую оптимизацию'}
                </button>
            </section>

            {msg && <div className={`msg-banner ${msg.type}`}>{msg.text}</div>}

            <div className="server-list">
                {nodes.map((node) => (
                    <div key={node.id} className="server-card glass-card" onClick={() => void handlePin(node.id)}>
                        <div className="server-info">
                            <span className="server-flag">{node.flag}</span>
                            <div className="server-details">
                                <span className="server-name">{node.name}</span>
                                {node.country_code && <span className="server-country">{node.country_code}</span>}
                            </div>
                        </div>

                        <div className="server-meta">
                            {node.distance_km !== null && (
                                <span className={`server-dist ${node.distance_km < 1000 ? 'good' : 'ok'}`}>
                                    {node.distance_km} km
                                </span>
                            )}
                            <button className="btn-select" disabled={pinning !== null || optimizing}>
                                {pinning === node.id ? 'Сохраняем...' : 'Выбрать'}
                            </button>
                        </div>
                    </div>
                ))}

                {nodes.length === 0 && (
                    <div className="empty-state">Доступные узлы не найдены</div>
                )}
            </div>

            <div className="info-box">
                <p>После выбора узла подписка закрепляется за ним, а ваши ссылки автоматически обновляются.</p>
            </div>
        </div>
    )
}

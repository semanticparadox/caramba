import { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useAuth } from '../context/AuthContext';
import './ServerSelector.css';

interface Node {
    id: number;
    country_code: string | null;
    flag: string;
    status: string;
    distance_km: number | null;
    name: string;
}

export default function ServerSelector() {
    const { subId } = useParams();
    const navigate = useNavigate();
    const { token } = useAuth();
    const [nodes, setNodes] = useState<Node[]>([]);
    const [loading, setLoading] = useState(true);
    const [pinning, setPinning] = useState<number | null>(null);
    const [msg, setMsg] = useState<{ type: 'success' | 'error', text: string } | null>(null);

    useEffect(() => {
        if (!token) return;
        fetch('/api/client/nodes', {
            headers: { Authorization: `Bearer ${token}` }
        })
            .then(r => r.json())
            .then(data => {
                setNodes(Array.isArray(data) ? data : []);
                setLoading(false);
            })
            .catch(err => {
                console.error(err);
                setLoading(false);
            });
    }, [token]);

    const handlePin = async (nodeId: number) => {
        if (!subId) return;
        setPinning(nodeId);
        setMsg(null);

        try {
            const res = await fetch(`/api/client/subscription/${subId}/server`, {
                method: 'POST',
                headers: {
                    'Authorization': `Bearer ${token}`,
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify({ node_id: nodeId })
            });

            if (res.ok) {
                setMsg({ type: 'success', text: '✅ Server pinned! Updating config...' });
                setTimeout(() => {
                    navigate('/subscription');
                }, 1500);
            } else {
                setMsg({ type: 'error', text: '❌ Failed to pin server' });
            }
        } catch (e) {
            setMsg({ type: 'error', text: 'Network error' });
        } finally {
            setPinning(null);
        }
    };

    if (loading) return <div className="page"><div className="loading">Checking network...</div></div>;

    return (
        <div className="page server-page">
            <header className="page-header">
                <button className="back-button" onClick={() => navigate('/subscription')}>←</button>
                <h2>Optimize Connection</h2>
                <span className="subtitle">Select the best server for you</span>
            </header>

            {msg && <div className={`msg-banner ${msg.type}`}>{msg.text}</div>}

            <div className="server-list">
                {nodes.map(node => (
                    <div key={node.id} className="server-card glass-card" onClick={() => handlePin(node.id)}>
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
                                    📏 {node.distance_km} km
                                </span>
                            )}
                            <button className="btn-select" disabled={pinning !== null}>
                                {pinning === node.id ? 'Saving...' : 'Select'}
                            </button>
                        </div>
                    </div>
                ))}

                {nodes.length === 0 && (
                    <div className="empty-state">No servers available</div>
                )}
            </div>

            <div className="info-box">
                <p>ℹ️ Selecting a server will pin your subscription to it. The system will automatically update your connection details.</p>
            </div>
        </div>
    );
}

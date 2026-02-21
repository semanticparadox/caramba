import { useNavigate } from 'react-router-dom'
import {
    Chart as ChartJS, ArcElement, Tooltip, Legend,
    CategoryScale, LinearScale, BarElement, Title, PointElement, LineElement,
} from 'chart.js'
import { Doughnut, Bar, Line } from 'react-chartjs-2'
import { useAuth } from '../context/AuthContext'
import { useState, useEffect } from 'react'
import './Statistics.css'

ChartJS.register(ArcElement, Tooltip, Legend, CategoryScale, LinearScale, BarElement, Title, PointElement, LineElement)

// Chart defaults for dark theme
ChartJS.defaults.color = 'rgba(255, 255, 255, 0.6)';

export default function Statistics() {
    const { userStats: stats, isLoading, token, error } = useAuth()
    const navigate = useNavigate()

    const [speeds, setSpeeds] = useState<number[]>(Array(10).fill(0))
    const [_lastTraffic, setLastTraffic] = useState<number>(0)

    useEffect(() => {
        if (!stats) return
        setLastTraffic(stats.traffic_used)
    }, [])

    useEffect(() => {
        if (!token) return;
        const interval = setInterval(async () => {
            try {
                const res = await fetch('/api/client/user/stats', {
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (res.ok) {
                    const data = await res.json();
                    setLastTraffic(prev => {
                        const diff = data.traffic_used - prev;
                        const speedBps = diff > 0 ? diff / 3 : 0;
                        setSpeeds(p => [...p.slice(1), speedBps]);
                        return data.traffic_used;
                    });
                }
            } catch (e) { console.error(e); }
        }, 3000)
        return () => clearInterval(interval)
    }, [token])

    const goBack = () => {
        if (window.history.length > 1) {
            navigate(-1)
        } else {
            navigate('/')
        }
    }

    if (isLoading) {
        return (
            <div className="page stats-page">
                <header className="page-header">
                    <button className="back-button" onClick={goBack}>←</button>
                    <h2>Statistics</h2>
                </header>
                <div className="loading">Loading stats...</div>
            </div>
        )
    }
    if (!stats) {
        return (
            <div className="page stats-page">
                <header className="page-header">
                    <button className="back-button" onClick={goBack}>←</button>
                    <h2>Statistics</h2>
                </header>
                <div className="empty-state">
                    <div className="empty-icon">🔐</div>
                    <h3>Statistics Unavailable</h3>
                    <p>{error || 'No active session. Reopen Mini App from the bot.'}</p>
                </div>
            </div>
        )
    }

    const usedGb = stats.traffic_used / 1024 / 1024 / 1024
    const limitGb = stats.traffic_limit / 1024 / 1024 / 1024
    const remainingGb = Math.max(0, limitGb - usedGb)

    const trafficData = {
        labels: ['Used', 'Remaining'],
        datasets: [{
            data: [usedGb, remainingGb],
            backgroundColor: ['rgba(124, 58, 237, 0.8)', 'rgba(59, 130, 246, 0.3)'],
            borderColor: ['#7C3AED', '#3B82F6'],
            borderWidth: 2,
        }],
    }

    const upDownData = {
        labels: ['Download', 'Upload'],
        datasets: [{
            label: 'Traffic (GB)',
            data: [
                (stats.total_download || 0) / 1024 / 1024 / 1024,
                (stats.total_upload || 0) / 1024 / 1024 / 1024
            ],
            backgroundColor: ['rgba(16, 185, 129, 0.7)', 'rgba(239, 68, 68, 0.7)'],
            borderRadius: 8,
        }]
    }

    const speedChartData = {
        labels: Array(10).fill('').map((_, i) => `${-3 * (9 - i)}s`),
        datasets: [{
            label: 'Speed (MB/s)',
            data: speeds.map(s => s / 1024 / 1024),
            borderColor: '#7C3AED',
            backgroundColor: 'rgba(124, 58, 237, 0.15)',
            tension: 0.4,
            fill: true,
            pointRadius: 0,
        }]
    }

    const chartOptions = {
        responsive: true,
        maintainAspectRatio: false,
        plugins: { legend: { display: false } },
        scales: {
            y: { beginAtZero: true, grid: { color: 'rgba(255,255,255,0.04)' }, ticks: { color: 'rgba(255,255,255,0.4)' } },
            x: { grid: { display: false }, ticks: { color: 'rgba(255,255,255,0.4)' } }
        }
    }

    return (
        <div className="page stats-page">
            <header className="page-header">
                <button className="back-button" onClick={goBack}>←</button>
                <h2>Statistics</h2>
            </header>

            <div className="stats-grid">
                <div className="stat-card glass-card">
                    <span className="stat-label">Plan</span>
                    <span className="stat-value gradient-text">{stats.plan_name}</span>
                </div>
                <div className="stat-card glass-card">
                    <span className="stat-label">Days Left</span>
                    <span className="stat-value">{stats.days_left}</span>
                </div>
            </div>

            <div className="chart-card glass-card">
                <h3>Live Speed</h3>
                <div className="chart-wrap">
                    <Line data={speedChartData} options={{
                        ...chartOptions,
                        plugins: {
                            ...chartOptions.plugins,
                            tooltip: { callbacks: { label: (ctx) => `${(ctx.parsed?.y ?? 0).toFixed(2)} MB/s` } }
                        }
                    }} />
                </div>
            </div>

            <div className="charts-row">
                <div className="chart-card glass-card">
                    <h3>Traffic</h3>
                    <div className="chart-wrap doughnut">
                        <Doughnut data={trafficData} options={{
                            responsive: true,
                            maintainAspectRatio: false,
                            plugins: { legend: { position: 'bottom', labels: { color: 'rgba(255,255,255,0.6)', padding: 12 } } },
                            cutout: '70%',
                        }} />
                    </div>
                    <p className="chart-footnote">{usedGb.toFixed(2)} / {limitGb > 0 ? limitGb.toFixed(2) + ' GB' : '∞'}</p>
                </div>

                <div className="chart-card glass-card">
                    <h3>Upload / Download</h3>
                    <div className="chart-wrap">
                        <Bar data={upDownData} options={chartOptions} />
                    </div>
                </div>
            </div>
        </div>
    )
}

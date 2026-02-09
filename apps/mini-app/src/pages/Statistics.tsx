import { useNavigate } from 'react-router-dom'
import {
    Chart as ChartJS,
    ArcElement,
    Tooltip,
    Legend,
    CategoryScale,
    LinearScale,
    BarElement,
    Title,
    PointElement,
    LineElement,
} from 'chart.js'
import { Doughnut, Bar, Line } from 'react-chartjs-2'
import { useAuth } from '../context/AuthContext'
import { useState, useEffect } from 'react'

ChartJS.register(
    ArcElement,
    Tooltip,
    Legend,
    CategoryScale,
    LinearScale,
    BarElement,
    Title,
    PointElement,
    LineElement
)

export default function Statistics() {
    const { userStats: stats, isLoading } = useAuth()
    const navigate = useNavigate()

    if (isLoading) return <div className="page"><div className="loading">Loading stats...</div></div>

    if (!stats) return <div className="page">{isLoading ? "Loading..." : "Failed to load statistics (No Auth?)"}</div>

    // Calculations
    const usedGb = stats.traffic_used / 1024 / 1024 / 1024
    const limitGb = stats.traffic_limit / 1024 / 1024 / 1024
    const remainingGb = Math.max(0, limitGb - usedGb)

    // Chart Data
    const trafficData = {
        labels: ['Used', 'Remaining'],
        datasets: [
            {
                data: [usedGb, remainingGb],
                backgroundColor: [
                    'rgba(255, 99, 132, 0.8)',
                    'rgba(54, 162, 235, 0.8)',
                ],
                borderColor: [
                    'rgba(255, 99, 132, 1)',
                    'rgba(54, 162, 235, 1)',
                ],
                borderWidth: 1,
            },
        ],
    }

    const [speeds, setSpeeds] = useState<number[]>(Array(10).fill(0))
    const [_lastTraffic, setLastTraffic] = useState<number>(0)
    const { token } = useAuth()

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
                    // data.traffic_used is total used bytes
                    // speed = (current - last) / 3s
                    setLastTraffic(prev => {
                        const diff = data.traffic_used - prev;
                        const speedBps = diff > 0 ? diff / 3 : 0;
                        // Convert to Mbps for display? or KB/s?
                        // Let's store bytes/sec and format in chart
                        setSpeeds(prevSpeeds => {
                            const newSpeeds = [...prevSpeeds.slice(1), speedBps];
                            return newSpeeds;
                        });
                        return data.traffic_used;
                    });
                }
            } catch (e) {
                console.error(e);
            }
        }, 3000)
        return () => clearInterval(interval)
    }, [token])

    const upDownData = {
        labels: ['Download', 'Upload'],
        datasets: [
            {
                label: 'Traffic (GB)',
                data: [
                    (stats.total_download || 0) / 1024 / 1024 / 1024,
                    (stats.total_upload || 0) / 1024 / 1024 / 1024
                ],
                backgroundColor: ['#4ade80', '#f87171'],
            }
        ]
    }

    const speedChartData = {
        labels: Array(10).fill('').map((_, i) => `${-3 * (9 - i)}s`),
        datasets: [
            {
                label: 'Speed (MB/s)',
                data: speeds.map(s => s / 1024 / 1024), // MB/s
                borderColor: '#6366f1',
                backgroundColor: 'rgba(99, 102, 241, 0.5)',
                tension: 0.4,
                fill: true,
            }
        ]
    }

    return (
        <div className="page stats-page">
            <div className="header">
                <h1>Statistics</h1>
                <button className="back-button" onClick={() => navigate('/')}>Back</button>
            </div>

            <div className="stats-grid">
                <div className="stat-card">
                    <h3>Current Plan</h3>
                    <p className="highlight">{stats.plan_name}</p>
                    <p className="subtext">{stats.days_left} days remaining</p>
                </div>

                {/* Live Speed Chart */}
                <div className="chart-container">
                    <h3>Live Speed</h3>
                    <div className="chart-wrapper">
                        <Line data={speedChartData} options={{
                            responsive: true,
                            plugins: {
                                legend: { display: false },
                                tooltip: {
                                    callbacks: {
                                        label: (ctx) => `${(ctx.parsed?.y ?? 0).toFixed(2)} MB/s`
                                    }
                                }
                            },
                            scales: {
                                y: {
                                    beginAtZero: true,
                                    grid: { color: 'rgba(255,255,255,0.1)' },
                                    ticks: { color: '#aaa' }
                                },
                                x: {
                                    grid: { display: false },
                                    ticks: { color: '#aaa' }
                                }
                            }
                        }} />
                    </div>
                </div>

                <div className="chart-container">
                    <h3>Traffic Usage</h3>
                    <div className="chart-wrapper">
                        <Doughnut data={trafficData} options={{
                            responsive: true,
                            plugins: {
                                legend: { position: 'bottom', labels: { color: '#fff' } }
                            }
                        }} />
                    </div>
                    <p className="chart-legend">
                        {usedGb.toFixed(2)} GB / {limitGb > 0 ? limitGb.toFixed(2) + ' GB' : '∞'}
                    </p>
                </div>

                <div className="chart-container">
                    <h3>Upload vs Download</h3>
                    <div className="chart-wrapper">
                        <Bar data={upDownData} options={{
                            responsive: true,
                            plugins: { legend: { display: false } },
                            scales: {
                                y: { ticks: { color: '#fff' }, grid: { color: 'rgba(255,255,255,0.1)' } },
                                x: { ticks: { color: '#fff' }, grid: { display: false } }
                            }
                        }} />
                    </div>
                </div>
            </div>

            <style>{`
                .stats-page {
                    padding: 20px;
                    color: white;
                    min-height: 100vh;
                    background: var(--bg-color, #1a1a1a);
                }
                .header {
                    display: flex;
                    justify-content: space-between;
                    align-items: center;
                    margin-bottom: 20px;
                }
                .back-button {
                    background: rgba(255,255,255,0.1);
                    border: none;
                    color: white;
                    padding: 8px 16px;
                    border-radius: 8px;
                    cursor: pointer;
                }
                .stats-grid {
                    display: grid;
                    gap: 20px;
                }
                .stat-card {
                    background: rgba(255,255,255,0.05);
                    padding: 20px;
                    border-radius: 16px;
                    text-align: center;
                }
                .highlight {
                    font-size: 24px;
                    font-weight: bold;
                    margin: 10px 0;
                    color: #4ade80;
                    margin-bottom: 5px;
                }
                .subtext {
                    color: #aaa;
                    font-size: 14px;
                }
                .chart-container {
                    background: rgba(255,255,255,0.05);
                    padding: 20px;
                    border-radius: 16px;
                }
                .chart-wrapper {
                    height: 250px;
                    display: flex;
                    justify-content: center;
                    align-items: center;
                }
                .chart-legend {
                    text-align: center;
                    margin-top: 10px;
                    font-weight: 500;
                }
            `}</style>
        </div>
    )
}

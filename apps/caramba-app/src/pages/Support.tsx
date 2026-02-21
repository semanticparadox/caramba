import { useNavigate } from 'react-router-dom'
import './Support.css'

const FAQS = [
    {
        q: "How do I connect?",
        a: "Go to Subscription, copy the link, and paste it into your VPN client (Hiddify, Sing-box, V2Ray, etc)."
    },
    {
        q: "Which server is fastest?",
        a: "Use the Servers page to see distances and find the closest server to your location."
    },
    {
        q: "How do I renew?",
        a: "Your subscription auto-renews if you have balance. Top up in the Billing section."
    },
    {
        q: "What VPN apps can I use?",
        a: "We support Sing-box, V2Ray/Xray, Clash, and Hiddify. Get your config from the Servers page."
    }
]

export default function Support() {
    const navigate = useNavigate()
    const goBack = () => {
        if (window.history.length > 1) {
            navigate(-1)
        } else {
            navigate('/')
        }
    }

    return (
        <div className="page support-page">
            <header className="page-header">
                <button className="back-button" onClick={goBack}>←</button>
                <h2>Support</h2>
            </header>

            <button className="contact-hero glass-card" onClick={() => window.open('https://t.me/SupportBot', '_blank')}>
                <span className="contact-icon">💬</span>
                <div>
                    <span className="contact-title">Chat with Support</span>
                    <span className="contact-desc">Get help from our team</span>
                </div>
                <span className="contact-arrow">→</span>
            </button>

            <div className="faq-section">
                <h3>FAQ</h3>
                <div className="faq-list">
                    {FAQS.map((faq, i) => (
                        <details key={i} className="faq-item glass-card">
                            <summary>{faq.q}</summary>
                            <p>{faq.a}</p>
                        </details>
                    ))}
                </div>
            </div>
        </div>
    )
}

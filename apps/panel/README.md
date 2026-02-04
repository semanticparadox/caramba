# CARAMBA Panel 🚀

CARAMBA is a high-performance, Rust-based control panel for managing VPN nodes, users, and subscriptions. It features a modern, responsive UI, automated payment processing, and advanced traffic management capabilities.

## ✨ Features

### 🖥️ User Interface
- **Modern Dashboard:** Real-time statistics, charts, and system status (Orange/Dark theme).
- **Responsive Design:** Fully mobile-friendly UI based on TailwindCSS.
- **Multi-Language Support:** Ready for localization.

### 🤖 Automation
- **Telegram Bot Integration:** Full control via Telegram bot (User & Admin).
- **Subscription Management:** Automated trials, renewals, and expiration handling.
- **Referral System:** Built-in referral tracking and bonus distribution.

### 💳 Payments (Multi-Gateway)
- **CrystalPay:** Crypto payments.
- **NowPayments:** Crypto payments.
- **Stripe:** Credit card processing.
- **Cryptomus:** (In Progress) Crypto payments.

### 🛡️ Security & Privacy
- **VPN Node Management:** Auto-deploy and manage nodes ensuring privacy.
- **Decoy Traffic:** Generate dummy traffic to mask VPN signatures.
- **Kill Switch:** Emergency cut-off if the panel connection is lost.

## 🛠️ Tech Stack

- **Backend:** Rust (Axum, Tokio, SQLx)
- **Database:** SQLite (Lightweight, fast)
- **Frontend:** HTML templates (Askama), TailwindCSS, Alpine.js
- **Charts:** ApexCharts.js
- **Infrastructure:** Docker & Docker Compose

## 🚀 Getting Started

### Prerequisites
- Docker & Docker Compose
- Rust Toolchain (for local dev)

### Installation

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/your-repo/caramba.git
    cd caramba
    ```

2.  **Configure Environment:**
    Copy `.env.example` to `.env` and fill in your API keys.
    ```bash
    cp .env.example .env
    nano .env
    ```

3.  **Run with Docker:**
    ```bash
    docker-compose up -d --build
    ```

4.  **Access the Panel:**
    - Admin Panel: `http://localhost:3000/admin` (Default: admin/admin)
    - Bot: Start your configured Telegram bot.

## ⚙️ Configuration

### Environment Variables
| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | SQLite path | `sqlite:exarobot.db` |
| `ADMIN_PATH` | Admin panel URL path | `/admin` |
| `BOT_TOKEN` | Telegram Bot Token | - |
| `PAYMENT_API_KEY` | Payment Gateway Key | - |

### Application Settings
Access `/admin/settings` to configure:
- **General:** Brand name, terms of service.
- **Payments:** Gateways, currency rates.
- **Trials:** Free trial duration, channel requirements.
- **System:** Decoy traffic, kill switch.

## 🤝 Contributing

1.  Fork the repo
2.  Create your feature branch (`git checkout -b feature/amazing-feature`)
3.  Commit your changes (`git commit -m 'Add amazing feature'`)
4.  Push to the branch (`git push origin feature/amazing-feature`)
5.  Open a Pull Request

## 📄 License
Private Property. All rights reserved.

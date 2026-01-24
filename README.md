# EXA ROBOT 🤖

**Next-Generation VPN Control Plane & Digital Store**

EXA ROBOT is a high-performance, Rust-based system for managing VPN infrastructure and selling digital goods via Telegram. Built with a modern microservices architecture, it combines a robust control panel, intelligent node agents, and a feature-rich Telegram bot into a unified platform.

![Version](https://img.shields.io/badge/version-0.5.0-blue)
![Rust](https://img.shields.io/badge/rust-2024-orange)
![Stack](https://img.shields.io/badge/stack-axum%20%7C%20sqlx%20%7C%20teloxide-green)
![License](https://img.shields.io/badge/license-MIT-green)

---

## ✨ Features

### �️ Architecture V2

**Modern Pull-Based Config Management:**
- **Panel** - Control plane with REST API
- **Agent** - Autonomous node daemon (Rust binary)
- **Config Sync** - Hash-based automatic synchronization
- **Zero SSH Dependency** - Agents pull configs via HTTPS

### �🏢 Admin Panel
- **Secure Access**: Configurable, obscured admin URL
- **Node Management**: 
  - One-click agent deployment
  - Real-time config synchronization
  - Automated Sing-box management (VLESS Reality / Hysteria 2)
- **User & Plan Management**:
  - Flexible plans (Time-based or Traffic-only)
  - Device limits and concurrent connection enforcement
  - Full user balance and subscription control
- **Dashboard**: Real-time traffic stats, revenue tracking, system health

### 🤖 Telegram Bot
- **Full-Cycle Store**: Browse plans, top up balance (CryptoBot/NowPayments), buy subscriptions
- **My Services**: View subscription status, traffic usage, connection links
- **Referral System**: Multi-level referral bonuses with custom aliases
- **Gift Codes**: Promo codes for marketing
- **Support**: Integrated support ticket system

### ⚙️ Core Technology
- **Performance**: Rust (Axum + Tokio) for minimal resource usage
- **Database**: SQLite with WAL mode — easy backups, zero setup
- **Frontend**: Server-Side Rendered (Askama) + HTMX for SPA-like experience
- **Security**: Zero-log policy, explicit device tracking
- **Architecture**: Cargo Workspace with shared types library

---

## 🚀 Quick Start

### One-Line Install (Recommended)

Install Panel and/or Agent directly from GitHub:

```bash
curl -sSL https://raw.githubusercontent.com/YOUR_USERNAME/exarobot/main/scripts/install.sh | sudo bash
```

**The installer will:**
1. Detect your OS (Debian 12+ / Ubuntu 22.04+)
2. Install dependencies (Rust, SQLite, build tools)
3. Prompt for component selection (Panel / Agent / Both)
4. Configure interactively (domain, tokens, etc.)
5. Build release binaries
6. Create systemd services
7. Start services automatically

**Default Admin Credentials:** `admin` / `admin123` ⚠️ **CHANGE IMMEDIATELY!**

---

## 📦 Manual Installation

### Prerequisites
- Debian 12+ or Ubuntu 22.04+
- Root access
- Git and Rust toolchain

### Panel Installation
```bash
# Clone repository
git clone https://github.com/YOUR_USERNAME/exarobot.git /opt/exarobot
cd /opt/exarobot

# Build panel
cargo build -p exarobot --release

# Setup
mkdir -p /opt/exarobot/panel
cp target/release/exarobot /opt/exarobot/panel/
cp .env.panel.example /opt/exarobot/panel/.env
nano /opt/exarobot/panel/.env  # Edit configuration

# Create systemd service
cp scripts/systemd/exarobot-panel.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now exarobot-panel
```

**Access panel at:** `https://your-domain.com/admin`

### Agent Installation (on VPN nodes)
```bash
# Build agent
cd /opt/exarobot
cargo build -p exarobot-agent --release

# Setup
mkdir -p /opt/exarobot/agent
cp target/release/exarobot-agent /opt/exarobot/agent/
cp .env.agent.example /opt/exarobot/agent/.env
nano /opt/exarobot/agent/.env  # Add panel URL and node token

# Install sing-box
curl -fsSL https://sing-box.app/gpg.key -o /etc/apt/keyrings/sagernet.asc
chmod a+r /etc/apt/keyrings/sagernet.asc
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/sagernet.asc] https://deb.sagernet.org/ * *" | \
    tee /etc/apt/sources.list.d/sagernet.list > /dev/null
apt update && apt install -y sing-box

# Create systemd service
cp scripts/systemd/exarobot-agent.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now exarobot-agent
```

---

## 🔧 Service Management

### Panel Commands
```bash
systemctl status exarobot-panel   # Check status
systemctl restart exarobot-panel  # Restart service
journalctl -u exarobot-panel -f   # View logs
```

### Agent Commands
```bash
systemctl status exarobot-agent   # Check status
systemctl restart exarobot-agent  # Restart service
journalctl -u exarobot-agent -f   # View logs
```

### Admin CLI
```bash
# Reset admin password
./exarobot admin reset-password admin NEW_PASSWORD

# Show panel info
./exarobot admin info
```

---

## 🌐 Node Management (V2 Architecture)

### How It Works

**Traditional Approach (V1):**
- Panel pushes config to nodes via SSH
- Requires open SSH ports
- Firewall/NAT issues

**Modern Approach (V2 - Current):**
- Agents pull config from panel via HTTPS API
- Works behind NAT/firewall
- No SSH credentials needed
- Automatic sync on config changes

### Adding a Node

1. **In Panel Admin:**
   - Go to **Nodes > Add Node**
   - Enter: Name, Description
   - Click **Save**
   - Copy the **Join Token**

2. **On Node Server:**
   ```bash
   # Option 1: Use installer
   curl -sSL https://raw.githubusercontent.com/YOU/exarobot/main/scripts/install.sh | sudo bash
   # Select "Agent", paste join token when prompted
   
   # Option 2: Manual
   # Download agent binary + create .env with PANEL_URL and NODE_TOKEN
   # Install systemd service
   ```

3. **Agent will automatically:**
   - Connect to panel
   - Fetch sing-box configuration
   - Start sing-box service
   - Report status every 10 seconds
   - Update config when changed

---

## 🛠 Development

### Project Structure (Cargo Workspace)
```
exarobot/
├── apps/
│   ├── panel/              # Control panel (Axum + Askama)
│   │   ├── src/
│   │   │   ├── main.rs       # Entry point & router
│   │   │   ├── handlers/     # Web endpoints
│   │   │   ├── bot/          # Telegram bot (Teloxide)
│   │   │   ├── services/     # Core logic (User, Store, Payments)
│   │   │   ├── models/       # Database structs (SQLx)
│   │   │   └── api/          # V2 REST API
│   │   ├── templates/        # Askama HTML templates
│   │   └── migrations/       # SQLite migrations
│   │
│   └── agent/              # Node agent daemon
│       └── src/
│           └── main.rs       # Config sync + process manager
│
├── libs/
│   └── shared/             # Shared types (Request/Response DTOs)
│
└── scripts/
    ├── install.sh          # Universal installer
    ├── uninstall.sh        # Uninstaller
    └── systemd/            # Service units
```

### Running Locally
```bash
# Setup environment
cp .env.panel.example .env
nano .env  # Edit DATABASE_URL, etc.

# Run panel
cargo run -p exarobot

# Run agent (in another terminal)
cargo run -p exarobot-agent
```

**Access:** `http://127.0.0.1:3000/admin/login`

### Building Release
```bash
# Build all components
cargo build --release --workspace

# Binaries:
# - target/release/exarobot (Panel)
# - target/release/exarobot-agent (Agent)
```

---

## 📊 Tech Stack Details

| Component | Technology | Purpose |
|-----------|-----------|---------|
| **Backend** | Rust (Axum) | HTTP server & routing |
| **Database** | SQLite (sqlx) | Data persistence |
| **Frontend** | Askama + HTMX | Server-side rendering |
| **Bot** | Teloxide | Telegram integration |
| **Async Runtime** | Tokio | Concurrency |
| **Payments** | CryptoBot, NowPayments | Crypto payments |
| **VPN** | Sing-box | VLESS Reality, Hysteria 2 |

---

## 🔒 Security Features

- **Zero-Log Policy**: No IP logging in production mode
- **Obscured Admin Path**: Configurable URL to avoid scanners
- **Session Management**: Secure cookie-based auth
- **SQL Injection Protection**: Prepared statements via sqlx
- **XSS Protection**: Askama template escaping
- **HTTPS Enforcement**: TLS required for production

---

## � Environment Variables

### Panel (.env)
```bash
SERVER_DOMAIN=panel.example.com
ADMIN_PATH=/admin
DATABASE_URL=sqlite:///opt/exarobot/panel/db.sqlite
BOT_TOKEN=your_telegram_bot_token
PAYMENT_API_KEY=cryptobot_api_key
NOWPAYMENTS_KEY=nowpayments_api_key
```

### Agent (.env)
```bash
PANEL_URL=https://panel.example.com
NODE_TOKEN=join_token_from_panel
CONFIG_PATH=/etc/sing-box/config.json
```

---

## 🗺️ Roadmap

### Completed ✅
- [x] Panel + Bot integration
- [x] Multi-protocol support (VLESS, Hysteria2)
- [x] Telegram Store & Payments
- [x] Device limit enforcement
- [x] Cargo Workspace architecture
- [x] Agent V2 (Pull-based config)
- [x] Universal installer

### Planned 🚀
- [ ] Multi-panel federation
- [ ] Grafana/Prometheus integration
- [ ] Web-based config editor
- [ ] Docker images
- [ ] CDN support for assets

---

## 📚 Documentation

- [Installation Guide](scripts/install.sh)
- [Environment Templates](.env.panel.example)
- [API Documentation](apps/panel/src/api/)
- [Systemd Services](scripts/systemd/)

---

## 🤝 Contributing

Contributions are welcome! Please:
1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Submit a pull request

---

## ⚖️ License

MIT License - see LICENSE file for details.

---

## 🙏 Acknowledgments

- [Sing-box](https://sing-box.sagernet.org/) - Modern VPN core
- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [Teloxide](https://github.com/teloxide/teloxide) - Telegram bot framework
- Inspired by [Remnawave](https://github.com/remnawave/backend) and [Hiddify](https://github.com/hiddify/hiddify-manager)

---

**Made with ❤️ in Rust**

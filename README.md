# EXA ROBOT: Advanced VPN Panel & Bot

**EXA ROBOT** is a high-performance, censorship-resistant VPN management system built with Rust. It leverages Sing-box, Reality, and Hysteria 2 to provide a robust, anti-censorship solution.

## 🚀 Key Features

*   **V3 Architecture:** "Disposable Front, Stable Back" design for maximum censorship resilience.
*   **Modular:** 5 core binaries (`panel`, `agent`, `bot`, `sub`, `miniapp`) for flexible deployment.
*   **High Performance:** Written in Rust for speed and low resource usage.
*   **Secure:** Admin panel hidden behind private networks; only disposable services face the public.
*   **Automated:** Auto-updates via GitHub Releases, automated node management.

## 📚 Documentation

The full documentation is available in the `docs/` directory:

- [**Deployment Guide**](docs/DEPLOYMENT.md): Installation instructions.
- [**Configuration**](docs/CONFIGURATION.md): Environment variables reference.
- [**Development**](docs/DEVELOPMENT.md): Build and test instructions.
- [**Architecture**](docs/MODULES.md): Codebase structure.
- [**Database**](docs/DATABASE.md): Schema details.
- [**API Reference**](docs/API.md): Integrations.

---

## 📦 Installation

**Security Notice:** Always verify the GPG signature of the installer.

```bash
# 1. Download
curl -sSLO https://raw.githubusercontent.com/semanticparadox/CARAMBA/main/scripts/install.sh
curl -sSLO https://raw.githubusercontent.com/semanticparadox/CARAMBA/main/scripts/install.sh.asc
curl -sSL https://raw.githubusercontent.com/semanticparadox/CARAMBA/main/gpg-key.asc | gpg --import

# 2. Verify
gpg --verify install.sh.asc install.sh

# 3. Install
sudo bash install.sh
```

### Quick One-Liner (Less Secure)
```bash
curl -sSL https://raw.githubusercontent.com/semanticparadox/CARAMBA/main/scripts/install.sh | sudo bash
```

---

## 🛠 Project Structure

*   `apps/panel`: The Core Brain (API, DB, Admin UI).
*   `apps/agent`: The Node Logic (Sing-box manager).
*   `apps/bot` (Planned): Separate Telegram Bot binary.
*   `apps/sub-link` (Planned): High-performance subscription distributor.
*   `apps/mini-app`: React-based User Frontend.

---

## 🔧 Deployment Strategies

### Monolith (Simple)
Install everything on one server. Good for testing or small deployments.

### Distributed (Anti-Censorship)
*   **Panel:** Hosted on a secure, private server (The Bunker).
*   **Bot:** Hosted on any cheap VPS.
*   **Subscription Service:** Hosted on disposable VPS or Edge.
*   **Nodes:** Distributed globally.

See [docs/02_censorship.md](docs/02_censorship.md) for details.

---

## License
Proprietary / Closed Source.

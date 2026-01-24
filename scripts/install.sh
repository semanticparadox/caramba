#!/bin/bash
# ==================================================
# EXA ROBOT Universal Installer
# ==================================================
# Installs Panel and/or Agent from GitHub
# Target OS: Debian 12+
# ==================================================

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# Functions
log_info() { echo -e "${CYAN}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

check_root() {
    if [ "$EUID" -ne 0 ]; then
        log_error "Please run as root"
        exit 1
    fi
}

detect_os() {
    if [ ! -f /etc/os-release ]; then
        log_error "Cannot detect OS"
        exit 1
    fi
    
    source /etc/os-release
    
    if [[ "$ID" != "debian" && "$ID" != "ubuntu" ]]; then
        log_warn "This script is designed for Debian/Ubuntu"
        log_warn "Your OS: $ID $VERSION_ID"
        read -p "Continue anyway? (y/N): " choice
        [[ "$choice" != "y" ]] && exit 1
    fi
    
    log_success "OS detected: $PRETTY_NAME"
}

install_dependencies() {
    log_info "Installing dependencies..."
    
    apt-get update -qq
    apt-get install -y curl git build-essential pkg-config libssl-dev sqlite3 -qq
    
    # Install Rust if not present
    if ! command -v cargo &> /dev/null; then
        log_info "Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
        log_success "Rust installed"
    else
        log_success "Rust already installed"
    fi
}

clone_repository() {
    REPO_URL="https://github.com/semanticparadox/EXA-ROBOT.git"
    INSTALL_DIR="/opt/exarobot"
    
    if [ -d "$INSTALL_DIR/.git" ]; then
        log_info "Repository exists, pulling latest..."
        cd "$INSTALL_DIR"
        git pull
    else
        log_info "Cloning repository..."
        git clone "$REPO_URL" "$INSTALL_DIR"
        cd "$INSTALL_DIR"
    fi
    
    log_success "Repository ready at $INSTALL_DIR"
}

install_panel() {
    log_info "=== Installing Panel ==="
    
    # Prompt for configuration
    read -p "Enter server domain (e.g. panel.example.com): " DOMAIN </dev/tty
    read -p "Enter admin path [/admin]: " ADMIN_PATH </dev/tty
    ADMIN_PATH=${ADMIN_PATH:-/admin}
    
    # Build panel
    log_info "Building panel (this may take a while)..."
    cd /opt/exarobot
    export SQLX_OFFLINE=true
    cargo build -p exarobot --release --quiet
    
    # Create directories
    mkdir -p /opt/exarobot/panel
    cp target/release/exarobot /opt/exarobot/panel/
    
    # Create .env
    cat > /opt/exarobot/panel/.env <<EOF
SERVER_DOMAIN=$DOMAIN
ADMIN_PATH=$ADMIN_PATH
DATABASE_URL=sqlite:///opt/exarobot/panel/db.sqlite
BOT_TOKEN=
PAYMENT_API_KEY=
NOWPAYMENTS_KEY=
EOF
    
    # Create systemd service
    cat > /etc/systemd/system/exarobot-panel.service <<EOF
[Unit]
Description=EXA ROBOT VPN Control Panel
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/exarobot/panel
ExecStart=/opt/exarobot/panel/exarobot serve
Restart=always
RestartSec=5s
EnvironmentFile=/opt/exarobot/panel/.env

[Install]
WantedBy=multi-user.target
EOF
    
    # Initialize database
    log_info "Initializing database..."
    cd /opt/exarobot/panel
    ./exarobot admin reset-password admin admin123 || true
    
    # Start service
    systemctl daemon-reload
    systemctl enable exarobot-panel
    systemctl restart exarobot-panel
    
    log_success "Panel installed and started"
    log_info "Access at: https://$DOMAIN$ADMIN_PATH"
    log_info "Default credentials: admin / admin123"
    log_warn "CHANGE PASSWORD IMMEDIATELY!"
}

install_agent() {
    log_info "=== Installing Agent ==="
    
    # Prompt for configuration
    read -p "Enter Panel URL (e.g. https://panel.example.com): " PANEL_URL </dev/tty
    read -p "Enter Node Token (from panel): " NODE_TOKEN </dev/tty
    
    # Build agent
    log_info "Building agent..."
    cd /opt/exarobot
    cargo build -p exarobot-agent --release --quiet
    
    # Create directories
    mkdir -p /opt/exarobot/agent
    cp target/release/exarobot-agent /opt/exarobot/agent/
    
    # Create .env
    cat > /opt/exarobot/agent/.env <<EOF
PANEL_URL=$PANEL_URL
NODE_TOKEN=$NODE_TOKEN
CONFIG_PATH=/etc/sing-box/config.json
EOF
    
    # Install sing-box
    log_info "Installing sing-box..."
    curl -fsSL https://sing-box.app/gpg.key -o /etc/apt/keyrings/sagernet.asc
    chmod a+r /etc/apt/keyrings/sagernet.asc
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/sagernet.asc] https://deb.sagernet.org/ * *" | \
        tee /etc/apt/sources.list.d/sagernet.list > /dev/null
    apt-get update -qq
    apt-get install -y sing-box -qq
    systemctl stop sing-box
    systemctl disable sing-box
    
    # Create systemd service
    cat > /etc/systemd/system/exarobot-agent.service <<EOF
[Unit]
Description=EXA ROBOT Node Agent
After=network.target
Before=sing-box.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/exarobot/agent
ExecStart=/opt/exarobot/agent/exarobot-agent
Restart=always
RestartSec=10s
EnvironmentFile=/opt/exarobot/agent/.env

[Install]
WantedBy=multi-user.target
EOF
    
    # Create sing-box service override
    mkdir -p /etc/systemd/system/sing-box.service.d
    cat > /etc/systemd/system/sing-box.service.d/override.conf <<EOF
[Unit]
After=exarobot-agent.service
Wants=exarobot-agent.service

[Service]
ExecStart=
ExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json
EOF
    
    # Start service
    systemctl daemon-reload
    systemctl enable exarobot-agent
    systemctl start exarobot-agent
    
    log_success "Agent installed and started"
    log_info "Agent will fetch config from panel automatically"
}

main() {
    echo -e "${CYAN}===================================================${NC}"
    echo -e "${CYAN}     EXA ROBOT Universal Installer                ${NC}"
    echo -e "${CYAN}===================================================${NC}"
    echo ""
    
    check_root
    detect_os
    install_dependencies
    clone_repository
    
    echo ""
    echo "Select components to install:"
    echo "1) Panel only"
    echo "2) Agent only"
    echo "3) Both Panel and Agent"
    read -p "Choice [1-3]: " CHOICE </dev/tty
    
    case $CHOICE in
        1)
            install_panel
            ;;
        2)
            install_agent
            ;;
        3)
            install_panel
            echo ""
            install_agent
            ;;
        *)
            log_error "Invalid choice"
            exit 1
            ;;
    esac
    
    echo ""
    log_success "Installation complete!"
    echo ""
    echo "Useful commands:"
    if [[ "$CHOICE" == "1" || "$CHOICE" == "3" ]]; then
        echo "  Panel status:  systemctl status exarobot-panel"
        echo "  Panel logs:    journalctl -u exarobot-panel -f"
    fi
    if [[ "$CHOICE" == "2" || "$CHOICE" == "3" ]]; then
        echo "  Agent status:  systemctl status exarobot-agent"
        echo "  Agent logs:    journalctl -u exarobot-agent -f"
    fi
    echo ""
}

main "$@"

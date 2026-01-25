#!/bin/bash
# ==================================================
# ExaRobot Universal Installer
# ==================================================
# Supports:
# - Clean install (binaries only) via --clean
# - Source install (default)
# - Panel/Agent/Both via --role
# ==================================================

set -e

# Configuration
REPO_URL="https://github.com/semanticparadox/EXA-ROBOT.git"
INSTALL_DIR="/opt/exarobot"
TEMP_BUILD_DIR="/tmp/exarobot_build"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# Defaults
CLEAN_INSTALL=false
ROLE=""
PANEL_URL=""
NODE_TOKEN=""
DOMAIN=""
ADMIN_PATH="" # Default empty to force prompt or use intelligent default later
FORCE_INSTALL=false

# --------------------------------------------------
# Logging
# --------------------------------------------------
log_info() { echo -e "${CYAN}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# --------------------------------------------------
# Pre-checks
# --------------------------------------------------
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
        log_warn "Designed for Debian/Ubuntu. Found: $ID"
        read -p "Continue anyway? (y/N): " choice
        [[ "$choice" != "y" ]] && exit 1
    fi
}

# --------------------------------------------------
# Dependency Installation
# --------------------------------------------------
install_dependencies() {
    log_info "Installing dependencies..."
    setup_firewall
    
    apt-get update -qq
    apt-get install -y curl git build-essential pkg-config libssl-dev sqlite3 -qq
    
    if ! command -v cargo &> /dev/null; then
        log_info "Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    else
        log_success "Rust already installed"
    fi
}

setup_firewall() {
    if command -v ufw &> /dev/null; then
        ufw allow 22/tcp
        ufw allow 80/tcp
        ufw allow 443/tcp
        ufw allow 9090/tcp # Hysteria/VLESS default range start?
        # Panel port opened later if needed
    fi
}

# --------------------------------------------------
# Conflict Detection
# --------------------------------------------------
check_conflicts() {
    local clash=false
    
    # Check if services are running
    if systemctl is-active --quiet exarobot-panel; then
        log_warn "Service 'exarobot-panel' is currently running."
        clash=true
    fi
    if systemctl is-active --quiet exarobot-agent; then
        log_warn "Service 'exarobot-agent' is currently running."
        clash=true
    fi
    
    # Check Ports (only if we know them, e.g. default 3000)
    # If lsof or ss is available
    if command -v ss &> /dev/null; then
        if ss -tuln | grep -q ":3000 "; then
             log_warn "Port 3000 is in use (Panel default)."
             clash=true
        fi
    fi
    
    if [ "$clash" = true ]; then
        echo ""
        echo -e "${RED}Conflicts detected!${NC}"
        echo "It seems a previous installation or another service is running."
        
        OVERWRITE="n"
        if [ "$FORCE_INSTALL" = true ]; then
            log_warn "Force flag detected. Overwriting..."
            OVERWRITE="y"
        else
            # Use set +e to prevent exit on read failure
            set +e
            # Force read from TTY specifically, separate echo for safety
            if [ -t 0 ]; then
                read -p "Do you want to stop existing services, DELETE DATA, and overwrite? (y/N): " OVERWRITE
            else
                 # Try direct TTY access
                 echo -n "Do you want to stop existing services, DELETE DATA, and overwrite? (y/N): "
                 read -r OVERWRITE < /dev/tty
            fi
            set -e
        fi

        if [[ "$OVERWRITE" == "y" || "$OVERWRITE" == "Y" ]]; then
            log_info "Stopping and removing existing services..."
            
            # Stop Services
            # Stop Services
            systemctl stop exarobot-panel &> /dev/null || true
            systemctl disable exarobot-panel &> /dev/null || true
            # Also stop "exarobot" alias if used
            systemctl stop exarobot &> /dev/null || true
            systemctl disable exarobot &> /dev/null || true
            rm -f /etc/systemd/system/exarobot-panel.service
            rm -f /etc/systemd/system/exarobot.service
            
            systemctl stop exarobot-agent &> /dev/null || true
            systemctl disable exarobot-agent &> /dev/null || true
            rm -f /etc/systemd/system/exarobot-agent.service
            
            # Remove Sing-box (if requested by user "remove sing-box also")
            # We assume if we are overwriting agent, we might want to clean sing-box too?
            # User specifically asked for this.
            if command -v sing-box &> /dev/null; then
                 log_info "Removing Sing-box..."
                 systemctl stop sing-box || true
                 systemctl disable sing-box || true
                 apt-get remove -y sing-box || true
                 rm -rf /etc/sing-box
                 rm -f /etc/systemd/system/sing-box.service.d/override.conf
            fi

            # Kill Ports
            if command -v fuser &> /dev/null; then
                 fuser -k 3000/tcp || true
            fi
            
            # Remove Files - SAFELY
            # Move out of the directory first to avoid 'getcwd' errors if running from there
            cd /tmp || exit 1
            
            log_info "Removing installation directory..."
            rm -rf "$INSTALL_DIR"
            
            systemctl daemon-reload
            log_success "Cleanup complete. Proceeding with fresh install..."
        else
            log_error "Installation aborted by user."
            exit 1
        fi
    fi
}


# --------------------------------------------------
# Building
# --------------------------------------------------
build_binaries() {
    local target_role=$1
    local src_dir=$2
    
    log_info "Building binaries from $src_dir..."
    cd "$src_dir"
    
    
    # Check if cargo works
    if ! command -v cargo &> /dev/null; then
        # Try sourcing again if lost
        if [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
    fi
    
    # Init dummy DB for build macros if needed
    if [[ "$target_role" == "panel" || "$target_role" == "both" ]]; then
        if [ ! -f "build_db.sqlite" ]; then
            touch build_db.sqlite
            export DATABASE_URL="sqlite://build_db.sqlite"
            if [ -f "apps/panel/migrations/001_complete_schema.sql" ]; then
                sqlite3 build_db.sqlite < apps/panel/migrations/001_complete_schema.sql
            fi
        fi
        
        log_info "Compiling Panel (this may take a few minutes)..."
        # Removed --quiet to show progress
        cargo build -p exarobot --release
    fi
    
    if [[ "$target_role" == "agent" || "$target_role" == "both" ]]; then
        log_info "Compiling Agent (this may take a few minutes)..."
        # Removed --quiet
        cargo build -p exarobot-agent --release
    fi
}

# --------------------------------------------------
# Installation Logic
# --------------------------------------------------
setup_directory() {
    mkdir -p "$INSTALL_DIR"
    # Files are kept in root: /opt/exarobot/{exarobot, exarobot-agent, .env, .env.agent}
}

configure_panel() {
    # Interactive Prompts
    if [[ -z "$DOMAIN" ]]; then

        if [ -t 0 ]; then
            read -p "Enter server domain (e.g. panel.example.com): " DOMAIN
        else
            echo -n "Enter server domain (e.g. panel.example.com): "
            read -r DOMAIN < /dev/tty
        fi
    fi
    if [[ -z "$PANEL_PORT" ]]; then

        if [ -t 0 ]; then
             read -p "Enter Panel Port [3000]: " PANEL_PORT
        else
             echo -n "Enter Panel Port [3000]: "
             read -r PANEL_PORT < /dev/tty
        fi
        PANEL_PORT=${PANEL_PORT:-3000}
    fi
    
    if [[ -z "$ADMIN_PATH" ]]; then
        if [ -t 0 ]; then
             read -p "Enter Admin Path (e.g. /admin): " ADMIN_PATH
        else
             echo -n "Enter Admin Path (e.g. /admin): "
             read -r ADMIN_PATH < /dev/tty
        fi
        ADMIN_PATH=${ADMIN_PATH:-/admin}
    fi
     # Ensure leading slash
    [[ "${ADMIN_PATH}" != /* ]] && ADMIN_PATH="/${ADMIN_PATH}"
    
    # Firewall
    if command -v ufw &> /dev/null; then
        ufw allow $PANEL_PORT/tcp
    fi
    
    # Environment File
    # Check if exists to avoid overwrite?
    ENV_FILE="$INSTALL_DIR/.env"
    if [ ! -f "$ENV_FILE" ]; then
        log_info "Creating $ENV_FILE..."
        cat > "$ENV_FILE" <<EOF
SERVER_DOMAIN=$DOMAIN
ADMIN_PATH=$ADMIN_PATH
PANEL_PORT=$PANEL_PORT
DATABASE_URL=sqlite://$INSTALL_DIR/exarobot.db
BOT_TOKEN=
PAYMENT_API_KEY=
NOWPAYMENTS_KEY=
EOF
    else
        log_warn "$ENV_FILE exists. Skipping creation."
    fi
    
    # Database
    DB_FILE="$INSTALL_DIR/exarobot.db"
    if [ ! -f "$DB_FILE" ]; then
        log_info "Initializing database..."
        touch "$DB_FILE"
        # We rely on binary embedded migrations usually, but for safety lets try apply schema if we have source
        # If clean install, we might not have migrations folder handy easily unless we use the temp path
        if [ -f "$TEMP_BUILD_DIR/apps/panel/migrations/001_complete_schema.sql" ]; then
             sqlite3 "$DB_FILE" < "$TEMP_BUILD_DIR/apps/panel/migrations/001_complete_schema.sql"
        elif [ -f "$INSTALL_DIR/source/apps/panel/migrations/001_complete_schema.sql" ]; then
             sqlite3 "$DB_FILE" < "$INSTALL_DIR/source/apps/panel/migrations/001_complete_schema.sql"
        fi
    fi
    
    # Service
    cat > /etc/systemd/system/exarobot-panel.service <<EOF
[Unit]
Description=VPN Control Panel
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=$INSTALL_DIR
ExecStart=$INSTALL_DIR/exarobot serve
Restart=always
RestartSec=5s
EnvironmentFile=$ENV_FILE
# ADMIN_PATH and other vars are loaded from EnvironmentFile, NOT hardcoded here!

[Install]
WantedBy=multi-user.target
EOF

    systemctl daemon-reload
    systemctl enable exarobot-panel
    systemctl restart exarobot-panel
    log_success "Panel installed. Access: https://$DOMAIN$ADMIN_PATH/login"
    echo ""
    echo -e "${YELLOW}IMPORTANT: Create your first Admin User:${NC}"
    echo "  cd $INSTALL_DIR"
    echo "  ./exarobot admin reset-password admin <your-password>"
    echo ""
}

configure_agent() {
    if [[ -z "$PANEL_URL" ]]; then

        if [ -t 0 ]; then
            read -p "Enter Panel URL (e.g. https://panel.example.com): " PANEL_URL
        else
            echo -n "Enter Panel URL (e.g. https://panel.example.com): "
            read -r PANEL_URL < /dev/tty
        fi
    fi
    if [[ -z "$NODE_TOKEN" ]]; then

        if [ -t 0 ]; then
             read -p "Enter Node Token: " NODE_TOKEN
        else
             echo -n "Enter Node Token: "
             read -r NODE_TOKEN < /dev/tty
        fi
    fi
    
    # Agent Env
    AGENT_ENV="$INSTALL_DIR/.env.agent"
    cat > "$AGENT_ENV" <<EOF
PANEL_URL=$PANEL_URL
NODE_TOKEN=$NODE_TOKEN
CONFIG_PATH=/etc/sing-box/config.json
EOF

    # Sing-box
    if ! command -v sing-box &> /dev/null; then
        log_info "Installing sing-box..."
         curl -fsSL https://sing-box.app/gpg.key -o /etc/apt/keyrings/sagernet.asc
        chmod a+r /etc/apt/keyrings/sagernet.asc
        echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/sagernet.asc] https://deb.sagernet.org/ * *" | \
            tee /etc/apt/sources.list.d/sagernet.list > /dev/null
        apt-get update -qq
        apt-get install -y sing-box -qq
        systemctl stop sing-box
        systemctl disable sing-box
    fi
    
    # Service
    # Note: We override sing-box service dependency
    mkdir -p /etc/systemd/system/sing-box.service.d
    cat > /etc/systemd/system/sing-box.service.d/override.conf <<EOF
[Unit]
After=exarobot-agent.service
Wants=exarobot-agent.service

[Service]
ExecStart=
ExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json
EOF

    cat > /etc/systemd/system/exarobot-agent.service <<EOF
[Unit]
Description=VPN Node Agent
After=network.target
Before=sing-box.service

[Service]
Type=simple
User=root
WorkingDirectory=$INSTALL_DIR
ExecStart=$INSTALL_DIR/exarobot-agent
Restart=always
RestartSec=10s
EnvironmentFile=$AGENT_ENV

[Install]
WantedBy=multi-user.target
EOF

    systemctl daemon-reload
    systemctl enable exarobot-agent
    systemctl restart exarobot-agent
    log_success "Agent installed."
}

# --------------------------------------------------
# Main Logic
# --------------------------------------------------
main() {
    check_root
    detect_os
    
    # Parse Args
    while [[ "$#" -gt 0 ]]; do
        case $1 in
            --clean) CLEAN_INSTALL=true ;;
            --role) ROLE="$2"; shift ;;
            --panel) PANEL_URL="$2"; shift ;;
            --token) NODE_TOKEN="$2"; shift ;;
            --domain) DOMAIN="$2"; shift ;;
            --admin-path) ADMIN_PATH="$2"; shift ;;
            --force) FORCE_INSTALL=true ;;
            *) echo "Unknown parameter: $1"; exit 1 ;;
        esac
        shift
    done
    
    if [[ -z "$ROLE" ]]; then
        echo "Select installation role:"
        echo "1) Panel"
        echo "2) Agent"
        echo "3) Both"
        # Robust read
        set +e
        if [ -t 0 ]; then
            read -p "Choice [1-3]: " C
        else
            echo -n "Choice [1-3]: "
            read -r C < /dev/tty
        fi
        set -e
        case $C in
            1) ROLE="panel" ;;
            2) ROLE="agent" ;;
            3) ROLE="both" ;;
            *) exit 1 ;;
        esac
    fi
    
    
    check_conflicts
    
    install_dependencies
    
    BUILD_SOURCE=""
    
    if [ "$CLEAN_INSTALL" = true ]; then
        log_info "Starting Clean Install (No source on server)..."
        rm -rf "$TEMP_BUILD_DIR"
        git clone "$REPO_URL" "$TEMP_BUILD_DIR"
        BUILD_SOURCE="$TEMP_BUILD_DIR"
    else
        log_info "Starting Standard Install (Source kept)..."
        SOURCE_DIR="$INSTALL_DIR/source"
        if [ ! -d "$SOURCE_DIR" ]; then
            git clone "$REPO_URL" "$SOURCE_DIR"
        else
            cd "$SOURCE_DIR" && git pull
        fi
        BUILD_SOURCE="$SOURCE_DIR"
    fi
    
    # Build
    build_binaries "$ROLE" "$BUILD_SOURCE"
    
    # Stop existing
    systemctl stop exarobot-panel &> /dev/null || true
    systemctl stop exarobot-agent &> /dev/null || true
    
    setup_directory
    
    # Copy Binaries
    if [[ "$ROLE" == "panel" || "$ROLE" == "both" ]]; then
        cp "$BUILD_SOURCE/target/release/exarobot" "$INSTALL_DIR/"
        chmod +x "$INSTALL_DIR/exarobot"
        configure_panel
    fi
    
    if [[ "$ROLE" == "agent" || "$ROLE" == "both" ]]; then
        cp "$BUILD_SOURCE/target/release/exarobot-agent" "$INSTALL_DIR/"
        chmod +x "$INSTALL_DIR/exarobot-agent"
        configure_agent
    fi
    
    # Cleanup Clean Install
    if [ "$CLEAN_INSTALL" = true ]; then
        rm -rf "$TEMP_BUILD_DIR"
    fi
    
    
    # Verification
    if [[ "$ROLE" == "agent" || "$ROLE" == "both" ]]; then
        echo ""
        log_info "Verifying Agent Connection..."
        sleep 2
        # Simple connectivity check
        if curl -s --max-time 5 "$PANEL_URL" > /dev/null; then
             log_success "Agent can reach Panel at $PANEL_URL"
        else
             log_warn "Agent CANNOT reach Panel at $PANEL_URL"
             echo "Possible issues: Firewall, DNS, or Panel is down."
        fi
        
        echo ""
        log_info "To view logs:"
        echo "  Panel: journalctl -u exarobot-panel -f"
        echo "  Agent: journalctl -u exarobot-agent -f"
    fi

    
    echo ""
    log_success "Installation Complete!"
    if [[ "$ROLE" == "panel" || "$ROLE" == "both" ]]; then
        echo -e "${CYAN}--------------------------------------------------${NC}"
        echo -e "Panel Address : https://${DOMAIN}${ADMIN_PATH}"
        echo -e "Admin Creation: cd ${INSTALL_DIR} && ./exarobot admin reset-password <user> <pass>"
        echo -e "${CYAN}--------------------------------------------------${NC}"
    fi
}

main "$@"
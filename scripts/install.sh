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
VERSION_TAG="2026-02-05-v3"

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
SKIP_GPG_CHECK=${SKIP_GPG_CHECK:-false}  # Allow GPG check skip via env var

# --------------------------------------------------
# Logging
# --------------------------------------------------
log_info() { echo -e "${CYAN}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# --------------------------------------------------
# Environment Loading
# --------------------------------------------------
load_env_settings() {
    local env_file=$1
    if [ -f "$env_file" ]; then
        log_info "Detecting existing configuration in $env_file..."
        # Extract specific variables if not already set by args
        while IFS='=' read -r key value || [ -n "$key" ]; do
            # Skip comments and empty lines
            [[ "$key" =~ ^#.*$ ]] && continue
            [[ -z "$key" ]] && continue
            
            # Trim whitespace from value (basic trim for shell)
            value=$(echo "$value" | xargs 2>/dev/null || echo "$value" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
            
            case "$key" in
                SERVER_DOMAIN) [[ -z "$DOMAIN" ]] && DOMAIN="$value" ;;
                ADMIN_PATH) [[ -z "$ADMIN_PATH" ]] && ADMIN_PATH="$value" ;;
                PANEL_PORT) [[ -z "$PANEL_PORT" ]] && PANEL_PORT="$value" ;;
                PANEL_URL) [[ -z "$PANEL_URL" ]] && PANEL_URL="$value" ;;
                NODE_TOKEN) [[ -z "$NODE_TOKEN" ]] && NODE_TOKEN="$value" ;;
            esac
        done < "$env_file"
    fi
}

# --------------------------------------------------
# GPG Signature Verification (Security)
# --------------------------------------------------
verify_gpg_signature() {
    # Skip if explicitly requested
    if [[ "$SKIP_GPG_CHECK" == "true" ]]; then
        log_warn "GPG verification skipped (--skip-gpg-check flag)"
        return 0
    fi
    
    # Skip if running from stdin pipe (can't verify)
    if [ ! -f "$0" ]; then
        log_info "Running from stdin - GPG verification skipped"
        log_info "For maximum security, download and verify manually"
        return 0
    fi
    
    # Check if GPG is available
    if ! command -v gpg &> /dev/null; then
        log_info "GPG not installed - signature verification skipped"
        return 0
    fi
    
    # Look for .asc signature file
    local sig_path="${0}.asc"
    if [ ! -f "$sig_path" ]; then
        log_info "No signature file found - verification skipped"
        return 0
    fi
    
    log_info "Verifying GPG signature..."
    if gpg --verify "$sig_path" "$0" 2>&1 | grep -q "Good signature"; then
        log_success "GPG signature verified"
        return 0
    else
        log_error "GPG signature verification FAILED!"
        log_error "This could indicate tampering. Use --skip-gpg-check to override (NOT RECOMMENDED)"
        exit 1
    fi
}

# Run GPG verification early
verify_gpg_signature

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
    apt-get install -y curl git build-essential pkg-config libssl-dev sqlite3 redis-server gnupg debian-keyring debian-archive-keyring apt-transport-https -qq
    
    # Configure and start Redis
    if command -v redis-server &> /dev/null; then
        log_info "Configuring Redis..."
        systemctl enable redis-server &> /dev/null || systemctl enable redis &> /dev/null || true
        systemctl start redis-server &> /dev/null || systemctl start redis &> /dev/null || true
        
        # Verify Redis is running
        if redis-cli ping &> /dev/null; then
            log_success "Redis is running"
        else
            log_warn "Redis installed but not responding to ping"
        fi
    fi
    
    if ! command -v cargo &> /dev/null; then
        log_info "Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    else
        log_success "Rust already installed"
    fi
}

install_singbox() {
    if ! command -v sing-box &> /dev/null; then
        log_info "Installing sing-box..."
        curl -fsSL https://sing-box.app/gpg.key -o /etc/apt/keyrings/sagernet.asc
        chmod a+r /etc/apt/keyrings/sagernet.asc
        echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/sagernet.asc] https://deb.sagernet.org/ * *" | \
            tee /etc/apt/sources.list.d/sagernet.list > /dev/null
        apt-get update -qq
        apt-get install -y sing-box -qq
        log_success "sing-box installed"
    else
        log_success "sing-box already installed"
    fi
    
    # Ensure we know where it is
    SINGBOX_BIN=$(command -v sing-box || echo "/usr/bin/sing-box")
    if [ ! -f "$SINGBOX_BIN" ]; then
        # Try common checks
        if [ -f "/usr/local/bin/sing-box" ]; then
             SINGBOX_BIN="/usr/local/bin/sing-box"
        elif [ -f "/usr/bin/sing-box" ]; then
             SINGBOX_BIN="/usr/bin/sing-box"
        else
             log_warn "Could not locate sing-box binary. Assuming /usr/bin/sing-box"
             SINGBOX_BIN="/usr/bin/sing-box"
        fi
    fi
    log_info "Sing-box binary found at: $SINGBOX_BIN"
}

setup_firewall() {
    if command -v ufw &> /dev/null; then
        ufw allow 22/tcp
        ufw allow 80/tcp
        ufw allow 443/tcp
        ufw allow 443/udp # Hysteria/QUIC
        
        # Common VPN ports
        ufw allow 8443/tcp
        ufw allow 8443/udp
        ufw allow 2053/tcp
        ufw allow 2053/udp
        
        # Panel port opened later if needed
    fi
}

# --------------------------------------------------
# Conflict Detection
# --------------------------------------------------
check_conflicts() {
    local clash=false
    
    # Check specifically for the requested role
    if [[ "$ROLE" == "panel" || "$ROLE" == "both" ]]; then
        if systemctl is-active --quiet exarobot; then
            clash=true
        fi
        if command -v ss &> /dev/null; then
            local TARGET_PORT=${PANEL_PORT:-3000}
            if ss -tuln | grep -q ":${TARGET_PORT} "; then
                 clash=true
            fi
        fi
    fi

    if [[ "$ROLE" == "agent" || "$ROLE" == "both" ]]; then
        if systemctl is-active --quiet exarobot-agent; then
            clash=true
        fi
    fi
    
    # For 'all' role, we check if ANY of the services are running to warn about overwrite
    if [[ "$ROLE" == "all" ]]; then
        if systemctl is-active --quiet exarobot || systemctl is-active --quiet exarobot-agent || systemctl is-active --quiet exarobot-frontend; then
            clash=true
        fi
    fi

    # Determine if we are just adding a missing role to an existing setup
    if [ "$clash" = false ]; then
        if [[ "$ROLE" == "agent" ]] && systemctl is-active --quiet exarobot; then
            log_info "Panel detected. Installing Agent alongside existing Panel..."
        fi
        if [[ "$ROLE" == "panel" ]] && systemctl is-active --quiet exarobot-agent; then
            log_info "Agent detected. Installing Panel alongside existing Agent..."
        fi
    fi
    
    if [ "$clash" = true ]; then
        echo ""
        echo -e "${YELLOW}Existing installation detected!${NC}"
        echo "Choose installation mode:"
        echo "1) Update (Recommended)"
        echo "   - Preserves database, users, and settings."
        echo "   - Replaces binaries and updates assets."
        echo "   - Automatically runs migrations on startup."
        echo "2) Clean Install"
        echo "   - BACKS UP existing database to /var/backups/exarobot/"
        echo "   - DELETES all current data and configuration."
        echo "   - Starts from a fresh state."
        echo "3) Cancel"
        
        ACTION=""
        if [ -t 0 ]; then
            read -p "Select [1-3]: " ACTION
        else
             read -r ACTION < /dev/tty
        fi
        
        case $ACTION in
            1)
                log_info "Starting Update process..."
                log_info "Stopping services..."
                systemctl stop exarobot &> /dev/null || true
                systemctl stop exarobot-agent &> /dev/null || true
                # Do NOT remove directory or files.
                # Just stop services so we can overwrite binaries.
                ;;
            2)
                log_info "Starting Clean Install..."
                
                # BACKUP DATABASE
                if [ -f "$INSTALL_DIR/exarobot.db" ]; then
                    BACKUP_DIR="/var/backups/exarobot"
                    mkdir -p "$BACKUP_DIR"
                    TIMESTAMP=$(date +%Y%m%d_%H%M%S)
                    cp "$INSTALL_DIR/exarobot.db" "$BACKUP_DIR/exarobot.db.bak_$TIMESTAMP"
                    log_success "Database backed up to: $BACKUP_DIR/exarobot.db.bak_$TIMESTAMP"
                fi
                
                log_info "Stopping services..."
                systemctl stop exarobot &> /dev/null || true
                systemctl disable exarobot &> /dev/null || true
                rm -f /etc/systemd/system/exarobot.service
                
                systemctl stop exarobot-agent &> /dev/null || true
                systemctl disable exarobot-agent &> /dev/null || true
                rm -f /etc/systemd/system/exarobot-agent.service
                
                # Cleanup files
                cd /tmp || exit 1
                rm -rf "$INSTALL_DIR"
                systemctl daemon-reload
                ;;
            *)
                log_error "Aborted."
                exit 1
                ;;
        esac
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
    
    # Init DB for sqlx compile-time verification
    if [[ "$target_role" == "panel" || "$target_role" == "both" || "$target_role" == "all" ]]; then
        local APP_PANEL_DIR="$src_dir/apps/panel"
        
        # Create database with consolidated migration for sqlx macros
        if [ ! -f "$APP_PANEL_DIR/exarobot.db" ]; then
            log_info "Preparing database for compilation..."
            export DATABASE_URL="sqlite://$APP_PANEL_DIR/exarobot.db"
            
            # Use the new consolidated migration
            if [ -f "$APP_PANEL_DIR/migrations/20260205000000_init.sql" ]; then
                sqlite3 "$APP_PANEL_DIR/exarobot.db" < "$APP_PANEL_DIR/migrations/20260205000000_init.sql" 2>/dev/null || {
                    log_warn "Failed to apply migration, trying minimal schema..."
                    # Fallback: create minimal schema with ALL required columns for sqlx! macros
                    sqlite3 "$APP_PANEL_DIR/exarobot.db" <<EOF
-- Core tables with all required columns
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tg_id INTEGER NOT NULL UNIQUE,
    username TEXT,
    first_name TEXT,
    last_name TEXT,
    full_name TEXT,
    language_code TEXT DEFAULT 'en',
    is_banned BOOLEAN DEFAULT 0,
    ban_reason TEXT,
    banned_at DATETIME,
    referrer_id INTEGER,
    referral_code TEXT UNIQUE,
    referred_by INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_seen DATETIME DEFAULT CURRENT_TIMESTAMP,
    balance INTEGER DEFAULT 0,
    trial_used INTEGER DEFAULT 0,
    trial_used_at TIMESTAMP NULL,
    channel_member_verified INTEGER DEFAULT 0,
    channel_verified_at TIMESTAMP,
    trial_source TEXT DEFAULT 'default',
    terms_accepted_at DATETIME,
    warning_count INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    plan_id INTEGER NOT NULL,
    duration_days INTEGER NOT NULL,
    amount REAL NOT NULL,
    total_amount INTEGER NOT NULL,
    currency TEXT DEFAULT 'USD',
    status TEXT NOT NULL DEFAULT 'pending',
    payment_provider TEXT,
    payment_id TEXT,
    paid_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS subscription_ip_tracking (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subscription_id INTEGER NOT NULL,
    client_ip TEXT NOT NULL,
    last_seen_at DATETIME NOT NULL
);
EOF
                }
                log_success "Database prepared for compilation"
            else
                log_error "Migration file not found: $APP_PANEL_DIR/migrations/20260205000000_init.sql"
                exit 1
            fi
        else
            log_info "Database already exists for compilation"
            export DATABASE_URL="sqlite://$APP_PANEL_DIR/exarobot.db"
        fi
        
        log_info "Compiling Panel (this may take a few minutes)..."
        cd "$APP_PANEL_DIR"
        cargo build --release --bin exarobot
        cd "$src_dir"
        cd "$src_dir"
        
        log_info "Compiling Frontend (for downloads/distribution)..."
        
        # FIX: The frontend requires apps/mini-app/dist to exist for RustEmbed.
        # Since we don't install Node.js/npm on the server, we create a placeholder if it's missing.
        MINI_APP_DIST="$src_dir/apps/mini-app/dist"
        if [ ! -d "$MINI_APP_DIST" ]; then
            log_warn "Miniapp dist not found (Node.js not installed). Creating placeholder..."
            mkdir -p "$MINI_APP_DIST"
            cat > "$MINI_APP_DIST/index.html" <<EOF
<!DOCTYPE html>
<html>
<head><title>ExaRobot MiniApp</title></head>
<body>
<h1>MiniApp Not Built</h1>
<p>The MiniApp was not built during installation (Node.js required).</p>
<p>Please build locally and upload <code>apps/mini-app/dist</code> if you need the Telegram App.</p>
</body>
</html>
EOF
        fi

        # Compile frontend binary
        cargo build -p exarobot-frontend --release
        
        log_info "Compiling Agent (for downloads/distribution)..."
        cargo build -p exarobot-agent --release
    fi
    
    if [[ "$target_role" == "agent" || "$target_role" == "both" ]]; then
        # If we are installing agent specifically, we ensure it's built (might be redundant if panel built it, but safe)
        if [[ "$target_role" != "both" ]]; then
             log_info "Compiling Agent..."
             cargo build -p exarobot-agent --release
        fi
    fi
}

# --------------------------------------------------
# Installation Logic
# --------------------------------------------------
setup_directory() {
    mkdir -p "$INSTALL_DIR"
    # Files are kept in root: /opt/exarobot/{exarobot, exarobot-agent, .env, .env.agent}
    # Ensure downloads directory exists for frontend binaries
    mkdir -p "$INSTALL_DIR/apps/panel/downloads"
}

    ensure_panel_port() {
      if [[ -z "$PANEL_PORT" ]]; then
        if [ -t 0 ]; then
             read -p "Enter Panel Port [3000]: " PANEL_PORT
        else
             echo -n "Enter Panel Port [3000]: "
             read -r PANEL_PORT < /dev/tty
        fi
        PANEL_PORT=${PANEL_PORT:-3000}
      fi
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
        # Trim whitespace
        # Sanitize: Keep only alphanumeric, dots, and hyphens. Remove hidden chars.
        DOMAIN=$(echo "$DOMAIN" | tr -cd '[:alnum:].-')
    else
        log_info "Using pre-configured domain: $DOMAIN"
    fi
    
    ensure_panel_port
    
    if [[ -z "$ADMIN_PATH" ]]; then
        if [ -t 0 ]; then
             read -p "Enter Admin Path (e.g. /admin): " ADMIN_PATH
        else
             echo -n "Enter Admin Path (e.g. /admin): "
             read -r ADMIN_PATH < /dev/tty
        fi
        ADMIN_PATH=${ADMIN_PATH:-/admin}
    else
        log_info "Using pre-configured admin path: $ADMIN_PATH"
    fi
     # Ensure leading slash
    [[ "${ADMIN_PATH}" != /* ]] && ADMIN_PATH="/${ADMIN_PATH}"
    
    # Install sing-box for key generation (Panel needs it for `sing-box generate reality-keypair`)
    install_singbox
    systemctl stop sing-box &> /dev/null || true
    systemctl disable sing-box &> /dev/null || true
    
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
        # Apply consolidated migration
        if [ -f "$TEMP_BUILD_DIR/apps/panel/migrations/20260205000000_init.sql" ]; then
             sqlite3 "$DB_FILE" < "$TEMP_BUILD_DIR/apps/panel/migrations/20260205000000_init.sql"
        elif [ -f "$INSTALL_DIR/source/apps/panel/migrations/20260205000000_init.sql" ]; then
             sqlite3 "$DB_FILE" < "$INSTALL_DIR/source/apps/panel/migrations/20260205000000_init.sql"
        else
             log_warn "Migration file not found - database will be initialized by application on first run"
        fi
    fi
    
    # Service
    cat > /etc/systemd/system/exarobot.service <<EOF
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
    systemctl enable exarobot
    systemctl restart exarobot
    log_success "Panel installed. Access: https://$DOMAIN$ADMIN_PATH/login"
    echo ""
    echo -e "${YELLOW}IMPORTANT: Create your first Admin User:${NC}"
    echo "  cd $INSTALL_DIR"
    echo "  ./exarobot admin reset-password admin <your-password>"
    echo ""
}

configure_agent() {
    # Download agent binary from Panel
    log_info "Downloading agent binary..."
    
    # Validate Panel URL first
    if [[ -z "$PANEL_URL" ]]; then
        if [ -t 0 ]; then
            read -p "Enter Panel URL (e.g. https://panel.example.com): " PANEL_URL
        else
            echo -n "Enter Panel URL (e.g. https://panel.example.com): "
            read -r PANEL_URL < /dev/tty
        fi
    else
        log_info "Using pre-configured Panel URL: $PANEL_URL"
    fi
    
    ARCH=$(uname -m)
    if [ "$ARCH" = "x86_64" ]; then
        BINARY_URL="${PANEL_URL}/downloads/exarobot-agent-linux-amd64"
    elif [ "$ARCH" = "aarch64" ]; then
        BINARY_URL="${PANEL_URL}/downloads/exarobot-agent-linux-arm64"
    else
        log_error "Unsupported architecture: $ARCH"
        exit 1
    fi
    
    if [[ "$ROLE" == "all" || "$ROLE" == "both" ]]; then
        log_info "Skipping download for '$ROLE' role (using local binary)..."
        if [ ! -f "$INSTALL_DIR/exarobot-agent" ]; then
             log_error "Local binary not found at $INSTALL_DIR/exarobot-agent"
             exit 1
        fi
    else
        mkdir -p "$INSTALL_DIR"
        wget -q --show-progress -O "$INSTALL_DIR/exarobot-agent" "$BINARY_URL" || {
            log_error "Failed to download agent binary from: $BINARY_URL"
            log_error "Make sure the Panel is running and has compiled the Agent binary."
            exit 1
        }
        chmod +x "$INSTALL_DIR/exarobot-agent"
        log_success "Binary installed"
    fi
    
    # Get Node Token
    if [[ -z "$NODE_TOKEN" ]]; then
        if [ -t 0 ]; then
             read -p "Enter Node Token: " NODE_TOKEN
        else
             echo -n "Enter Node Token: "
             read -r NODE_TOKEN < /dev/tty
        fi
    else
        log_info "Using pre-configured Node Token"
    fi
    
    # Agent Env
    AGENT_ENV="$INSTALL_DIR/.env.agent"
    cat > "$AGENT_ENV" <<EOF
PANEL_URL=$PANEL_URL
NODE_TOKEN=$NODE_TOKEN
CONFIG_PATH=/etc/sing-box/config.json
EOF

    # Install sing-box (shared function)
    install_singbox
    
    # Stop default sing-box service (we manage it via Agent)
    systemctl stop sing-box &> /dev/null || true
    systemctl disable sing-box &> /dev/null || true
    
    # Generate self-signed certificates for Hysteria2 (Smart Check)
    log_info "Verifying TLS certificates for Hysteria2..."
    
    # Ensure directory exists
    if [ ! -d "/etc/sing-box/certs" ]; then
        mkdir -p /etc/sing-box/certs
    fi

    # 1. Detect SNI from Panel Config (if possible)
    TARGET_SNI="drive.google.com"
    if [ -n "$PANEL_URL" ] && [ -n "$NODE_TOKEN" ]; then
        log_info "Fetching config to detect SNI..."
        # Try to fetch config
        CONFIG_JSON=$(curl -s -m 5 "$PANEL_URL/api/v2/node/config" -H "Authorization: Bearer $NODE_TOKEN" || echo "")
        
        # Simple grep extraction to avoid jq dependency
        FETCHED_SNI=$(echo "$CONFIG_JSON" | grep -o '"server_name": *"[^"]*"' | head -1 | cut -d'"' -f4)
        
        if [ -n "$FETCHED_SNI" ] && [ "$FETCHED_SNI" != "null" ]; then
             TARGET_SNI="$FETCHED_SNI"
             log_info "Detected SNI from config: $TARGET_SNI"
        else
             log_warn "Could not detect SNI from config (using default: $TARGET_SNI)"
        fi
    fi

    # 2. Check existing certificate CN
    CURRENT_CN=""
    if [ -f /etc/sing-box/certs/cert.pem ]; then
         # Extract CN from subject
         CURRENT_CN=$(openssl x509 -in /etc/sing-box/certs/cert.pem -noout -subject 2>/dev/null | sed -n 's/^.*CN *= *\([^,]*\).*$/\1/p')
    fi

    # 3. Generate or Regenerate if needed
    if [ ! -f /etc/sing-box/certs/cert.pem ] || [ ! -f /etc/sing-box/certs/key.pem ] || [ "$CURRENT_CN" != "$TARGET_SNI" ]; then
        
        if [ "$CURRENT_CN" != "$TARGET_SNI" ] && [ -n "$CURRENT_CN" ]; then
            log_warn "SNI Mismatch (Current: '$CURRENT_CN', Target: '$TARGET_SNI'). Regenerating..."
        else
            log_info "Generating certificates for SNI: $TARGET_SNI"
        fi

        openssl req -x509 -newkey rsa:2048 -keyout /etc/sing-box/certs/key.pem \
            -out /etc/sing-box/certs/cert.pem -days 3650 -nodes \
            -subj "/CN=$TARGET_SNI" 2>/dev/null || log_warning "Failed to generate certificates"
        
        # Security permissions
        chmod 644 /etc/sing-box/certs/cert.pem
        chmod 600 /etc/sing-box/certs/key.pem
        log_success "TLS certificates generated/updated"
    else
        log_info "TLS certificates match SNI: $TARGET_SNI"
    fi
    
    # 4. Masquerade Setup (Prevent "file not found" crash)
    MASQ_DIR="/var/www/html"
    if [ ! -d "$MASQ_DIR" ]; then
        log_info "Creating default masquerade directory: $MASQ_DIR"
        mkdir -p "$MASQ_DIR"
        # Create a professional "API Gateway" dummy page
        cat > "$MASQ_DIR/index.html" <<EOF
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>API Gateway</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; background: #f5f5f7; color: #333; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
        .container { text-align: center; padding: 40px; background: white; border-radius: 12px; box-shadow: 0 4px 6px rgba(0,0,0,0.05); max-width: 400px; width: 90%; }
        h1 { font-size: 24px; font-weight: 600; margin-bottom: 10px; color: #1d1d1f; }
        p { font-size: 14px; color: #86868b; line-height: 1.5; margin-bottom: 20px; }
        .code { font-family: monospace; background: #f0f0f0; padding: 4px 8px; border-radius: 4px; color: #555; }
        .status { display: inline-block; width: 10px; height: 10px; background: #34c759; border-radius: 50%; margin-right: 6px; }
        .footer { margin-top: 30px; font-size: 12px; color: #d2d2d7; }
    </style>
</head>
<body>
    <div class="container">
        <div style="margin-bottom: 20px;">
            <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="#007aff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="8" rx="2" ry="2"></rect><rect x="2" y="14" width="20" height="8" rx="2" ry="2"></rect><line x1="6" y1="6" x2="6.01" y2="6"></line><line x1="6" y1="18" x2="6.01" y2="18"></line></svg>
        </div>
        <h1>API Gateway</h1>
        <p>This endpoint is operational.</p>
        <div style="font-size: 13px; color: #555; background: #fafafa; padding: 15px; border-radius: 8px; text-align: left;">
            <div><span class="status"></span>System Status: <strong>Normal</strong></div>
            <div style="margin-top: 8px;">Region: <span class="code">us-east-1</span></div>
            <div style="margin-top: 8px;">Protocol: <span class="code">h3</span></div>
        </div>
        <div class="footer">Protected by ExaWall &copy; 2026</div>
    </div>
</body>
</html>
EOF
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
ExecStart=$SINGBOX_BIN run -c /etc/sing-box/config.json
EOF

    cat > /etc/systemd/system/exarobot-agent.service <<EOF
[Unit]
Description=VPN Node Agent
After=network.target
Before=sing-box.service

[Service]
# Force clean config on startup (Agent will fetch new one)
ExecStartPre=/bin/rm -f /etc/sing-box/config.json
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
    
    # Wait for Agent to start and fetch config
    log_info "Waiting for Agent to fetch configuration..."
    sleep 5
    
    # Verify config was created
    if [ -f /etc/sing-box/config.json ]; then
        log_success "Configuration synchronized"
        
        # Restart sing-box with new config
        systemctl restart sing-box
        sleep 2
        
        # Check if sing-box started successfully
        if systemctl is-active --quiet sing-box; then
            log_success "Sing-box started successfully"
        else
            log_error "Sing-box failed to start. Check logs: journalctl -u sing-box -n 50"
        fi
    else
        log_warning "Config not found. Agent may need more time to connect to Panel."
    fi
    
    log_success "Agent installed."
}
# Frontend installation helper functions - to be inserted into install.sh

# --------------------------------------------------
# Frontend Installation Function
# --------------------------------------------------
configure_frontend() {
    log_info "Installing Frontend Module..."
    
    # Validate required arguments
    if [[ -z "$FRONTEND_DOMAIN" ]] || [[ -z "$FRONTEND_TOKEN" ]]; then
        log_error "Frontend installation requires: --domain, --token"
        exit 1
    fi
    
    # Default region if missing
    FRONTEND_REGION=${FRONTEND_REGION:-"global"}
    
    if [[ -z "$PANEL_URL" ]]; then
        log_error "Frontend installation requires: --panel <URL>"
        exit 1
    fi
    
    # Download frontend binary
    log_info "Downloading frontend binary..."
    ARCH=$(uname -m)
    if [ "$ARCH" = "x86_64" ]; then
        BINARY_URL="${PANEL_URL}/downloads/exarobot-frontend-linux-amd64"
    elif [ "$ARCH" = "aarch64" ]; then
        BINARY_URL="${PANEL_URL}/downloads/exarobot-frontend-linux-arm64"
    else
        log_error "Unsupported architecture: $ARCH"
        exit 1
    fi
    
    if [[ "$ROLE" == "all" ]]; then
        # Local install: copy from cache to /usr/local/bin
        if [ "$ARCH" = "x86_64" ]; then 
            FE_SUFFIX="linux-amd64"
        elif [ "$ARCH" = "aarch64" ]; then 
            FE_SUFFIX="linux-arm64" 
        else
            log_error "Unsupported architecture: $ARCH"
            exit 1
        fi
        
        # We assume INSTALL_DIR is set (it is global)
        LOCAL_SRC="$INSTALL_DIR/apps/panel/downloads/exarobot-frontend-$FE_SUFFIX"
        if [ -f "$LOCAL_SRC" ]; then
             log_info "Installing Frontend from local cache ($LOCAL_SRC)..."
             mkdir -p /usr/local/bin
             cp "$LOCAL_SRC" /usr/local/bin/exarobot-frontend
        else
             log_error "Local frontend binary not found at $LOCAL_SRC"
             exit 1
        fi
    else
        mkdir -p /usr/local/bin
        wget -q --show-progress -O /usr/local/bin/exarobot-frontend "$BINARY_URL" || {
            log_error "Failed to download frontend binary"
            exit 1
        }
    fi
    chmod +x /usr/local/bin/exarobot-frontend
    log_success "Binary installed"
    
    # Create configuration
    log_info "Creating configuration..."
    mkdir -p /etc/exarobot
    cat > /etc/exarobot/frontend.toml <<EOF
domain = "$FRONTEND_DOMAIN"
panel_url = "$PANEL_URL"
auth_token = "$FRONTEND_TOKEN"
region = "$FRONTEND_REGION"
listen_port = ${FRONTEND_PORT:-8080}
EOF
    chmod 600 /etc/exarobot/frontend.toml
    log_success "Configuration created"
    
    # Install Caddy for HTTPS
    install_caddy_if_needed
    
    # Configure Caddy reverse proxy
    configure_caddy_for_frontend
    
    # Create systemd service
    create_frontend_service
    
    # Register with panel
    register_with_panel
    
    log_success "Frontend installed: https://$FRONTEND_DOMAIN"
}

# Helper: Install Caddy
install_caddy_if_needed() {
    if command -v caddy &> /dev/null; then
        log_info "Caddy already installed"
        return
    fi
    
    log_info "Installing Caddy..."
    if [ "$ID" = "ubuntu" ] || [ "$ID" = "debian" ]; then
        log_info "Adding Caddy repository..."
        apt-get install -y debian-keyring debian-archive-keyring apt-transport-https curl
        curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
        curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | tee /etc/apt/sources.list.d/caddy-stable.list
        apt-get update
        log_info "Installing Caddy package..."
        apt-get install -y caddy
    elif [ "$ID" = "centos" ] || [ "$ID" = "rhel" ]; then
        yum install yum-plugin-copr -y
        yum copr enable @caddy/caddy -y
        yum install caddy -y
    fi
    
    if command -v caddy &> /dev/null; then
        log_success "Caddy installed successfully"
    else
        log_error "Caddy installation failed! Please check your package manager settings."
        exit 1
    fi
}

# Helper: Configure Caddy
configure_caddy_for_frontend() {
    log_info "Configuring Caddy reverse proxy..."
    
    # Prepare Caddyfile content
    mkdir -p /etc/caddy
    # Check if domains are identical to avoid ambiguity
    if [ "$FRONTEND_DOMAIN" == "$MINIAPP_DOMAIN" ]; then
        log_info "Frontend and Miniapp domains are identical. Generating combined configuration..."
        SUB_PATH=${SUB_PATH:-"/sub/"}
        
        cat > /etc/caddy/Caddyfile <<EOF
$FRONTEND_DOMAIN {
    # Proxy subscription path to Panel
    handle_path ${SUB_PATH}* {
        rewrite * /sub{path}
        reverse_proxy $PANEL_URL {
            header_up Host {upstream_hostport}
            header_up X-Real-IP {remote}
        }
    }

    # Proxy API calls to Panel
    handle /api/* {
        reverse_proxy $PANEL_URL {
            header_up Host {upstream_hostport}
            header_up X-Real-IP {remote}
        }
    }

    # Proxy everything else to Frontend (Standard UI)
    handle {
        reverse_proxy localhost:${FRONTEND_PORT:-8080}
        
        header {
            Strict-Transport-Security "max-age=31536000; includeSubDomains; preload"
            X-Content-Type-Options "nosniff"
            X-Frame-Options "SAMEORIGIN"
            X-XSS-Protection "1; mode=block"
            Referrer-Policy "no-referrer-when-downgrade"
        }
    }

    log {
        output file /var/log/caddy/frontend.log
        format json
    }
}
EOF
    else
        # Separate Configs
        cat > /etc/caddy/Caddyfile <<EOF
$FRONTEND_DOMAIN {
    reverse_proxy localhost:${FRONTEND_PORT:-8080}
    
    header {
        Strict-Transport-Security "max-age=31536000; includeSubDomains; preload"
        X-Content-Type-Options "nosniff"
        X-Frame-Options "SAMEORIGIN"
        X-XSS-Protection "1; mode=block"
        Referrer-Policy "no-referrer-when-downgrade"
    }
    
    log {
        output file /var/log/caddy/frontend.log
        format json
    }
}
EOF

        # Add Miniapp Domain block if specified (and different)
        if [ -n "$MINIAPP_DOMAIN" ]; then
            SUB_PATH=${SUB_PATH:-"/sub/"}
            
            cat >> /etc/caddy/Caddyfile <<EOF

$MINIAPP_DOMAIN {
    # Proxy subscription path to Panel
    handle_path ${SUB_PATH}* {
        rewrite * /sub{path}
        reverse_proxy $PANEL_URL {
            header_up Host {upstream_hostport}
            header_up X-Real-IP {remote}
        }
    }

    # Proxy API calls to Panel
    handle /api/* {
        reverse_proxy $PANEL_URL {
            header_up Host {upstream_hostport}
            header_up X-Real-IP {remote}
        }
    }

    # Proxy everything else to Frontend
    handle {
        reverse_proxy localhost:${FRONTEND_PORT:-8080}
    }

    log {
        output file /var/log/caddy/miniapp.log
        format json
    }
}
EOF
        fi
    fi

    mkdir -p /var/log/caddy
    chown caddy:caddy /var/log/caddy 2>/dev/null || true
    
    systemctl enable caddy > /dev/null 2>&1
    systemctl restart caddy
    log_success "Caddy configured"
}

# Helper: Create systemd service
create_frontend_service() {
    log_info "Creating systemd service..."
    cat > /etc/systemd/system/exarobot-frontend.service <<EOF
[Unit]
Description=EXA-ROBOT Frontend Module
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=root
WorkingDirectory=/var/lib/exarobot
ExecStart=/usr/local/bin/exarobot-frontend --config /etc/exarobot/frontend.toml
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/exarobot

[Install]
WantedBy=multi-user.target
EOF

    mkdir -p /var/lib/exarobot
    systemctl daemon-reload
    systemctl enable exarobot-frontend > /dev/null 2>&1
    systemctl start exarobot-frontend
    log_success "Service created and started"
}

# Helper: Register with panel
register_with_panel() {
    log_info "Registering with main panel..."
    SERVER_IP=$(curl -s ifconfig.me || curl -s icanhazip.com || echo "unknown")
    
    sleep 3  # Wait for service to start
    
    # Update IP via heartbeat endpoint since we are already created
    # Use HTTP 200 check
    
    curl -X POST "$PANEL_URL/api/admin/frontends/$FRONTEND_DOMAIN/heartbeat" \
      -H "Authorization: Bearer $FRONTEND_TOKEN" \
      -H "Content-Type: application/json" \
      -d "{
        \"requests_count\": 0,
        \"bandwidth_used\": 0,
        \"ip_address\": \"$SERVER_IP\"
      }" > /dev/null 2>&1 || {
        log_warn "Could not register IP with panel automatically"
        log_warn "Please verify connection in admin dashboard"
    }
    log_success "Registration/Heartbeat attempt complete"
}

# Helper: Seed Local Tokens for "All-in-One" install
seed_local_tokens() {
    log_info "Seeding local tokens for All-in-One setup..."
    
    # Wait for DB to be created by Panel service
    local max_retries=30
    local count=0
    local db_file="$INSTALL_DIR/exarobot.db"
    
    while [ ! -f "$db_file" ]; do
        sleep 1
        count=$((count+1))
        if [ $count -ge $max_retries ]; then
             log_error "Database file not found after waiting. Panel might have failed to start."
             exit 1
        fi
    done
    
    # Wait a bit more for migrations
    sleep 2
    
    # Generate Tokens
    if [ -z "$NODE_TOKEN" ]; then
        NODE_TOKEN=$(openssl rand -hex 16)
        log_info "Generated NODE_TOKEN: $NODE_TOKEN"
    fi
    
    if [ -z "$FRONTEND_TOKEN" ]; then
        FRONTEND_TOKEN=$(openssl rand -hex 16)
        log_info "Generated FRONTEND_TOKEN: $FRONTEND_TOKEN"
    fi
    
    # Insert Node
    sqlite3 "$db_file" "INSERT OR IGNORE INTO nodes (name, ip, join_token, status, is_enabled) VALUES ('Local Node', '127.0.0.1', '$NODE_TOKEN', 'active', 1);"
    
    # Insert Frontend
    # Default region 'local', domain from arg or default
    if [ -z "$FRONTEND_DOMAIN" ]; then
        FRONTEND_DOMAIN="localhost"
    fi
    
    sqlite3 "$db_file" "INSERT OR IGNORE INTO frontend_servers (domain, ip_address, region, auth_token, is_active) VALUES ('$FRONTEND_DOMAIN', '127.0.0.1', 'local', '$FRONTEND_TOKEN', 1);"
    
    export NODE_TOKEN
    export FRONTEND_TOKEN
    export PANEL_URL="http://127.0.0.1:3000" # Force local connection
    
    log_success "Tokens seeded into database."
}

# --------------------------------------------------
# Main Logic
# --------------------------------------------------
main() {
    echo "--------------------------------------------------"
    echo "ExaRobot Installer - Version: $VERSION_TAG"
    echo "--------------------------------------------------"
    check_root
    detect_os
    
    # Parse Args
    while [[ "$#" -gt 0 ]]; do
        case $1 in
            --clean) CLEAN_INSTALL=true ;;
            --role) ROLE="$2"; shift ;;
            --panel) PANEL_URL="$2"; shift ;;
            --port) PANEL_PORT="$2"; shift ;;
            --token) 
                # Token used for both frontend and agent
                FRONTEND_TOKEN="$2"
                NODE_TOKEN="$2"
                shift ;;
            --domain) 
                # Domain used for both panel and frontend
                DOMAIN="$2"
                FRONTEND_DOMAIN="$2"
                shift ;;
            --region) FRONTEND_REGION="$2"; shift ;;
            --miniapp-domain) MINIAPP_DOMAIN="$2"; shift ;;
            --sub-path) SUB_PATH="$2"; shift ;;
            --admin-path) ADMIN_PATH="$2"; shift ;;
            --force) FORCE_INSTALL=true ;;
            --skip-gpg-check) SKIP_GPG_CHECK=true ;;  # Security: skip GPG verification
            *) echo "Unknown parameter: $1"; exit 1 ;;
        esac
        shift
    done
    
    # Load existing configuration if available
    load_env_settings "$INSTALL_DIR/.env"
    load_env_settings "$INSTALL_DIR/.env.agent"
    
    # Determine installation role
    # If no --role specified, default to Panel (Distribution Hub)
    # --role agent/frontend are for child nodes (via generated commands)
    if [[ -z "$ROLE" ]]; then
        ROLE="panel"
        log_info "No --role specified, defaulting to Panel installation (Distribution Hub)"
    fi
    
    
    if [[ "$ROLE" == "panel" || "$ROLE" == "both" ]]; then
        ensure_panel_port
    fi
    
    check_conflicts
    
    install_dependencies
    
    BUILD_SOURCE=""
    
    # Only clone/build for Panel (or both)
    # Agent and Frontend download pre-compiled binaries from Panel
    if [[ "$ROLE" == "panel" || "$ROLE" == "both" || "$ROLE" == "all" ]]; then
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
                log_info "Updating source code (Force Reset to origin/main)..."
                cd "$SOURCE_DIR"
                git fetch --all
                git reset --hard origin/main
            fi
            BUILD_SOURCE="$SOURCE_DIR"
        fi
        
        # Build binaries (Panel always compiles Panel + Agent + Frontend)
        build_binaries "$ROLE" "$BUILD_SOURCE"
        
        # Stop existing services
        systemctl stop exarobot &> /dev/null || true
        systemctl stop exarobot-agent &> /dev/null || true
    else
        log_info "Skipping build (Role: $ROLE - will download pre-compiled binary)"
    fi
    
    setup_directory
    
    # Copy Binaries
    if [[ "$ROLE" == "panel" || "$ROLE" == "both" || "$ROLE" == "all" ]]; then
        cp "$BUILD_SOURCE/target/release/exarobot" "$INSTALL_DIR/"
        chmod +x "$INSTALL_DIR/exarobot"
        
        # Copy Assets
        mkdir -p "$INSTALL_DIR/apps/panel"
        cp -r "$BUILD_SOURCE/apps/panel/assets" "$INSTALL_DIR/apps/panel/"
        
        # Copy Frontend Binary for Downloads
        ARCH=$(uname -m)
        if [ "$ARCH" = "x86_64" ]; then
            FE_SUFFIX="linux-amd64"
        elif [ "$ARCH" = "aarch64" ]; then
            FE_SUFFIX="linux-arm64"
        else
            FE_SUFFIX="linux-unknown"
        fi
        
        mkdir -p "$INSTALL_DIR/apps/panel/downloads"
        
        # Copy Frontend
        if [ -f "$BUILD_SOURCE/target/release/exarobot-frontend" ]; then
            cp "$BUILD_SOURCE/target/release/exarobot-frontend" "$INSTALL_DIR/apps/panel/downloads/exarobot-frontend-$FE_SUFFIX"
            log_success "Frontend binary cached: exarobot-frontend-$FE_SUFFIX"
        else
            log_warn "Frontend binary not found."
        fi
        
        # Copy Agent (for distribution)
        if [ -f "$BUILD_SOURCE/target/release/exarobot-agent" ]; then
            cp "$BUILD_SOURCE/target/release/exarobot-agent" "$INSTALL_DIR/apps/panel/downloads/exarobot-agent-$FE_SUFFIX"
            log_success "Agent binary cached: exarobot-agent-$FE_SUFFIX"
        else
             log_warn "Agent binary not found for cache."
        fi
        
        # Create Masquerade content for Hysteria 2
        MASQ_DIR="$INSTALL_DIR/apps/panel/assets/masquerade"
        mkdir -p "$MASQ_DIR"
        if [ ! -f "$MASQ_DIR/index.html" ]; then
            cat > "$MASQ_DIR/index.html" <<EOF
<!DOCTYPE html>
<html>
<head><title>502 Bad Gateway</title></head>
<body>
<center><h1>502 Bad Gateway</h1></center>
<hr><center>nginx</center>
</body>
</html>
EOF
        fi
        
        configure_panel
    fi
    
    if [[ "$ROLE" == "all" ]]; then
        seed_local_tokens
        
        # Configure Agent (local binary)
        log_info "Installing Agent for 'all' role..."
        cp "$BUILD_SOURCE/target/release/exarobot-agent" "$INSTALL_DIR/"
        chmod +x "$INSTALL_DIR/exarobot-agent"
        configure_agent
        
        # Configure Frontend (local binary)
        log_info "Installing Frontend for 'all' role..."
        configure_frontend
        
    elif [[ "$ROLE" == "agent" ]]; then
        # Agent downloads binary from Panel in configure_agent
        configure_agent
    elif [[ "$ROLE" == "both" ]]; then
        # Both: agent binary was compiled, just configure it
        cp "$BUILD_SOURCE/target/release/exarobot-agent" "$INSTALL_DIR/"
        chmod +x "$INSTALL_DIR/exarobot-agent"
        configure_agent
    elif [[ "$ROLE" == "frontend" ]]; then
        # Frontend downloads binary from Panel in configure_frontend
        configure_frontend
    elif [[ "$ROLE" == "panel" ]]; then
        # Only Panel: ensure agent services are gone
        log_info "Ensuring Agent service is disabled (Role: $ROLE)..."
        systemctl stop exarobot-agent &> /dev/null || true
        systemctl disable exarobot-agent &> /dev/null || true
        systemctl stop sing-box &> /dev/null || true
        systemctl disable sing-box &> /dev/null || true
    fi
    
    if [[ "$ROLE" == "panel" ]]; then
        # Ensure we don't have a dangling panel service if we are Agent-only
        # (Wait, if ROLE is panel we WANT exarobot.service, so this logic needs to be inverse)
        :
    elif [[ "$ROLE" == "agent" ]]; then
        log_info "Ensuring Panel service is disabled (Role: $ROLE)..."
        systemctl stop exarobot &> /dev/null || true
        systemctl disable exarobot &> /dev/null || true
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
        echo "  Panel: journalctl -u exarobot -f"
        echo "  Agent: journalctl -u exarobot-agent -f"
    fi

    
    echo ""
    echo ""
    log_success "Installation Complete!"
    if [[ "$ROLE" == "panel" || "$ROLE" == "both" || "$ROLE" == "all" ]]; then
        echo -e "${CYAN}--------------------------------------------------${NC}"
        echo -e "Panel Address : https://${DOMAIN}${ADMIN_PATH}"
        if [[ "$ROLE" == "all" ]]; then
            echo -e "All-in-One    : Panel + Agent + Frontend (Local)"
        fi
        echo -e "Admin Creation: cd ${INSTALL_DIR} && ./exarobot admin reset-password <user> <pass>"
        echo -e "${CYAN}--------------------------------------------------${NC}"
    fi
}

main "$@"
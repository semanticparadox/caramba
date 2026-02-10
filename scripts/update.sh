#!/bin/bash
# ==================================================
# ExaRobot Updater
# ==================================================
# Automatically detects if this is a Clean or Source install
# and updates accordingly.
# ==================================================

set -e

INSTALL_DIR="/opt/exarobot"
SOURCE_DIR="$INSTALL_DIR/source"
TEMP_BUILD_DIR="/tmp/exarobot_update_build"
REPO_URL="https://github.com/semanticparadox/EXA-ROBOT.git"

# Colors
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info() { echo -e "${CYAN}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }

if [ "$EUID" -ne 0 ]; then
    echo "Please run as root"
    exit 1
fi

log_info "Checking installation type..."

IS_CLEAN=true
if [ -d "$SOURCE_DIR/.git" ]; then
    IS_CLEAN=false
    log_info "Detected: Source Installation"
else
    log_info "Detected: Clean Installation (Binaries only)"
fi

BUILD_PATH=""

if [ "$IS_CLEAN" = true ]; then
    log_info "Cloning latest source to temp..."
    rm -rf "$TEMP_BUILD_DIR"
    git clone "$REPO_URL" "$TEMP_BUILD_DIR"
    BUILD_PATH="$TEMP_BUILD_DIR"
else
    log_info "Pulling latest changes..."
    cd "$SOURCE_DIR"
    git fetch --all
    git reset --hard origin/main
    BUILD_PATH="$SOURCE_DIR"
fi

# Determine what to build based on existing binaries
BUILD_PANEL=false
BUILD_AGENT=false

if [ -f "$INSTALL_DIR/exarobot" ]; then BUILD_PANEL=true; fi
if [ -f "$INSTALL_DIR/exarobot-agent" ]; then BUILD_AGENT=true; fi

cd "$BUILD_PATH"

# Dummy DB check (required for panel build)
if [ "$BUILD_PANEL" = true ]; then
    if [ ! -f "build_db.sqlite" ] && [ ! -f "db.sqlite" ] && [ ! -f "$INSTALL_DIR/exarobot.db" ]; then
          # Very edge case, but safe fallback
          touch build_db.sqlite
          export DATABASE_URL="sqlite://build_db.sqlite"
          if [ -f "apps/panel/migrations/001_complete_schema.sql" ]; then
                sqlite3 build_db.sqlite < apps/panel/migrations/001_complete_schema.sql
          fi
    elif [ -f "$INSTALL_DIR/exarobot.db" ]; then
          export DATABASE_URL="sqlite://$INSTALL_DIR/exarobot.db"
    fi
    
    log_info "Building Panel..."
    cargo build -p exarobot --release --quiet
fi

if [ "$BUILD_AGENT" = true ]; then
    log_info "Building Agent..."
    cargo build -p exarobot-agent --release --quiet
fi

# Update Binaries
log_info "Stopping services..."
systemctl stop exarobot-panel || true
systemctl stop exarobot-agent || true

log_info "Updating binaries..."
if [ "$BUILD_PANEL" = true ]; then
    cp target/release/exarobot "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/exarobot"
fi

if [ "$BUILD_AGENT" = true ]; then
    cp target/release/exarobot-agent "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/exarobot-agent"
fi

# Fix migration checksums (in case init.sql was modified since last apply)
if [ "$BUILD_PANEL" = true ] && [ -f "$INSTALL_DIR/exarobot.db" ]; then
    log_info "Resetting migration checksums..."
    sqlite3 "$INSTALL_DIR/exarobot.db" "DELETE FROM _sqlx_migrations;" 2>/dev/null || true
fi

# Restart
log_info "Restarting services..."
if [ "$BUILD_PANEL" = true ]; then systemctl start exarobot-panel; fi
if [ "$BUILD_AGENT" = true ]; then systemctl start exarobot-agent; fi

# Cleanup
if [ "$IS_CLEAN" = true ]; then
    rm -rf "$TEMP_BUILD_DIR"
fi

log_success "Update Complete!"

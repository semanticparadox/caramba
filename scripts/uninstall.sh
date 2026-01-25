#!/bin/bash
# ==================================================
# ExaRobot Uninstaller
# ==================================================

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

INSTALL_DIR="/opt/exarobot"

log_info() { echo -e "${CYAN}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }

check_root() {
    if [ "$EUID" -ne 0 ]; then
        echo -e "${RED}Please run as root${NC}"
        exit 1
    fi
}

main() {
    check_root
    
    echo -e "${RED}!!! WARNING !!!${NC}"
    echo "This will uninstall ExaRobot components."
    echo ""
    echo "Select components to uninstall:"
    echo "1) Panel (Service, Binary, Config)"
    echo "2) Agent (Service, Binary, Config)"
    echo "3) Everything (Completely remove /opt/exarobot)"
    echo "4) Cancel"
    echo "4) Cancel"
    
    # Use robust reading due to pipe usage (curl | bash)
    set +e
    if [ -t 0 ]; then
        read -p "Choice [1-4]: " CHOICE
    else
        echo -n "Choice [1-4]: "
        read -r CHOICE < /dev/tty
    fi
    set -e
    
    [[ "$CHOICE" == "4" ]] && exit 0
    
    # Panel
    if [[ "$CHOICE" == "1" || "$CHOICE" == "3" ]]; then
        if systemctl is-active --quiet exarobot-panel; then
            log_info "Stopping Panel service..."
            systemctl stop exarobot-panel || true
        fi
        log_info "Disabling Panel service..."
        systemctl disable exarobot-panel || true
        rm -f /etc/systemd/system/exarobot-panel.service
        
        log_info "Removing Panel binary..."
        rm -f "$INSTALL_DIR/exarobot"
        
        # Only remove config/db if requested or explicit?
        # Option 3 implies everything. Option 1 implies just "Panel component"
        if [[ "$CHOICE" == "3" ]]; then
             # handled at end
             true
        else
         else
             set +e
             if [ -t 0 ]; then
                 read -p "Remove Panel Database and Config? (y/N): " RMPANEL
             else
                 echo -n "Remove Panel Database and Config? (y/N): "
                 read -r RMPANEL < /dev/tty
             fi
             set -e
             
             if [[ "$RMPANEL" == "y" ]]; then
                rm -f "$INSTALL_DIR/exarobot.db"
                rm -f "$INSTALL_DIR/.env"
                log_info "Panel data removed."
             fi
        fi
    fi
    
    # Agent
    if [[ "$CHOICE" == "2" || "$CHOICE" == "3" ]]; then
        if systemctl is-active --quiet exarobot-agent; then
            log_info "Stopping Agent service..."
            systemctl stop exarobot-agent || true
        fi
        log_info "Disabling Agent service..."
        systemctl disable exarobot-agent || true
        rm -f /etc/systemd/system/exarobot-agent.service
        
        # Remove Sing-box override
        rm -f /etc/systemd/system/sing-box.service.d/override.conf
        rmdir /etc/systemd/system/sing-box.service.d 2>/dev/null || true
        
        log_info "Removing Agent binary..."
        rm -f "$INSTALL_DIR/exarobot-agent"
        
        if [[ "$CHOICE" != "3" ]]; then
             set +e
             if [ -t 0 ]; then
                 read -p "Remove Agent Config? (y/N): " RMAGENT
             else
                 echo -n "Remove Agent Config? (y/N): "
                 read -r RMAGENT < /dev/tty
             fi
             set -e

             if [[ "$RMAGENT" == "y" ]]; then
                rm -f "$INSTALL_DIR/.env.agent"
                log_info "Agent data removed."
             fi
        fi
    fi
    
    systemctl daemon-reload
    
    # Full Cleanup
    if [[ "$CHOICE" == "3" ]]; then
    # Full Cleanup
    if [[ "$CHOICE" == "3" ]]; then
        set +e
        if [ -t 0 ]; then
            read -p "Are you sure you want to delete ALL data in $INSTALL_DIR? (y/N): " CONFIRM
        else
            echo -n "Are you sure you want to delete ALL data in $INSTALL_DIR? (y/N): "
            read -r CONFIRM < /dev/tty
        fi
        set -e
        
        if [[ "$CONFIRM" == "y" ]]; then
            log_info "Removing directory $INSTALL_DIR..."
            rm -rf "$INSTALL_DIR"
            log_success "All ExaRobot files removed."
        else
            log_info "Skipping directory removal."
        fi
    else
        # Try to remove dir if empty
        rmdir "$INSTALL_DIR" 2>/dev/null || true
    fi
    
    log_success "Uninstallation process finished."
}

main "$@"

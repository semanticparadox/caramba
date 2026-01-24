#!/bin/bash
# ==================================================
# EXA ROBOT Uninstaller
# ==================================================

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }

check_root() {
    if [ "$EUID" -ne 0 ]; then
        echo -e "${RED}Please run as root${NC}"
        exit 1
    fi
}

main() {
    check_root
    
    echo "Select components to uninstall:"
    echo "1) Panel only"
    echo "2) Agent only"
    echo "3) Both"
    read -p "Choice [1-3]: " CHOICE
    
    case $CHOICE in
        1|3)
            log_info "Stopping and removing Panel..."
            systemctl stop exarobot-panel || true
            systemctl disable exarobot-panel || true
            rm -f /etc/systemd/system/exarobot-panel.service
            rm -rf /opt/exarobot/panel
            ;;
    esac
    
    case $CHOICE in
        2|3)
            log_info "Stopping and removing Agent..."
            systemctl stop exarobot-agent || true
            systemctl disable exarobot-agent || true
            rm -f /etc/systemd/system/exarobot-agent.service
            rm -rf /opt/exarobot/agent
            ;;
    esac
    
    systemctl daemon-reload
    
    read -p "Remove source code from /opt/exarobot? (y/N): " REMOVE_SRC
    if [[ "$REMOVE_SRC" == "y" ]]; then
        rm -rf /opt/exarobot
        log_info "Source code removed"
    fi
    
    log_info "Uninstallation complete"
}

main "$@"

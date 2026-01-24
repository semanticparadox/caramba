#!/bin/bash
# EXA ROBOT Node Setup Script
# This script is served by the panel to bootstrap new nodes.

set -e

# Default values (will be replaced by the panel if templated, strictly speaking checking ENV or args)
# In the V2 architecture, the user copies a command that looks like:
# curl ... | bash -s -- --token <token> --panel <url>

PANEL_URL=""
JOIN_TOKEN=""
NODE_DIR="/opt/exarobotnode"

# Parse arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --panel) PANEL_URL="$2"; shift ;;
        --token) JOIN_TOKEN="$2"; shift ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

if [ -z "$PANEL_URL" ] || [ -z "$JOIN_TOKEN" ]; then
    echo "Error: --panel and --token are required"
    exit 1
fi

echo "🚀 EXA ROBOT Node Setup"
echo "--------------------------------"
echo "Panel: $PANEL_URL"
echo "Token: $JOIN_TOKEN"
echo "--------------------------------"

# 1. Install Dependencies (sing-box)
echo "📦 Installing sing-box..."
curl -fsSL https://sing-box.app/gpg.key -o /etc/apt/keyrings/sagernet.asc
chmod a+r /etc/apt/keyrings/sagernet.asc
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/sagernet.asc] https://deb.sagernet.org/ * *" | \
    tee /etc/apt/sources.list.d/sagernet.list > /dev/null
apt-get update -qq
apt-get install -y sing-box -qq

# 2. Setup Directory
echo "📂 Creating directories..."
mkdir -p "$NODE_DIR"/{logs,scripts}

# 3. Create/Update Agent Config
# Note: The actual agent binary is usually installed via the main installer for the 'agent' role.
# This script is for 'Smart Setup' which might be different. 
# However, if we assume the user uses the universal installer, this script might just configure it.
# BUT, the V2 plan implies the agent is a Rust binary. 
# If this script is just for "registering" a node, it might just write the .env?

# Let's assume this script installs the Rust agent or configures it.
# For now, let's just make sure the environment is set up for the agent if it exists.

if [ -f "/opt/exarobot/agent/exarobot-agent" ]; then
    echo "⚙️ Configuring Agent..."
    cat > /opt/exarobot/agent/.env <<EOF
PANEL_URL=$PANEL_URL
NODE_TOKEN=$JOIN_TOKEN
CONFIG_PATH=/etc/sing-box/config.json
EOF
    systemctl restart exarobot-agent
    echo "✅ Agent configured and restarted!"
else
    echo "⚠️ Agent binary not found. Please install the agent using the universal installer first."
    echo "Run: curl .../install.sh | sudo bash"
fi

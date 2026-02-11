#!/bin/bash
# EXA ROBOT Node Setup Script
# DEPRECATED: Use install.sh --role agent instead.
# This script is served by the panel to bootstrap new nodes (Legacy).

echo "⚠️  WARNING: This script is deprecated. Please use the universal installer:"
echo "   curl .../install.sh | bash -s -- --role agent --panel ... --token ..."
sleep 3

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

# 4. Install & Configure Nginx (Corporate Mimicry - Economic Shield)
echo "🔒 Setting up Corporate Mimicry (Nginx)..."

# Detect OS/Distro for apt vs yum (Assuming apt given existing code uses apt-get)
if command -v apt-get &> /dev/null; then
    apt-get install -y nginx -qq
fi

# Download Mimicry Template
mkdir -p /usr/share/nginx/html
# Remove default index to verify mimics work
rm -f /usr/share/nginx/html/index.nginx-debian.html
rm -f /usr/share/nginx/html/index.html

echo "📥 Downloading Corporate Identity..."
curl -sL "$PANEL_URL/assets/corporate_mimicry.html" -o /usr/share/nginx/html/corporate_mimicry.html

# Apply Nginx Config
# Note: This config assumes Sing-box/Xray listens on 10000 locally
cat > /etc/nginx/nginx.conf <<EOF
user  www-data;
worker_processes  auto;

error_log  /var/log/nginx/error.log notice;
pid        /var/run/nginx.pid;

events {
    worker_connections  1024;
}

stream {
    # 1. Inspect SNI (Server Name Indication) without terminating SSL
    map \$ssl_preread_server_name \$backend_name {
        hostnames;
        
        # Sing-box Reality/Hysteria port
        default                singbox_backend;
    }

    upstream singbox_backend {
        server 127.0.0.1:10000; 
    }

    # 443 Multiplexer (SNI Routing)
    server {
        listen 443 reuseport;
        listen [::]:443 reuseport;
        
        proxy_pass \$backend_name;
        ssl_preread on; 
    }
}

http {
    include       /etc/nginx/mime.types;
    default_type  application/octet-stream;
    
    # Hide Nginx Version
    server_tokens off;

    server {
        listen 80 default_server;
        server_name _;
        
        # Redirect all HTTP to HTTPS or show Mimicry?
        # Standard Corp VPNs redirect to HTTPS.
        # But here we show the decoy on HTTP 80 directly to avoid SSL complication on port 80
        # OR we can serve it on a fallback port if 443 stream fails? 
        # Stream block handles 443. This HTTP block handles 80.
        
        # Decoy "Corporate VPN" site for unauthorized scanners
        location / {
            root   /usr/share/nginx/html;
            index  corporate_mimicry.html;
            try_files \$uri \$uri/ /corporate_mimicry.html;
        }
    }
}
EOF

# Restart Nginx
systemctl restart nginx
echo "✅ Corporate Mimicry Active (Port 80/443 Multiplexed)!"


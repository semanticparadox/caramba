#!/bin/bash
# EXA ROBOT Doomsday Protocol (Hans ICMP Tunnel)
# Usage: ./setup_hans.sh --password <PASSWORD>

PASSWORD=""

for i in "$@"
do
case $i in
    -p=*|--password=*)
    PASSWORD="${i#*=}"
    shift
    ;;
    *)
    ;;
esac
done

if [ -z "$PASSWORD" ]; then
    echo "Usage: ./setup_hans.sh --password=<PASSWORD>"
    exit 1
fi

echo "🚀 Setting up ICMP Tunnel (Hans)..."

# 1. Install Build Dependencies
if command -v apt-get &> /dev/null; then
    apt-get update -qq
    apt-get install -y build-essential git net-tools -qq
fi

# 2. Clone and Build
if [ -d "/opt/hans" ]; then
    echo "Hans already installed."
else
    git clone https://github.com/friedrich/hans.git /opt/hans
    cd /opt/hans
    make
fi

# 3. Create Service
echo "⚙️ Creating Systemd Service..."
cat > /etc/systemd/system/hans.service <<EOF
[Unit]
Description=Hans ICMP Tunnel
After=network.target

[Service]
# Server mode, IP range 10.1.2.0
ExecStart=/opt/hans/hans -s 10.1.2.0 -p $PASSWORD -r -u nobody -v
Restart=always
User=root
# Capability needed for raw sockets
AmbientCapabilities=CAP_NET_RAW

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable hans
systemctl restart hans

# 4. Enable IP Forwarding
echo "net.ipv4.ip_forward=1" >> /etc/sysctl.conf
sysctl -p

# 5. NAT Rules (iptables)
# Assuming eth0 is interface
IFACE=$(ip route get 8.8.8.8 | grep -oP 'dev \K\S+')
iptables -t nat -A POSTROUTING -s 10.1.2.0/24 -o $IFACE -j MASQUERADE

echo "✅ Hans ICMP Tunnel Active!"
echo "---------------------------------------------------"
echo "Server IP: $(curl -s ifconfig.me)"
echo "Password:  $PASSWORD"
echo "Client Command (Linux): sudo hans -c <SERVER_IP> -p $PASSWORD"
echo "---------------------------------------------------"

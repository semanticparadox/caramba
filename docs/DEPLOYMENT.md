# Deployment Guide

This guide covers the deployment of the Caramba Panel and its components.

## Prerequisites

*   Linux Server (Ubuntu 22.04+ recommended)
*   PostgreSQL 15+
*   Redis 7+
*   Domain name pointing to server IP
*   SSL Certificates (Let's Encrypt handling recommended via Caddy/Nginx)

## Method 1: Automatic Script (Recommended)

We provide an all-in-one installation script that sets up the Panel, Database, Redis, and Systemd services.

```bash
curl -sSL https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh | sudo bash
```

Follow the interactive prompts to configure your domain and admin user.

Role-based installs are also supported:

```bash
# Node with enrollment key
curl -sSL https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh \
  | sudo bash -s -- --role node --panel "https://panel.example.com" --token "EXA-ENROLL-XXXX"

# Frontend/sub edge
curl -sSL https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh \
  | sudo bash -s -- --role frontend --panel "https://panel.example.com" --domain "sub.example.com" --token "frontend_token"
```

## Method 2: Manual Installation (Systemd)

### 1. Database Setup
```sql
CREATE DATABASE caramba;
CREATE USER caramba WITH PASSWORD 'secure_password';
GRANT ALL PRIVILEGES ON DATABASE caramba TO caramba;
```

### 2. Environment
Create `/opt/caramba/.env`:

```bash
DATABASE_URL=postgres://caramba:secure_password@localhost/caramba
REDIS_URL=redis://127.0.0.1:6379
SERVER_DOMAIN=panel.yourdomain.com
BOT_TOKEN=123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11
ADMIN_PATH=/secret-admin
```

### 3. Binary Installation
Download the latest release to `/usr/local/bin/caramba-panel` and make it executable.

### 4. Systemd Service
Create `/etc/systemd/system/caramba-panel.service`:

```ini
[Unit]
Description=Caramba VPN Panel
After=network.target postgresql.service redis-server.service

[Service]
User=root
WorkingDirectory=/opt/caramba
ExecStart=/usr/local/bin/caramba-panel serve
EnvironmentFile=/opt/caramba/.env
Restart=always

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
systemctl daemon-reload
systemctl enable --now caramba-panel
```

## Method 3: Docker (Coming Soon)

Docker support is planned for the next release.

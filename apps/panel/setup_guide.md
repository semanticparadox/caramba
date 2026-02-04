# CARAMBA Deployment Guide 📦

This guide covers the deployment of the CARAMBA Panel on a Linux server (Ubuntu/Debian recommended).

## 1. Environment Preparation

### Install Dependencies
```bash
sudo apt update && sudo apt install -y curl git build-essential
```

### Install Docker
```bash
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh
sudo usermod -aG docker $USER
# Log out and log back in
```

## 2. Deploy CARAMBA

### Clone Repository
```bash
git clone <repo-url> /opt/caramba
cd /opt/caramba
```

### Configure Environment
1.  **Create `.env` file:**
    ```bash
    cp .env.example .env
    ```
2.  **Generate Keys:**
    - `BOT_TOKEN`: Get from @BotFather.
    - `ADMIN_PASSWORD_HASH`: Generate a bcrypt hash for your password.
    - `ENCRYPTION_KEY`: Generate a random 32-char string.

### Config Files
Ensure `config/` directory exists and has permissions.
```bash
mkdir -p config
```

### Start Services
```bash
docker-compose up -d --build
```

## 3. Verify Installation

Check logs to ensure everything is running:
```bash
docker-compose logs -f panel
```

Access the panel at:
`http://<SERVER_IP>:3000/admin`

## 4. Reverse Proxy (Nginx + SSL)

It is highly recommended to use Nginx with SSL (Certbot).

### Install Nginx
```bash
sudo apt install -y nginx certbot python3-certbot-nginx
```

### Configure Nginx
Create `/etc/nginx/sites-available/caramba`:
```nginx
server {
    server_name panel.yourdomain.com;

    location / {
        proxy_pass http://localhost:3000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

### Enable & Secure
```bash
sudo ln -s /etc/nginx/sites-available/caramba /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl reload nginx
sudo certbot --nginx -d panel.yourdomain.com
```

## 5. Maintenance

### Backup Database
The database is located at `exarobot.db`.
Use the **DB Export** tool in the Admin Panel (`Settings -> System`) to download backups.

### Updates
```bash
git pull
docker-compose up -d --build
```

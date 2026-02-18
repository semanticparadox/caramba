#!/bin/bash
set -euo pipefail

REPO="semanticparadox/caramba"
INSTALL_BIN_DIR="/usr/local/bin"
INSTALLER_ASSET="caramba-installer"
INSTALL_DIR="/opt/caramba"

ROLE="hub"
DOMAIN=""
SUB_DOMAIN=""
ADMIN_PATH=""
DB_PASS=""
PANEL_URL=""
TOKEN=""
REGION="global"
LISTEN_PORT="8080"
BOT_TOKEN=""
PANEL_TOKEN=""
VERSION_OVERRIDE=""

usage() {
  cat <<'EOF'
Usage:
  install.sh [options]

Roles:
  --role hub       Install hub (panel + sub; optional bot/node)
  --role panel     Install panel only
  --role node      Install node
  --role agent     Alias for node
  --role sub       Install sub/frontend edge
  --role frontend  Alias for sub
  --role bot       Install bot

Common options:
  --install-dir <dir>       Default: /opt/caramba
  --version <tag>           Force release tag (e.g. v0.3.0)

Hub/panel options:
  --domain <domain>
  --sub-domain <domain>
  --admin-path <path>
  --db-pass <password>

Node options:
  --panel <url>             Panel URL
  --token <token>           Join token OR enrollment key

Sub/frontend options:
  --panel <url>             Panel URL
  --domain <domain>         Frontend domain
  --token <token>           Internal/frontend auth token
  --region <name>           Default: global
  --listen-port <port>      Default: 8080

Bot options:
  --panel <url>             Panel URL
  --bot-token <token>       Telegram bot token
  --panel-token <token>     Optional panel API token for bot
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --role)
      ROLE="${2:-}"
      shift 2
      ;;
    --domain)
      DOMAIN="${2:-}"
      shift 2
      ;;
    --sub-domain)
      SUB_DOMAIN="${2:-}"
      shift 2
      ;;
    --admin-path)
      ADMIN_PATH="${2:-}"
      shift 2
      ;;
    --db-pass)
      DB_PASS="${2:-}"
      shift 2
      ;;
    --install-dir)
      INSTALL_DIR="${2:-}"
      shift 2
      ;;
    --panel)
      PANEL_URL="${2:-}"
      shift 2
      ;;
    --token)
      TOKEN="${2:-}"
      shift 2
      ;;
    --region)
      REGION="${2:-}"
      shift 2
      ;;
    --listen-port)
      LISTEN_PORT="${2:-}"
      shift 2
      ;;
    --bot-token)
      BOT_TOKEN="${2:-}"
      shift 2
      ;;
    --panel-token)
      PANEL_TOKEN="${2:-}"
      shift 2
      ;;
    --version)
      VERSION_OVERRIDE="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "❌ Unknown argument: $1"
      usage
      exit 1
      ;;
  esac
done

ROLE=$(echo "$ROLE" | tr '[:upper:]' '[:lower:]')
case "$ROLE" in
  agent) ROLE="node" ;;
  frontend) ROLE="sub" ;;
esac

case "$ROLE" in
  hub|panel|node|sub|bot) ;;
  *)
    echo "❌ Unsupported role: $ROLE"
    usage
    exit 1
    ;;
esac

if [[ -n "$VERSION_OVERRIDE" ]]; then
  VERSION="$VERSION_OVERRIDE"
else
  echo "🔍 Resolving latest version..."
  RELEASES_JSON=$(curl -fsSL "https://api.github.com/repos/$REPO/releases" || true)
  VERSION=$(printf "%s" "$RELEASES_JSON" \
    | grep -oE '"tag_name":[[:space:]]*"v[0-9]+\.[0-9]+\.[0-9]+"' \
    | head -n1 \
    | sed -E 's/.*"([^"]+)".*/\1/')

  if [[ -z "$VERSION" ]]; then
    LATEST_URL=$(curl -Ls -o /dev/null -w %{url_effective} "https://github.com/$REPO/releases/latest")
    VERSION=$(basename "$LATEST_URL")
  fi
fi

if [[ -z "${VERSION:-}" ]]; then
  echo "❌ Failed to detect release version."
  exit 1
fi

echo "✅ Using version: $VERSION"

DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$INSTALLER_ASSET"
echo "⬇️ Downloading installer from $DOWNLOAD_URL..."
TMP_BIN=$(mktemp)
curl -fL "$DOWNLOAD_URL" -o "$TMP_BIN"
chmod +x "$TMP_BIN"

echo "📦 Installing caramba to $INSTALL_BIN_DIR/caramba..."
mv "$TMP_BIN" "$INSTALL_BIN_DIR/caramba"
chmod +x "$INSTALL_BIN_DIR/caramba"

INSTALL_ARGS=(install)
case "$ROLE" in
  hub)
    INSTALL_ARGS+=(--hub --install-dir "$INSTALL_DIR")
    [[ -n "$DOMAIN" ]] && INSTALL_ARGS+=(--domain "$DOMAIN")
    [[ -n "$SUB_DOMAIN" ]] && INSTALL_ARGS+=(--sub-domain "$SUB_DOMAIN")
    [[ -n "$ADMIN_PATH" ]] && INSTALL_ARGS+=(--admin-path "$ADMIN_PATH")
    [[ -n "$DB_PASS" ]] && INSTALL_ARGS+=(--db-pass "$DB_PASS")
    [[ -n "$TOKEN" ]] && INSTALL_ARGS+=(--token "$TOKEN")
    [[ -n "$BOT_TOKEN" ]] && INSTALL_ARGS+=(--bot-token "$BOT_TOKEN")
    [[ -n "$REGION" ]] && INSTALL_ARGS+=(--region "$REGION")
    ;;
  panel)
    INSTALL_ARGS+=(--panel --install-dir "$INSTALL_DIR")
    [[ -n "$DOMAIN" ]] && INSTALL_ARGS+=(--domain "$DOMAIN")
    [[ -n "$ADMIN_PATH" ]] && INSTALL_ARGS+=(--admin-path "$ADMIN_PATH")
    [[ -n "$DB_PASS" ]] && INSTALL_ARGS+=(--db-pass "$DB_PASS")
    ;;
  node)
    if [[ -z "$PANEL_URL" ]]; then
      echo "❌ --panel is required for role node"
      exit 1
    fi
    if [[ -z "$TOKEN" ]]; then
      echo "❌ --token is required for role node"
      exit 1
    fi
    INSTALL_ARGS+=(--node --install-dir "$INSTALL_DIR" --panel-url "$PANEL_URL" --token "$TOKEN")
    ;;
  sub)
    if [[ -z "$PANEL_URL" ]]; then
      echo "❌ --panel is required for role sub/frontend"
      exit 1
    fi
    if [[ -z "$DOMAIN" ]]; then
      echo "❌ --domain is required for role sub/frontend"
      exit 1
    fi
    if [[ -z "$TOKEN" ]]; then
      echo "❌ --token is required for role sub/frontend"
      exit 1
    fi
    INSTALL_ARGS+=(--sub --install-dir "$INSTALL_DIR" --panel-url "$PANEL_URL" --domain "$DOMAIN" --token "$TOKEN" --region "$REGION" --listen-port "$LISTEN_PORT")
    ;;
  bot)
    if [[ -z "$PANEL_URL" ]]; then
      echo "❌ --panel is required for role bot"
      exit 1
    fi
    if [[ -z "$BOT_TOKEN" ]]; then
      echo "❌ --bot-token is required for role bot"
      exit 1
    fi
    INSTALL_ARGS+=(--bot --install-dir "$INSTALL_DIR" --panel-url "$PANEL_URL" --bot-token "$BOT_TOKEN")
    [[ -n "$PANEL_TOKEN" ]] && INSTALL_ARGS+=(--panel-token "$PANEL_TOKEN")
    ;;
esac

echo "🚀 Running installer role: $ROLE"
export CARAMBA_VERSION="$VERSION"
if [[ "$EUID" -ne 0 ]]; then
  sudo CARAMBA_VERSION="$VERSION" "$INSTALL_BIN_DIR/caramba" "${INSTALL_ARGS[@]}"
else
  "$INSTALL_BIN_DIR/caramba" "${INSTALL_ARGS[@]}"
fi

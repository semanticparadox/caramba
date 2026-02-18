#!/bin/bash
set -e

REPO="semanticparadox/caramba"
INSTALL_DIR="/usr/local/bin"
BINARY="caramba-installer"

echo "🔍 Resolving latest version..."
# Prefer stable semantic tags only (vX.Y.Z), ignore CI tags like v0.0.0-ci-*
RELEASES_JSON=$(curl -fsSL "https://api.github.com/repos/$REPO/releases" || true)
VERSION=$(printf "%s" "$RELEASES_JSON" \
  | grep -oE '"tag_name":[[:space:]]*"v[0-9]+\.[0-9]+\.[0-9]+"' \
  | head -n1 \
  | sed -E 's/.*"([^"]+)".*/\1/')

# Fallback to GitHub latest if API parsing fails
if [ -z "$VERSION" ]; then
  LATEST_URL=$(curl -Ls -o /dev/null -w %{url_effective} "https://github.com/$REPO/releases/latest")
  VERSION=$(basename "$LATEST_URL")
fi

if [ -z "$VERSION" ]; then
    echo "❌ Failed to detect latest version."
    exit 1
fi

echo "✅ Detected version: $VERSION"

DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$BINARY"

echo "⬇️ Downloading installer from $DOWNLOAD_URL..."
curl -L -o "$BINARY" "$DOWNLOAD_URL"
chmod +x "$BINARY"

# Install binary to /usr/local/bin
echo "📦 Installing caramba to $INSTALL_DIR/caramba..."
mv "$BINARY" "$INSTALL_DIR/caramba"
chmod +x "$INSTALL_DIR/caramba"

echo "🚀 Starting Caramba Installer..."
export CARAMBA_VERSION="$VERSION"

# Run the installer
if [ "$EUID" -ne 0 ]; then 
    sudo CARAMBA_VERSION="$VERSION" "$INSTALL_DIR/caramba" install --hub
else
    "$INSTALL_DIR/caramba" install --hub
fi

#!/bin/bash
# Prepares sqlx-data.json for offline building
set -e

echo "🔧 Preparing SQLx offline data..."

# Check for sqlx-cli
if ! command -v sqlx &> /dev/null; then
    echo "Installing sqlx-cli..."
    cargo install sqlx-cli --no-default-features --features postgres,sqlite
fi

# Create temp DB
export DATABASE_URL=sqlite://tmp_build.db
echo "Creating temporary database..."
sqlx database create
sqlx migrate run --source apps/panel/migrations

# Prepare
echo "Running cargo sqlx prepare..."
cargo sqlx prepare --workspace

# Cleanup
rm -f tmp_build.db*

echo "✅ Done! sqlx-data.json updated."

#!/bin/bash
set -e

# Define paths
PROJECT_ROOT=$(pwd)
PANEL_DIR="$PROJECT_ROOT/apps/panel"
DB_FILE="$PROJECT_ROOT/offline_gen.db"
MIGRATION_FILE="$PANEL_DIR/migrations/001_complete_schema.sql"

echo "🔧 Setting up temporary database for SQLx preparation..."

# 1. Clean old
rm -f "$DB_FILE"
touch "$DB_FILE"

# 2. Apply Schema
if [ -f "$MIGRATION_FILE" ]; then
    sqlite3 "$DB_FILE" < "$MIGRATION_FILE"
else
    echo "❌ Migration file not found at $MIGRATION_FILE"
    exit 1
fi

# 3. Prepare using cargo sqlx
export DATABASE_URL="sqlite://$DB_FILE"

echo "🚀 Running cargo sqlx prepare..."
# We need to run this inside the panel directory or point to it
cd "$PANEL_DIR"
cargo sqlx prepare --database-url "$DATABASE_URL"

# 4. Clean up
cd "$PROJECT_ROOT"
rm -f "$DB_FILE"

echo "✅ sqlx-data.json generated successfully!"

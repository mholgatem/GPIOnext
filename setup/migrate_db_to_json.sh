#!/bin/bash
# migrate_db_to_json.sh — One-time migration from config.db (SQLite) to gpionext.json.
#
# Usage: migrate_db_to_json.sh <config.db path> <gpionext.json output path>
#
# Reads the GPIOnext and I2C tables from the SQLite database and emits a JSON
# file matching the schema expected by gpionext-config.
# Requires: sqlite3

set -euo pipefail

DB="$1"
OUT="$2"

if [ ! -f "$DB" ]; then
    echo "Error: $DB not found" >&2
    exit 1
fi

# Dump devices table as JSON array
DEVICES=$(sqlite3 -json "$DB" \
    "SELECT device, name, type, command, pins FROM GPIOnext;" 2>/dev/null || echo "[]")

# Dump I2C tables
MCP=$(sqlite3 -json "$DB" \
    "SELECT bus, address, COALESCE(int_pin,'') AS int_pin FROM I2C_MCP23017;" 2>/dev/null || echo "[]")

ADS=$(sqlite3 -json "$DB" \
    "SELECT bus, address FROM I2C_ADS1115;" 2>/dev/null || echo "[]")

PCF=$(sqlite3 -json "$DB" \
    "SELECT bus, address, COALESCE(int_pin,'') AS int_pin FROM I2C_PCF8574;" 2>/dev/null || echo "[]")

# Write JSON using a here-document; Python formats it if available, else jq
JSON=$(cat <<EOF
{
  "version": 1,
  "daemon": {
    "combo_delay": 50,
    "key_hold_delay": 350,
    "debounce": 1,
    "pins": "default",
    "pulldown": true,
    "dev": false,
    "debug": false
  },
  "devices": $DEVICES,
  "i2c": {
    "mcp23017": $MCP,
    "ads1115": $ADS,
    "pcf8574": $PCF
  }
}
EOF
)

# Pretty-print if python3 or jq are available
if command -v python3 &>/dev/null; then
    echo "$JSON" | python3 -m json.tool > "$OUT"
elif command -v jq &>/dev/null; then
    echo "$JSON" | jq '.' > "$OUT"
else
    echo "$JSON" > "$OUT"
fi

echo "Migrated $DB → $OUT"

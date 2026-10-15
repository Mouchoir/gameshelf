#!/usr/bin/env sh
# Runs the development server with a local data folder and the fake connector (synthetic libraries).
# The key and database live in ./.data, which git ignores.
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$root/.data"
key="$root/.data/dev-key.txt"
[ -f "$key" ] || head -c 32 /dev/urandom | base64 > "$key"
export GAMESHELF_SECRET_KEY_FILE="$key"
export GAMESHELF_DATA_DIR="$root/.data"
export GAMESHELF_DEV_FAKE_CONNECTOR="${GAMESHELF_DEV_FAKE_CONNECTOR:-true}"
export GAMESHELF_LOG_FORMAT=pretty
export GAMESHELF_LOG="${GAMESHELF_LOG:-info}"
cd "$root"
exec cargo leptos watch

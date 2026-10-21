#!/usr/bin/env bash
# Build for the Pi and deploy: cross-compile → back up the DB → copy files → restart → check.
# Usage: PI_HOST=pi@raspberrypi [HEALTH_URL=http://100.x.y.z:8080/healthz] scripts/deploy.sh
# First-time setup of the Pi: docs/DEPLOY.md.
set -euo pipefail

PI_HOST="${PI_HOST:?set PI_HOST, e.g. PI_HOST=pi@raspberrypi}"
APP_DIR="${APP_DIR:-/opt/ttrpg-scheduling}"
HEALTH_URL="${HEALTH_URL:-}"
TARGET=aarch64-unknown-linux-musl

cd "$(dirname "$0")/.."

if [[ -n "$(git status --porcelain)" ]]; then
    echo "warning: uncommitted changes; the version will say -dirty" >&2
fi

echo "==> building for $TARGET"
cargo zigbuild --release --target "$TARGET"
BIN="target/$TARGET/release/ttrpg-scheduling"

# The backup runs the binary that is already on the Pi. On the first deploy there is none yet,
# and no database to back up either.
if ssh "$PI_HOST" "test -x '$APP_DIR/ttrpg-scheduling'"; then
    echo "==> backing up the database on $PI_HOST (new migrations may run on start)"
    ssh "$PI_HOST" "sudo systemctl start ttrpg-scheduling-backup.service"
else
    echo "==> first deploy: no binary on $PI_HOST yet, skipping the backup"
fi

echo "==> copying files to $PI_HOST:$APP_DIR"
rsync -az --delete templates static "$PI_HOST:$APP_DIR/"
# Copy under a temporary name, then rename: the running binary is never half-written.
rsync -az "$BIN" "$PI_HOST:$APP_DIR/ttrpg-scheduling.new"
ssh "$PI_HOST" "mv '$APP_DIR/ttrpg-scheduling.new' '$APP_DIR/ttrpg-scheduling' \
    && sudo systemctl restart ttrpg-scheduling.service"

echo "==> waiting for the app"
if [[ -n "$HEALTH_URL" ]]; then
    for _ in $(seq 1 20); do
        if curl -fsS "$HEALTH_URL"; then
            exit 0
        fi
        sleep 1
    done
    echo "error: $HEALTH_URL didn't answer; see: ssh $PI_HOST journalctl -u ttrpg-scheduling -n 50" >&2
    exit 1
else
    sleep 2
    ssh "$PI_HOST" "systemctl is-active ttrpg-scheduling.service && journalctl -u ttrpg-scheduling -n 3 --no-pager"
fi

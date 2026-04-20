#!/usr/bin/env sh
set -eu

BASE_DIR=${1:-"$HOME/harkonnen-local"}
TYPEDB_DIR="$BASE_DIR/typedb"
TYPEDB_DATA="$TYPEDB_DIR/data"
BIN_DIR="$BASE_DIR/bin"

mkdir -p "$TYPEDB_DATA" "$BIN_DIR"

cat > "$TYPEDB_DIR/docker-compose.yml" <<'COMPOSE'
services:
  typedb:
    image: typedb/typedb:latest
    container_name: harkonnen-typedb
    restart: unless-stopped
    ports:
      - "1729:1729"
      - "8000:8000"
    volumes:
      - ./data:/var/lib/typedb/data
COMPOSE

cat > "$BIN_DIR/typedb-up" <<WRAP
#!/usr/bin/env sh
set -eu
cd "$TYPEDB_DIR"
docker compose up -d
WRAP

cat > "$BIN_DIR/typedb-down" <<WRAP
#!/usr/bin/env sh
set -eu
cd "$TYPEDB_DIR"
docker compose down
WRAP

cat > "$BIN_DIR/typedb-logs" <<WRAP
#!/usr/bin/env sh
set -eu
cd "$TYPEDB_DIR"
docker compose logs -f
WRAP

cat > "$BIN_DIR/typedb-console" <<WRAP
#!/usr/bin/env sh
set -eu
cd "$TYPEDB_DIR"
docker compose exec typedb typedb console
WRAP

chmod +x \
  "$BIN_DIR/typedb-up" \
  "$BIN_DIR/typedb-down" \
  "$BIN_DIR/typedb-logs" \
  "$BIN_DIR/typedb-console"

printf 'Calvin Archive TypeDB scaffolded.\n'
printf 'Compose file: %s\n' "$TYPEDB_DIR/docker-compose.yml"
printf 'Data dir:     %s\n' "$TYPEDB_DATA"
printf 'Start:        %s\n' "$BIN_DIR/typedb-up"
printf 'Logs:         %s\n' "$BIN_DIR/typedb-logs"
printf 'Console:      %s\n' "$BIN_DIR/typedb-console"
printf 'Stop:         %s\n' "$BIN_DIR/typedb-down"

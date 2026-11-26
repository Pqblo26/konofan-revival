#!/usr/bin/env bash
# Arranca Axel en segundo plano, con una copia de la base de datos al día.
#
# Uso:
#   ./scripts/konofan/arrancar.sh
#
# - Si hoy no hay copia automática, la hace antes (copia-bd.sh).
# - Si Axel ya está en marcha, no hace nada.
# - El registro va a logs/axel-<fecha>.log; espera a que el servidor diga que ha arrancado.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
DIR="${KONOFAN_BACKUPS:-$HOME/backups}"
BIN="$REPO/target/release/axel"

if pgrep -x axel >/dev/null; then
  echo "Axel ya está en marcha."
  exit 0
fi
[ -x "$BIN" ] || { echo "Falta $BIN: compila con 'cargo build --release'." >&2; exit 1; }

if ! ls "$DIR"/axel-"$(date +%Y%m%d)"-*-auto.sql.gz >/dev/null 2>&1; then
  "$HERE/copia-bd.sh" || echo "AVISO: no he podido hacer la copia de hoy; arranco igualmente." >&2
fi

mkdir -p "$REPO/logs"
LOG="$REPO/logs/axel-$(date +%Y%m%d-%H%M).log"
cd "$REPO"
RUST_LOG="${RUST_LOG:-info}" nohup "$BIN" >"$LOG" 2>&1 &
PID=$!
for _ in $(seq 1 60); do
  if grep -q "api server started" "$LOG"; then
    echo "Axel arrancado (pid $PID). Registro: $LOG"
    exit 0
  fi
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "Axel se ha parado al arrancar. Últimas líneas de $LOG:" >&2
    tail -n 20 "$LOG" >&2
    exit 1
  fi
  sleep 1
done
echo "Axel sigue arrancando tras 60 s; mira $LOG" >&2

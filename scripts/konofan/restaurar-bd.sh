#!/usr/bin/env bash
# Vuelve la base de datos de Axel a una copia hecha con copia-bd.sh.
#
# Uso:
#   ./scripts/konofan/restaurar-bd.sh ~/backups/axel-20261126-0415-auto.sql.gz
#
# Sin argumentos lista las copias que hay. Antes de restaurar:
#   - Axel tiene que estar parado (el script lo comprueba).
#   - Se hace otra copia del estado actual («antes-de-restaurar»), por si acaso.
#   - Hay que escribir RESTAURAR para confirmar.
set -euo pipefail

DB="${KONOFAN_DB:-host=/var/run/postgresql dbname=axel}"
DIR="${KONOFAN_BACKUPS:-$HOME/backups}"
HERE="$(cd "$(dirname "$0")" && pwd)"

if [ $# -lt 1 ]; then
  sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
  echo
  echo "Copias en $DIR:"
  ls -1t "$DIR"/axel-*.sql.gz 2>/dev/null | head -n 30 || echo "  (ninguna)"
  exit 1
fi

FILE="$1"
[ -f "$FILE" ] || { echo "No existe: $FILE" >&2; exit 1; }
gzip -t "$FILE" || { echo "La copia está dañada: $FILE" >&2; exit 1; }

if pgrep -x axel >/dev/null; then
  echo "Axel está en marcha. Páralo antes de restaurar (pkill -x axel)." >&2
  exit 1
fi

echo "Se va a sustituir TODA la base de datos por: $FILE"
read -r -p "Escribe RESTAURAR para seguir: " ANSWER
[ "$ANSWER" = RESTAURAR ] || { echo "Cancelado."; exit 1; }

"$HERE/copia-bd.sh" antes-de-restaurar
gunzip -c "$FILE" | psql "$DB" -v ON_ERROR_STOP=1 -q -1 >/dev/null
echo "Restaurada. Ya puedes arrancar Axel."

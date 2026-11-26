#!/usr/bin/env bash
# Copia completa de la base de datos de Axel en ~/backups (comprimida).
#
# Uso:
#   ./scripts/konofan/copia-bd.sh [etiqueta]
#
# Ejemplos:
#   ./scripts/konofan/copia-bd.sh                    # ~/backups/axel-20261126-0415-auto.sql.gz
#   ./scripts/konofan/copia-bd.sh antes-migracion    # ~/backups/axel-20261126-0415-antes-migracion.sql.gz
#
# Las copias «auto» (sin etiqueta) se rotan: se guardan las 14 más recientes
# (KONOFAN_COPIAS para cambiarlo). Las que llevan etiqueta no se borran nunca.
# Para restaurar una: ./scripts/konofan/restaurar-bd.sh <fichero>
set -euo pipefail

LABEL="${1:-auto}"
case "$LABEL" in -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;; esac
[[ "$LABEL" =~ ^[A-Za-z0-9_-]+$ ]] || { echo "La etiqueta solo puede llevar letras, números, - y _" >&2; exit 1; }

DB="${KONOFAN_DB:-host=/var/run/postgresql dbname=axel}"
DIR="${KONOFAN_BACKUPS:-$HOME/backups}"
KEEP="${KONOFAN_COPIAS:-14}"
mkdir -p "$DIR"

FILE="$DIR/axel-$(date +%Y%m%d-%H%M)-$LABEL.sql.gz"
TMP="$FILE.tmp"
# --clean --if-exists: la copia se puede cargar encima de una base de datos existente
pg_dump --clean --if-exists --no-owner "$DB" | gzip > "$TMP"
mv "$TMP" "$FILE"
echo "Copia hecha: $FILE ($(du -h "$FILE" | cut -f1))"

if [ "$LABEL" = auto ]; then
  ls -1t "$DIR"/axel-*-auto.sql.gz 2>/dev/null | tail -n +"$((KEEP + 1))" | while read -r old; do
    rm -f -- "$old"
    echo "Borrada copia automática antigua: $old"
  done
fi

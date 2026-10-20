#!/usr/bin/env bash
# Falla si una crate prohibida aparece en más de una versión del grafo.
# Implementa el control de ADR-009 y ADR-013 sobre `cargo tree --duplicates`.
set -euo pipefail

# Crates que nunca deben duplicarse. Si este control falla, NO se sube la
# versión: se corrige la causa raíz (ver ADR-009 y ADR-013).
readonly PROHIBITED=(
    reqwest
    hyper
    hyper-util
    rustls
    zbus
    rusqlite
    libsqlite3-sys
    aws-lc-rs
    native-tls
    openssl
    openssl-sys
)

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo no está en PATH" >&2
    exit 2
fi

# Pares únicos "crate version" de las crates que cargo considera duplicadas.
# Se normaliza porque `cargo tree --duplicates` repite un nodo por cada padre y
# agrega el marcador "(*)" a los ya listados: lo que importa es el conjunto de
# versiones distintas, no la cantidad de apariciones.
pairs="$(cargo tree --workspace --duplicates 2>/dev/null \
    | sed -nE 's/^([A-Za-z0-9_-]+) v([0-9][^ ]*)( \(\*\))?$/\1 \2/p' \
    | sort -u)"

# Agrupa por crate y queda con las que tienen más de una versión distinta.
duplicated="$(awk '
    { versions[$1] = versions[$1] " " $2 }
    END {
        for (name in versions) {
            count = split(versions[name], parts, " ")
            if (count > 1) {
                printf "%s %s\n", name, versions[name]
            }
        }
    }
' <<<"$pairs" | sort)"

status=0
for crate in "${PROHIBITED[@]}"; do
    if match="$(grep -E "^${crate} " <<<"$duplicated")"; then
        echo "ERROR: '$crate' aparece en más de una versión:" >&2
        sed 's/^/  /' <<<"$match" >&2
        status=1
    fi
done

if [[ $status -eq 0 ]]; then
    echo "OK: ninguna crate prohibida está duplicada."
    if [[ -n "$duplicated" ]]; then
        echo
        echo "Duplicados informativos (justificar en el PR si son nuevos):"
        sed 's/^/  - /' <<<"$duplicated"
    fi
fi

exit $status

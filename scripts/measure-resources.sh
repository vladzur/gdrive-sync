#!/usr/bin/env bash
# Mide memoria (RSS y PSS) y CPU de uno o más procesos.
# Implementa la metodología de los $8.4 y $11.1 del plan de implementación:
#
#   - RSS y PSS se leen de /proc/<pid>/smaps_rollup.
#   - RSS es el criterio de aceptación (es lo que muestra el monitor del sistema).
#   - PSS se reporta en paralelo porque descuenta las librerías compartidas;
#     en GTK4 la diferencia entre ambos es grande (ADR-004).
#   - CPU se calcula por delta de utime+stime sobre el intervalo medido.
set -euo pipefail

# Números con punto decimal, independientemente del locale del usuario.
export LC_NUMERIC=C

interval=1
duration=60
declare -a targets=()

usage() {
    cat <<'EOF'
Uso: scripts/measure-resources.sh (-p PID | -n NOMBRE) [-p PID ...] [-d SEGUNDOS] [-i SEGUNDOS]

  -p PID        Mide el proceso indicado (repetible).
  -n NOMBRE     Mide todos los procesos cuyo nombre exacto sea NOMBRE (repetible).
  -d SEGUNDOS   Duración total de la medición (por defecto 60).
  -i SEGUNDOS   Intervalo entre muestras (por defecto 1).
  -h            Muestra esta ayuda.

Ejemplo:
  scripts/measure-resources.sh -n gdrive-sync-daemon -d 60
  scripts/measure-resources.sh -n gdrive-sync-daemon -p 4242 -d 30
EOF
}

while getopts ":p:n:d:i:h" opt; do
    case "$opt" in
        p) targets+=("pid:$OPTARG") ;;
        n) targets+=("name:$OPTARG") ;;
        d) duration="$OPTARG" ;;
        i) interval="$OPTARG" ;;
        h)
            usage
            exit 0
            ;;
        :)
            echo "error: -$OPTARG requiere un valor" >&2
            exit 2
            ;;
        \?)
            echo "error: opción desconocida: -$OPTARG" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ ${#targets[@]} -eq 0 ]]; then
    echo "error: indicar al menos un -p PID o -n NOMBRE" >&2
    usage >&2
    exit 2
fi

clock_ticks="$(getconf CLK_TCK 2>/dev/null || echo 100)"

rss_kb() { awk '/^Rss:/ { print $2 }' "/proc/$1/smaps_rollup" 2>/dev/null || echo 0; }
pss_kb() { awk '/^Pss:/ { print $2 }' "/proc/$1/smaps_rollup" 2>/dev/null || echo 0; }

# utime y stime son los campos 14 y 15 de /proc/<pid>/stat. Se recorta
# "pid (comm) " tomando el último ")" porque comm puede contener paréntesis.
cpu_ticks() {
    local stat
    stat="$(<"/proc/$1/stat")" || return 1
    stat="${stat##*) }"
    # shellcheck disable=SC2086
    set -- $stat
    echo $((${12:-0} + ${13:-0}))
}

resolve_pids() {
    local pids=()
    for target in "${targets[@]}"; do
        case "$target" in
            pid:*)
                local pid="${target#pid:}"
                [[ -d "/proc/$pid" ]] || {
                    echo "error: el PID $pid no existe" >&2
                    exit 1
                }
                pids+=("$pid")
                ;;
            name:*)
                local name="${target#name:}"
                local found
                found="$(pgrep -x -- "$name" || true)"
                if [[ -z "$found" ]]; then
                    echo "error: no hay procesos llamados '$name'" >&2
                    exit 1
                fi
                while IFS= read -r pid; do
                    [[ -n "$pid" ]] && pids+=("$pid")
                done <<<"$found"
                ;;
        esac
    done
    printf '%s\n' "${pids[@]}"
}

pids="$(resolve_pids)"
if [[ -z "$pids" ]]; then
    echo "error: no se resolvió ningún proceso" >&2
    exit 1
fi

declare -A start_ticks=()
declare -A sum_rss=()
declare -A sum_pss=()
declare -A samples=()
declare -A labels=()

for pid in $pids; do
    start_ticks["$pid"]="$(cpu_ticks "$pid" 2>/dev/null || echo 0)"
    labels["$pid"]="$(awk '{print $1}' "/proc/$pid/comm" 2>/dev/null || echo "?")"
done

end_time=$((SECONDS + duration))
while ((SECONDS < end_time)); do
    for pid in $pids; do
        [[ -d "/proc/$pid" ]] || continue
        sum_rss["$pid"]=$(( ${sum_rss[$pid]:-0} + $(rss_kb "$pid") ))
        sum_pss["$pid"]=$(( ${sum_pss[$pid]:-0} + $(pss_kb "$pid") ))
        samples["$pid"]=$(( ${samples[$pid]:-0} + 1 ))
    done
    sleep "$interval"
done

printf '\n%-24s %8s %12s %12s %8s\n' "PROCESO" "PID" "RSS (MB)" "PSS (MB)" "CPU (%)"
printf '%-24s %8s %12s %12s %8s\n' "------------------------" "--------" "------------" "------------" "--------"

total_rss=0
total_pss=0
for pid in $pids; do
    [[ ${samples[$pid]:-0} -gt 0 ]] || {
        printf '%-24s %8s %12s %12s %8s\n' "${labels[$pid]}" "$pid" "(terminó)" "-" "-"
        continue
    }

    avg_rss=$(( ${sum_rss[$pid]} / ${samples[$pid]} ))
    avg_pss=$(( ${sum_pss[$pid]} / ${samples[$pid]} ))
    total_rss=$((total_rss + avg_rss))
    total_pss=$((total_pss + avg_pss))

    end_ticks="$(cpu_ticks "$pid" 2>/dev/null || echo "${start_ticks[$pid]}")"
    cpu_pct="$(awk -v start="${start_ticks[$pid]}" -v end="$end_ticks" \
        -v hz="$clock_ticks" -v secs="$duration" \
        'BEGIN { printf "%.2f", (secs > 0) ? (end - start) / hz / secs * 100 : 0 }')"

    printf '%-24s %8s %12.1f %12.1f %8s\n' \
        "${labels[$pid]}" "$pid" \
        "$(awk -v kb="$avg_rss" 'BEGIN { print kb / 1024 }')" \
        "$(awk -v kb="$avg_pss" 'BEGIN { print kb / 1024 }')" \
        "$cpu_pct"
done

if (( $(wc -w <<<"$pids") > 1 )); then
    printf '%-24s %8s %12.1f %12.1f %8s\n' "TOTAL" "-" \
        "$(awk -v kb="$total_rss" 'BEGIN { print kb / 1024 }')" \
        "$(awk -v kb="$total_pss" 'BEGIN { print kb / 1024 }')" "-"
fi

cat <<EOF

Metodología: $((duration / interval)) muestras cada ${interval}s en reposo.
Criterio: RSS contra el presupuesto del \$11 del plan; PSS es informativo.
EOF

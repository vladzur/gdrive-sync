#!/usr/bin/env bash
# Prepara el entorno de desarrollo de gdrive-sync (T0.1, T0.2).
# Idempotente: puede ejecutarse varias veces sin efectos secundarios.
set -euo pipefail

with_tools=0
system_deps=1

for arg in "$@"; do
    case "$arg" in
        --with-tools) with_tools=1 ;;
        --no-system-deps) system_deps=0 ;;
        -h | --help)
            cat <<'EOF'
Uso: scripts/bootstrap.sh [opciones]

  --with-tools       Instala cargo-deny y git-cliff (compila, tarda varios minutos).
  --no-system-deps   No instala paquetes del sistema (útil dentro de un contenedor
                     o en Flatpak SDK, donde ya están disponibles).
  -h, --help         Muestra esta ayuda.
EOF
            exit 0
            ;;
        *)
            echo "error: opción desconocida: $arg" >&2
            exit 2
            ;;
    esac
done

log() { printf '\n==> %s\n' "$*"; }

install_system_deps() {
    local sudo_cmd=()
    if [[ $EUID -ne 0 ]]; then
        if command -v sudo >/dev/null 2>&1; then
            sudo_cmd=(sudo)
        else
            echo "error: se necesitan privilegios para instalar paquetes del sistema" >&2
            exit 1
        fi
    fi

    if command -v apt-get >/dev/null 2>&1; then
        # gtk4/libadwaita: solo necesarios para la feature `gui` (Hito 4).
        "${sudo_cmd[@]}" apt-get update
        "${sudo_cmd[@]}" apt-get install -y --no-install-recommends \
            build-essential pkg-config libgtk-4-dev libadwaita-1-dev gettext sqlite3
    elif command -v dnf >/dev/null 2>&1; then
        "${sudo_cmd[@]}" dnf install -y \
            gcc gcc-c++ make pkgconf-pkg-config gtk4-devel libadwaita-devel gettext sqlite
    elif command -v pacman >/dev/null 2>&1; then
        "${sudo_cmd[@]}" pacman -S --needed --noconfirm \
            base-devel pkgconf gtk4 libadwaita gettext sqlite
    else
        echo "error: distribución no soportada por este script." >&2
        echo "Instalar manualmente: compilador C, pkg-config, GTK4, Libadwaita y gettext." >&2
        exit 1
    fi
}

install_rust() {
    if command -v cargo >/dev/null 2>&1; then
        log "Rust ya está instalado: $(cargo --version)"
        return
    fi

    log "Instalando rustup (sin privilegios)"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain none --profile default --no-modify-path >/dev/null
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
}

install_cargo_tools() {
    local missing=()
    command -v cargo-deny >/dev/null 2>&1 || missing+=("cargo-deny")
    command -v git-cliff >/dev/null 2>&1 || missing+=("git-cliff")

    if [[ ${#missing[@]} -eq 0 ]]; then
        return
    fi

    log "Instalando herramientas de CI: ${missing[*]}"
    for tool in "${missing[@]}"; do
        cargo install --locked "$tool"
    done
}

if [[ $system_deps -eq 1 ]]; then
    log "Instalando dependencias del sistema"
    install_system_deps
else
    log "Omitiendo dependencias del sistema (--no-system-deps)"
fi

install_rust

log "Instalando la toolchain fijada en rust-toolchain.toml"
rustup show active-toolchain >/dev/null

if [[ $with_tools -eq 1 ]]; then
    install_cargo_tools
else
    log "Omitiendo cargo-deny/git-cliff (usar --with-tools para instalarlos)"
fi

log "Descargando dependencias del workspace"
cargo fetch

log "Entorno listo. Siguientes pasos:"
cat <<'EOF'
  cargo test --workspace     # tests
  cargo clippy --workspace --all-targets -- -D warnings
  cargo run -p gdrive-sync-cli -- --help
EOF

# gdrive-sync-app

Cliente nativo de Google Drive para Linux: sincronización bidireccional en
tiempo real entre un directorio local y Google Drive.

> `gdrive-sync-app` es el nombre visible de la aplicación (ventana, `.desktop` y
> metadatos). El repositorio se llama `gdrive-sync` y los binarios conservan
> `gdrive-sync` como nombre de comando.

- **Stack:** Rust, GTK4 / Libadwaita, Tokio, SQLite, Google Drive API v3.
- **Diseño:** [especificaci_n_t_cnica.md](./especificaci_n_t_cnica.md) (RFC).
- **Plan de ejecución:** [plan_de_implementacion.md](./plan_de_implementacion.md).

## Estado

Hitos 0 y 1 completados: workspace reproducible con CI, y núcleo de
autenticación OAuth2 con PKCE, almacenamiento de tokens en el gestor de
secretos y cliente REST de Drive.

| Hito | Alcance | Estado |
| --- | --- | --- |
| H0 | Bootstrap, toolchain, CI | ✅ |
| H1 | Núcleo CLI y OAuth2 (RF-01, RF-02) | ✅ |
| H2 | Persistencia y watcher (RF-03, RF-04) | pendiente |
| H3 | Motor de sincronización (RF-05, RF-06) | pendiente |
| H4 | GUI GTK4 / Libadwaita (RF-07) | pendiente |
| H5 | Daemon, bandeja y Flatpak (RF-08) | pendiente |

## Requisitos

- Linux con `inotify` (ext4, btrfs o xfs) y escritorio GNOME 46 o superior para la GUI.
- Rust: la toolchain la fija [rust-toolchain.toml](./rust-toolchain.toml).
  El MSRV soportado es **1.92** (`rust-version` en `Cargo.toml`).
- Para compilar la GUI: `libgtk-4-dev` y `libadwaita-1-dev`.

## Puesta en marcha

```bash
scripts/bootstrap.sh          # dependencias del sistema + rustup + toolchain
cargo test --workspace
cargo run -p gdrive-sync-cli -- --help
```

El workspace compila **sin GTK** por defecto. La capa gráfica se compila con
`--features gui` y requiere las librerías de desarrollo instaladas:

```bash
cargo check -p gdrive-sync-gui --features gui
```

## Uso

Primero hay que crear un cliente OAuth propio en Google Cloud y entregarle las
credenciales a la aplicación: ver [docs/GOOGLE_SETUP.md](./docs/GOOGLE_SETUP.md).

```bash
# Vincular la cuenta (abre el navegador)
cargo run -p gdrive-sync-cli -- auth login

# Ver el estado sin exponer los tokens
cargo run -p gdrive-sync-cli -- auth status

# Listar archivos de la raíz de Drive
cargo run -p gdrive-sync-cli -- ls --limit 20

# Listar el contenido de una carpeta remota
cargo run -p gdrive-sync-cli -- ls --remote documentos/fotos

# Eliminar los tokens almacenados
cargo run -p gdrive-sync-cli -- auth logout
```

Los tokens se guardan en el gestor de secretos del sistema (GNOME Keyring /
KWallet) y nunca se escriben en disco ni en los logs.

## Estructura

```
crates/
  core/      dominio compartido (sin red, sin GTK, sin base de datos)
  auth/      OAuth2 + PKCE y Secret Service
  drive/     cliente REST de Google Drive API v3
  db/        SQLite: nodos, cola de operaciones y conflictos
  watcher/   monitoreo del sistema de archivos (inotify)
  engine/    reconciliación, planificador y ejecutor
  gui/       GTK4 / Libadwaita (feature `gui`, Hito 4)
  cli/       binario `gdrive-sync`
docs/
  GOOGLE_SETUP.md       cómo crear el cliente OAuth propio
scripts/
  bootstrap.sh          prepara el entorno
  check-duplicates.sh   falla ante duplicados de dependencias prohibidos
  measure-resources.sh  mide RSS/PSS y CPU (RNF-01, RNF-02)
```

## Verificación

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/check-duplicates.sh
cargo deny check
```

## Licencia

Este proyecto se distribuye bajo la **GNU General Public License, versión 3 o
posterior** (`GPL-3.0-or-later`). El texto completo está en [LICENSE](./LICENSE).

Es una licencia *copyleft*: cualquier distribución de una versión modificada
debe publicarse bajo la misma licencia y ofrecer el código fuente.

Las dependencias del proyecto son permisivas (MIT, Apache-2.0, ISC, BSD y
similares), por lo que son compatibles con GPL-3. GTK4 y Libadwaita son
LGPL-2.1-or-later, que permite el relicenciamiento bajo GPL-3.

# Contribuir a gdrive-sync

## Convenciones

- **Identificadores en inglés** (funciones, tipos, variables, nombres de
  columnas). Los **comentarios y los textos dirigidos al usuario, en español**.
- **Commits en inglés** con [Conventional Commits](https://www.conventionalcommits.org/):
  `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `style`, `perf`, `ci`, `build`.
- El changelog se genera con `git-cliff` a partir de esos commits
  ([cliff.toml](./cliff.toml)).

## Ramas y pull requests

- Nunca se hace commit directo a `master`.
- Nombre de rama con prefijo por tipo: `feat/...`, `fix/...`, `chore/...`, `docs/...`.
- Un PR por unidad entregable. Los PRs van contra `master` y se integran con
  *squash merge*.
- La plantilla del PR (`.github/pull_request_template.md`) exige tres secciones
  en inglés: **Description**, **Changes** y **QA**.

## Definition of Done

- [ ] Tests unitarios junto al código, en el archivo de specs correspondiente.
- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `./scripts/check-duplicates.sh` (duplicados prohibidos: `reqwest`, `hyper`,
      `rustls`, `zbus`, `rusqlite`, `aws-lc-rs`, `native-tls`, `openssl`).
- [ ] `cargo deny check`
- [ ] Sin secretos en el código ni en los logs.
- [ ] Documentación afectada actualizada (README, `docs/`, ADR).
- [ ] Si el cambio toca una dependencia, actualizar el Anexo C del plan de
      implementación en el mismo PR.

## Dependencias

Las versiones se declaran **solo** en `[workspace.dependencies]` del
`Cargo.toml` raíz y los crates las heredan con `workspace = true` (ADR-013).
Antes de agregar una dependencia:

1. Verificar que su licencia esté en la allowlist de [deny.toml](./deny.toml).
2. Verificar que su MSRV no supere 1.92.
3. Correr `./scripts/check-duplicates.sh`; si el nuevo crate duplica uno
   prohibido, resolver la causa raíz en lugar de subir la versión.

## Recursos y rendimiento

El presupuesto de memoria y los criterios de aceptación están en el §11 del
plan. Para medir:

```bash
scripts/measure-resources.sh -n gdrive-sync-daemon -d 60
```

Se mide **RSS** (criterio) y **PSS** (informativo, descuenta librerías
compartidas). Cualquier PR que toque el motor debe adjuntar la medición.

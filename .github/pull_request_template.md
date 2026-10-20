## Description

<!-- Qué problema resuelve este cambio y por qué. En inglés, según el estándar del equipo. -->

## Changes

<!-- Lista de cambios concretos. Incluir las decisiones de diseño relevantes y los ADR afectados. -->

## QA

<!-- Pasos verificables que realizó quien abre el PR. Ser específicos. -->

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `./scripts/check-duplicates.sh`
- [ ] `cargo deny check`

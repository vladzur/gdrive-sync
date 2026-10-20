// SPDX-License-Identifier: GPL-3.0-or-later
//! Persistencia local en SQLite (ADR-002).
//!
//! Contiene el esquema versionado (`sync_state`, `sync_roots`, `sync_nodes`,
//! `pending_ops`, `conflicts`), las migraciones y los DAOs. Se implementa en el
//! Hito 2 (tareas T2.1 a T2.3).

/// Versión de esquema que este crate espera encontrar en la base.
///
/// Se usa con `PRAGMA user_version` para aplicar migraciones de forma
/// incremental e idempotente.
pub const SCHEMA_VERSION: u32 = 0;

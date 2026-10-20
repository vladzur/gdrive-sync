//! Dominio compartido de `gdrive-sync`.
//!
//! Este crate no depende de GTK, de la red ni de la base de datos: contiene los
//! tipos y las reglas que el resto del sistema comparte. La matriz de decisión
//! de sincronización (ADR-001) vivirá aquí como función pura, testeable sin I/O.

pub mod status;
pub mod tls;

pub use status::{ParseSyncStatusError, SyncStatus};

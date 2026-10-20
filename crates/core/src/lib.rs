// SPDX-License-Identifier: GPL-3.0-or-later
//! Dominio compartido de `gdrive-sync`.
//!
//! Este crate no depende de GTK, de la red ni de la base de datos: contiene los
//! tipos y las reglas que el resto del sistema comparte. La matriz de decisión
//! de sincronización (ADR-001) vivirá aquí como función pura, testeable sin I/O.

pub mod clock;
pub mod status;
pub mod tls;
pub mod token;

pub use clock::{Clock, SystemClock};
pub use status::{ParseSyncStatusError, SyncStatus};
pub use token::{SecretString, TokenSet, TokenStore, TokenStoreError};

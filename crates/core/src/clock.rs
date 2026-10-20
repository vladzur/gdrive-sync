// SPDX-License-Identifier: GPL-3.0-or-later
//! Reloj inyectable.
//!
//! La lógica de expiración de tokens depende del tiempo real; en los tests se
//! sustituye por un reloj controlado para no depender de `sleep`.

use std::time::SystemTime;

/// Fuente de tiempo del sistema.
pub trait Clock: Send + Sync {
    /// Instante actual.
    fn now(&self) -> SystemTime;
}

/// Reloj real.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

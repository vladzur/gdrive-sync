// SPDX-License-Identifier: GPL-3.0-or-later
//! Instalación del proveedor criptográfico de `rustls`.
//!
//! El proyecto usa `reqwest` con la feature `rustls-no-provider` para no
//! arrastrar `aws-lc-rs`, que exige `cmake` y `nasm` en el build y complica el
//! empaquetado dentro del sandbox Flatpak (ADR-009). Como contrapartida, el
//! proveedor `ring` debe instalarse una sola vez por proceso, antes de crear
//! cualquier cliente HTTPS.

use std::sync::Once;

/// Instala el proveedor `ring` como predeterminado del proceso.
///
/// Es idempotente: llamarla varias veces, o después de que otro componente haya
/// instalado un proveedor, no es un error. Las funciones binarias (`cli`,
/// `daemon`) la invocan al arrancar.
pub fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Si ya existe un proveedor instalado, se respeta el existente.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_a_provider_installed_and_is_idempotent() {
        init();
        init();

        assert!(
            rustls::crypto::CryptoProvider::get_default().is_some(),
            "debe existir un proveedor criptográfico después de init()"
        );
    }
}

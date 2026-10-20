// SPDX-License-Identifier: GPL-3.0-or-later
//! Autenticación OAuth 2.0 con PKCE (RF-01) y persistencia de tokens (RF-02).
//!
//! El flujo se compone de cuatro piezas que el llamador orquesta:
//!
//! 1. [`callback::bind`] abre un listener en `127.0.0.1` con puerto efímero.
//! 2. [`client::AuthClient::authorization_url`] genera la URL, el `state` y el
//!    verificador PKCE.
//! 3. [`browser::open`] abre el navegador y [`callback::await_callback`] espera
//!    el `code`, validando el `state`.
//! 4. [`client::AuthClient::exchange_code`] obtiene los tokens y
//!    [`store::KeyringTokenStore`] los persiste en Secret Service.

pub mod browser;
pub mod callback;
pub mod client;
pub mod http_client;
pub mod pkce;
pub mod store;

pub use client::{AuthClient, AuthError, OAuthSettings, REFRESH_MARGIN};

/// Scope de Google Drive requerido por la aplicación.
///
/// Es un scope *restringido*: sincronizar archivos preexistentes del usuario no
/// es posible con `drive.file`. La implicancia para la distribución está
/// documentada en ADR-005.
pub const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";

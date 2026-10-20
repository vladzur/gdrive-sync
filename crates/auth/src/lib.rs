//! Autenticación OAuth 2.0 con PKCE (RF-01) y persistencia de tokens (RF-02).
//!
//! El flujo completo, el listener loopback y el `TokenStore` sobre Secret
//! Service se implementan en el Hito 1 (tareas T1.1 a T1.6).

/// Scope de Google Drive requerido por la aplicación.
///
/// Es un scope *restringido*: sincronizar archivos preexistentes del usuario no
/// es posible con `drive.file`. La implicancia para la distribución está
/// documentada en ADR-005.
pub const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";

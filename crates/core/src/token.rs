// SPDX-License-Identifier: GPL-3.0-or-later
//! Tokens OAuth y su almacenamiento abstracto (RF-02).
//!
//! Los tipos de este módulo existen para que ningún token pueda terminar en un
//! log por accidente: `SecretString` enmascara `Debug` y `Display`, y
//! `TokenSet` se deriva de ellos (ADR-011).

use std::{
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Cadena sensible cuyo contenido nunca se imprime.
///
/// El valor se borra de la memoria al liberarse (`zeroize`). Para leerlo hay que
/// pedirlo de forma explícita con [`SecretString::expose`], lo que hace visible
/// en el código cualquier punto donde el secreto se usa en claro.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    /// Envuelve un valor sensible.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Expone el valor en claro. Solo para usarlo en peticiones salientes.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Indica si la cadena está vacía.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(***)")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// Conjunto de tokens de una cuenta.
///
/// `expires_at_ms` es el instante de expiración en milisegundos desde la época
/// Unix, en UTC. Se guarda como entero para que la serialización al gestor de
/// secretos sea estable y no dependa del formato de `SystemTime`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSet {
    /// Token de acceso vigente.
    pub access_token: SecretString,
    /// Token de renovación. Google solo lo entrega en el primer consentimiento.
    pub refresh_token: Option<SecretString>,
    /// Expiración del token de acceso, en milisegundos desde la época Unix.
    pub expires_at_ms: Option<u64>,
    /// Scopes concedidos por el usuario.
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl TokenSet {
    /// Construye un conjunto de tokens a partir de una duración de validez.
    pub fn new(
        access_token: SecretString,
        refresh_token: Option<SecretString>,
        expires_in: Option<Duration>,
        now: SystemTime,
        scopes: Vec<String>,
    ) -> Self {
        let expires_at_ms = expires_in.and_then(|ttl| {
            let expires = now.checked_add(ttl)?;
            expires
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok())
        });

        Self {
            access_token,
            refresh_token,
            expires_at_ms,
            scopes,
        }
    }

    /// Instante de expiración, si el proveedor lo informó.
    #[must_use]
    pub fn expires_at(&self) -> Option<SystemTime> {
        self.expires_at_ms
            .map(|ms| UNIX_EPOCH + Duration::from_millis(ms))
    }

    /// Indica si el token conviene renovarlo antes de usarlo.
    ///
    /// Se renueva de forma proactiva cuando falta menos que `margin` para que
    /// expire: así una transferencia larga no falla a mitad de camino por un
    /// token que venció durante la operación (T1.6).
    #[must_use]
    pub fn needs_refresh(&self, now: SystemTime, margin: Duration) -> bool {
        let Some(expires_at) = self.expires_at() else {
            return self.refresh_token.is_none() && self.access_token.is_empty();
        };

        match expires_at.duration_since(now) {
            Ok(remaining) => remaining <= margin,
            // Ya expiró.
            Err(_) => true,
        }
    }

    /// Indica si hay un token de renovación disponible.
    #[must_use]
    pub fn can_refresh(&self) -> bool {
        self.refresh_token
            .as_ref()
            .is_some_and(|token| !token.is_empty())
    }
}

/// Error de acceso al almacén de tokens.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenStoreError {
    /// El backend (Secret Service) no está disponible o rechazó la operación.
    #[error("no se pudo acceder al servicio de secretos: {0}")]
    Backend(String),
    /// El contenido almacenado no se pudo interpretar.
    #[error("los tokens almacenados están corruptos: {0}")]
    Corrupt(String),
}

/// Persistencia de los tokens de una cuenta.
///
/// Se define como trait para poder sustituir el gestor de secretos por un
/// almacén en memoria en los tests.
pub trait TokenStore {
    /// Carga los tokens, si existen.
    fn load(&self) -> impl Future<Output = Result<Option<TokenSet>, TokenStoreError>> + Send;

    /// Guarda o reemplaza los tokens.
    fn save(&self, tokens: &TokenSet) -> impl Future<Output = Result<(), TokenStoreError>> + Send;

    /// Elimina los tokens almacenados.
    fn delete(&self) -> impl Future<Output = Result<(), TokenStoreError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    #[test]
    fn secret_never_leaks_in_debug_or_display() {
        let secret = SecretString::new("super-secreto");

        assert_eq!(format!("{secret:?}"), "SecretString(***)");
        assert_eq!(format!("{secret}"), "***");
        assert!(!format!("{secret:?}").contains("super-secreto"));
    }

    #[test]
    fn tokens_do_not_leak_in_debug() {
        let tokens = TokenSet::new(
            SecretString::new("access-123"),
            Some(SecretString::new("refresh-456")),
            Some(Duration::from_secs(3600)),
            now(),
            vec!["drive".to_owned()],
        );

        let rendered = format!("{tokens:?}");
        assert!(
            !rendered.contains("access-123"),
            "el token de acceso se filtró"
        );
        assert!(
            !rendered.contains("refresh-456"),
            "el refresh token se filtró"
        );
    }

    #[test]
    fn computes_expiry_from_ttl() {
        let tokens = TokenSet::new(
            SecretString::new("a"),
            None,
            Some(Duration::from_secs(3600)),
            now(),
            Vec::new(),
        );

        assert_eq!(
            tokens.expires_at(),
            Some(now() + Duration::from_secs(3600)),
            "la expiración debe derivarse del TTL"
        );
    }

    #[test]
    fn refresh_is_required_inside_the_margin() {
        let tokens = TokenSet::new(
            SecretString::new("a"),
            Some(SecretString::new("r")),
            Some(Duration::from_secs(300)),
            now(),
            Vec::new(),
        );

        // Faltan 5 minutos: con un margen de 5 minutos hay que renovar ya.
        assert!(tokens.needs_refresh(now(), Duration::from_secs(300)));
        // Con un margen menor, todavía sirve.
        assert!(!tokens.needs_refresh(now(), Duration::from_secs(60)));
        // Pasado el vencimiento, siempre hay que renovar.
        assert!(tokens.needs_refresh(now() + Duration::from_secs(301), Duration::ZERO));
    }

    #[test]
    fn token_without_expiry_is_not_refreshed_proactively() {
        let tokens = TokenSet::new(
            SecretString::new("a"),
            Some(SecretString::new("r")),
            None,
            now(),
            Vec::new(),
        );

        assert!(!tokens.needs_refresh(now(), Duration::from_secs(3600)));
        assert!(tokens.can_refresh());
    }

    #[test]
    fn token_set_round_trips_through_json() {
        let tokens = TokenSet::new(
            SecretString::new("access"),
            Some(SecretString::new("refresh")),
            Some(Duration::from_secs(60)),
            now(),
            vec!["https://www.googleapis.com/auth/drive".to_owned()],
        );

        let json = serde_json::to_string(&tokens).expect("serializa");
        let parsed: TokenSet = serde_json::from_str(&json).expect("deserializa");

        assert_eq!(parsed, tokens);
    }
}

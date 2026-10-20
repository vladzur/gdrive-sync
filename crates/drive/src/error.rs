// SPDX-License-Identifier: GPL-3.0-or-later
//! Errores del cliente de Drive y su clasificación.
//!
//! La clasificación implementa la tabla de §4.6 del plan: decide si un fallo se
//! reintenta, si requiere renovar el token o si es definitivo.

use std::time::Duration;

use reqwest::StatusCode;

/// Error de una operación contra la API de Drive.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DriveError {
    /// El token de acceso no es válido o expiró (`401`).
    #[error("no autorizado (401); hay que renovar el token de acceso")]
    Unauthorized,
    /// La cuenta no tiene permiso para la operación (`403`).
    #[error("acceso denegado (403): {0}")]
    Forbidden(String),
    /// El recurso no existe o el usuario perdió el acceso (`404`).
    #[error("recurso no encontrado (404)")]
    NotFound,
    /// Se superó el límite de peticiones.
    #[error("límite de peticiones excedido")]
    RateLimited {
        /// Valor de la cabecera `Retry-After`, si el servidor la envió.
        retry_after: Option<Duration>,
    },
    /// Error interno del servidor (`5xx`).
    #[error("error del servidor ({status})")]
    Server {
        /// Código HTTP.
        status: u16,
    },
    /// Fallo de red o de transporte.
    #[error("error de red: {0}")]
    Network(String),
    /// La respuesta no tiene el contenido esperado.
    #[error("no se pudo interpretar la respuesta: {0}")]
    Decode(String),
    /// Código de estado no contemplado.
    #[error("respuesta inesperada ({status})")]
    Unexpected {
        /// Código HTTP.
        status: u16,
    },
}

/// Acción recomendada ante un error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Conviene reintentar con espera exponencial.
    Retryable,
    /// Hay que renovar el token y reintentar una vez.
    Auth,
    /// No se resuelve reintentando.
    Fatal,
}

impl DriveError {
    /// Clase de recuperación del error.
    #[must_use]
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Network(_) | Self::RateLimited { .. } | Self::Server { .. } => {
                ErrorClass::Retryable
            }
            Self::Unauthorized => ErrorClass::Auth,
            Self::Forbidden(_) | Self::NotFound | Self::Decode(_) | Self::Unexpected { .. } => {
                ErrorClass::Fatal
            }
        }
    }

    /// Espera indicada por el servidor, si la hubo.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after } => *retry_after,
            _ => None,
        }
    }

    /// Traduce una respuesta HTTP no exitosa.
    ///
    /// `403` es ambiguo en Drive: puede ser un permiso faltante (definitivo) o
    /// un límite de velocidad (reintentable). Se distingue por el cuerpo.
    pub(crate) fn from_response(
        status: StatusCode,
        body: &str,
        retry_after: Option<Duration>,
    ) -> Self {
        match status.as_u16() {
            401 => Self::Unauthorized,
            403 => {
                let is_rate_limit =
                    body.contains("rateLimitExceeded") || body.contains("userRateLimitExceeded");

                if is_rate_limit {
                    Self::RateLimited { retry_after }
                } else {
                    Self::Forbidden(body.to_owned())
                }
            }
            404 => Self::NotFound,
            429 => Self::RateLimited { retry_after },
            status if (500..=599).contains(&status) => Self::Server { status },
            status => Self::Unexpected { status },
        }
    }

    /// Traduce un error de transporte.
    pub(crate) fn from_reqwest(error: &reqwest::Error) -> Self {
        if error.is_decode() {
            Self::Decode(error.to_string())
        } else {
            Self::Network(error.to_string())
        }
    }
}

/// Extrae `Retry-After` en segundos.
pub(crate) fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_status_codes() {
        let cases = [
            (401, "", ErrorClass::Auth),
            (
                403,
                r#"{"error":{"errors":[{"reason":"forbidden"}]}}"#,
                ErrorClass::Fatal,
            ),
            (404, "", ErrorClass::Fatal),
            (429, "", ErrorClass::Retryable),
            (500, "", ErrorClass::Retryable),
            (503, "", ErrorClass::Retryable),
            (418, "", ErrorClass::Fatal),
        ];

        for (status, body, expected) in cases {
            let error = DriveError::from_response(
                StatusCode::from_u16(status).expect("código válido"),
                body,
                None,
            );
            assert_eq!(error.class(), expected, "código {status}: {error:?}");
        }
    }

    #[test]
    fn treats_rate_limit_403_as_retryable() {
        let body = r#"{"error":{"errors":[{"reason":"userRateLimitExceeded"}]}}"#;
        let error = DriveError::from_response(StatusCode::FORBIDDEN, body, None);

        assert!(matches!(error, DriveError::RateLimited { .. }));
        assert_eq!(error.class(), ErrorClass::Retryable);
    }

    #[test]
    fn keeps_the_retry_after_hint() {
        let error = DriveError::from_response(
            StatusCode::TOO_MANY_REQUESTS,
            "",
            Some(Duration::from_secs(42)),
        );

        assert_eq!(error.retry_after(), Some(Duration::from_secs(42)));
    }

    #[test]
    fn parses_retry_after_header() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "17".parse().expect("valor"));

        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(17)));

        headers.insert(
            reqwest::header::RETRY_AFTER,
            "no-numérico".parse().expect("valor"),
        );
        assert_eq!(parse_retry_after(&headers), None);
    }
}

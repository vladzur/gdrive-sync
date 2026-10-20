// SPDX-License-Identifier: GPL-3.0-or-later
//! Listener loopback para recibir el `code` de autorización (RF-01).
//!
//! El servidor escucha **solo** en `127.0.0.1` y sobre un puerto efímero
//! asignado por el sistema (ADR-006): un puerto fijo puede estar ocupado y
//! queda expuesto a otros procesos locales. Atiende una única petición y se
//! cierra, con un límite duro de tiempo.

use std::collections::HashMap;
use std::time::Duration;

use oauth2::RedirectUrl;
use oauth2::url::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Ruta que Google usará para devolver el `code`.
pub const CALLBACK_PATH: &str = "/callback";

/// Tamaño máximo de la petición aceptada. Evita que un cliente local malicioso
/// agote la memoria del proceso.
const MAX_REQUEST_BYTES: usize = 8 * 1024;

/// Error del flujo de callback.
#[derive(Debug, thiserror::Error)]
pub enum CallbackError {
    /// Falló la operación de red local.
    #[error("error en el listener local: {0}")]
    Io(#[from] std::io::Error),
    /// Se agotó el tiempo de espera.
    #[error("se agotó el tiempo esperando la autorización en el navegador")]
    Timeout,
    /// El proveedor informó un error (por ejemplo, el usuario canceló).
    #[error("el proveedor rechazó la autorización: {0}")]
    Denied(String),
    /// La petición no tiene la forma esperada.
    #[error("respuesta de callback inválida: {0}")]
    Invalid(String),
    /// El `state` devuelto no coincide con el enviado.
    #[error("el parámetro state no coincide; se descartó la respuesta (posible CSRF)")]
    StateMismatch,
}

/// Abre un listener en `127.0.0.1` con puerto efímero.
pub async fn bind() -> Result<TcpListener, CallbackError> {
    TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(CallbackError::Io)
}

/// Construye el `redirect_uri` correspondiente al puerto ya asignado.
pub fn redirect_uri(listener: &TcpListener) -> Result<RedirectUrl, CallbackError> {
    let port = listener.local_addr()?.port();

    RedirectUrl::new(format!("http://127.0.0.1:{port}{CALLBACK_PATH}"))
        .map_err(|err| CallbackError::Invalid(err.to_string()))
}

/// Espera el callback y devuelve el `code` autorizado.
///
/// Falla si el `state` no coincide con `expected_state`, si el proveedor
/// devuelve un error o si se agota `limit`.
pub async fn await_callback(
    listener: TcpListener,
    expected_state: &str,
    limit: Duration,
) -> Result<String, CallbackError> {
    match tokio::time::timeout(limit, accept_once(listener, expected_state)).await {
        Ok(result) => result,
        Err(_) => Err(CallbackError::Timeout),
    }
}

async fn accept_once(listener: TcpListener, expected_state: &str) -> Result<String, CallbackError> {
    let (mut stream, _peer) = listener.accept().await?;
    let target = read_target(&mut stream).await?;

    let url = Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|err| CallbackError::Invalid(format!("URL de callback inválida: {err}")))?;

    let outcome = validate(&url, expected_state);

    let (status, reason, body) = match &outcome {
        Ok(_) => (200, "OK", page(true, "Autorización completada")),
        Err(CallbackError::Denied(_)) => (200, "OK", page(false, "Autorización cancelada")),
        Err(CallbackError::StateMismatch) => (400, "Bad Request", page(false, "Estado inválido")),
        Err(_) => (400, "Bad Request", page(false, "Solicitud inválida")),
    };

    respond(&mut stream, status, reason, &body).await?;

    outcome
}

fn validate(url: &Url, expected_state: &str) -> Result<String, CallbackError> {
    if url.path() != CALLBACK_PATH {
        return Err(CallbackError::Invalid(format!(
            "ruta inesperada: {}",
            url.path()
        )));
    }

    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

    if let Some(error) = params.get("error") {
        let description = params.get("error_description").cloned().unwrap_or_default();
        return Err(CallbackError::Denied(format!("{error}: {description}")));
    }

    let state = params
        .get("state")
        .ok_or_else(|| CallbackError::Invalid("falta el parámetro state".to_owned()))?;

    if state != expected_state {
        return Err(CallbackError::StateMismatch);
    }

    params
        .get("code")
        .cloned()
        .ok_or_else(|| CallbackError::Invalid("falta el parámetro code".to_owned()))
}

/// Lee la petición y devuelve el objetivo de la línea de request (`/callback?…`).
async fn read_target(stream: &mut TcpStream) -> Result<String, CallbackError> {
    let mut buffer = Vec::with_capacity(512);
    let mut chunk = [0_u8; 512];

    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);

        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            return Err(CallbackError::Invalid(
                "petición demasiado grande".to_owned(),
            ));
        }
    }

    let request = String::from_utf8(buffer)
        .map_err(|_| CallbackError::Invalid("la petición no es UTF-8".to_owned()))?;

    let request_line = request
        .lines()
        .next()
        .ok_or_else(|| CallbackError::Invalid("petición vacía".to_owned()))?;

    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| CallbackError::Invalid("línea de request vacía".to_owned()))?;
    let target = parts
        .next()
        .ok_or_else(|| CallbackError::Invalid("falta el objetivo de la petición".to_owned()))?;

    if method != "GET" {
        return Err(CallbackError::Invalid(format!(
            "método no soportado: {method}"
        )));
    }

    Ok(target.to_owned())
}

async fn respond(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &str,
) -> Result<(), CallbackError> {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    stream.shutdown().await?;
    Ok(())
}

fn page(success: bool, title: &str) -> String {
    let (accent, message) = if success {
        (
            "#2ec27e",
            "Ya puedes cerrar esta pestaña y volver a gdrive-sync-app.",
        )
    } else {
        (
            "#e01b24",
            "La vinculación no se completó. Revisa la terminal para más detalles.",
        )
    };

    format!(
        "<!DOCTYPE html><html lang=\"es\"><head><meta charset=\"utf-8\">\
         <title>{title}</title></head>\
         <body style=\"font-family:system-ui,sans-serif;text-align:center;padding:4rem\">\
         <h1 style=\"color:{accent}\">{title}</h1><p>{message}</p></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpStream;

    async fn request(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("conexión");
        let raw = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        stream.write_all(raw.as_bytes()).await.expect("envío");

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("lectura");
        String::from_utf8(response).expect("respuesta UTF-8")
    }

    /// Ejecuta el listener en segundo plano y le envía `target`.
    async fn round_trip(
        target: String,
        expected_state: &str,
    ) -> (Result<String, CallbackError>, String) {
        let listener = bind().await.expect("listener");
        let port = listener.local_addr().expect("dirección").port();
        let state = expected_state.to_owned();

        let server =
            tokio::spawn(
                async move { await_callback(listener, &state, Duration::from_secs(5)).await },
            );

        let http_response = request(port, &target).await;
        let outcome = server.await.expect("tarea");

        (outcome, http_response)
    }

    #[tokio::test]
    async fn returns_the_code_when_state_matches() {
        let (outcome, http) = round_trip("/callback?code=abc123&state=xyz".to_owned(), "xyz").await;

        assert_eq!(outcome.expect("debe aceptarse"), "abc123");
        assert!(http.contains("200 OK"), "respuesta inesperada: {http}");
        assert!(http.contains("Autorización completada"));
    }

    #[tokio::test]
    async fn rejects_a_mismatched_state() {
        let (outcome, http) =
            round_trip("/callback?code=abc&state=otro".to_owned(), "esperado").await;

        assert!(matches!(outcome, Err(CallbackError::StateMismatch)));
        assert!(
            http.contains("400 Bad Request"),
            "respuesta inesperada: {http}"
        );
    }

    #[tokio::test]
    async fn reports_a_provider_error() {
        let (outcome, _) = round_trip(
            "/callback?error=access_denied&error_description=user_cancelled&state=s".to_owned(),
            "s",
        )
        .await;

        match outcome {
            Err(CallbackError::Denied(message)) => {
                assert!(message.contains("access_denied"), "mensaje: {message}");
            }
            other => panic!("se esperaba Denied, se obtuvo {other:?}"),
        }
    }

    #[tokio::test]
    async fn rejects_an_unexpected_path() {
        let (outcome, http) = round_trip("/otra?code=abc&state=s".to_owned(), "s").await;

        assert!(matches!(outcome, Err(CallbackError::Invalid(_))));
        assert!(http.contains("400 Bad Request"));
    }

    #[tokio::test]
    async fn rejects_a_missing_code() {
        let (outcome, _) = round_trip("/callback?state=s".to_owned(), "s").await;

        match outcome {
            Err(CallbackError::Invalid(message)) => assert!(message.contains("code")),
            other => panic!("se esperaba Invalid, se obtuvo {other:?}"),
        }
    }

    #[tokio::test]
    async fn times_out_when_nobody_calls_back() {
        let listener = bind().await.expect("listener");

        let outcome = await_callback(listener, "s", Duration::from_millis(50)).await;

        assert!(matches!(outcome, Err(CallbackError::Timeout)));
    }

    #[tokio::test]
    async fn binds_only_to_loopback() {
        let listener = bind().await.expect("listener");
        let addr = listener.local_addr().expect("dirección");

        assert!(
            addr.ip().is_loopback(),
            "la dirección debe ser loopback: {addr}"
        );
        assert_ne!(addr.port(), 0, "el puerto debe estar asignado");
    }
}

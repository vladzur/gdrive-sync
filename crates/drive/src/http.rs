//! Construcción del cliente HTTP compartido.
//!
//! Todos los módulos que hablan con Google usan este cliente para que las
//! políticas de red se definan en un solo lugar.

use std::time::Duration;

use gdrive_sync_core::tls;

/// Tiempo máximo para establecer la conexión TCP y el handshake TLS.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Tiempo máximo de inactividad durante una transferencia.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Construye el cliente HTTP del proyecto.
///
/// Fija tres políticas que no deben variar entre módulos:
///
/// - **no sigue redirecciones**: `oauth2` lo exige explícitamente para evitar
///   SSRF en el flujo de token (ADR-006);
/// - **timeouts explícitos**: sin ellos, una conexión colgada bloquea una
///   transferencia indefinidamente;
/// - **`User-Agent` propio**: permite identificar el cliente en los logs de Drive.
///
/// # Errors
///
/// Devuelve error si `rustls` no tiene proveedor criptográfico disponible o si
/// la configuración del cliente es inválida.
pub fn client() -> reqwest::Result<reqwest::Client> {
    tls::init();

    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .user_agent(concat!("gdrive-sync/", env!("CARGO_PKG_VERSION")))
        .build()
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;

    #[test]
    fn builds_a_client_with_the_configured_provider() {
        assert!(client().is_ok(), "el cliente HTTP debe construirse");
    }

    #[tokio::test]
    async fn does_not_follow_redirects() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/target"))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/target"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let response = client()
            .expect("cliente HTTP")
            .get(format!("{}/redirect", server.uri()))
            .send()
            .await
            .expect("petición");

        assert_eq!(
            response.status().as_u16(),
            302,
            "una redirección no debe seguirse automáticamente (ADR-006)"
        );
    }
}

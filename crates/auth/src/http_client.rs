// SPDX-License-Identifier: GPL-3.0-or-later
//! Adaptador entre `oauth2` y el cliente `reqwest` del proyecto.
//!
//! `oauth2` 5.0 se usa con `default-features = false` para no arrastrar su
//! propia dependencia de `reqwest` 0.12 (ADR-009). En su lugar se implementa
//! `AsyncHttpClient`, que es la interfaz pública prevista para clientes propios.

use std::future::Future;
use std::pin::Pin;

use oauth2::{AsyncHttpClient, HttpRequest, HttpResponse};
use reqwest::Client;

/// Error de transporte del adaptador.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// El método HTTP preparado por `oauth2` no es válido.
    #[error("método HTTP inválido: {0}")]
    InvalidMethod(String),
    /// Falló la petición HTTP.
    #[error("error de transporte HTTP: {0}")]
    Request(#[from] reqwest::Error),
}

/// Adaptador que permite a `oauth2` enviar peticiones con el cliente del
/// proyecto, de modo que las políticas de red (timeouts, sin redirecciones,
/// `User-Agent`) sean las mismas que en el resto de la aplicación.
#[derive(Debug, Clone)]
pub struct ReqwestHttpClient {
    client: Client,
}

impl ReqwestHttpClient {
    /// Envuelve un cliente `reqwest` ya configurado.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl<'c> AsyncHttpClient<'c> for ReqwestHttpClient {
    type Error = TransportError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, Self::Error>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(async move {
            let (parts, body) = request.into_parts();

            let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
                .map_err(|_| TransportError::InvalidMethod(parts.method.to_string()))?;

            let mut builder = self.client.request(method, parts.uri.to_string());
            for (name, value) in &parts.headers {
                builder = builder.header(name, value);
            }

            let response = builder.body(body).send().await?;
            let status = response.status();
            let headers = response.headers().clone();
            let bytes = response.bytes().await?;

            let mut adapted = HttpResponse::new(bytes.to_vec());
            *adapted.status_mut() = status;
            *adapted.headers_mut() = headers;

            Ok(adapted)
        })
    }
}

#[cfg(test)]
mod tests {
    use oauth2::http::{Method, Request};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string, header, method, path},
    };

    use super::*;

    fn transport() -> ReqwestHttpClient {
        gdrive_sync_core::tls::init();
        ReqwestHttpClient::new(Client::builder().build().expect("cliente"))
    }

    #[tokio::test]
    async fn forwards_method_headers_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(header("content-type", "application/x-www-form-urlencoded"))
            .and(body_string("grant_type=authorization_code"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"ok\":true}"))
            .mount(&server)
            .await;

        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("{}/token", server.uri()))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(b"grant_type=authorization_code".to_vec())
            .expect("petición");

        let response = transport().call(request).await.expect("respuesta");

        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), br#"{"ok":true}"#);
    }

    #[tokio::test]
    async fn surfaces_transport_errors() {
        let request = Request::builder()
            .method(Method::GET)
            // Puerto 1 en loopback: conexión rechazada de forma determinista.
            .uri("http://127.0.0.1:1/token")
            .body(Vec::new())
            .expect("petición");

        let error = transport().call(request).await.expect_err("debe fallar");

        assert!(
            matches!(error, TransportError::Request(_)),
            "error inesperado: {error:?}"
        );
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Cliente OAuth 2.0 con PKCE y renovación de tokens (T1.1, T1.2, T1.6).

use std::time::{Duration, SystemTime};

use gdrive_sync_core::{SecretString, TokenSet};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
    basic::BasicClient, url::Url,
};

use crate::http_client::ReqwestHttpClient;

/// Margen con el que se renueva el token antes de que expire.
pub const REFRESH_MARGIN: Duration = Duration::from_secs(300);

type ConfiguredClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// Configuración del cliente OAuth.
#[derive(Debug, Clone)]
pub struct OAuthSettings {
    /// Identificador del cliente creado en Google Cloud Console.
    pub client_id: String,
    /// Secreto del cliente. En clientes de escritorio no es confidencial: la
    /// seguridad real la aporta PKCE (ADR-006).
    pub client_secret: String,
    /// Endpoint de autorización.
    pub auth_url: String,
    /// Endpoint de tokens.
    pub token_url: String,
    /// Scopes solicitados.
    pub scopes: Vec<String>,
}

impl OAuthSettings {
    /// Configuración de Google con el scope de Drive (ADR-005).
    #[must_use]
    pub fn google(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_owned(),
            token_url: "https://oauth2.googleapis.com/token".to_owned(),
            scopes: vec![crate::DRIVE_SCOPE.to_owned()],
        }
    }
}

/// Error del flujo OAuth.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// La configuración contiene URLs inválidas.
    #[error("configuración OAuth inválida: {0}")]
    Configuration(String),
    /// Falló el intercambio del código de autorización.
    #[error("error al intercambiar el código de autorización: {0}")]
    Exchange(String),
    /// Falló la renovación del token de acceso.
    #[error("error al renovar el token de acceso: {0}")]
    Refresh(String),
    /// No hay token de renovación: hace falta volver a iniciar sesión.
    #[error("no hay token de renovación disponible; hay que volver a iniciar sesión")]
    MissingRefreshToken,
}

/// Cliente OAuth con PKCE.
pub struct AuthClient {
    inner: ConfiguredClient,
    http: ReqwestHttpClient,
    scopes: Vec<Scope>,
}

impl std::fmt::Debug for AuthClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthClient")
            .field("scopes", &self.scopes)
            .finish_non_exhaustive()
    }
}

impl AuthClient {
    /// Construye el cliente.
    ///
    /// El `redirect_uri` es opcional porque solo lo necesita el intercambio del
    /// código: la renovación de un token no lo usa. En el login se pasa el puerto
    /// que el listener obtuvo al abrirse (ADR-006).
    ///
    /// # Errors
    ///
    /// Devuelve [`AuthError::Configuration`] si alguna URL es inválida.
    pub fn new(
        settings: &OAuthSettings,
        http: reqwest::Client,
        redirect_uri: Option<RedirectUrl>,
    ) -> Result<Self, AuthError> {
        let auth_url = AuthUrl::new(settings.auth_url.clone())
            .map_err(|err| AuthError::Configuration(err.to_string()))?;
        let token_url = TokenUrl::new(settings.token_url.clone())
            .map_err(|err| AuthError::Configuration(err.to_string()))?;

        let mut inner = BasicClient::new(ClientId::new(settings.client_id.clone()))
            .set_client_secret(ClientSecret::new(settings.client_secret.clone()))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url);

        if let Some(redirect_uri) = redirect_uri {
            inner = inner.set_redirect_uri(redirect_uri);
        }

        Ok(Self {
            inner,
            http: ReqwestHttpClient::new(http),
            scopes: settings
                .scopes
                .iter()
                .map(|scope| Scope::new(scope.clone()))
                .collect(),
        })
    }

    /// Construye la URL de autorización junto con el `state` y el verificador
    /// PKCE que hay que conservar hasta el callback.
    ///
    /// Se piden `access_type=offline` y `prompt=consent` para que Google emita
    /// un `refresh_token`, que solo entrega en el primer consentimiento.
    pub fn authorization_url(&self) -> (Url, CsrfToken, PkceCodeVerifier) {
        let (challenge, verifier) = crate::pkce::generate();

        let (url, state) = self
            .inner
            .authorize_url(CsrfToken::new_random)
            .add_scopes(self.scopes.iter().cloned())
            .set_pkce_challenge(challenge)
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .url();

        (url, state, verifier)
    }

    /// Intercambia el `code` por tokens.
    ///
    /// # Errors
    ///
    /// Devuelve [`AuthError::Exchange`] si el proveedor rechaza la petición.
    pub async fn exchange_code(
        &self,
        code: &str,
        verifier: PkceCodeVerifier,
    ) -> Result<TokenSet, AuthError> {
        let response = self
            .inner
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .set_pkce_verifier(verifier)
            .request_async(&self.http)
            .await
            .map_err(|err| AuthError::Exchange(err.to_string()))?;

        Ok(self.token_set_from(&response, None))
    }

    /// Renueva el token de acceso.
    ///
    /// Google no devuelve un `refresh_token` nuevo al renovar, así que se
    /// conserva el anterior.
    ///
    /// # Errors
    ///
    /// Devuelve [`AuthError::MissingRefreshToken`] si no hay token de
    /// renovación, o [`AuthError::Refresh`] si el proveedor lo rechaza.
    pub async fn refresh(&self, current: &TokenSet) -> Result<TokenSet, AuthError> {
        let refresh_token = current
            .refresh_token
            .as_ref()
            .ok_or(AuthError::MissingRefreshToken)?;

        let response = self
            .inner
            .exchange_refresh_token(&RefreshToken::new(refresh_token.expose().to_owned()))
            .request_async(&self.http)
            .await
            .map_err(|err| AuthError::Refresh(err.to_string()))?;

        Ok(self.token_set_from(&response, current.refresh_token.clone()))
    }

    /// Renueva el token solo si está por vencer (T1.6).
    ///
    /// # Errors
    ///
    /// Propaga los errores de [`AuthClient::refresh`].
    pub async fn ensure_fresh(
        &self,
        current: TokenSet,
        now: SystemTime,
        margin: Duration,
    ) -> Result<TokenSet, AuthError> {
        if current.needs_refresh(now, margin) && current.can_refresh() {
            self.refresh(&current).await
        } else {
            Ok(current)
        }
    }

    fn token_set_from<TR>(&self, response: &TR, previous_refresh: Option<SecretString>) -> TokenSet
    where
        TR: TokenResponse,
    {
        let refresh_token = response
            .refresh_token()
            .map(|token| SecretString::new(token.secret().clone()))
            .or(previous_refresh);

        let scopes = response.scopes().map_or_else(
            || {
                self.scopes
                    .iter()
                    .map(|scope| scope.as_ref().to_owned())
                    .collect()
            },
            |scopes| {
                scopes
                    .iter()
                    .map(|scope| scope.as_ref().to_owned())
                    .collect()
            },
        );

        TokenSet::new(
            SecretString::new(response.access_token().secret().clone()),
            refresh_token,
            response.expires_in(),
            SystemTime::now(),
            scopes,
        )
    }
}

#[cfg(test)]
mod tests {
    use oauth2::url::Url;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string_contains, method, path},
    };

    use super::*;

    fn redirect_uri() -> RedirectUrl {
        RedirectUrl::new("http://127.0.0.1:1234/callback".to_owned()).expect("redirect uri")
    }

    fn client_for(server: &MockServer) -> AuthClient {
        gdrive_sync_core::tls::init();
        let settings = OAuthSettings {
            client_id: "client-id".to_owned(),
            client_secret: "client-secret".to_owned(),
            auth_url: format!("{}/auth", server.uri()),
            token_url: format!("{}/token", server.uri()),
            scopes: vec!["scope-a".to_owned(), "scope-b".to_owned()],
        };
        let http = reqwest::Client::builder().build().expect("cliente HTTP");

        AuthClient::new(&settings, http, Some(redirect_uri())).expect("cliente OAuth")
    }

    fn token_json(extra: &str) -> ResponseTemplate {
        let body = format!(
            r#"{{"access_token":"access-new","token_type":"Bearer","expires_in":3600,"scope":"scope-a scope-b"{extra}}}"#
        );

        ResponseTemplate::new(200).set_body_raw(body, "application/json")
    }

    #[tokio::test]
    async fn authorization_url_carries_pkce_state_and_offline_access() {
        let server = MockServer::start().await;
        let client = client_for(&server);

        let (url, state, verifier) = client.authorization_url();
        let params: std::collections::HashMap<_, _> = Url::parse(url.as_str())
            .expect("url")
            .query_pairs()
            .into_owned()
            .collect();

        let expected_challenge = crate::pkce::s256_challenge(verifier.secret());

        assert_eq!(
            params.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(
            params.get("code_challenge").map(String::as_str),
            Some(expected_challenge.as_str())
        );
        assert_eq!(
            params.get("state").map(String::as_str),
            Some(state.secret().as_str())
        );
        assert_eq!(
            params.get("access_type").map(String::as_str),
            Some("offline")
        );
        assert_eq!(params.get("prompt").map(String::as_str), Some("consent"));
        assert_eq!(
            params.get("client_id").map(String::as_str),
            Some("client-id")
        );
        assert_eq!(
            params.get("redirect_uri").map(String::as_str),
            Some("http://127.0.0.1:1234/callback")
        );
        assert_eq!(
            params.get("scope").map(String::as_str),
            Some("scope-a scope-b")
        );
    }

    #[tokio::test]
    async fn exchange_code_returns_a_complete_token_set() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code_verifier="))
            .respond_with(token_json(r#","refresh_token":"refresh-new""#))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let (_, _, verifier) = client.authorization_url();
        let tokens = client
            .exchange_code("the-code", verifier)
            .await
            .expect("intercambio");

        assert_eq!(tokens.access_token.expose(), "access-new");
        assert_eq!(
            tokens.refresh_token.as_ref().map(SecretString::expose),
            Some("refresh-new")
        );
        assert_eq!(
            tokens.scopes,
            vec!["scope-a".to_owned(), "scope-b".to_owned()]
        );
        assert!(
            tokens.expires_at().is_some(),
            "debe registrarse la expiración"
        );
    }

    #[tokio::test]
    async fn refresh_keeps_the_previous_refresh_token_when_none_is_returned() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(token_json(""))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let current = TokenSet::new(
            SecretString::new("old-access"),
            Some(SecretString::new("refresh-old")),
            Some(Duration::from_secs(10)),
            SystemTime::now(),
            Vec::new(),
        );

        let refreshed = client.refresh(&current).await.expect("renovación");

        assert_eq!(refreshed.access_token.expose(), "access-new");
        assert_eq!(
            refreshed.refresh_token.as_ref().map(SecretString::expose),
            Some("refresh-old"),
            "el refresh token anterior no debe perderse"
        );
    }

    #[tokio::test]
    async fn refresh_without_a_refresh_token_is_an_error() {
        let server = MockServer::start().await;
        let client = client_for(&server);
        let current = TokenSet::new(
            SecretString::new("a"),
            None,
            Some(Duration::from_secs(10)),
            SystemTime::now(),
            Vec::new(),
        );

        let error = client.refresh(&current).await.expect_err("debe fallar");

        assert!(matches!(error, AuthError::MissingRefreshToken));
    }

    #[tokio::test]
    async fn ensure_fresh_only_refreshes_when_expiring_soon() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(token_json(r#","refresh_token":"refresh-new""#))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let now = SystemTime::now();

        let fresh = TokenSet::new(
            SecretString::new("still-valid"),
            Some(SecretString::new("refresh")),
            Some(Duration::from_secs(3600)),
            now,
            Vec::new(),
        );
        let returned = client
            .ensure_fresh(fresh.clone(), now, REFRESH_MARGIN)
            .await
            .expect("sin cambios");
        assert_eq!(returned.access_token.expose(), "still-valid");

        let expiring = TokenSet::new(
            SecretString::new("about-to-expire"),
            Some(SecretString::new("refresh")),
            Some(Duration::from_secs(60)),
            now,
            Vec::new(),
        );
        let renewed = client
            .ensure_fresh(expiring, now, REFRESH_MARGIN)
            .await
            .expect("renovado");
        assert_eq!(renewed.access_token.expose(), "access-new");
    }

    #[tokio::test]
    async fn surfaces_a_rejected_exchange() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_raw(
                r#"{"error":"invalid_grant","error_description":"code expired"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let (_, _, verifier) = client.authorization_url();
        let error = client
            .exchange_code("expired", verifier)
            .await
            .expect_err("debe fallar");

        assert!(
            matches!(error, AuthError::Exchange(_)),
            "error inesperado: {error:?}"
        );
    }
    /// Verifica la garantía de ADR-011: un `TokenSet` puede registrarse con
    /// `tracing` sin que sus secretos lleguen a la salida.
    #[test]
    fn tokens_are_not_written_to_logs() {
        use std::io::Write as _;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct Sink(Arc<Mutex<Vec<u8>>>);

        impl std::io::Write for Sink {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("lock").extend_from_slice(buffer);
                Ok(buffer.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
            type Writer = Self;

            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let sink = Sink::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(sink.clone())
            .without_time()
            .with_ansi(false)
            .finish();

        let tokens = TokenSet::new(
            SecretString::new("access-token-secreto"),
            Some(SecretString::new("refresh-token-secreto")),
            Some(Duration::from_secs(3600)),
            SystemTime::now(),
            vec!["scope".to_owned()],
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(?tokens, "tokens recibidos");
        });

        let logged = {
            let buffer = sink.0.lock().expect("lock").clone();
            let _ = std::io::stdout().flush();
            String::from_utf8(buffer).expect("log UTF-8")
        };

        assert!(!logged.is_empty(), "el log debe haberse escrito");
        assert!(
            !logged.contains("access-token-secreto"),
            "el access token se filtró: {logged}"
        );
        assert!(
            !logged.contains("refresh-token-secreto"),
            "el refresh token se filtró: {logged}"
        );
        assert!(
            logged.contains("SecretString(***)"),
            "se esperaba el enmascarado: {logged}"
        );
    }
}

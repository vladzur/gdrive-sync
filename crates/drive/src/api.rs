// SPDX-License-Identifier: GPL-3.0-or-later
//! Operaciones de Drive API v3 usadas en el Hito 1.
//!
//! Se piden `fields` explícitos en todas las peticiones (Anexo D del plan):
//! sin ese parámetro, Drive devuelve el recurso completo, con más datos y más
//! latencia de los necesarios.

use gdrive_sync_core::SecretString;
use reqwest::{Client, Response};
use serde::{Deserialize, de::DeserializeOwned};

use crate::error::{DriveError, parse_retry_after};
use crate::retry::RetryPolicy;

/// URL base de la API de Drive.
pub const DEFAULT_BASE_URL: &str = "https://www.googleapis.com/drive/v3";

/// Campos pedidos en las respuestas de archivos.
pub const FILE_FIELDS: &str = "id,name,mimeType,size,modifiedTime,parents,trashed";

/// MIME type de una carpeta de Drive.
pub const FOLDER_MIME_TYPE: &str = "application/vnd.google-apps.folder";

/// Identificador de la raíz del usuario.
pub const ROOT_FOLDER_ID: &str = "root";

/// Información del usuario autenticado.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AboutUser {
    /// Correo de la cuenta.
    #[serde(rename = "emailAddress")]
    pub email_address: Option<String>,
    /// Nombre para mostrar.
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

/// Respuesta de `about.get`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AboutResponse {
    /// Usuario autenticado.
    pub user: AboutUser,
}

/// Metadatos de un archivo o carpeta.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FileMetadata {
    /// Identificador estable dentro de Drive.
    pub id: String,
    /// Nombre visible.
    pub name: String,
    /// MIME type.
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
    /// Tamaño en bytes, como cadena (Drive usa `int64` en JSON).
    pub size: Option<String>,
    /// Última modificación (RFC 3339).
    #[serde(rename = "modifiedTime")]
    pub modified_time: Option<String>,
    /// Carpetas padre.
    pub parents: Option<Vec<String>>,
    /// Indica si está en la papelera.
    pub trashed: Option<bool>,
}

impl FileMetadata {
    /// Indica si el nodo es una carpeta.
    #[must_use]
    pub fn is_folder(&self) -> bool {
        self.mime_type.as_deref() == Some(FOLDER_MIME_TYPE)
    }

    /// Tamaño en bytes, si Drive lo informó.
    #[must_use]
    pub fn size_bytes(&self) -> Option<u64> {
        self.size.as_deref()?.parse().ok()
    }
}

/// Respuesta de `files.list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct FileListResponse {
    /// Archivos de la página.
    #[serde(default)]
    pub files: Vec<FileMetadata>,
    /// Token de la página siguiente.
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

/// Respuesta de `changes.getStartPageToken`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StartPageToken {
    /// Token desde el cual consultar cambios.
    #[serde(rename = "startPageToken")]
    pub start_page_token: String,
}

/// Cliente de la API de Drive.
pub struct DriveClient {
    http: Client,
    base_url: String,
    access_token: SecretString,
    policy: RetryPolicy,
}

impl std::fmt::Debug for DriveClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriveClient")
            .field("base_url", &self.base_url)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl DriveClient {
    /// Crea un cliente contra la API real.
    #[must_use]
    pub fn new(http: Client, access_token: SecretString) -> Self {
        Self::with_base_url(http, access_token, DEFAULT_BASE_URL)
    }

    /// Crea un cliente contra una URL base alternativa (tests, proxy).
    #[must_use]
    pub fn with_base_url(
        http: Client,
        access_token: SecretString,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            http,
            base_url: base_url.into(),
            access_token,
            policy: RetryPolicy::default(),
        }
    }

    /// Sustituye la política de reintentos.
    #[must_use]
    pub fn with_policy(mut self, policy: RetryPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Obtiene la cuenta autenticada.
    ///
    /// # Errors
    ///
    /// Devuelve el error clasificado de la API.
    pub async fn about(&self) -> Result<AboutResponse, DriveError> {
        let params = vec![(
            "fields".to_owned(),
            "user(emailAddress,displayName)".to_owned(),
        )];

        self.get_json("/about", params).await
    }

    /// Lista archivos con una consulta opcional.
    ///
    /// # Errors
    ///
    /// Devuelve el error clasificado de la API.
    pub async fn list_files(
        &self,
        query: Option<&str>,
        page_token: Option<&str>,
        page_size: u32,
    ) -> Result<FileListResponse, DriveError> {
        let mut params = vec![
            ("fields".to_owned(), FILE_FIELDS.to_owned()),
            ("pageSize".to_owned(), page_size.clamp(1, 1000).to_string()),
        ];

        if let Some(query) = query {
            params.push(("q".to_owned(), query.to_owned()));
        }
        if let Some(token) = page_token {
            params.push(("pageToken".to_owned(), token.to_owned()));
        }

        self.get_json("/files", params).await
    }

    /// Obtiene el token inicial para `changes.list` (RF-04).
    ///
    /// # Errors
    ///
    /// Devuelve el error clasificado de la API.
    pub async fn start_page_token(&self) -> Result<String, DriveError> {
        let response: StartPageToken = self
            .get_json("/changes/getStartPageToken", Vec::new())
            .await?;

        Ok(response.start_page_token)
    }

    /// Resuelve una ruta de carpetas como `"documentos/fotos"` a su identificador.
    ///
    /// Devuelve `Ok(None)` si algún segmento no existe. La comparación es por
    /// nombre exacto dentro del padre correspondiente.
    ///
    /// # Errors
    ///
    /// Devuelve el error clasificado de la API.
    pub async fn resolve_folder_path(&self, path: &str) -> Result<Option<String>, DriveError> {
        let mut parent = ROOT_FOLDER_ID.to_owned();

        for segment in path.split('/').filter(|segment| !segment.is_empty()) {
            let query = format!(
                "'{parent}' in parents and name = '{}' and mimeType = '{FOLDER_MIME_TYPE}' and trashed = false",
                escape_query_value(segment)
            );

            let found = self.list_files(Some(&query), None, 1).await?;
            let Some(folder) = found.files.into_iter().next() else {
                return Ok(None);
            };

            parent = folder.id;
        }

        Ok(Some(parent))
    }

    async fn get_json<T>(&self, path: &str, params: Vec<(String, String)>) -> Result<T, DriveError>
    where
        T: DeserializeOwned,
    {
        let url = format!("{}{path}", self.base_url);
        let mut attempt = 0;

        loop {
            let response = self
                .http
                .get(&url)
                .bearer_auth(self.access_token.expose())
                .query(&params)
                .send()
                .await;

            match self.decode::<T>(response).await {
                Ok(value) => return Ok(value),
                Err(error) => {
                    if !self.policy.should_retry(attempt, &error) {
                        return Err(error);
                    }

                    tokio::time::sleep(self.policy.wait_for(attempt, &error)).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn decode<T>(&self, response: Result<Response, reqwest::Error>) -> Result<T, DriveError>
    where
        T: DeserializeOwned,
    {
        let response = response.map_err(|error| DriveError::from_reqwest(&error))?;
        let status = response.status();
        let retry_after = parse_retry_after(response.headers());

        let body = response
            .text()
            .await
            .map_err(|error| DriveError::from_reqwest(&error))?;

        if status.is_success() {
            serde_json::from_str(&body).map_err(|error| DriveError::Decode(error.to_string()))
        } else {
            Err(DriveError::from_response(status, &body, retry_after))
        }
    }
}

/// Escapa un valor para incluirlo en una consulta `q` de Drive.
fn escape_query_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param, query_param_contains},
    };

    use super::*;

    fn client(server: &MockServer) -> DriveClient {
        gdrive_sync_core::tls::init();
        let http = Client::builder().build().expect("cliente HTTP");

        DriveClient::with_base_url(http, SecretString::new("token"), server.uri())
            .with_policy(RetryPolicy::immediate())
    }

    fn json(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(body, "application/json")
    }

    #[tokio::test]
    async fn reads_the_authenticated_account() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/about"))
            .and(query_param("fields", "user(emailAddress,displayName)"))
            .respond_with(json(
                r#"{"user":{"emailAddress":"persona@example.com","displayName":"Persona"}}"#,
            ))
            .mount(&server)
            .await;

        let about = client(&server).about().await.expect("about");

        assert_eq!(
            about.user.email_address.as_deref(),
            Some("persona@example.com")
        );
        assert_eq!(about.user.display_name.as_deref(), Some("Persona"));
    }

    #[tokio::test]
    async fn sends_authorization_and_explicit_fields() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/files"))
            .and(query_param("fields", FILE_FIELDS))
            .and(query_param("q", "trashed = false"))
            .and(query_param("pageSize", "10"))
            .respond_with(json(
                r#"{"files":[{"id":"f1","name":"hola.txt","mimeType":"text/plain","size":"12"}]}"#,
            ))
            .mount(&server)
            .await;

        let list = client(&server)
            .list_files(Some("trashed = false"), None, 10)
            .await
            .expect("listado");

        assert_eq!(list.files.len(), 1);
        assert_eq!(list.files[0].name, "hola.txt");
        assert_eq!(list.files[0].size_bytes(), Some(12));
        assert!(!list.files[0].is_folder());
    }

    #[tokio::test]
    async fn retries_a_server_error_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/about"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/about"))
            .respond_with(json(r#"{"user":{"emailAddress":"a@b.c"}}"#))
            .mount(&server)
            .await;

        let about = client(&server).about().await.expect("debe recuperarse");

        assert_eq!(about.user.email_address.as_deref(), Some("a@b.c"));
        assert_eq!(
            server.received_requests().await.expect("peticiones").len(),
            3
        );
    }

    #[tokio::test]
    async fn does_not_retry_an_unauthorized_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/about"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let error = client(&server).about().await.expect_err("debe fallar");

        assert_eq!(error, DriveError::Unauthorized);
        assert_eq!(error.class(), crate::error::ErrorClass::Auth);
        assert_eq!(
            server.received_requests().await.expect("peticiones").len(),
            1
        );
    }

    #[tokio::test]
    async fn maps_a_missing_resource() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/files"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let error = client(&server)
            .list_files(None, None, 5)
            .await
            .expect_err("debe fallar");

        assert_eq!(error, DriveError::NotFound);
    }

    #[tokio::test]
    async fn reads_the_start_page_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/changes/getStartPageToken"))
            .respond_with(json(r#"{"startPageToken":"token-123"}"#))
            .mount(&server)
            .await;

        let token = client(&server).start_page_token().await.expect("token");

        assert_eq!(token, "token-123");
    }

    #[tokio::test]
    async fn resolves_a_nested_folder_path() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/files"))
            .and(query_param_contains("q", "name = 'documentos'"))
            .respond_with(json(r#"{"files":[{"id":"folder-1","name":"documentos"}]}"#))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/files"))
            .and(query_param_contains("q", "'folder-1' in parents"))
            .respond_with(json(r#"{"files":[{"id":"folder-2","name":"fotos"}]}"#))
            .mount(&server)
            .await;

        let folder = client(&server)
            .resolve_folder_path("documentos/fotos")
            .await
            .expect("resolución");

        assert_eq!(folder.as_deref(), Some("folder-2"));
    }

    #[tokio::test]
    async fn returns_none_when_a_segment_does_not_exist() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/files"))
            .respond_with(json(r#"{"files":[]}"#))
            .mount(&server)
            .await;

        let folder = client(&server)
            .resolve_folder_path("inexistente")
            .await
            .expect("resolución");

        assert_eq!(folder, None);
    }

    #[test]
    fn escapes_quotes_in_query_values() {
        assert_eq!(escape_query_value("o'brien"), "o\\'brien");
        assert_eq!(escape_query_value(r"a\b"), r"a\\b");
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Flujo PKCE completo: autorización, callback y canje del código (T1.1–T1.3).

use std::time::Duration;

use gdrive_sync_auth::callback;
use gdrive_sync_auth::client::{AuthClient, OAuthSettings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_string_contains, method, path},
};

fn settings(token_url: String) -> OAuthSettings {
    OAuthSettings {
        client_id: "client-id".to_owned(),
        client_secret: "client-secret".to_owned(),
        auth_url: "https://accounts.example/auth".to_owned(),
        token_url,
        scopes: vec!["https://www.googleapis.com/auth/drive".to_owned()],
    }
}

#[tokio::test]
async fn completes_the_pkce_flow_end_to_end() {
    gdrive_sync_core::tls::init();

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=code-abc"))
        .and(body_string_contains("code_verifier="))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"access_token":"access-1","refresh_token":"refresh-1","expires_in":3600,"token_type":"Bearer","scope":"https://www.googleapis.com/auth/drive"}"#,
            "application/json",
        ))
        .expect(1)
        .mount(&server)
        .await;

    let listener = callback::bind().await.expect("listener");
    let port = listener.local_addr().expect("dirección").port();
    let redirect = callback::redirect_uri(&listener).expect("redirect uri");

    let http = reqwest::Client::builder().build().expect("cliente HTTP");
    let client = AuthClient::new(
        &settings(format!("{}/token", server.uri())),
        http,
        Some(redirect),
    )
    .expect("cliente OAuth");

    let (url, state, verifier) = client.authorization_url();
    let query = url.query().unwrap_or_default().to_owned();
    assert!(query.contains("code_challenge="), "la URL debe llevar PKCE");
    assert!(query.contains("access_type=offline"));

    let expected_state = state.secret().clone();
    let browser = tokio::spawn(async move {
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("conexión");
        let request = format!(
            "GET /callback?code=code-abc&state={expected_state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.expect("envío");

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("lectura");
        String::from_utf8_lossy(&response).into_owned()
    });

    let code = callback::await_callback(listener, state.secret(), Duration::from_secs(5))
        .await
        .expect("callback aceptado");
    assert_eq!(code, "code-abc");

    let page = browser.await.expect("tarea del navegador");
    assert!(page.contains("200 OK"), "respuesta al navegador: {page}");

    let tokens = client.exchange_code(&code, verifier).await.expect("canje");

    assert_eq!(tokens.access_token.expose(), "access-1");
    assert_eq!(
        tokens
            .refresh_token
            .as_ref()
            .map(gdrive_sync_core::SecretString::expose),
        Some("refresh-1")
    );
    assert!(tokens.expires_at().is_some());
    assert_eq!(
        tokens.scopes,
        vec!["https://www.googleapis.com/auth/drive".to_owned()]
    );
}

#[tokio::test]
async fn refuses_a_callback_with_a_foreign_state() {
    gdrive_sync_core::tls::init();

    let listener = callback::bind().await.expect("listener");
    let port = listener.local_addr().expect("dirección").port();

    let browser = tokio::spawn(async move {
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("conexión");
        let request =
            "GET /callback?code=stolen&state=atacante HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        stream.write_all(request.as_bytes()).await.expect("envío");

        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response).await;
        String::from_utf8_lossy(&response).into_owned()
    });

    let outcome = callback::await_callback(listener, "esperado", Duration::from_secs(5)).await;

    assert!(
        matches!(outcome, Err(callback::CallbackError::StateMismatch)),
        "se esperaba StateMismatch, se obtuvo {outcome:?}"
    );
    let page = browser.await.expect("tarea del navegador");
    assert!(page.contains("400 Bad Request"), "respuesta: {page}");
}

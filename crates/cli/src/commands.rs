// SPDX-License-Identifier: GPL-3.0-or-later
//! Comandos de la CLI (T1.9).

use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow, bail};
use gdrive_sync_auth::client::{AuthClient, OAuthSettings, REFRESH_MARGIN};
use gdrive_sync_auth::store::{DEFAULT_ACCOUNT, KeyringTokenStore};
use gdrive_sync_auth::{browser, callback};
use gdrive_sync_core::{TokenSet, TokenStore};
use gdrive_sync_drive::{DriveClient, DriveError, FileListResponse};

/// Tiempo máximo que se espera la autorización en el navegador.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// Vincula una cuenta de Google.
pub(crate) async fn login(settings: &OAuthSettings) -> Result<()> {
    let http = gdrive_sync_drive::http::client()?;
    let store = KeyringTokenStore::connect(DEFAULT_ACCOUNT).await?;

    let listener = callback::bind().await?;
    let redirect = callback::redirect_uri(&listener)?;
    let auth = AuthClient::new(settings, http.clone(), Some(redirect))?;

    let (url, state, verifier) = auth.authorization_url();

    println!("Se abrirá el navegador para autorizar el acceso a Google Drive.");
    println!("Si no se abre, visita esta URL:\n\n{url}\n");

    if let Err(error) = browser::open(&url) {
        tracing::warn!(%error, "no se pudo abrir el navegador automáticamente");
    }

    let code = callback::await_callback(listener, state.secret(), LOGIN_TIMEOUT).await?;
    let tokens = auth.exchange_code(&code, verifier).await?;

    // Se verifica la cuenta antes de guardar: si el token no sirve, no tiene
    // sentido persistirlo.
    let drive = DriveClient::new(http, tokens.access_token.clone());
    let about = drive
        .about()
        .await
        .context("no se pudo verificar la cuenta recién autorizada")?;

    store.save(&tokens).await?;

    let account = about
        .user
        .email_address
        .unwrap_or_else(|| "cuenta sin correo informado".to_owned());

    println!("Cuenta vinculada: {account}");
    println!("Los tokens quedaron guardados en el gestor de secretos del sistema.");

    Ok(())
}

/// Elimina los tokens almacenados.
pub(crate) async fn logout() -> Result<()> {
    let store = KeyringTokenStore::connect(DEFAULT_ACCOUNT).await?;

    if store.load().await?.is_none() {
        println!("No hay una cuenta vinculada.");
        return Ok(());
    }

    store.delete().await?;
    println!("Se eliminaron los tokens del gestor de secretos.");
    println!(
        "Si quieres revocar el acceso por completo, hazlo en la configuración de tu cuenta de Google."
    );

    Ok(())
}

/// Muestra el estado de la cuenta vinculada, sin exponer los tokens.
pub(crate) async fn status() -> Result<()> {
    let store = KeyringTokenStore::connect(DEFAULT_ACCOUNT).await?;

    let Some(tokens) = store.load().await? else {
        println!("Sin cuenta vinculada. Ejecuta `gdrive-sync auth login`.");
        return Ok(());
    };

    let now = SystemTime::now();
    let access_state = if tokens.needs_refresh(now, REFRESH_MARGIN) {
        "requiere renovación"
    } else {
        "vigente"
    };

    println!("Cuenta vinculada: sí");
    println!("Token de acceso: {access_state}");
    println!("Expiración: {}", describe_expiry(&tokens, now));
    println!(
        "Token de renovación: {}",
        if tokens.can_refresh() {
            "presente"
        } else {
            "ausente"
        }
    );
    println!("Scopes: {}", tokens.scopes.join(" "));

    Ok(())
}

/// Lista archivos de Drive.
pub(crate) async fn ls(settings: &OAuthSettings, remote: Option<&str>, limit: u32) -> Result<()> {
    let http = gdrive_sync_drive::http::client()?;
    let store = KeyringTokenStore::connect(DEFAULT_ACCOUNT).await?;

    let tokens = store
        .load()
        .await?
        .ok_or_else(|| anyhow!("no hay una cuenta vinculada; ejecuta `gdrive-sync auth login`"))?;

    let auth = AuthClient::new(settings, http.clone(), None)?;
    let mut tokens = auth
        .ensure_fresh(tokens, SystemTime::now(), REFRESH_MARGIN)
        .await?;
    store.save(&tokens).await?;

    let folder_id = match remote {
        Some(path) => {
            let client = DriveClient::new(http.clone(), tokens.access_token.clone());
            match client.resolve_folder_path(path).await? {
                Some(id) => Some(id),
                None => bail!("no se encontró la carpeta remota '{path}'"),
            }
        }
        None => None,
    };

    let query = folder_id.map(|id| format!("'{id}' in parents and trashed = false"));

    let list = match list_page(&http, &tokens, query.as_deref(), limit).await {
        // Renovación reactiva: un 401 significa que el token expiró antes de lo
        // previsto (reloj desfasado o revocación parcial). Se renueva y se
        // reintenta una sola vez.
        Err(DriveError::Unauthorized) => {
            tokens = auth.refresh(&tokens).await?;
            store.save(&tokens).await?;
            list_page(&http, &tokens, query.as_deref(), limit).await?
        }
        other => other?,
    };

    print_files(&list);

    Ok(())
}

async fn list_page(
    http: &reqwest::Client,
    tokens: &TokenSet,
    query: Option<&str>,
    limit: u32,
) -> Result<FileListResponse, DriveError> {
    let client = DriveClient::new(http.clone(), tokens.access_token.clone());
    client.list_files(query, None, limit).await
}

fn print_files(list: &FileListResponse) {
    if list.files.is_empty() {
        println!("(sin resultados)");
        return;
    }

    println!(
        "{:>10}  {:<16} {:<50} MODIFICADO",
        "TAMAÑO", "TIPO", "NOMBRE"
    );
    println!("{}", "-".repeat(96));

    for file in &list.files {
        let kind = if file.is_folder() {
            "carpeta"
        } else {
            "archivo"
        };
        let size = file.size_bytes().map_or_else(|| "-".to_owned(), human_size);

        println!(
            "{:>10}  {:<16} {:<50} {}",
            size,
            kind,
            truncate(&file.name, 50),
            file.modified_time.as_deref().unwrap_or("-")
        );
    }
}

fn describe_expiry(tokens: &TokenSet, now: SystemTime) -> String {
    let Some(expires_at) = tokens.expires_at() else {
        return "no informada".to_owned();
    };

    match expires_at.duration_since(now) {
        Ok(remaining) => format!("en {} min", remaining.as_secs().div_ceil(60)),
        Err(_) => "vencido".to_owned(),
    }
}

fn human_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    let (unit, divisor) = if bytes >= GIB {
        ("GiB", GIB)
    } else if bytes >= MIB {
        ("MiB", MIB)
    } else if bytes >= KIB {
        ("KiB", KIB)
    } else {
        return format!("{bytes} B");
    };

    let whole = bytes / divisor;
    let tenth = (bytes % divisor) * 10 / divisor;

    format!("{whole}.{tenth} {unit}")
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }

    let mut truncated: String = value.chars().take(width.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdrive_sync_core::SecretString;

    #[test]
    fn formats_sizes_with_binary_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn truncates_long_names_by_characters() {
        assert_eq!(truncate("corto", 10), "corto");
        assert_eq!(truncate("ñandú-ñandú", 8), "ñandú-ñ…");
    }

    #[test]
    fn describes_the_expiry_relatively_to_now() {
        let now = SystemTime::now();
        let tokens = TokenSet::new(
            SecretString::new("a"),
            None,
            Some(Duration::from_secs(120)),
            now,
            Vec::new(),
        );

        assert_eq!(describe_expiry(&tokens, now), "en 2 min");

        let expired = TokenSet::new(
            SecretString::new("a"),
            None,
            Some(Duration::from_secs(1)),
            now - Duration::from_secs(10),
            Vec::new(),
        );
        assert_eq!(describe_expiry(&expired, now), "vencido");
    }

    #[test]
    fn reports_unknown_expiry() {
        let tokens = TokenSet::new(
            SecretString::new("a"),
            None,
            None,
            SystemTime::now(),
            Vec::new(),
        );

        assert_eq!(describe_expiry(&tokens, SystemTime::now()), "no informada");
    }
}

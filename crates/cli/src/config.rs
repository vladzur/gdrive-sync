// SPDX-License-Identifier: GPL-3.0-or-later
//! Configuración de la aplicación (ADR-005).
//!
//! El `client_id` y el `client_secret` los aporta cada usuario: el proyecto no
//! embebe credenciales. Se leen primero del entorno (útil en CI y en scripts) y
//! después de `$XDG_CONFIG_HOME/gdrive-sync/config.toml`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use gdrive_sync_auth::OAuthSettings;
use serde::Deserialize;

/// Variables de entorno que tienen prioridad sobre el archivo.
pub(crate) const ENV_CLIENT_ID: &str = "GDRIVE_SYNC_CLIENT_ID";
/// Variable de entorno para el secreto del cliente.
pub(crate) const ENV_CLIENT_SECRET: &str = "GDRIVE_SYNC_CLIENT_SECRET";

#[derive(Debug, Deserialize)]
struct ConfigFile {
    oauth: OAuthSection,
}

#[derive(Debug, Deserialize)]
struct OAuthSection {
    client_id: String,
    client_secret: String,
}

/// Ruta del archivo de configuración.
#[must_use]
pub(crate) fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("gdrive-sync").join("config.toml"))
}

/// Carga las credenciales OAuth.
///
/// # Errors
///
/// Devuelve un error con instrucciones si faltan las credenciales o si el
/// archivo existe pero no se puede interpretar.
pub(crate) fn load_oauth_settings() -> Result<OAuthSettings> {
    let from_env = match (
        std::env::var(ENV_CLIENT_ID),
        std::env::var(ENV_CLIENT_SECRET),
    ) {
        (Ok(client_id), Ok(client_secret))
            if !client_id.is_empty() && !client_secret.is_empty() =>
        {
            Some((client_id, client_secret))
        }
        _ => None,
    };

    let credentials = match from_env {
        Some(credentials) => credentials,
        None => read_from_file()?,
    };

    Ok(OAuthSettings::google(credentials.0, credentials.1))
}

fn read_from_file() -> Result<(String, String)> {
    let path = config_path().context("no se pudo determinar el directorio de configuración")?;

    if !path.exists() {
        bail!(
            "faltan las credenciales OAuth.\n\
             Definir {ENV_CLIENT_ID} y {ENV_CLIENT_SECRET}, o crear {} \
             con una sección [oauth] (ver docs/GOOGLE_SETUP.md).",
            path.display()
        );
    }

    if let Some(warning) = permissions_warning(&path) {
        tracing::warn!("{warning}");
    }

    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("no se pudo leer {}", path.display()))?;
    let parsed: ConfigFile = toml::from_str(&contents)
        .with_context(|| format!("configuración inválida en {}", path.display()))?;

    Ok((parsed.oauth.client_id, parsed.oauth.client_secret))
}

/// Verifica los permisos de un archivo de configuración con secretos.
///
/// En Unix, un `config.toml` legible por otros usuarios expone el secreto del
/// cliente. Se informa como advertencia y no como error: el `client_secret` de
/// un cliente de escritorio no es confidencial (ADR-006), pero el archivo no
/// debería quedar abierto.
#[must_use]
pub(crate) fn permissions_warning(path: &Path) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;

    let mode = std::fs::metadata(path).ok()?.permissions().mode();
    if mode & 0o077 != 0 {
        Some(format!(
            "{} es accesible por otros usuarios (modo {:o}); se recomienda `chmod 600`",
            path.display(),
            mode & 0o777
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_valid_configuration() {
        let parsed: ConfigFile = toml::from_str(
            r#"
            [oauth]
            client_id = "id-123"
            client_secret = "secret-456"
            "#,
        )
        .expect("configuración válida");

        assert_eq!(parsed.oauth.client_id, "id-123");
        assert_eq!(parsed.oauth.client_secret, "secret-456");
    }

    #[test]
    fn rejects_a_configuration_without_oauth_section() {
        assert!(toml::from_str::<ConfigFile>("otra = 1").is_err());
    }

    #[test]
    fn warns_when_the_file_is_readable_by_others() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("directorio temporal");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").expect("escritura");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        assert!(permissions_warning(&path).is_none());

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(permissions_warning(&path).is_some());
    }
}

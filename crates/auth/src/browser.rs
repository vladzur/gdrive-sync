// SPDX-License-Identifier: GPL-3.0-or-later
//! Apertura del navegador del sistema (RF-01).
//!
//! Dentro de Flatpak no se puede asumir `xdg-open`: se usa el portal `OpenURI`
//! a través de `gio open`, que es el camino sancionado por el sandbox (ADR-006).

use std::path::Path;
use std::process::{Command, Stdio};

use oauth2::url::Url;

/// Ruta que existe únicamente dentro de un sandbox Flatpak.
const FLATPAK_MARKER: &str = "/.flatpak-info";

/// Error al lanzar el navegador.
#[derive(Debug, thiserror::Error)]
#[error("no se pudo abrir el navegador con '{program}': {source}")]
pub struct BrowserError {
    /// Programa que se intentó ejecutar.
    pub program: &'static str,
    /// Error del sistema operativo.
    pub source: std::io::Error,
}

/// Indica si el proceso corre dentro de un sandbox Flatpak.
#[must_use]
pub fn is_flatpak() -> bool {
    Path::new(FLATPAK_MARKER).exists()
}

/// Programa y subcomando que abren una URL en el entorno actual.
fn launcher(flatpak: bool) -> (&'static str, Option<&'static str>) {
    if flatpak {
        ("gio", Some("open"))
    } else {
        ("xdg-open", None)
    }
}

/// Abre `url` en el navegador predeterminado sin bloquear.
///
/// Devuelve error si no se pudo lanzar el proceso; en ese caso el llamador debe
/// mostrar la URL para que el usuario la abra manualmente.
pub fn open(url: &Url) -> Result<(), BrowserError> {
    let (program, flag) = launcher(is_flatpak());

    let mut command = Command::new(program);
    if let Some(flag) = flag {
        command.arg(flag);
    }
    command
        .arg(url.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command
        .spawn()
        .map(|_child| ())
        .map_err(|source| BrowserError { program, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_openuri_portal_inside_flatpak() {
        assert_eq!(launcher(true), ("gio", Some("open")));
    }

    #[test]
    fn uses_xdg_open_on_the_host() {
        assert_eq!(launcher(false), ("xdg-open", None));
    }
}

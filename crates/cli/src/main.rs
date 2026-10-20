// SPDX-License-Identifier: GPL-3.0-or-later
//! Interfaz de línea de comandos de `gdrive-sync`.
//!
//! Hito 1: autenticación OAuth con PKCE, gestión de la cuenta y listado remoto.
//! Los comandos de sincronización llegan en los hitos 2 y 3.

mod commands;
mod config;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

/// Cliente nativo de Google Drive para Linux.
#[derive(Debug, Parser)]
#[command(
    name = "gdrive-sync",
    version,
    about = "Sincroniza un directorio local con Google Drive",
    long_about = None
)]
struct Cli {
    /// Incrementa el detalle del log (repetible: -v, -vv).
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Gestiona la cuenta de Google vinculada
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Lista archivos y carpetas de Drive
    Ls {
        /// Ruta remota, por ejemplo "documentos/fotos"
        #[arg(long)]
        remote: Option<String>,
        /// Cantidad máxima de elementos a listar
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
}

#[derive(Debug, Subcommand)]
enum AuthAction {
    /// Vincula una cuenta mediante el navegador
    Login,
    /// Elimina los tokens almacenados
    Logout,
    /// Muestra el estado de la cuenta vinculada
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli.command {
        Command::Auth { action } => match action {
            AuthAction::Login => commands::login(&config::load_oauth_settings()?).await,
            AuthAction::Logout => commands::logout().await,
            AuthAction::Status => commands::status().await,
        },
        Command::Ls { remote, limit } => {
            commands::ls(&config::load_oauth_settings()?, remote.as_deref(), limit).await
        }
    }
}

/// Configura `tracing` según la verbosidad pedida, con `RUST_LOG` como
/// mecanismo de anulación (ADR-011). Los logs van a `stderr` para no mezclarse
/// con la salida de los comandos.
fn init_tracing(verbose: u8) {
    let default_level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn verbosity_defaults_to_zero_and_counts_repetitions() {
        let default =
            Cli::try_parse_from(["gdrive-sync", "auth", "status"]).expect("argumentos válidos");
        assert_eq!(default.verbose, 0);

        let verbose = Cli::try_parse_from(["gdrive-sync", "-vv", "auth", "status"])
            .expect("argumentos válidos");
        assert_eq!(verbose.verbose, 2);
    }

    #[test]
    fn parses_the_auth_actions() {
        assert!(matches!(
            Cli::try_parse_from(["gdrive-sync", "auth", "login"])
                .expect("login")
                .command,
            Command::Auth {
                action: AuthAction::Login
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["gdrive-sync", "auth", "logout"])
                .expect("logout")
                .command,
            Command::Auth {
                action: AuthAction::Logout
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["gdrive-sync", "auth", "status"])
                .expect("status")
                .command,
            Command::Auth {
                action: AuthAction::Status
            }
        ));
    }

    #[test]
    fn parses_ls_options() {
        let cli = Cli::try_parse_from([
            "gdrive-sync",
            "ls",
            "--remote",
            "documentos",
            "--limit",
            "5",
        ])
        .expect("argumentos válidos");

        match cli.command {
            Command::Ls { remote, limit } => {
                assert_eq!(remote.as_deref(), Some("documentos"));
                assert_eq!(limit, 5);
            }
            other @ Command::Auth { .. } => panic!("se esperaba Ls, se obtuvo {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_arguments() {
        assert!(Cli::try_parse_from(["gdrive-sync", "--no-existe"]).is_err());
        assert!(
            Cli::try_parse_from(["gdrive-sync"]).is_err(),
            "el subcomando es obligatorio"
        );
    }
}

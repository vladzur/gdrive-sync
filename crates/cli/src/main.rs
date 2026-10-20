// SPDX-License-Identifier: GPL-3.0-or-later
//! Interfaz de línea de comandos de `gdrive-sync`.
//!
//! En el Hito 0 solo valida la definición de la CLI y la configuración de logs.
//! Los subcomandos (`auth`, `ls`, `status`, `sync`, `conflicts`) se agregan en
//! los hitos 1 a 3.

use clap::Parser;
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
}

fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "gdrive-sync iniciado");
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
        let default = Cli::try_parse_from(["gdrive-sync"]).expect("argumentos válidos");
        assert_eq!(default.verbose, 0);

        let verbose = Cli::try_parse_from(["gdrive-sync", "-vv"]).expect("argumentos válidos");
        assert_eq!(verbose.verbose, 2);
    }

    #[test]
    fn rejects_unknown_arguments() {
        assert!(Cli::try_parse_from(["gdrive-sync", "--no-existe"]).is_err());
    }
}

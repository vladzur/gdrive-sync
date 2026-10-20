//! Interfaz gráfica GTK4 / Libadwaita (RF-07, RNF-04).
//!
//! La GUI es un consumidor de estado: no contiene lógica de negocio ni accede a
//! la red. Se implementa en el Hito 4 (tareas T4.1 a T4.12).
//!
//! Este crate existe desde el Hito 0 para fijar las versiones de GTK4 y
//! Libadwaita en el grafo de dependencias (ADR-013). El código se compila solo
//! con `--features gui`, porque requiere `libgtk-4-dev` y `libadwaita-1-dev`
//! instalados en el sistema.

/// Identificador de la aplicación, compartido con el `.desktop`, el metadato
/// `AppStream` y el nombre del servicio D-Bus.
pub const APP_ID: &str = "io.github.gdrive_sync";

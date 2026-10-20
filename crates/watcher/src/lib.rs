// SPDX-License-Identifier: GPL-3.0-or-later
//! Monitoreo del sistema de archivos (RF-03).
//!
//! Usa `inotify` mediante `notify` y agrupa ráfagas con
//! `notify-debouncer-full` para no subir archivos a medio escribir (ADR-007).
//! Se implementa en el Hito 2 (tareas T2.4 a T2.7).

/// Ventana de agrupación por defecto para los eventos del sistema de archivos.
pub const DEFAULT_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(500);

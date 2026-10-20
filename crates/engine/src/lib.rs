//! Motor de sincronización (RF-05, RF-06).
//!
//! Implementa la matriz de decisión de ADR-001 como función pura, el
//! planificador de operaciones y el ejecutor concurrente con cola persistente.
//! Se implementa en el Hito 3 (tareas T3.1 a T3.15).

/// Máximo de transferencias simultáneas por defecto (RNF-03).
pub const DEFAULT_CONCURRENT_TRANSFERS: usize = 4;

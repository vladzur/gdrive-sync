// SPDX-License-Identifier: GPL-3.0-or-later
//! Cliente REST de Google Drive API v3.
//!
//! Cubre la detección de cambios remotos (`changes.list`, RF-04) y las
//! transferencias de contenido (RF-05). Los endpoints y parámetros usados están
//! documentados en el Anexo D del plan de implementación.

pub mod http;

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// Estado de sincronización de un nodo.
///
/// La representación textual es parte del contrato con la base de datos: debe
/// coincidir con la restricción `CHECK` de `sync_nodes.sync_status` definida en
/// el Anexo A del plan de implementación.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyncStatus {
    /// Local y remoto coinciden con el último estado sincronizado.
    Synced,
    /// Hay cambios locales pendientes de subir.
    PendingUpload,
    /// Hay cambios remotos pendientes de descargar.
    PendingDownload,
    /// La eliminación se originó en el remoto y aún no se aplicó en disco.
    PendingDeleteLocal,
    /// La eliminación se originó en local y aún no se aplicó en Drive.
    PendingDeleteRemote,
    /// Ambas copias cambiaron desde la última sincronización.
    Conflict,
    /// La última operación falló; requiere intervención o reintento agotado.
    Error,
}

impl SyncStatus {
    /// Todos los estados, en el orden del esquema.
    pub const ALL: [Self; 7] = [
        Self::Synced,
        Self::PendingUpload,
        Self::PendingDownload,
        Self::PendingDeleteLocal,
        Self::PendingDeleteRemote,
        Self::Conflict,
        Self::Error,
    ];

    /// Representación canónica persistida en la base de datos.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Synced => "SYNCED",
            Self::PendingUpload => "PENDING_UPLOAD",
            Self::PendingDownload => "PENDING_DOWNLOAD",
            Self::PendingDeleteLocal => "PENDING_DELETE_LOCAL",
            Self::PendingDeleteRemote => "PENDING_DELETE_REMOTE",
            Self::Conflict => "CONFLICT",
            Self::Error => "ERROR",
        }
    }

    /// Indica si el nodo requiere trabajo adicional para converger.
    #[must_use]
    pub const fn is_pending(self) -> bool {
        !matches!(self, Self::Synced)
    }
}

impl fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error devuelto al interpretar un estado desconocido.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("estado de sincronización desconocido: {0}")]
pub struct ParseSyncStatusError(String);

impl ParseSyncStatusError {
    /// Valor que no pudo interpretarse.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.0
    }
}

impl FromStr for SyncStatus {
    type Err = ParseSyncStatusError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "SYNCED" => Ok(Self::Synced),
            "PENDING_UPLOAD" => Ok(Self::PendingUpload),
            "PENDING_DOWNLOAD" => Ok(Self::PendingDownload),
            "PENDING_DELETE_LOCAL" => Ok(Self::PendingDeleteLocal),
            "PENDING_DELETE_REMOTE" => Ok(Self::PendingDeleteRemote),
            "CONFLICT" => Ok(Self::Conflict),
            "ERROR" => Ok(Self::Error),
            other => Err(ParseSyncStatusError(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_representation_round_trips_for_every_variant() {
        for status in SyncStatus::ALL {
            let parsed: SyncStatus = status.as_str().parse().expect("estado válido");
            assert_eq!(parsed, status, "round-trip falló para {status}");
        }
    }

    #[test]
    fn every_variant_has_a_distinct_representation() {
        let mut seen = Vec::new();
        for status in SyncStatus::ALL {
            assert!(
                !seen.contains(&status.as_str()),
                "representación duplicada: {}",
                status.as_str()
            );
            seen.push(status.as_str());
        }
        assert_eq!(seen.len(), SyncStatus::ALL.len());
    }

    #[test]
    fn serde_uses_the_database_representation() {
        let json = serde_json::to_string(&SyncStatus::PendingUpload).expect("serializa");
        assert_eq!(json, "\"PENDING_UPLOAD\"");

        let parsed: SyncStatus = serde_json::from_str("\"CONFLICT\"").expect("deserializa");
        assert_eq!(parsed, SyncStatus::Conflict);
    }

    #[test]
    fn rejects_unknown_values() {
        let error = "SYNCED "
            .parse::<SyncStatus>()
            .expect_err("no debe aceptarse");
        assert_eq!(error.value(), "SYNCED ");
    }

    #[test]
    fn only_synced_is_not_pending() {
        assert!(!SyncStatus::Synced.is_pending());
        for status in SyncStatus::ALL {
            if status != SyncStatus::Synced {
                assert!(status.is_pending(), "{status} debería estar pendiente");
            }
        }
    }
}

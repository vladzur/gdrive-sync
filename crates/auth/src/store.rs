// SPDX-License-Identifier: GPL-3.0-or-later
//! Almacenamiento de tokens en el gestor de secretos (RF-02).
//!
//! `oo7` elige el backend según el entorno: `org.freedesktop.Secrets` en el
//! host y el portal `Secret` dentro de Flatpak. Los tokens se guardan como JSON
//! bajo atributos estables, nunca en el sistema de archivos ni en la base.

use std::collections::HashMap;

use gdrive_sync_core::{TokenSet, TokenStore, TokenStoreError};

/// Etiqueta visible del ítem en el gestor de contraseñas.
const LABEL: &str = "gdrive-sync-app: tokens de Google Drive";

/// Atributo que identifica a la aplicación.
const SERVICE: &str = "gdrive-sync-app";

/// Cuenta por defecto mientras no exista soporte multicuenta.
pub const DEFAULT_ACCOUNT: &str = "default";

/// Almacén de tokens sobre Secret Service.
pub struct KeyringTokenStore {
    keyring: oo7::Keyring,
    attributes: HashMap<String, String>,
}

impl std::fmt::Debug for KeyringTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyringTokenStore")
            .field("attributes", &self.attributes)
            .finish_non_exhaustive()
    }
}

impl KeyringTokenStore {
    /// Conecta con el gestor de secretos del entorno.
    ///
    /// # Errors
    ///
    /// Devuelve [`TokenStoreError::Backend`] si no hay un servicio de secretos
    /// disponible (por ejemplo, en una sesión sin D-Bus).
    pub async fn connect(account: &str) -> Result<Self, TokenStoreError> {
        let keyring = oo7::Keyring::new()
            .await
            .map_err(|err| TokenStoreError::Backend(err.to_string()))?;

        Ok(Self {
            keyring,
            attributes: attributes(account),
        })
    }
}

fn attributes(account: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("service".to_owned(), SERVICE.to_owned());
    map.insert("account".to_owned(), account.to_owned());
    map
}

impl TokenStore for KeyringTokenStore {
    async fn load(&self) -> Result<Option<TokenSet>, TokenStoreError> {
        let items = self
            .keyring
            .search_items(&self.attributes)
            .await
            .map_err(|err| TokenStoreError::Backend(err.to_string()))?;

        let Some(item) = items.into_iter().next() else {
            return Ok(None);
        };

        let secret = item
            .secret()
            .await
            .map_err(|err| TokenStoreError::Backend(err.to_string()))?;

        let tokens = serde_json::from_slice(secret.as_bytes())
            .map_err(|err| TokenStoreError::Corrupt(err.to_string()))?;

        Ok(Some(tokens))
    }

    async fn save(&self, tokens: &TokenSet) -> Result<(), TokenStoreError> {
        let payload =
            serde_json::to_vec(tokens).map_err(|err| TokenStoreError::Corrupt(err.to_string()))?;

        self.keyring
            .create_item(LABEL, &self.attributes, oo7::Secret::blob(payload), true)
            .await
            .map_err(|err| TokenStoreError::Backend(err.to_string()))
    }

    async fn delete(&self) -> Result<(), TokenStoreError> {
        self.keyring
            .delete(&self.attributes)
            .await
            .map_err(|err| TokenStoreError::Backend(err.to_string()))
    }
}

/// Implementaciones auxiliares para tests de la propia crate.
#[cfg(test)]
pub(crate) mod test_support {
    use std::future::Future;
    use std::sync::{Arc, Mutex};

    use gdrive_sync_core::{TokenSet, TokenStore, TokenStoreError};

    /// Almacén en memoria con la misma semántica que el de Secret Service.
    #[derive(Debug, Clone, Default)]
    pub(crate) struct InMemoryTokenStore {
        inner: Arc<Mutex<Option<TokenSet>>>,
    }

    impl InMemoryTokenStore {
        pub(crate) fn snapshot(&self) -> Option<TokenSet> {
            self.inner.lock().expect("lock").clone()
        }
    }

    impl TokenStore for InMemoryTokenStore {
        fn load(&self) -> impl Future<Output = Result<Option<TokenSet>, TokenStoreError>> + Send {
            std::future::ready(Ok(self.inner.lock().expect("lock").clone()))
        }

        fn save(
            &self,
            tokens: &TokenSet,
        ) -> impl Future<Output = Result<(), TokenStoreError>> + Send {
            *self.inner.lock().expect("lock") = Some(tokens.clone());
            std::future::ready(Ok(()))
        }

        fn delete(&self) -> impl Future<Output = Result<(), TokenStoreError>> + Send {
            *self.inner.lock().expect("lock") = None;
            std::future::ready(Ok(()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use gdrive_sync_core::SecretString;

    use super::test_support::InMemoryTokenStore;
    use super::*;

    #[test]
    fn attributes_include_the_account() {
        let map = attributes("persona@example.com");

        assert_eq!(map.get("service").map(String::as_str), Some(SERVICE));
        assert_eq!(
            map.get("account").map(String::as_str),
            Some("persona@example.com")
        );
    }

    #[tokio::test]
    async fn in_memory_store_round_trips_tokens() {
        let store = InMemoryTokenStore::default();
        assert!(store.load().await.expect("load").is_none());

        let tokens = TokenSet::new(
            SecretString::new("access"),
            Some(SecretString::new("refresh")),
            None,
            SystemTime::now(),
            vec!["drive".to_owned()],
        );

        store.save(&tokens).await.expect("save");
        assert_eq!(store.snapshot(), Some(tokens.clone()));
        assert_eq!(store.load().await.expect("load"), Some(tokens));

        store.delete().await.expect("delete");
        assert!(store.load().await.expect("load").is_none());
    }
}

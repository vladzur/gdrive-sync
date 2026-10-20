# Configurar el acceso a Google Drive

`gdrive-sync-app` no incluye credenciales de Google: cada instalación usa su
propio `client_id`, creado en Google Cloud Console. Esto evita depender de un
secreto compartido y desacopla el proyecto del proceso de verificación de
scopes restringidos de Google (ADR-005).

El trámite se hace una sola vez y toma unos 10 minutos.

## 1. Crear el proyecto

1. Entrar en <https://console.cloud.google.com/projectcreate>.
2. Elegir un nombre (por ejemplo `gdrive-sync-app`) y crear el proyecto.

## 2. Habilitar la API de Drive

1. Ir a **APIs y servicios → Biblioteca**.
2. Buscar **Google Drive API** y pulsar **Habilitar**.

## 3. Configurar la pantalla de consentimiento

1. Ir a **APIs y servicios → Pantalla de consentimiento de OAuth**.
2. Tipo de usuario: **Externo**.
3. Completar nombre de la aplicación, correo de asistencia y datos de contacto.
4. En **Scopes**, agregar:

   ```
   https://www.googleapis.com/auth/drive
   ```

   Es un scope **restringido**: permite leer y escribir todos los archivos de
   Drive del usuario, que es lo mínimo necesario para sincronizar un directorio
   con contenido preexistente. `drive.file` no sirve para ese caso porque solo
   da acceso a archivos creados por la aplicación.

5. Mientras la aplicación esté en modo **Prueba**, agregar tu cuenta en
   **Usuarios de prueba**. Sin ese paso, Google bloquea el acceso con
   *"Access blocked: this app's request is invalid"*.

## 4. Crear las credenciales

1. Ir a **APIs y servicios → Credenciales → Crear credenciales → ID de cliente
   de OAuth**.
2. Tipo de aplicación: **Aplicación de escritorio**.
3. Copiar el `client_id` y el `client_secret`.

No hace falta registrar la URL de redirección: el flujo usa *loopback* en
`http://127.0.0.1` con un puerto efímero, que Google acepta para este tipo de
cliente sin configuración adicional (ADR-006).

## 5. Entregar las credenciales a la aplicación

### Opción A — variables de entorno (útil en scripts y CI)

```bash
export GDRIVE_SYNC_CLIENT_ID="tu-client-id.apps.googleusercontent.com"
export GDRIVE_SYNC_CLIENT_SECRET="tu-client-secret"
```

Tienen prioridad sobre el archivo de configuración.

### Opción B — archivo de configuración (recomendada para uso diario)

```bash
mkdir -p ~/.config/gdrive-sync
cat > ~/.config/gdrive-sync/config.toml <<'CONFIG'
[oauth]
client_id = "tu-client-id.apps.googleusercontent.com"
client_secret = "tu-client-secret"
CONFIG
chmod 600 ~/.config/gdrive-sync/config.toml
```

La aplicación advierte si el archivo es legible por otros usuarios.

## 6. Vincular la cuenta

```bash
cargo run -p gdrive-sync-cli -- auth login
```

Se abre el navegador; al autorizar, la pestaña muestra una confirmación y la
terminal informa qué cuenta quedó vinculada. Los tokens se guardan en el gestor
de secretos del sistema (GNOME Keyring / KWallet), no en disco.

Para verificar el resultado:

```bash
cargo run -p gdrive-sync-cli -- auth status
cargo run -p gdrive-sync-cli -- ls --limit 10
```

## Problemas frecuentes

| Síntoma | Causa | Solución |
| --- | --- | --- |
| `Access blocked: this app's request is invalid` | La cuenta no está en **Usuarios de prueba** | Agregarla en la pantalla de consentimiento |
| `invalid_grant` al renovar | El acceso fue revocado o el `refresh_token` caducó | `auth logout` y volver a hacer `auth login` |
| `redirect_uri_mismatch` | Se creó una credencial de tipo *Web*, no *Desktop* | Crear un cliente de tipo **Aplicación de escritorio** |
| `no se pudo acceder al servicio de secretos` | No hay Secret Service disponible (sesión sin D-Bus) | Iniciar sesión en el escritorio o desbloquear el llavero |
| El navegador no se abre | No hay `xdg-open` ni portal `OpenURI` | Copiar la URL que imprime la terminal |
| `faltan las credenciales OAuth` | No se definieron las variables ni el archivo | Repetir el paso 5 |

## Qué datos salen del equipo

Únicamente las peticiones a `oauth2.googleapis.com` y `www.googleapis.com` con
el token de acceso. No hay telemetría ni envío de datos a terceros. Para revocar
el acceso por completo, hacerlo desde <https://myaccount.google.com/permissions>
y después ejecutar `auth logout`.

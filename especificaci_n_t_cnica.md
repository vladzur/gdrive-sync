# Documento de Diseño de Solución y Requerimientos Técnicos

**Proyecto:** Cliente Nativo de Google Drive para Linux (`gdrive-sync`)

**Autor:** Equipo de Arquitectura de Software

**Estado:** Propuesta de Arquitectura (RFC)

**Tecnologías:** Rust, GTK4, Libadwaita, Tokio, SQLite, Google Drive API v3

## 1. Visión General del Proyecto

### 1.1 Objetivo

Desarrollar un cliente nativo de escritorio para Linux que proporcione sincronización bidireccional en tiempo real entre un directorio local del usuario y el servicio de almacenamiento en la nube Google Drive.

### 1.2 Justificación

La falta de un cliente oficial de Google Drive para sistemas operativos basados en Linux obliga a los usuarios a recurrir a herramientas en línea de comandos (`rclone`) o interfaces no nativas. Una aplicación desarrollada en **Rust** e integrada con **GTK4 / Libadwaita** ofrece máxima eficiencia de recursos, seguridad de memoria y alineación visual con el ecosistema moderno de GNOME.

## 2. Requerimientos del Sistema

### 2.1 Requerimientos Funcionales (RF)

| ID | Requerimiento | Descripción | 
| ----- | ----- | ----- | 
| **RF-01** | Autenticación OAuth2 | El usuario debe poder vincular su cuenta de Google mediante el navegador web predeterminado usando OAuth 2.0 (PKCE). | 
| **RF-02** | Almacenamiento Seguro | Guardar tokens de acceso (`access_token`, `refresh_token`) en el gestor de contraseñas nativo (*Gnome Keyring* / *Secret Service*). | 
| **RF-03** | Monitoreo Local | Detectar eventos de creación, modificación y eliminación de archivos/carpetas en el directorio sincronizado en tiempo real. | 
| **RF-04** | Detección Remota | Consultar de forma eficiente los cambios remotos mediante el endpoint `changes.list` de Google Drive API. | 
| **RF-05** | Sincronización Bidireccional | Transferir archivos modificados tanto local como remotamente para mantener ambos entornos equivalentes. | 
| **RF-06** | Resolución de Conflictos | Crear copias de conflicto locales cuando un mismo archivo haya sido modificado simultáneamente en local y remoto. | 
| **RF-07** | Interfaz de Usuario (GUI) | Proveer una ventana principal para seleccionar carpetas, ver el progreso de transferencia y gestionar la cuenta activa. | 
| **RF-08** | Indicador de Estado | Integrar un ícono en el área de notificación (System Tray / AppIndicator) para reportar el estado de sincronización. | 

### 2.2 Requerimientos No Funcionales (RNF)

| ID | Criterio | Métrica / Restricción | 
| ----- | ----- | ----- | 
| **RNF-01** | Rendimiento | Consumo de memoria RAM $\le 80 \text{ MB}$ en estado de reposo (*idle*). | 
| **RNF-02** | Eficiencia de CPU | Uso de CPU $\le 1\%$ cuando no existan transferencias activas (uso de `inotify` en lugar de *polling* local). | 
| **RNF-03** | Concurrencia | Capacidad de realizar cargas/descargas en paralelo usando *async/await* (`tokio`). | 
| **RNF-04** | Integración OS | Cumplimiento con las guías de diseño de GNOME (*GNOME Human Interface Guidelines*). | 
| **RNF-05** | Resiliencia | Reintento automático de peticiones fallidas ante pérdidas temporales de conexión a Internet (*exponential backoff*). | 

## 3. Arquitectura del Sistema

El sistema utiliza una arquitectura orientada a eventos (*event-driven architecture*) desacoplada en cuatro capas principales a través de canales asíncronos (`tokio::sync::mpsc`).

```
 +-------------------------------------------------------+
 |                 Capa de Presentación                  |
 |            (GTK4 / Libadwaita / AppIndicator)         |
 +---------------------------+---------------------------+
                             | Eventos UI / Estado
                             v
 +-------------------------------------------------------+
 |               Motor de Sincronización                 |
 |            (Sync Engine / Task Scheduler)             |
 +--------------+----------------------------+-----------+
                |                            |
       Lectura / Escritura                  API Requests
                v                            v
 +------------------------------+   +--------------------+
 | Base de Datos & File System  |   |  Google Drive API  |
 |   (SQLite3 + inotify)        |   |   (REST Client)    |
 +------------------------------+   +--------------------+

```

## 4. Diseño de Componentes

### 4.1 Módulo de Autenticación (`auth`)

* **Flujo:** OAuth 2.0 con *Proof Key for Code Exchange* (PKCE) para aplicaciones nativas (`RFC 7636`).

* **Servidor Local:** Levanta un listener HTTP temporal en `http://127.0.0.1:8080/callback` para recibir el código de autorización.

* **Persistencia:** Almacenamiento de tokens en el servicio D-Bus Secret Service mediante la crate `oo7` / `keyring`.

### 4.2 Módulo del FileSystem Watcher (`watcher`)

* Utiliza el subsistema `inotify` del kernel Linux a través de la crate `notify`.

* Mapea eventos del sistema de archivos (`Create`, `Modify`, `Remove`, `Rename`) a eventos internos `LocalSyncEvent`.

* Aplica una estrategia de *debounce* (espera de $500 \text{ ms}$) para evitar subir archivos parciales mientras están siendo escritos en disco.

### 4.3 Módulo de Persistencia Local (`db`)

Se utiliza **SQLite 3** (`rusqlite`) para guardar el árbol de metadatos locales y establecer una correspondencia 1:1 con la nube.

#### Esquema de Base de Datos:

```
CREATE TABLE sync_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    local_path TEXT NOT NULL UNIQUE,
    google_drive_id TEXT,
    is_directory BOOLEAN NOT NULL DEFAULT 0,
    md5_checksum TEXT,
    last_modified_local INTEGER NOT NULL,
    last_modified_remote INTEGER,
    sync_status TEXT CHECK(sync_status IN ('SYNCED', 'PENDING_UPLOAD', 'PENDING_DOWNLOAD', 'CONFLICT', 'ERROR')),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_local_path ON sync_nodes(local_path);
CREATE INDEX idx_google_id ON sync_nodes(google_drive_id);

```

### 4.4 Motor de Sincronización (`engine`)

El motor de sincronización gestiona el estado global comparando tres fuentes de verdad:

1. **El Estado Local:** Sistema de archivos actual.

2. **La Base de Datos Indexada:** Estado de la última sincronización exitosa.

3. **El Estado Remoto:** Metadatos provenientes de la API v3 de Google Drive.

#### Matriz de Decisiones de Sincronización:

| Estado en DB | Cambio Local | Cambio Remoto | Acción del Motor | 
| ----- | ----- | ----- | ----- | 
| Existe | Sin cambios | Sin cambios | **Ninguna** (Sincronizado) | 
| Existe | Modificado | Sin cambios | Subir actualización a Google Drive (`files.update`) | 
| Existe | Sin cambios | Modificado | Descargar actualización local (`files.get`) | 
| Existe | Modificado | Modificado | **Conflicto:** Renombrar archivo local a `nombre (Copia de Conflicto).ext` | 
| No existe | Creado | No existe | Crear recurso en la nube (`files.create`) | 
| No existe | No existe | Creado | Descargar recurso nuevo a disco local | 

## 5. Integración con Google Drive API (v3)

### 5.1 Endpoints Principales

| Operación | Método HTTP | Endpoint | 
| ----- | ----- | ----- | 
| Obtener Token Inicial | `POST` | `https://oauth2.googleapis.com/token` | 
| Listar Cambios Remotos | `GET` | `https://www.googleapis.com/drive/v3/changes` | 
| Subir Archivo Pequeno ($\le 5\text{MB}$) | `POST` | `https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart` | 
| Subir Archivo Grande ($> 5\text{MB}$) | `POST` / `PUT` | `https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable` | 
| Descargar Contenido | `GET` | `https://www.googleapis.com/drive/v3/files/{fileId}?alt=media` | 

## 6. Stack Tecnológico Sugerido

```
[dependencies]
# GUI Framework
gtk4 = { version = "0.8", features = ["v4_12"] }
libadwaita = { version = "0.6", features = ["v1_4"] }

# Asincronía y Red
tokio = { version = "1.35", features = ["full"] }
reqwest = { version = "0.11", features = ["json", "stream", "multipart"] }

# OAuth2 y Seguridad
oauth2 = "4.4"
oo7 = "0.2" # Secret Service integration

# Monitoreo y Base de Datos
notify = "6.1"
rusqlite = { version = "0.30", features = ["bundled"] }

# Serialización
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

```

## 7. Plan de Implementación (Hitos)

1. **Hito 1: Núcleo CLI & OAuth2**

   Implementar autenticación PKCE, persistencia de tokens y peticiones básicas REST a Google Drive.

2. **Hito 2: Monitoreo Local y Base de Datos**

   Configurar `notify` e integrar la base de datos SQLite para realizar seguimiento de cambios locales.

3. **Hito 3: Motor de Sincronización Básica**

   Lógica de subida/descarga bidireccional y resolución de conflictos simple.

4. **Hito 4: Interfaz Gráfica GTK4 / Libadwaita**

   Crear la ventana principal, selector de carpetas y panel de monitoreo.

5. **Hito 5: AppIndicator & Empaquetado**

   Integración con la barra superior de GNOME y empaquetado en formato **Flatpak**.
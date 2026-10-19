//! Synchro Internet Mode 0.4 - private Rust coordination and encrypted-transport relay.
//! Nginx/Cloudflare must terminate HTTPS; this service binds loopback only.
mod api;
mod websocket;
mod setup;

use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use tokio::sync::{broadcast, Mutex as AsyncMutex, Semaphore};
use tower_http::limit::RequestBodyLimitLayer;
use tracing::{error, info};

pub(crate) const CHUNK_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_FILE_BYTES: i64 = 2 * 1024 * 1024 * 1024;
pub(crate) const MAX_FILE_NAME_BYTES: usize = 180;

#[derive(Clone)]
pub(crate) struct Config {
    pub storage: PathBuf,
    pub admin_token: String,
    pub storage_quota: i64,
    pub ttl_hours: i64,
}

#[derive(Clone)]
pub(crate) struct State {
    pub db: Arc<Mutex<Connection>>,
    pub config: Arc<Config>,
    pub notifications: broadcast::Sender<Signal>,
    pub chunk_slots: Arc<Semaphore>,
    // One mutation gate keeps PUT, finish, ACK, and expiry cleanup consistent.
    // Intentionally conservative for a small, single-owner VPS.
    pub mutation_lock: Arc<AsyncMutex<()>>,
    pub ws_slots: Arc<Semaphore>,
    pub download_slots: Arc<Semaphore>,
}

#[derive(Clone, Serialize)]
pub(crate) struct Signal {
    #[serde(rename = "type")]
    pub event_type: String,
    pub device_id: String,
    pub transfer_id: String,
}

#[derive(Clone, Serialize)]
pub(crate) struct Device {
    pub id: String,
    pub name: String,
}

#[derive(Debug)]
pub(crate) struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }
    pub fn internal() -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
    }
    pub fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "Authentication required".to_string())
    }
    pub fn forbidden() -> Self {
        Self(StatusCode::FORBIDDEN, "Access denied".to_string())
    }
    pub fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "Transfer or device not found".to_string())
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, message.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(crate) fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub(crate) fn new_token() -> String {
    let mut data = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut data);
    hex::encode(data)
}

pub(crate) fn token_from_header(headers: &HeaderMap) -> Result<&str, ApiError> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(ApiError::unauthorized)?;
    let token = raw.strip_prefix("Bearer ").ok_or_else(ApiError::unauthorized)?;
    if token.len() < 32 || token.len() > 128 || !token.is_ascii() {
        return Err(ApiError::unauthorized());
    }
    Ok(token)
}

pub(crate) fn verify_admin(headers: &HeaderMap, state: &State) -> Result<(), ApiError> {
    let supplied = token_from_header(headers)?;
    if bool::from(supplied.as_bytes().ct_eq(state.config.admin_token.as_bytes())) {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}

pub(crate) fn verify_device(headers: &HeaderMap, state: &State) -> Result<Device, ApiError> {
    let token = token_from_header(headers)?;
    let hash = hash_token(token);
    let conn = state.db.lock().map_err(|_| ApiError::internal())?;
    conn.query_row(
        "SELECT id, name FROM devices WHERE token_sha256 = ?1 AND revoked = 0",
        [hash],
        |row| Ok(Device { id: row.get(0)?, name: row.get(1)? }),
    )
    .optional()
    .map_err(|_| ApiError::internal())?
    .ok_or_else(ApiError::unauthorized)
}

pub(crate) fn safe_id(id: &str) -> Result<String, ApiError> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| ApiError::bad("Invalid transfer id"))?;
    let canonical = parsed.to_string();
    if id != canonical { return Err(ApiError::bad("Invalid transfer id")); }
    Ok(canonical)
}

pub(crate) fn safe_filename(name: &str) -> Result<(), ApiError> {
    if name.is_empty() || name.as_bytes().len() > MAX_FILE_NAME_BYTES ||
        name.trim() != name || name == "." || name == ".." ||
        name.contains('/') || name.contains('\\') ||
        name.chars().any(|c| c.is_control())
    {
        return Err(ApiError::bad("Invalid filename (use a basename, no paths or control characters)"));
    }
    Ok(())
}

fn init_db(path: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    let db = Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS devices (
             id TEXT PRIMARY KEY,
             name TEXT NOT NULL,
             token_sha256 TEXT NOT NULL UNIQUE,
             created_at INTEGER NOT NULL,
             revoked INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS transfers (
             id TEXT PRIMARY KEY,
             sender_id TEXT NOT NULL REFERENCES devices(id),
             recipient_id TEXT NOT NULL REFERENCES devices(id),
             filename TEXT NOT NULL,
             size_bytes INTEGER NOT NULL CHECK (size_bytes > 0),
             sha256 TEXT NOT NULL,
             status TEXT NOT NULL CHECK(status IN ('uploading','finalizing','ready','delivered')),
             created_at INTEGER NOT NULL,
             expires_at INTEGER NOT NULL,
             delivered_at INTEGER
         );
         CREATE INDEX IF NOT EXISTS idx_transfer_recipient
         ON transfers(recipient_id,status,created_at);
         CREATE INDEX IF NOT EXISTS idx_transfer_expiry
         ON transfers(expires_at,status);
         CREATE TABLE IF NOT EXISTS system_settings (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );"
    )?;
    // An interrupted finalization should be retried rather than stuck forever.
    db.execute(
        "UPDATE transfers SET status = 'uploading' WHERE status = 'finalizing'",
        [],
    )?;
    Ok(db)
}

pub(crate) async fn prune_expired(state: &State) -> Result<usize, ApiError> {
    // Guard the entire metadata + file removal operation against concurrent writes.
    let _guard = state.mutation_lock.lock().await;
    let expired: Vec<String> = {
        let db = state.db.lock().map_err(|_| ApiError::internal())?;
        let mut statement = db.prepare(
            "SELECT id FROM transfers WHERE expires_at <= ?1 OR (status = 'delivered' AND delivered_at <= ?2)"
        ).map_err(|_| ApiError::internal())?;
        let rows = statement.query_map(params![now(), now() - 3600], |row| row.get::<_, String>(0))
            .map_err(|_| ApiError::internal())?;
        rows.filter_map(Result::ok).collect()
    };
    let mut deleted = 0;
    'items: for id in expired {
        // UUID is validated even though values came from SQLite.
        let id = safe_id(&id)?;
        let paths = [
            state.config.storage.join("objects").join(format!("{id}.blob")),
            state.config.storage.join("objects").join(format!("{id}.tmp")),
        ];
        for path in paths {
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    error!("cleanup retry needed for {id}: {e}");
                    continue 'items;
                }
            }
        }
        let chunk_dir = state.config.storage.join("chunks").join(&id);
        match tokio::fs::remove_dir_all(&chunk_dir).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                error!("chunk cleanup retry needed for {id}: {e}");
                continue 'items;
            }
        }
        // Retain metadata until cleanup succeeds, so future prunes can retry.
        let db = state.db.lock().map_err(|_| ApiError::internal())?;
        let changed = db.execute("DELETE FROM transfers WHERE id = ?1", [&id])
            .map_err(|_| ApiError::internal())?;
        deleted += changed;
    }
    Ok(deleted)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "synchro_server=info,tower_http=warn".into()),
        )
        .init();

    let admin_token = env::var("SYNCHRO_ADMIN_TOKEN")
        .map_err(|_| "Set SYNCHRO_ADMIN_TOKEN to a cryptographically random secret (at least 32 characters)")?;
    if admin_token.len() != 64 || !admin_token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("SYNCHRO_ADMIN_TOKEN must be 64 random hexadecimal characters (openssl rand -hex 32)".into());
    }
    let storage = PathBuf::from(env::var("SYNCHRO_STORAGE_DIR")
        .unwrap_or_else(|_| "/var/lib/synchro/files".to_string()));
    let db_path = PathBuf::from(env::var("SYNCHRO_DB_PATH")
        .unwrap_or_else(|_| "/var/lib/synchro/synchro.sqlite3".to_string()));
    let bind: SocketAddr = env::var("SYNCHRO_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_string())
        .parse()?;
    if !bind.ip().is_loopback() {
        return Err("Refusing to expose HTTP on a public interface. Bind to 127.0.0.1 and proxy HTTPS via Nginx.".into());
    }
    let storage_quota: i64 = env::var("SYNCHRO_STORAGE_QUOTA_BYTES")
        .unwrap_or_else(|_| (5_i64 * 1024 * 1024 * 1024).to_string())
        .parse()?;
    if storage_quota < MAX_FILE_BYTES {
        return Err("Storage quota must be at least the maximum 2 GiB file size.".into());
    }
    let ttl_hours: i64 = env::var("SYNCHRO_TTL_HOURS")
        .unwrap_or_else(|_| "72".to_string())
        .parse()?;
    if !(1..=720).contains(&ttl_hours) {
        return Err("SYNCHRO_TTL_HOURS must be 1..720.".into());
    }
    tokio::fs::create_dir_all(storage.join("chunks")).await?;
    tokio::fs::create_dir_all(storage.join("objects")).await?;
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let db = init_db(&db_path)?;
    let (notifications, _) = broadcast::channel::<Signal>(256);
    let state = State {
        db: Arc::new(Mutex::new(db)),
        config: Arc::new(Config { storage, admin_token, storage_quota, ttl_hours }),
        notifications,
        chunk_slots: Arc::new(Semaphore::new(3)),
        mutation_lock: Arc::new(AsyncMutex::new(())),
        ws_slots: Arc::new(Semaphore::new(32)),
        download_slots: Arc::new(Semaphore::new(4)),
    };
    // Periodic cleanup; no direct public admin access required.
    let cleaning = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            if let Err(err) = prune_expired(&cleaning).await {
                error!("prune failed: {}", err.1);
            }
        }
    });
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({"status":"ok","version":"0.4.1"})) }))
        .route("/api/v1/setup/status", get(setup::status))
        .route("/api/v1/setup/check", get(setup::check))
        .route("/api/v1/setup/finish", post(setup::finish))
        .route("/api/v1/admin/origin", post(setup::update_origin))
        .route("/api/v1/admin/stats", get(setup::stats))
        .route("/api/v1/admin/devices", post(api::register_device))
        .route("/api/v1/admin/devices/{id}/revoke", post(api::revoke_device))
        .route("/api/v1/admin/prune", post(api::prune))
        .route("/api/v1/me", get(api::me))
        .route("/api/v1/devices", get(api::devices))
        .route("/api/v1/transfers", post(api::create_transfer))
        .route("/api/v1/outbox", get(api::outbox))
        .route("/api/v1/transfers/{id}", get(api::get_transfer))
        .route("/api/v1/transfers/{id}/chunks/{index}", put(api::put_chunk))
        .route("/api/v1/transfers/{id}/finish", post(api::finish_transfer))
        .route("/api/v1/transfers/{id}/download", get(api::download))
        .route("/api/v1/transfers/{id}/ack", post(api::ack))
        .route("/api/v1/inbox", get(api::inbox))
        .route("/ws", get(websocket::connect))
        // Bounds oversized chunk requests before their bodies reach any handler.
        .layer(RequestBodyLimitLayer::new(CHUNK_BYTES + 64 * 1024))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!("Synchro Internet Mode listening on {}", bind);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_control_filenames() {
        for value in ["../bad.apk", "foo/bar", r"foo\bar", ".", " name ", "a\nb"] {
            assert!(safe_filename(value).is_err(), "{value:?}");
        }
        assert!(safe_filename("release-2026.apk").is_ok());
        assert!(safe_filename("українська-назва.zip").is_ok());
    }

    #[test]
    fn transfer_path_only_uses_canonical_uuids() {
        let id = uuid::Uuid::new_v4().to_string();
        assert_eq!(safe_id(&id).unwrap(), id);
        assert!(safe_id("../../etc/passwd").is_err());
    }

    #[tokio::test]
    async fn prune_respects_delivery_grace_and_mutation_lock() {
        let root = std::env::temp_dir().join(format!("synchro-security-{}", uuid::Uuid::new_v4()));
        let storage = root.join("files");
        tokio::fs::create_dir_all(storage.join("objects")).await.unwrap();
        tokio::fs::create_dir_all(storage.join("chunks")).await.unwrap();
        let db = init_db(&root.join("test.sqlite3")).unwrap();
        let sender = uuid::Uuid::new_v4().to_string();
        let recipient = uuid::Uuid::new_v4().to_string();
        let transfer = uuid::Uuid::new_v4().to_string();
        db.execute("INSERT INTO devices(id,name,token_sha256,created_at)
                    VALUES(?1,'Windows',?2,?3)", params![sender, "a".repeat(64), now()]).unwrap();
        db.execute("INSERT INTO devices(id,name,token_sha256,created_at)
                    VALUES(?1,'Android',?2,?3)", params![recipient, "b".repeat(64), now()]).unwrap();
        db.execute("INSERT INTO transfers(id,sender_id,recipient_id,filename,
                    size_bytes,sha256,status,created_at,expires_at,delivered_at)
                    VALUES(?1,?2,?3,'ok.apk',3,?4,'delivered',?5,?6,?7)",
            params![transfer, sender, recipient, "c".repeat(64), now(),
                    now() + 7200, now()]).unwrap();
        let blob = storage.join("objects").join(format!("{transfer}.blob"));
        tokio::fs::write(&blob, b"apk").await.unwrap();
        let (notifications, _) = broadcast::channel(16);
        let state = State {
            db: Arc::new(Mutex::new(db)),
            config: Arc::new(Config { storage, admin_token: String::new(),
                storage_quota: 5 * 1024 * 1024 * 1024, ttl_hours: 72 }),
            notifications,
            chunk_slots: Arc::new(Semaphore::new(3)),
            mutation_lock: Arc::new(AsyncMutex::new(())),
            ws_slots: Arc::new(Semaphore::new(32)),
            download_slots: Arc::new(Semaphore::new(4)),
        };
        let guard = state.mutation_lock.lock().await;
        let cloned = state.clone();
        let task = tokio::spawn(async move { prune_expired(&cloned).await });
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(!task.is_finished(), "cleanup must wait for an in-flight file mutation");
        drop(guard);
        assert_eq!(task.await.unwrap().unwrap(), 0, "ACK must retain copy during grace");
        assert!(blob.exists());

        {
            let db = state.db.lock().unwrap();
            db.execute("UPDATE transfers SET delivered_at=?1 WHERE id=?2",
                params![now()-4000, transfer]).unwrap();
        }
        assert_eq!(prune_expired(&state).await.unwrap(), 1);
        assert!(!blob.exists());
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tokens_are_random_and_hasher_is_stable() {
        let a = new_token();
        let b = new_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert_eq!(hash_token(&a), hash_token(&a));
    }
}

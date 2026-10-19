//! One-time browser setup: verifies infrastructure and provisions initial devices.
//! The app cannot run root commands; aaPanel/systemd deployment remains manual/admin-only.
use crate::{new_token, hash_token, now, verify_admin, ApiError, State};
use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tokio::{fs, io::AsyncWriteExt};
use url::Url;
use uuid::Uuid;

#[derive(Serialize)]
pub(crate) struct SetupStatus {
    pub installed: bool,
    pub version: &'static str,
    pub database: &'static str,
}

fn is_installed(state: &State) -> Result<bool, ApiError> {
    let db = state.db.lock().map_err(|_| ApiError::internal())?;
    let configured: Option<String> = db.query_row(
        "SELECT value FROM system_settings WHERE key='installed'",
        [], |r| r.get(0),
    ).optional().map_err(|_| ApiError::internal())?;
    Ok(configured.as_deref() == Some("1"))
}

pub(crate) async fn status(
    AxumState(state): AxumState<State>,
) -> Result<Json<SetupStatus>, ApiError> {
    Ok(Json(SetupStatus {
        installed: is_installed(&state)?,
        version: "0.4.1",
        database: "SQLite",
    }))
}

#[derive(Serialize)]
pub(crate) struct CheckReport {
    pub database_ready: bool,
    pub storage_writable: bool,
    pub device_count: i64,
    pub quota_bytes: i64,
    pub free_bytes: u64,
    pub ttl_hours: i64,
    pub server_url: Option<String>,
    pub installed: bool,
}

pub(crate) async fn check(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
) -> Result<Json<CheckReport>, ApiError> {
    verify_admin(&headers, &state)?;
    let installed = is_installed(&state)?;
    let (database_ready, device_count, server_url) = {
        let db = state.db.lock().map_err(|_| ApiError::internal())?;
        let integrity = db.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
            .map_err(|_| ApiError::internal())? == "ok";
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM devices WHERE revoked=0", [], |r| r.get(0),
        ).map_err(|_| ApiError::internal())?;
        let url: Option<String> = db.query_row(
            "SELECT value FROM system_settings WHERE key='public_url'",
            [], |r| r.get(0),
        ).optional().map_err(|_| ApiError::internal())?;
        (integrity, count, url)
    };
    // Probe storage under the same unprivileged account running systemd.
    let probe = state.config.storage.join("objects").join(format!(".setup-{}.tmp", Uuid::new_v4()));
    let writable = match fs::OpenOptions::new().write(true).create_new(true).open(&probe).await {
        Ok(mut file) => {
            let written = file.write_all(b"synchro-setup").await.is_ok()
                && file.sync_data().await.is_ok();
            drop(file);
            let _ = fs::remove_file(&probe).await;
            written
        }
        Err(_) => false,
    };
    Ok(Json(CheckReport {
        database_ready,
        storage_writable: writable,
        device_count,
        quota_bytes: state.config.storage_quota,
        free_bytes: fs2::available_space(&state.config.storage)
            .map_err(|_| ApiError::internal())?,
        ttl_hours: state.config.ttl_hours,
        server_url,
        installed,
    }))
}

#[derive(Deserialize)]
pub(crate) struct FinishRequest {
    pub public_url: String,
    pub windows_name: String,
    pub android_name: String,
}

#[derive(Serialize)]
pub(crate) struct IssuedDevice {
    pub id: String,
    pub name: String,
    pub token: String,
}

#[derive(Serialize)]
pub(crate) struct FinishResponse {
    pub installed: bool,
    pub public_url: String,
    pub devices: Vec<IssuedDevice>,
    pub warning: &'static str,
}

fn valid_name(name: &str) -> Result<String, ApiError> {
    let value = name.trim();
    if value.is_empty() || value.as_bytes().len() > 64 || value.chars().any(char::is_control) {
        return Err(ApiError::bad("Device name must be 1 to 64 bytes, without control characters"));
    }
    Ok(value.to_owned())
}

fn valid_origin(value: &str) -> Result<String, ApiError> {
    if value != value.trim() {
        return Err(ApiError::bad("The HTTPS domain cannot contain surrounding whitespace"));
    }
    let url = Url::parse(value).map_err(|_| ApiError::bad("Enter a valid HTTPS address"))?;
    if url.scheme() != "https" || url.host_str().is_none()
        || !url.username().is_empty() || url.password().is_some()
        || url.path() != "/" || url.query().is_some() || url.fragment().is_some()
    {
        return Err(ApiError::bad("Use an HTTPS origin only, e.g. https://disk.sitezilla.net"));
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

pub(crate) async fn finish(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
    Json(input): Json<FinishRequest>,
) -> Result<(StatusCode, Json<FinishResponse>), ApiError> {
    verify_admin(&headers, &state)?;
    let origin = valid_origin(&input.public_url)?;
    let windows_name = valid_name(&input.windows_name)?;
    let android_name = valid_name(&input.android_name)?;
    if windows_name == android_name {
        return Err(ApiError::bad("Give Windows and Android different device names"));
    }

    // Do not mark setup complete when storage is not writable.
    let probe = state.config.storage.join("objects").join(format!(".setup-{}.tmp", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&probe).await
        .map_err(|_| ApiError::conflict("Storage is not writable. Check systemd permissions"))?;
    let write_result = file.write_all(b"test").await;
    drop(file);
    let _ = fs::remove_file(&probe).await;
    write_result.map_err(|_| ApiError::conflict("Storage write failed"))?;

    let windows = IssuedDevice { id: Uuid::new_v4().to_string(), name: windows_name, token: new_token() };
    let android = IssuedDevice { id: Uuid::new_v4().to_string(), name: android_name, token: new_token() };
    {
        let mut db = state.db.lock().map_err(|_| ApiError::internal())?;
        let tx = db.transaction().map_err(|_| ApiError::internal())?;
        let locked: Option<String> = tx.query_row(
            "SELECT value FROM system_settings WHERE key='installed'",
            [], |r| r.get(0),
        ).optional().map_err(|_| ApiError::internal())?;
        if locked.as_deref() == Some("1") {
            return Err(ApiError::conflict("Setup is already completed and locked"));
        }
        // Existing deployments may have devices already. Never overwrite/revoke them.
        for device in [&windows, &android] {
            tx.execute(
                "INSERT INTO devices(id,name,token_sha256,created_at) VALUES (?1,?2,?3,?4)",
                params![device.id, device.name, hash_token(&device.token), now()],
            ).map_err(|_| ApiError::internal())?;
        }
        tx.execute(
            "INSERT INTO system_settings(key,value) VALUES('public_url',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [&origin],
        ).map_err(|_| ApiError::internal())?;
        tx.execute(
            "INSERT INTO system_settings(key,value) VALUES('installed','1')
             ON CONFLICT(key) DO UPDATE SET value='1'",
            [],
        ).map_err(|_| ApiError::internal())?;
        tx.commit().map_err(|_| ApiError::internal())?;
    }
    Ok((StatusCode::CREATED, Json(FinishResponse {
        installed: true, public_url: origin, devices: vec![windows, android],
        warning: "Tokens appear only in this response. Copy both to a safe password manager now.",
    })))
}

/// Update only the public display URL. DNS, TLS and Nginx still belong to aaPanel;
/// this intentionally cannot change the filesystem or proxy from a browser.
#[derive(Deserialize)]
pub(crate) struct UpdateOriginRequest { pub public_url: String }

pub(crate) async fn update_origin(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
    Json(input): Json<UpdateOriginRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_admin(&headers, &state)?;
    let origin = valid_origin(&input.public_url)?;
    let db = state.db.lock().map_err(|_| ApiError::internal())?;
    let installed: Option<String> = db.query_row(
        "SELECT value FROM system_settings WHERE key='installed'",
        [], |r| r.get(0),
    ).optional().map_err(|_| ApiError::internal())?;
    if installed.as_deref() != Some("1") {
        return Err(ApiError::conflict("Complete setup before changing the public URL"));
    }
    db.execute(
        "INSERT INTO system_settings(key,value) VALUES('public_url',?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [&origin],
    ).map_err(|_| ApiError::internal())?;
    Ok(Json(serde_json::json!({
        "public_url": origin,
        "note": "Update DNS, HTTPS certificate and aaPanel Nginx separately"
    })))
}

#[derive(Serialize)]
pub(crate) struct ServerStats {
    pub devices: i64,
    pub uploading: i64,
    pub ready: i64,
    pub delivered: i64,
    pub reserved_bytes: i64,
    pub quota_bytes: i64,
    pub free_bytes: u64,
    pub ttl_hours: i64,
    pub public_url: Option<String>,
}

pub(crate) async fn stats(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
) -> Result<Json<ServerStats>, ApiError> {
    verify_admin(&headers, &state)?;
    let (devices, uploading, ready, delivered, reserved_bytes, public_url) = {
        let db = state.db.lock().map_err(|_| ApiError::internal())?;
        let devices: i64 = db.query_row(
            "SELECT COUNT(*) FROM devices WHERE revoked=0", [], |r| r.get(0),
        ).map_err(|_| ApiError::internal())?;
        let uploading: i64 = db.query_row(
            "SELECT COUNT(*) FROM transfers WHERE status IN ('uploading','finalizing') AND expires_at>?1",
            [now()], |r| r.get(0),
        ).map_err(|_| ApiError::internal())?;
        let ready: i64 = db.query_row(
            "SELECT COUNT(*) FROM transfers WHERE status='ready' AND expires_at>?1",
            [now()], |r| r.get(0),
        ).map_err(|_| ApiError::internal())?;
        let delivered: i64 = db.query_row(
            "SELECT COUNT(*) FROM transfers WHERE status='delivered'", [],
            |r| r.get(0),
        ).map_err(|_| ApiError::internal())?;
        let reserved_bytes: i64 = db.query_row(
            "SELECT COALESCE(SUM(CASE WHEN status IN ('uploading','finalizing') THEN 2*size_bytes
                ELSE size_bytes END),0) FROM transfers
             WHERE status IN ('uploading','finalizing','ready') AND expires_at>?1",
            [now()], |r| r.get(0),
        ).map_err(|_| ApiError::internal())?;
        let public_url = db.query_row(
            "SELECT value FROM system_settings WHERE key='public_url'", [],
            |r| r.get::<_, String>(0),
        ).optional().map_err(|_| ApiError::internal())?;
        (devices, uploading, ready, delivered, reserved_bytes, public_url)
    };
    Ok(Json(ServerStats {
        devices, uploading, ready, delivered, reserved_bytes,
        quota_bytes: state.config.storage_quota,
        free_bytes: fs2::available_space(&state.config.storage)
            .map_err(|_| ApiError::internal())?,
        ttl_hours: state.config.ttl_hours,
        public_url,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_https_origin_and_name() {
        assert_eq!(valid_origin("https://disk.sitezilla.net").unwrap(), "https://disk.sitezilla.net");
        assert_eq!(valid_origin("https://cloud.example.net/").unwrap(), "https://cloud.example.net");
        assert!(valid_origin("http://disk.sitezilla.net").is_err());
        assert!(valid_origin("https://user:pass@example.com").is_err());
        assert!(valid_origin("https://site.net/install").is_err());
        assert!(valid_origin("https://site.net?q=1").is_err());
        assert!(valid_name("../PC").is_ok()); // displayed labels aren't paths
        assert!(valid_name("").is_err());
    }
}

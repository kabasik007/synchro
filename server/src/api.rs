use crate::{
    hash_token, new_token, now, prune_expired, safe_filename, safe_id, verify_admin,
    verify_device, ApiError, Device, Signal, State, CHUNK_BYTES, MAX_FILE_BYTES,
};
use axum::{
    body::{to_bytes, Body},
    extract::{Path, State as AxumState},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
    Json,
};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::{fs, io::{AsyncReadExt, AsyncWriteExt}};
use tokio_util::io::ReaderStream;
use tracing::error;

#[derive(Deserialize)]
pub(crate) struct DeviceRequest { name: String }

#[derive(Serialize)]
pub(crate) struct DeviceRegistration {
    id: String,
    name: String,
    token: String,
    warning: &'static str,
}

pub(crate) async fn register_device(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
    Json(payload): Json<DeviceRequest>,
) -> Result<(StatusCode, Json<DeviceRegistration>), ApiError> {
    verify_admin(&headers, &state)?;
    let name = payload.name.trim();
    if name.is_empty() || name.as_bytes().len() > 64 ||
        name.chars().any(char::is_control)
    {
        return Err(ApiError::bad("Device name must be 1-64 bytes without control characters"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let token = new_token();
    let hash = hash_token(&token);
    let db = state.db.lock().map_err(|_| ApiError::internal())?;
    db.execute(
        "INSERT INTO devices(id,name,token_sha256,created_at) VALUES(?1,?2,?3,?4)",
        params![id, name, hash, now()],
    ).map_err(|_| ApiError::internal())?;
    Ok((
        StatusCode::CREATED,
        Json(DeviceRegistration {
            id,
            name: name.to_owned(),
            token,
            warning: "The token is shown only once. Store it in a secure password manager.",
        }),
    ))
}

pub(crate) async fn me(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
) -> Result<Json<Device>, ApiError> {
    Ok(Json(verify_device(&headers, &state)?))
}

pub(crate) async fn devices(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
) -> Result<Json<Vec<Device>>, ApiError> {
    verify_device(&headers, &state)?;
    let db = state.db.lock().map_err(|_| ApiError::internal())?;
    let mut statement = db.prepare("SELECT id,name FROM devices ORDER BY created_at,id")
        .map_err(|_| ApiError::internal())?;
    let rows = statement.query_map([], |r| Ok(Device { id: r.get(0)?, name: r.get(1)? }))
        .map_err(|_| ApiError::internal())?;
    let result = rows.collect::<Result<Vec<_>, _>>().map_err(|_| ApiError::internal())?;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub(crate) struct NewTransfer {
    to_device_id: String,
    filename: String,
    size_bytes: i64,
    sha256: String,
}

#[derive(Serialize)]
pub(crate) struct TransferInfo {
    id: String,
    sender_id: String,
    recipient_id: String,
    filename: String,
    size_bytes: i64,
    sha256: String,
    status: String,
    created_at: i64,
    expires_at: i64,
}

#[derive(Serialize)]
pub(crate) struct InitResponse {
    transfer: TransferInfo,
    chunk_size: usize,
    chunk_count: i64,
}

fn find_transfer(state: &State, transfer_id: &str) -> Result<TransferInfo, ApiError> {
    let db = state.db.lock().map_err(|_| ApiError::internal())?;
    db.query_row(
        "SELECT id,sender_id,recipient_id,filename,size_bytes,sha256,status,created_at,expires_at
         FROM transfers WHERE id=?1",
        [transfer_id],
        |r| Ok(TransferInfo {
            id: r.get(0)?, sender_id: r.get(1)?, recipient_id: r.get(2)?,
            filename: r.get(3)?, size_bytes: r.get(4)?, sha256: r.get(5)?,
            status: r.get(6)?, created_at: r.get(7)?, expires_at: r.get(8)?,
        }),
    ).optional().map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::not_found)
}

fn active_transfer_for_sender(state: &State, id: &str, sender: &Device)
    -> Result<TransferInfo, ApiError>
{
    let info = find_transfer(state, id)?;
    if info.sender_id != sender.id || info.expires_at <= now() {
        return Err(ApiError::not_found());
    }
    Ok(info)
}

fn chunk_count(size: i64) -> i64 {
    (size + CHUNK_BYTES as i64 - 1) / CHUNK_BYTES as i64
}

fn expected_chunk_size(size: i64, index: u32) -> Option<usize> {
    let count = chunk_count(size);
    if i64::from(index) >= count { return None; }
    let offset = i64::from(index) * CHUNK_BYTES as i64;
    Some((size - offset).min(CHUNK_BYTES as i64) as usize)
}

pub(crate) async fn create_transfer(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
    Json(payload): Json<NewTransfer>,
) -> Result<(StatusCode, Json<InitResponse>), ApiError> {
    let sender = verify_device(&headers, &state)?;
    let recipient = safe_id(&payload.to_device_id)?;
    safe_filename(&payload.filename)?;
    if payload.size_bytes <= 0 || payload.size_bytes > MAX_FILE_BYTES {
        return Err(ApiError::bad("Size must be between 1 byte and 2 GiB"));
    }
    if payload.sha256.len() != 64 || !payload.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::bad("SHA-256 must be 64 hexadecimal characters"));
    }
    if recipient == sender.id {
        return Err(ApiError::bad("Sender and recipient cannot be identical"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let created = now();
    let expires = created + state.config.ttl_hours * 3600;
    let digest = payload.sha256.to_ascii_lowercase();
    {
        let db = state.db.lock().map_err(|_| ApiError::internal())?;
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM devices WHERE id=?1)", [&recipient],
            |r| r.get(0),
        ).map_err(|_| ApiError::internal())?;
        if !exists { return Err(ApiError::not_found()); }
        let reserved: i64 = db.query_row(
            "SELECT COALESCE(SUM(size_bytes),0) FROM transfers
             WHERE status IN ('uploading','finalizing','ready') AND expires_at > ?1",
            [created], |r| r.get(0),
        ).map_err(|_| ApiError::internal())?;
        if reserved > state.config.storage_quota - payload.size_bytes {
            return Err(ApiError(StatusCode::INSUFFICIENT_STORAGE,
                "Server storage quota exhausted; acknowledge or prune older files".into()));
        }
        let open: i64 = db.query_row(
            "SELECT COUNT(1) FROM transfers WHERE sender_id=?1 AND status IN
             ('uploading','finalizing') AND expires_at>?2",
            params![sender.id, created], |r| r.get(0),
        ).map_err(|_| ApiError::internal())?;
        if open >= 20 { return Err(ApiError::conflict("Too many pending uploads (limit 20)")); }
        db.execute(
            "INSERT INTO transfers
             (id,sender_id,recipient_id,filename,size_bytes,sha256,status,created_at,expires_at)
             VALUES(?1,?2,?3,?4,?5,?6,'uploading',?7,?8)",
            params![id, sender.id, recipient, payload.filename, payload.size_bytes,
                    digest, created, expires],
        ).map_err(|_| ApiError::internal())?;
    }
    Ok((
        StatusCode::CREATED,
        Json(InitResponse {
            chunk_count: chunk_count(payload.size_bytes),
            chunk_size: CHUNK_BYTES,
            transfer: TransferInfo {
                id, sender_id: sender.id, recipient_id: recipient,
                filename: payload.filename, size_bytes: payload.size_bytes,
                sha256: digest, status: "uploading".into(),
                created_at: created, expires_at: expires,
            },
        }),
    ))
}

fn chunk_path(storage: &std::path::Path, id: &str, index: u32) -> PathBuf {
    storage.join("chunks").join(id).join(format!("{index:04}.chunk"))
}

pub(crate) async fn put_chunk(
    AxumState(state): AxumState<State>,
    Path((id, index)): Path<(String, u32)>,
    headers: HeaderMap,
    body: Body,
) -> Result<StatusCode, ApiError> {
    let sender = verify_device(&headers, &state)?;
    let id = safe_id(&id)?;
    let transfer = active_transfer_for_sender(&state, &id, &sender)?;
    if transfer.status != "uploading" {
        return Err(ApiError::conflict("Transfer is not accepting chunks"));
    }
    let expected = expected_chunk_size(transfer.size_bytes, index)
        .ok_or_else(|| ApiError::bad("Chunk index out of bounds"))?;
    let bytes = to_bytes(body, CHUNK_BYTES + 1).await
        .map_err(|_| ApiError(StatusCode::PAYLOAD_TOO_LARGE,
            "Chunk exceeds 8 MiB".into()))?;
    if bytes.len() != expected {
        return Err(ApiError::bad(format!("Chunk must contain exactly {expected} bytes")));
    }
    let folder = state.config.storage.join("chunks").join(&id);
    fs::create_dir_all(&folder).await.map_err(|_| ApiError::internal())?;
    let path = chunk_path(&state.config.storage, &id, index);
    if fs::try_exists(&path).await.map_err(|_| ApiError::internal())? {
        let previous = fs::read(&path).await.map_err(|_| ApiError::internal())?;
        return if previous == bytes {
            Ok(StatusCode::OK) // idempotent retry
        } else {
            Err(ApiError::conflict("Chunk already uploaded with different content"))
        };
    }
    let staging = folder.join(format!("{index:04}.{}.partial", uuid::Uuid::new_v4()));
    let write_result = async {
        let mut output = fs::OpenOptions::new().create_new(true).write(true)
            .open(&staging).await.map_err(|_| ApiError::internal())?;
        output.write_all(&bytes).await.map_err(|_| ApiError::internal())?;
        output.sync_data().await.map_err(|_| ApiError::internal())?;
        // Hard links make the committed chunk immutable on POSIX and Windows: an
        // overlapping retry cannot replace a file already committed by another request.
        match fs::hard_link(&staging, &path).await {
            Ok(()) => Ok(StatusCode::CREATED),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let previous = fs::read(&path).await.map_err(|_| ApiError::internal())?;
                if previous == bytes {
                    Ok(StatusCode::OK)
                } else {
                    Err(ApiError::conflict("Chunk already uploaded with different content"))
                }
            }
            Err(_) => Err(ApiError::internal()),
        }
    }.await;
    let _ = fs::remove_file(&staging).await;
    write_result
}

pub(crate) async fn finish_transfer(
    AxumState(state): AxumState<State>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<TransferInfo>, ApiError> {
    let sender = verify_device(&headers, &state)?;
    let id = safe_id(&id)?;
    let transfer = active_transfer_for_sender(&state, &id, &sender)?;
    if transfer.status != "uploading" {
        return Err(ApiError::conflict("Transfer is not in uploading state"));
    }
    {
        let db = state.db.lock().map_err(|_| ApiError::internal())?;
        let changed = db.execute(
            "UPDATE transfers SET status='finalizing'
             WHERE id=?1 AND sender_id=?2 AND status='uploading'",
            params![id, sender.id],
        ).map_err(|_| ApiError::internal())?;
        if changed != 1 { return Err(ApiError::conflict("Transfer already finalizing")); }
    }
    let result = assemble_transfer(&state, &transfer).await;
    match result {
        Ok(()) => {
            {
                let db = state.db.lock().map_err(|_| ApiError::internal())?;
                db.execute(
                    "UPDATE transfers SET status='ready' WHERE id=?1 AND status='finalizing'",
                    [&id],
                ).map_err(|_| ApiError::internal())?;
            }
            let _ = fs::remove_dir_all(state.config.storage.join("chunks").join(&id)).await;
            let _ = state.notifications.send(Signal {
                event_type: "transfer_ready".into(),
                device_id: transfer.recipient_id.clone(),
                transfer_id: id,
            });
            Ok(Json(TransferInfo { status: "ready".into(), ..transfer }))
        }
        Err(err) => {
            let _ = fs::remove_file(state.config.storage.join("objects").join(format!("{id}.tmp"))).await;
            let _ = fs::remove_file(state.config.storage.join("objects").join(format!("{id}.blob"))).await;
            let db = state.db.lock().map_err(|_| ApiError::internal())?;
            let _ = db.execute(
                "UPDATE transfers SET status='uploading' WHERE id=?1 AND status='finalizing'",
                [&id],
            );
            Err(err)
        }
    }
}

async fn assemble_transfer(state: &State, transfer: &TransferInfo) -> Result<(), ApiError> {
    let path = state.config.storage.join("objects").join(format!("{}.tmp", transfer.id));
    let ready = state.config.storage.join("objects").join(format!("{}.blob", transfer.id));
    let mut output = fs::File::create(&path).await.map_err(|_| ApiError::internal())?;
    let mut sha = Sha256::new();
    let mut total = 0_i64;
    let mut buffer = vec![0u8; 64 * 1024];
    for index in 0..chunk_count(transfer.size_bytes) {
        let source = chunk_path(&state.config.storage, &transfer.id, index as u32);
        let mut input = fs::File::open(&source).await
            .map_err(|_| ApiError::conflict(format!("Missing chunk {index}")))?;
        let expected = expected_chunk_size(transfer.size_bytes, index as u32).unwrap_or(0) as u64;
        if input.metadata().await.map_err(|_| ApiError::internal())?.len() != expected {
            return Err(ApiError::conflict(format!("Chunk {index} has an invalid size")));
        }
        loop {
            let n = input.read(&mut buffer).await.map_err(|_| ApiError::internal())?;
            if n == 0 { break; }
            output.write_all(&buffer[..n]).await.map_err(|_| ApiError::internal())?;
            sha.update(&buffer[..n]);
            total += n as i64;
        }
    }
    if total != transfer.size_bytes || hex::encode(sha.finalize()) != transfer.sha256 {
        return Err(ApiError::conflict("Transfer checksum mismatch; retry with verified chunks"));
    }
    output.sync_all().await.map_err(|_| ApiError::internal())?;
    drop(output);
    fs::rename(path, ready).await.map_err(|_| ApiError::internal())?;
    Ok(())
}

pub(crate) async fn inbox(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
) -> Result<Json<Vec<TransferInfo>>, ApiError> {
    let device = verify_device(&headers, &state)?;
    let db = state.db.lock().map_err(|_| ApiError::internal())?;
    let mut statement = db.prepare(
        "SELECT id,sender_id,recipient_id,filename,size_bytes,sha256,status,created_at,expires_at
         FROM transfers WHERE recipient_id=?1 AND status='ready' AND expires_at>?2
         ORDER BY created_at DESC LIMIT 100",
    ).map_err(|_| ApiError::internal())?;
    let rows = statement.query_map(params![device.id, now()], |r| Ok(TransferInfo {
        id: r.get(0)?, sender_id: r.get(1)?, recipient_id: r.get(2)?,
        filename: r.get(3)?, size_bytes: r.get(4)?, sha256: r.get(5)?,
        status: r.get(6)?, created_at: r.get(7)?, expires_at: r.get(8)?,
    })).map_err(|_| ApiError::internal())?;
    let found = rows.collect::<Result<Vec<_>, _>>().map_err(|_| ApiError::internal())?;
    Ok(Json(found))
}

pub(crate) async fn download(
    AxumState(state): AxumState<State>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let device = verify_device(&headers, &state)?;
    let id = safe_id(&id)?;
    let transfer = find_transfer(&state, &id)?;
    if transfer.recipient_id != device.id || transfer.status != "ready"
        || transfer.expires_at <= now()
    {
        return Err(ApiError::not_found());
    }
    let file = fs::File::open(state.config.storage.join("objects").join(format!("{id}.blob")))
        .await.map_err(|_| ApiError::not_found())?;
    let mut response = Response::new(Body::from_stream(ReaderStream::new(file)));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_str(&transfer.size_bytes.to_string())
        .map_err(|_| ApiError::internal())?);
    let disposition = format!("attachment; filename*=UTF-8''{}",
        utf8_percent_encode(&transfer.filename, NON_ALPHANUMERIC));
    headers.insert(header::CONTENT_DISPOSITION, HeaderValue::from_str(&disposition)
        .map_err(|_| ApiError::internal())?);
    headers.insert("x-synchro-sha256", HeaderValue::from_str(&transfer.sha256)
        .map_err(|_| ApiError::internal())?);
    Ok(response)
}

#[derive(Deserialize)]
pub(crate) struct AckRequest { sha256: String }

pub(crate) async fn ack(
    AxumState(state): AxumState<State>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<AckRequest>,
) -> Result<StatusCode, ApiError> {
    let device = verify_device(&headers, &state)?;
    let id = safe_id(&id)?;
    let transfer = find_transfer(&state, &id)?;
    if transfer.recipient_id != device.id || transfer.expires_at <= now() ||
        transfer.status != "ready"
    {
        return Err(ApiError::not_found());
    }
    if !transfer.sha256.eq_ignore_ascii_case(payload.sha256.trim()) {
        return Err(ApiError::bad("Checksum mismatch: do not acknowledge an unverified download"));
    }
    {
        let db = state.db.lock().map_err(|_| ApiError::internal())?;
        let changed = db.execute(
            "UPDATE transfers SET status='delivered',delivered_at=?1
             WHERE id=?2 AND recipient_id=?3 AND status='ready'",
            params![now(), id, device.id],
        ).map_err(|_| ApiError::internal())?;
        if changed != 1 { return Err(ApiError::conflict("Already acknowledged")); }
    }
    // Retained in metadata until the hourly pruner, but no longer downloadable.
    if let Err(err) = fs::remove_file(state.config.storage.join("objects").join(format!("{id}.blob"))).await {
        error!("delivered object cleanup will be retried: {err}");
    }
    let _ = state.notifications.send(Signal {
        event_type: "transfer_acknowledged".into(),
        device_id: transfer.sender_id,
        transfer_id: id,
    });
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn prune(
    AxumState(state): AxumState<State>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_admin(&headers, &state)?;
    let removed = prune_expired(&state).await?;
    Ok(Json(json!({"removed":removed})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_boundaries() {
        assert_eq!(chunk_count(1), 1);
        assert_eq!(expected_chunk_size(1, 0), Some(1));
        assert_eq!(expected_chunk_size(1, 1), None);
        assert_eq!(chunk_count(CHUNK_BYTES as i64 + 1), 2);
        assert_eq!(expected_chunk_size(CHUNK_BYTES as i64 + 1, 1), Some(1));
        assert_eq!(expected_chunk_size(MAX_FILE_BYTES, 0), Some(CHUNK_BYTES));
    }
}

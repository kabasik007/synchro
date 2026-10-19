//! Optional live notifications. The initial WebSocket message authenticates the
//! device so tokens never travel in URLs or appear in reverse proxy access logs.
use crate::{verify_device, State};
use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State as AxumState},
    http::{header, HeaderMap, HeaderValue},
    response::Response,
};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

#[derive(Deserialize)]
struct Handshake {
    #[serde(rename = "type")]
    message_type: String,
    token: String,
}

pub(crate) async fn connect(
    ws: WebSocketUpgrade,
    AxumState(state): AxumState<State>,
) -> Response {
    ws.max_message_size(2048)
        .max_frame_size(2048)
        .on_upgrade(move |socket| serve(socket, state))
}

async fn serve(mut socket: WebSocket, state: State) {
    let first = match tokio::time::timeout(Duration::from_secs(8), socket.recv()).await {
        Ok(Some(Ok(Message::Text(body)))) => body,
        _ => return,
    };
    let Ok(handshake) = serde_json::from_str::<Handshake>(first.as_str()) else { return };
    if handshake.message_type != "auth" { return; }

    let mut headers = HeaderMap::new();
    let Ok(header_value) = HeaderValue::from_str(&format!("Bearer {}", handshake.token)) else {
        return;
    };
    headers.insert(header::AUTHORIZATION, header_value);
    let Ok(device) = verify_device(&headers, &state) else {
        let _ = socket.send(Message::Text(
            json!({"type":"error","message":"authentication failed"}).to_string().into()
        )).await;
        return;
    };
    let mut events = state.notifications.subscribe();
    if socket.send(Message::Text(
        json!({"type":"ready","device_id":device.id}).to_string().into()
    )).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            next = events.recv() => {
                match next {
                    Ok(signal) if signal.device_id == device.id => {
                        let payload = serde_json::to_string(&signal).unwrap_or_default();
                        if socket.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                        if signal.event_type == "device_revoked" { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
            next = socket.recv() => {
                match next {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() { break; }
                    }
                    _ => {}
                }
            }
        }
    }
}

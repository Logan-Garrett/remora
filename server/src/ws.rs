use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use uuid::Uuid;

use crate::commands;
use crate::db::{Database, DatabaseBackend};
use crate::state::AppState;
use remora_common::{ClientMsg, ServerMsg};

pub async fn handle_socket(
    state: Arc<AppState>,
    session_id: Uuid,
    name: String,
    socket: WebSocket,
) {
    let (mut sink, mut stream) = socket.split();

    // Verify session exists and is active
    let session_status = state
        .db
        .get_session_status(session_id)
        .await
        .unwrap_or(None);
    let error_msg = match session_status.as_deref() {
        Some("active") => None,
        Some("expired") => Some(
            "This session was cleaned up due to inactivity. Please create a new session.".into(),
        ),
        Some(s) => Some(format!("Session is not available (status: {s})")),
        None => Some("Session not found.".into()),
    };
    if let Some(message) = error_msg {
        let msg = ServerMsg::Error { message };
        let _ = sink
            .send(Message::Text(serde_json::to_string(&msg).unwrap()))
            .await;
        return;
    }

    // Enforce unique display names per session
    if state.is_participant_connected(session_id, &name).await {
        let msg = ServerMsg::Error {
            message: format!(
                "Display name '{}' is already in use in this session. \
                 Please reconnect with a different name.",
                name
            ),
        };
        let _ = sink
            .send(Message::Text(serde_json::to_string(&msg).unwrap()))
            .await;
        return;
    }

    // Track participant
    state.participant_join(session_id, &name).await;

    // If no owner yet for this session, the first participant becomes the owner
    state.set_session_owner(session_id, &name).await;

    // Subscribe BEFORE inserting join event so we don't miss it in the live stream
    let (mut rx, cancel_token) = state.subscribe(session_id, &name).await;

    // Backfill: send the most recent events for this session (bounded by config limit)
    let backfill = state
        .db
        .get_recent_events_for_session(session_id, state.config.backfill_limit)
        .await
        .unwrap_or_default();

    let mut last_backfill_id: i64 = 0;
    for event in backfill {
        last_backfill_id = event.id;
        let msg = ServerMsg::Event { data: event };
        if sink
            .send(Message::Text(serde_json::to_string(&msg).unwrap()))
            .await
            .is_err()
        {
            state.participant_leave(session_id, &name).await;
            return;
        }
    }

    // Emit join event after backfill so it appears at the end
    let _ = insert_event(
        &state.db,
        session_id,
        "system",
        "system",
        serde_json::json!({"text": format!("{name} joined")}),
    )
    .await;

    // Forward live events to the WS client, skipping anything already backfilled.
    // Sends a WebSocket ping every 30s to keep the connection alive through
    // proxies/tunnels (e.g. Cloudflare) that drop idle connections.
    let ws_name = name.clone();
    let cancel = cancel_token.clone();
    let mut send_task = tokio::spawn(async move {
        let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(30));
        ping_interval.tick().await; // skip immediate first tick
        loop {
            tokio::select! {
                maybe_event = rx.recv() => {
                    let Some(event) = maybe_event else { break; };
                    if event.id <= last_backfill_id {
                        continue;
                    }
                    // Check if this is a kick event targeting us
                    if event.kind == "kick" {
                        if let Some(target) = event.payload.get("target").and_then(|v| v.as_str()) {
                            if target == ws_name {
                                // Send the kick event then close
                                let msg = ServerMsg::Event { data: event };
                                let text = serde_json::to_string(&msg).unwrap();
                                let _ = sink.send(Message::Text(text)).await;
                                break;
                            }
                        }
                    }
                    let msg = ServerMsg::Event { data: event };
                    let text = serde_json::to_string(&msg).unwrap();
                    if sink.send(Message::Text(text)).await.is_err() {
                        break;
                    }
                }
                _ = ping_interval.tick() => {
                    if sink.send(Message::Ping(vec![])).await.is_err() {
                        break;
                    }
                }
                _ = cancel.cancelled() => {
                    break;
                }
            }
        }
    });

    // Read messages from WS client and dispatch.
    // The authenticated `name` from the WebSocket connection is passed as the verified
    // author, overriding any client-supplied author field to prevent impersonation.
    let recv_state = state.clone();
    let recv_name = name.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(client_msg) = serde_json::from_str::<ClientMsg>(&text) {
                        commands::dispatch(recv_state.clone(), session_id, client_msg, &recv_name)
                            .await;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to finish, then abort the other
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    // Remove participant and clean up subscription
    state.participant_leave(session_id, &name).await;
    state.unsubscribe_closed(session_id).await;

    // Emit leave event
    let _ = insert_event(
        &state.db,
        session_id,
        "system",
        "system",
        serde_json::json!({"text": format!("{name} left")}),
    )
    .await;

    // Update idle_since if no more participants
    let remaining = state.get_participants(session_id).await;
    if remaining.is_empty() {
        let _ = state.db.set_idle_since_now(session_id).await;
    }

    tracing::info!("{name} disconnected from session {session_id}");
}

pub async fn insert_event(
    db: &Arc<DatabaseBackend>,
    session_id: Uuid,
    author: &str,
    kind: &str,
    payload: serde_json::Value,
) -> Result<i64, anyhow::Error> {
    db.insert_event(session_id, author, kind, payload).await
}

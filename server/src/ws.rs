use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use uuid::Uuid;

use remora_common::{ClientMsg, Event, ServerMsg};
use crate::commands;
use crate::state::AppState;

pub async fn handle_socket(
    state: Arc<AppState>,
    session_id: Uuid,
    name: String,
    socket: WebSocket,
) {
    let (mut sink, mut stream) = socket.split();

    // Verify session exists
    let exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM sessions WHERE id = $1)")
        .bind(session_id)
        .fetch_one(&state.db)
        .await
        .unwrap_or(false);

    if !exists {
        let msg = ServerMsg::Error { message: "session not found".into() };
        let _ = sink.send(Message::Text(serde_json::to_string(&msg).unwrap().into())).await;
        return;
    }

    // Track participant
    state.participant_join(session_id, &name).await;

    // Subscribe BEFORE inserting join event so we don't miss it in the live stream
    let (mut rx, cancel_token) = state.subscribe(session_id).await;

    // Backfill: send all existing events for this session
    let backfill = sqlx::query_as::<_, (i64, Uuid, chrono::DateTime<chrono::Utc>, Option<String>, String, serde_json::Value)>(
        "SELECT id, session_id, timestamp, author, kind, payload FROM events WHERE session_id = $1 ORDER BY id",
    )
    .bind(session_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut last_backfill_id: i64 = 0;
    for (id, sid, timestamp, author, kind, payload) in backfill {
        last_backfill_id = id;
        let event = Event { id, session_id: sid, timestamp, author, kind, payload };
        let msg = ServerMsg::Event { data: event };
        if sink.send(Message::Text(serde_json::to_string(&msg).unwrap().into())).await.is_err() {
            state.participant_leave(session_id, &name).await;
            return;
        }
    }

    // Emit join event after backfill so it appears at the end
    let _ = insert_event(&state.db, session_id, "system", "system",
        serde_json::json!({"text": format!("{name} joined")})).await;

    // Forward live events to the WS client, skipping anything already backfilled
    let ws_name = name.clone();
    let cancel = cancel_token.clone();
    let mut send_task = tokio::spawn(async move {
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
                                let _ = sink.send(Message::Text(text.into())).await;
                                break;
                            }
                        }
                    }
                    let msg = ServerMsg::Event { data: event };
                    let text = serde_json::to_string(&msg).unwrap();
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                _ = cancel.cancelled() => {
                    break;
                }
            }
        }
    });

    // Read messages from WS client and dispatch
    let recv_state = state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(client_msg) = serde_json::from_str::<ClientMsg>(&text) {
                        commands::dispatch(recv_state.clone(), session_id, client_msg).await;
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
    let _ = insert_event(&state.db, session_id, "system", "system",
        serde_json::json!({"text": format!("{name} left")})).await;

    // Update idle_since if no more participants
    let remaining = state.get_participants(session_id).await;
    if remaining.is_empty() {
        let _ = sqlx::query("UPDATE sessions SET idle_since = now() WHERE id = $1")
            .bind(session_id)
            .execute(&state.db)
            .await;
    }

    tracing::info!("{name} disconnected from session {session_id}");
}

pub async fn insert_event(
    db: &sqlx::PgPool,
    session_id: Uuid,
    author: &str,
    kind: &str,
    payload: serde_json::Value,
) -> Result<i64, sqlx::Error> {
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO events (session_id, author, kind, payload) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(session_id)
    .bind(author)
    .bind(kind)
    .bind(payload)
    .fetch_one(db)
    .await?;
    Ok(id)
}

use crate::db::{Database, DatabaseBackend};
use std::sync::Arc;
use uuid::Uuid;

/// Context assembly mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMode {
    /// Events since the last claude_response (or clear_marker).
    SinceLast,
    /// All events in the session.
    Full,
}

impl ContextMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SinceLast => "since_last",
            Self::Full => "full",
        }
    }
}

/// Assemble a prompt string from the session's event history.
pub async fn assemble_context(
    db: &Arc<DatabaseBackend>,
    session_id: Uuid,
    mode: ContextMode,
) -> anyhow::Result<String> {
    let min_id = match mode {
        ContextMode::Full => 0i64,
        ContextMode::SinceLast => db.get_last_context_boundary(session_id).await?,
    };

    let rows = db.get_events_since(session_id, min_id).await?;

    let mut parts = Vec::new();

    for (_id, author, kind, payload) in rows {
        let formatted = format_event(&author.unwrap_or_default(), &kind, &payload);
        if !formatted.is_empty() {
            parts.push(formatted);
        }
    }

    Ok(parts.join("\n\n"))
}

fn format_event(author: &str, kind: &str, payload: &serde_json::Value) -> String {
    match kind {
        "chat" => {
            let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("");
            format!(
                "<untrusted_content source=\"chat\" author=\"{author}\">\n{text}\n</untrusted_content>"
            )
        }
        "file" => {
            let path = payload
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let content = payload
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!(
                "<untrusted_content source=\"file\" path=\"{path}\">\n{content}\n</untrusted_content>"
            )
        }
        "fetch" => {
            let url = payload
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let content = payload
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!(
                "<untrusted_content source=\"url\" url=\"{url}\">\n{content}\n</untrusted_content>"
            )
        }
        "diff" => {
            let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("");
            text.to_string()
        }
        "claude_response" => {
            let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("");
            format!("[Claude]: {text}")
        }
        "tool_call" => {
            let tool = payload
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let args = payload
                .get("args")
                .map(|v| v.to_string())
                .unwrap_or_default();
            format!("[Claude tool_call]: {tool}({args})")
        }
        "tool_result" => {
            let output = payload.get("output").and_then(|v| v.as_str()).unwrap_or("");
            format!("<untrusted_content source=\"tool_result\">\n{output}\n</untrusted_content>")
        }
        "system" => {
            let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("");
            format!("[system]: {text}")
        }
        _ => String::new(),
    }
}

//! remora-cli: interactive REPL client for Remora sessions.
//!
//! Usage:
//!   remora-cli connect <url> <session-id> <token> [--name <name>]
//!   remora-cli sessions <url> <token>
//!   remora-cli create <url> <token> --description <desc> [--repos <urls>...]

use std::io::IsTerminal;

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use colored::Colorize;
use futures_util::{SinkExt, StreamExt};
use remora_common::{Event, ServerMsg, SessionInfo};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_tungstenite::tungstenite::Message;

// ── CLI argument definitions ────────────────────────────────────────

#[derive(Parser)]
#[command(name = "remora-cli", version, about = "CLI client for Remora sessions")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Connect to a session via WebSocket
    Connect {
        /// Server URL (http:// or https://)
        url: String,
        /// Session UUID
        session_id: String,
        /// Auth token (reads from REMORA_TOKEN env var if not provided)
        #[arg(short, long, env = "REMORA_TOKEN", hide_env_values = true)]
        token: String,
        /// Display name
        #[arg(short, long, default_value = "cli-user")]
        name: String,
    },
    /// List available sessions
    Sessions {
        /// Server URL (http:// or https://)
        url: String,
        /// Auth token (reads from REMORA_TOKEN env var if not provided)
        #[arg(short, long, env = "REMORA_TOKEN", hide_env_values = true)]
        token: String,
    },
    /// Create a new session
    Create {
        /// Server URL (http:// or https://)
        url: String,
        /// Auth token (reads from REMORA_TOKEN env var if not provided)
        #[arg(short, long, env = "REMORA_TOKEN", hide_env_values = true)]
        token: String,
        /// Session description
        #[arg(short, long)]
        description: String,
        /// Repository URLs to attach
        #[arg(short, long)]
        repos: Vec<String>,
    },
}

// ── Slash-command parser ────────────────────────────────────────────

/// Parse user input into a JSON ClientMsg value.
///
/// Handles /slash commands and plain chat text. This function is
/// extracted so it can be unit-tested independently.
pub fn parse_input(input: &str, author: &str) -> serde_json::Value {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return serde_json::json!({"type": "chat", "author": author, "text": ""});
    }

    if !trimmed.starts_with('/') {
        return serde_json::json!({"type": "chat", "author": author, "text": trimmed});
    }

    let parts: Vec<&str> = trimmed[1..].splitn(2, char::is_whitespace).collect();
    let cmd = parts[0].to_lowercase();
    let rest = parts.get(1).map(|s| s.trim()).unwrap_or("");

    match cmd.as_str() {
        "run" => serde_json::json!({"type": "run", "author": author}),
        "run-all" | "runall" => serde_json::json!({"type": "run_all", "author": author}),
        "who" => serde_json::json!({"type": "who", "author": author}),
        "help" | "?" => serde_json::json!({"type": "help", "author": author}),
        "clear" => serde_json::json!({"type": "clear", "author": author}),
        "diff" => serde_json::json!({"type": "diff", "author": author}),
        "info" | "session" => serde_json::json!({"type": "session_info", "author": author}),
        "add" => {
            if rest.is_empty() {
                serde_json::json!({"type": "chat", "author": author, "text": trimmed})
            } else {
                serde_json::json!({"type": "add", "author": author, "path": rest})
            }
        }
        "fetch" => {
            if rest.is_empty() {
                serde_json::json!({"type": "chat", "author": author, "text": trimmed})
            } else {
                serde_json::json!({"type": "fetch", "author": author, "url": rest})
            }
        }
        "trust" => {
            if rest.is_empty() {
                serde_json::json!({"type": "chat", "author": author, "text": trimmed})
            } else {
                serde_json::json!({"type": "trust", "author": author, "target": rest})
            }
        }
        "untrust" => {
            if rest.is_empty() {
                serde_json::json!({"type": "chat", "author": author, "text": trimmed})
            } else {
                serde_json::json!({"type": "untrust", "author": author, "target": rest})
            }
        }
        "kick" => {
            if rest.is_empty() {
                serde_json::json!({"type": "chat", "author": author, "text": trimmed})
            } else {
                serde_json::json!({"type": "kick", "author": author, "target": rest})
            }
        }
        _ => serde_json::json!({"type": "chat", "author": author, "text": trimmed}),
    }
}

// ── WebSocket URL builder ───────────────────────────────────────────

fn build_ws_url(base_url: &str, session_id: &str, token: &str, name: &str) -> String {
    let ws_base = if base_url.starts_with("https://") {
        base_url.replacen("https://", "wss://", 1)
    } else if base_url.starts_with("http://") {
        base_url.replacen("http://", "ws://", 1)
    } else {
        base_url.to_string()
    };
    let ws_base = ws_base.trim_end_matches('/');
    let base = format!("{}/sessions/{}", ws_base, session_id);
    // Use url crate to safely encode query parameters (prevents injection via
    // tokens or names containing &, =, #, etc.)
    let mut parsed = url::Url::parse(&base).expect("invalid server URL");
    parsed
        .query_pairs_mut()
        .append_pair("token", token)
        .append_pair("name", name);
    parsed.to_string()
}

// ── Event formatting ────────────────────────────────────────────────

fn format_timestamp(ts: &DateTime<Utc>) -> String {
    ts.format("%H:%M").to_string()
}

fn format_event_interactive(event: &Event) {
    let ts = format_timestamp(&event.timestamp);
    let author = event.author.as_deref().unwrap_or("system");

    match event.kind.as_str() {
        "chat" => {
            let text = event
                .payload
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            println!("[{}] {}: {}", ts, author.blue(), text);
        }
        "system" => {
            let text = event
                .payload
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            println!("{}", format!("[{}] \u{2699} {}", ts, text).yellow());
        }
        "claude_response" => {
            let text = event
                .payload
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            println!("{}", format!("[{}] Claude: {}", ts, text).green());
        }
        "tool_call" => {
            let tool = event
                .payload
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let args = event
                .payload
                .get("args")
                .map(|v| {
                    if v.is_string() {
                        v.as_str().unwrap().to_string()
                    } else {
                        serde_json::to_string(v).unwrap_or_default()
                    }
                })
                .unwrap_or_default();
            println!(
                "{}",
                format!("[{}] \u{1f527} {}({})", ts, tool, args).magenta()
            );
        }
        "tool_result" => {
            let output = event
                .payload
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let lines: Vec<&str> = output.lines().collect();
            let truncated = if lines.len() > 5 {
                let mut t: Vec<&str> = lines[..5].to_vec();
                t.push("... (truncated)");
                t.join("\n")
            } else {
                output.to_string()
            };
            println!("{}", format!("[{}] \u{2192} {}", ts, truncated).cyan());
        }
        _ => {
            let payload_str = serde_json::to_string(&event.payload).unwrap_or_default();
            println!("[{}] [{}] {}", ts, event.kind, payload_str);
        }
    }
}

// ── Connect subcommand ──────────────────────────────────────────────

async fn cmd_connect(url: String, session_id: String, token: String, name: String) {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let ws_url = build_ws_url(&url, &session_id, &token, &name);
    let interactive = std::io::stdin().is_terminal();

    if interactive {
        eprintln!(
            "{}",
            format!(
                "Connecting to session {}...",
                &session_id[..8.min(session_id.len())]
            )
            .cyan()
        );
    }

    let (ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .unwrap_or_else(|_| {
            // Don't print the error directly — it may contain the full URL with token
            eprintln!(
                "{}",
                format!(
                    "Failed to connect to {} (session {})",
                    url,
                    &session_id[..8.min(session_id.len())]
                )
                .red()
            );
            std::process::exit(1);
        });

    let (mut sink, mut stream) = ws.split();

    if interactive {
        eprintln!("{}", "Connected. Type /help for commands.".green());
    }

    let name_clone = name.clone();

    // Receive task
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(text) => {
                    if interactive {
                        match serde_json::from_str::<ServerMsg>(&text) {
                            Ok(ServerMsg::Event { data }) => {
                                format_event_interactive(&data);
                            }
                            Ok(ServerMsg::StreamStart { .. }) => {
                                eprint!("{}", "Claude is generating...".cyan());
                            }
                            Ok(ServerMsg::StreamDelta { delta, .. }) => {
                                print!("{}", delta.green());
                                // Flush to show partial output immediately
                                use std::io::Write;
                                std::io::stdout().flush().ok();
                            }
                            Ok(ServerMsg::StreamEnd { .. }) => {
                                println!();
                            }
                            Ok(ServerMsg::Error { message }) => {
                                eprintln!("{}", format!("Error: {}", message).red());
                            }
                            Err(_) => {
                                // Unknown message — log to stderr, not stdout
                                eprintln!("{}", "Received unknown message format".yellow());
                            }
                        }
                    } else {
                        // Non-interactive: output raw JSON lines
                        println!("{}", text);
                    }
                }
                Message::Close(_) => {
                    if interactive {
                        eprintln!("{}", "Connection closed by server.".yellow());
                    }
                    break;
                }
                _ => {}
            }
        }
    });

    // Send task
    let send_task = tokio::spawn(async move {
        let reader = BufReader::new(tokio::io::stdin());
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let msg = if interactive {
                let parsed = parse_input(&line, &name_clone);
                serde_json::to_string(&parsed).unwrap()
            } else {
                // Non-interactive: validate JSON before sending
                if serde_json::from_str::<serde_json::Value>(&line).is_err() {
                    eprintln!("Skipping invalid JSON: {}", &line[..80.min(line.len())]);
                    continue;
                }
                line
            };
            if sink.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Graceful shutdown on Ctrl-C
    let shutdown = async {
        tokio::signal::ctrl_c().await.ok();
        if interactive {
            eprintln!("\n{}", "Disconnecting...".yellow());
        }
    };

    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
        _ = shutdown => {}
    }
}

// ── Sessions subcommand ─────────────────────────────────────────────

async fn cmd_sessions(url: String, token: String) {
    let client = reqwest::Client::new();
    let base = url.trim_end_matches('/');

    let resp = client
        .get(format!("{}/sessions", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap_or_else(|e| {
            eprintln!("{}", format!("Request failed: {}", e).red());
            std::process::exit(1);
        });

    if !resp.status().is_success() {
        eprintln!("{}", format!("Server returned {}", resp.status()).red());
        std::process::exit(1);
    }

    let sessions: Vec<SessionInfo> = resp.json().await.unwrap_or_else(|e| {
        eprintln!("{}", format!("Failed to parse response: {}", e).red());
        std::process::exit(1);
    });

    if sessions.is_empty() {
        println!("No sessions found.");
        return;
    }

    // Print header
    println!(
        "{:<10} {:<40} {}",
        "ID".bold(),
        "Description".bold(),
        "Created".bold()
    );
    println!("{}", "-".repeat(70));

    for session in &sessions {
        let id_str = session.id.to_string();
        let id_short = &id_str[..8.min(id_str.len())];
        let created = session.created_at.format("%Y-%m-%d %H:%M").to_string();
        println!("{:<10} {:<40} {}", id_short, session.description, created);
    }
}

// ── Create subcommand ───────────────────────────────────────────────

async fn cmd_create(url: String, token: String, description: String, repos: Vec<String>) {
    let client = reqwest::Client::new();
    let base = url.trim_end_matches('/');

    let mut body = serde_json::json!({
        "description": description,
    });

    if !repos.is_empty() {
        body["repos"] = serde_json::json!(repos);
    }

    let resp = client
        .post(format!("{}/sessions", base))
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|e| {
            eprintln!("{}", format!("Request failed: {}", e).red());
            std::process::exit(1);
        });

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        eprintln!(
            "{}",
            format!("Server returned {}: {}", status, body_text).red()
        );
        std::process::exit(1);
    }

    let session: SessionInfo = resp.json().await.unwrap_or_else(|e| {
        eprintln!("{}", format!("Failed to parse response: {}", e).red());
        std::process::exit(1);
    });

    println!("{}", "Session created!".green());
    println!("  ID: {}", session.id);
    println!("  Description: {}", session.description);
    if let Some(key) = &session.owner_key {
        // Print to stderr so it doesn't get captured in piped output
        eprintln!("  Owner key: {}", key);
        eprintln!("  (save this key securely — it grants session ownership)");
    }
}

// ── Main ────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Connect {
            url,
            session_id,
            token,
            name,
        } => cmd_connect(url, session_id, token, name).await,
        Command::Sessions { url, token } => cmd_sessions(url, token).await,
        Command::Create {
            url,
            token,
            description,
            repos,
        } => cmd_create(url, token, description, repos).await,
    }
}

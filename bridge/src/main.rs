//! remora-bridge: stdio <-> WebSocket bridge for the Neovim plugin.
//!
//! Usage: remora-bridge <ws-url>
//!   e.g. remora-bridge "ws://localhost:7200/sessions/<uuid>?token=secret&name=alice"
//!
//! Reads line-delimited JSON from stdin, sends as WS text frames.
//! Receives WS text frames, writes as line-delimited JSON to stdout.

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    // Install the default rustls crypto provider for TLS (wss://) connections
    let _ = rustls::crypto::ring::default_provider().install_default();

    let url = std::env::args()
        .nth(1)
        .expect("usage: remora-bridge <ws-url>");

    let (ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("failed to connect");

    let (mut sink, mut stream) = ws.split();

    // stdin -> WS
    let send_task = tokio::spawn(async move {
        let reader = BufReader::new(tokio::io::stdin());
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if sink.send(Message::Text(line)).await.is_err() {
                break;
            }
        }
    });

    // WS -> stdout
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(text) => {
                    // Write as a single line to stdout
                    println!("{}", text);
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let shutdown = async {
        #[cfg(unix)]
        {
            let mut sig = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to register SIGTERM handler");
            sig.recv().await;
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await.ok();
        }
    };

    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
        _ = shutdown => {}
    }
}

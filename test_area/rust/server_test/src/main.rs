use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Extension},
    response::Html,
    routing::get,
    Router,
};
use axum::extract::ws::Utf8Bytes;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, time::Duration};
use tokio::sync::broadcast;
use tokio::time::sleep;

/// A message containing text, styling, and optional media to display
#[derive(Serialize, Deserialize, Debug, Clone)]
struct LineMessage {
    text: String,
    speaker: Option<String>,
    style: Style,
    media: Option<Media>,
}

/// Styling information for displaying a line
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Style {
    color: String,
    font_size: String,
    font_family: String,
    position: Position,
}

/// Position coordinates for displaying a line
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Position {
    x: String,
    y: String,
}

/// Media (image or video) to display alongside a line
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Media {
    kind: String, // "image" or "video"
    url: String,
    duration_ms: Option<u64>,
}

/// Serve the HTML client
async fn html_handler() -> Html<&'static str> {
    Html(include_str!("client.html"))
}

/// Handle WebSocket upgrade and broadcast messages to clients
async fn ws_handler(
    ws: WebSocketUpgrade,
    Extension(tx): Extension<broadcast::Sender<LineMessage>>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(|mut socket: WebSocket| async move {
        // Subscribe to broadcast channel
        let mut rx = tx.subscribe();

        // Keep sending messages received from broadcast channel to this websocket.
        loop {
            match rx.recv().await {
                Ok(line_msg) => {
                    // Convert to JSON
                    match serde_json::to_string(&line_msg) {
                        Ok(text) => {
                            if socket.send(Message::Text(Utf8Bytes::from(text))).await.is_err() {
                                // client disconnected
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("serialize error: {}", e);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("lagged by {} messages", n);
                }
            }
        }
    })
}

/// Load lines from a JSON file
fn load_lines_from_file(path: &str) -> Vec<LineMessage> {
    let data = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read lines file at '{}': {}", path, e));
    serde_json::from_str::<Vec<LineMessage>>(&data)
        .unwrap_or_else(|e| panic!("Failed to parse lines.json: {}", e))
}

#[tokio::main]
async fn main() {
    // broadcast channel for pushing LineMessage to all connected clients.
    let (tx, _) = broadcast::channel::<LineMessage>(16);
    let lines = load_lines_from_file("src/lines.json");

    // Spawn a simple producer that sends sample lines based on their duration.
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        loop {
            for lm in &lines {
                let _ = tx_clone.send(lm.clone());
                sleep(Duration::from_millis(
                    lm.media
                        .as_ref()
                        .and_then(|m| m.duration_ms)
                        .unwrap_or(5000),
                ))
                .await;
            }
        }
    });

    let app = Router::new()
        .route("/", get(html_handler))
        .route("/ws", get(ws_handler))
        .layer(Extension(tx));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind to {}: {}", addr, e));
    println!("Listening on http://{}", addr);
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| panic!("Server error: {}", e));
}

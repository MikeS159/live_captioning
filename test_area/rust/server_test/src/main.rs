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
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::watch;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use tokio::sync::mpsc;
use axum::routing::get_service;
use tower_http::services::ServeDir;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct LineMessage {
    text: String,
    speaker: Option<String>,
    style: Style,
    media: Option<Media>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Style {
    color: String,
    font_size: String,
    font_family: String,
    position: Position,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Position {
    x: String,
    y: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Media {
    kind: String, // "image" or "video"
    url: String,
    duration_ms: Option<u64>,
    padding: Option<Padding>,
    img_size: Option<ImageSize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Padding {
    top: String,
    right: String,
    bottom: String,
    left: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ImageSize {
    width: String,
    height: String,
}

async fn html_handler() -> Html<&'static str> {
    Html(include_str!("client.html"))
}

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

fn load_lines_from_file(path: &str) -> Vec<LineMessage> {
    let data = std::fs::read_to_string(path).expect("Failed to read lines file");
    serde_json::from_str::<Vec<LineMessage>>(&data).expect("Failed to parse lines.json")
}

#[tokio::main]
async fn main() {
    // broadcast channel for pushing LineMessage to all connected clients.
    let (tx, _rx) = broadcast::channel::<LineMessage>(16);
    let lines = load_lines_from_file("src/short.json");

    // Use a watch channel to track the current index
    let (idx_tx, mut idx_rx) = watch::channel(0usize);
    // Channel for keyboard commands
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<String>(8);

    // Spawn a blocking thread for single-key input using crossterm
    {
        let cmd_tx = cmd_tx.clone();
        std::thread::spawn(move || {
            enable_raw_mode().unwrap();
            println!("Press 'n' or → for next, 'p' or ← for previous, 'q' to quit.");
            loop {
                if event::poll(std::time::Duration::from_millis(100)).unwrap() {
                    if let Event::Key(key_event) = event::read().unwrap() {
                        let cmd = match key_event.code {
                            KeyCode::Char('n') | KeyCode::Right => "n",
                            KeyCode::Char('p') | KeyCode::Left => "p",
                            KeyCode::Char('q') => {
                                disable_raw_mode().unwrap();
                                "q"
                            },
                            _ => continue,
                        };
                        let _ = cmd_tx.blocking_send(cmd.to_string());
                    }
                }
            }
        });
    }

    // Async task to process keyboard commands and update idx
    {
        let idx_tx = idx_tx.clone();
        let lines = lines.clone();
        tokio::spawn(async move {
            let mut idx = 0usize;
            let max_idx = lines.len().saturating_sub(1);

            while let Some(line) = cmd_rx.recv().await {
                println!("Keyboard input: {} (current idx: {})", line.trim(), idx);
                match line.trim() {
                    "n" => {
                        if idx < max_idx {
                            idx += 1;
                        }
                        println!("Next: idx = {}", idx);
                        let _ = idx_tx.send(idx);
                    }
                    "p" => {
                        if idx > 0 {
                            idx -= 1;
                        }
                        println!("Prev: idx = {}", idx);
                        let _ = idx_tx.send(idx);
                    }
                    "q" => {
                        println!("Quitting.");
                        std::process::exit(0);
                    }
                    _ => {
                        println!("Unknown command. Use 'n', 'p', or 'q'.");
                        let _ = idx_tx.send(idx); // Still send current idx
                    }
                }
            }
        });
    }

    // Spawn a simple producer that sends sample lines every 4 seconds.
    // {
    //     let tx = tx.clone();
    //     tokio::spawn(async move {
    //         loop {
    //             for lm in &lines {
    //                 let _ = tx.send(lm.clone());
    //                 sleep(Duration::from_millis(
    //                     lm.media
    //                         .as_ref()
    //                         .and_then(|m| m.duration_ms)
    //                         .unwrap_or(5000),
    //                 ))
    //                 .await;
    //             }
    //         }
    //     });
    // }

    // Producer task: sends the current line when index changes
    {
        let tx = tx.clone();
        let lines = lines.clone();
        tokio::spawn(async move {
            // Send the first line immediately
            if let Some(lm) = lines.get(0) {
                println!("Producer: sending initial idx = 0");
                let _ = tx.send(lm.clone());
            }
            loop {
                idx_rx.changed().await.unwrap();
                let idx = *idx_rx.borrow();
                println!("Producer: sending idx = {}", idx);
                if let Some(lm) = lines.get(idx) {
                    let _ = tx.send(lm.clone());
                }
            }
        });
    }

    let app = Router::new()
        .route("/", get(html_handler))
        .route("/ws", get(ws_handler))
        .nest_service("/src", get_service(ServeDir::new("src")))
        .layer(Extension(tx));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

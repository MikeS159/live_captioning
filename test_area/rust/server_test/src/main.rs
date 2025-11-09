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
    let lines = load_lines_from_file("src/lines.json");

    // Spawn a simple producer that sends sample lines every 4 seconds.
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            loop {
                for lm in &lines {
                    let _ = tx.send(lm.clone());
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
        // tokio::spawn(async move {
        //     let lines = vec![
        //         LineMessage {
        //             text: "Welcome to the creative captioning demo".into(),
        //             style: Style {
        //                 color: "#FFCC00".into(),
        //                 font_size: "28px".into(),
        //                 font_family: "Arial".into(),
        //                 position: Position { x: "50%".into(), y: "10%".into() },
        //             },
        //             media: None,
        //         },
        //         LineMessage {
        //             text: "Now showing an image behind the text".into(),
        //             style: Style {
        //                 color: "#FFFFFF".into(),
        //                 font_size: "24px".into(),
        //                 font_family: "Helvetica".into(),
        //                 position: Position { x: "50%".into(), y: "80%".into() },
        //             },
        //             media: Some(Media {
        //                 kind: "image".into(),
        //                 url: "https://picsum.photos/1200/800".into(),
        //                 duration_ms: Some(6000),
        //             }),
        //         },
        //         LineMessage {
        //             text: "And a short caption while a video plays".into(),
        //             style: Style {
        //                 color: "#00FF99".into(),
        //                 font_size: "26px".into(),
        //                 font_family: "Georgia".into(),
        //                 position: Position { x: "10%".into(), y: "50%".into() },
        //             },
        //             media: Some(Media {
        //                 kind: "video".into(),
        //                 url: "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4".into(),
        //                 duration_ms: Some(8000),
        //             }),
        //         },
        //     ];

        //     loop {
        //         for lm in &lines {
        //             if tx.send(lm.clone()).is_err() {
        //                 // no subscribers, continue; sender can ignore error
        //             }
        //             sleep(Duration::from_secs(4)).await;
        //         }
        //     }
        // });
    }

    let app = Router::new()
        .route("/", get(html_handler))
        .route("/ws", get(ws_handler))
        .layer(Extension(tx));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

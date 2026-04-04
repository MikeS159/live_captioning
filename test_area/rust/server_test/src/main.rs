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
use tokio::net::TcpListener as TokioTcpListener;
use tokio::sync::watch;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use tokio::sync::mpsc;
use axum::routing::get_service;
use tower_http::services::ServeDir;
use std::collections::{HashMap, HashSet};

macro_rules! raw_println {
    ($($arg:tt)*) => {
        {
            use std::io::Write;
            print!("{}\r\n", format!($($arg)*));
            let _ = std::io::stdout().flush();
        }
    };
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct LineMessage {
    text: String,
    speaker: Option<String>,
    style: Option<Style>,
    media: Option<Media>,
}

fn string_or_f64<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrFloat {
        Float(f64),
        String(String),
    }
    match StringOrFloat::deserialize(deserializer)? {
        StringOrFloat::Float(f) => Ok(f),
        StringOrFloat::String(s) => s.parse::<f64>().map_err(serde::de::Error::custom),
    }
}

#[derive(Deserialize, Debug)]
struct TcpSegment {
    #[serde(deserialize_with = "string_or_f64")]
    start: f64,
    #[serde(deserialize_with = "string_or_f64")]
    end: f64,
    text: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Style {
    color: String,
    font_size: String,
    font_family: String,
    font_style: String,
    font_weight: String,
    position: Position,
    media: Option<Media>,
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

fn word_jaccard(a: &str, b: &str) -> f64 {
    let set_a: HashSet<String> = a.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    let set_b: HashSet<String> = b.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    let intersection = set_a.intersection(&set_b).count() as f64;
    let union = set_a.union(&set_b).count() as f64;
    if union == 0.0 { 0.0 } else { intersection / union }
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
                            eprint!("serialize error: {}\r\n", e);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprint!("lagged by {} messages\r\n", n);
                }
            }
        }
    })
}

fn load_lines_from_file(path: &str) -> Vec<LineMessage> {
    let data = std::fs::read_to_string(path).expect("Failed to read lines file");
    let raw_lines: Vec<LineMessage> = serde_json::from_str(&data).expect("Failed to parse lines.json");
    let max_len = 120; // Set your desired max length here

    let mut result = Vec::new();

    for line in raw_lines {
        if line.text.len() <= max_len {
            result.push(line);
        } else {
            // Split at full stops, keeping the full stop with the sentence
            let mut buffer = String::new();
            for sentence in line.text.split_inclusive('.') {
                if buffer.len() + sentence.len() > max_len && !buffer.is_empty() {
                    // Push current buffer as a new line
                    let mut new_line = line.clone();
                    new_line.text = buffer.trim().to_string();
                    result.push(new_line);
                    buffer.clear();
                }
                buffer.push_str(sentence);
            }
            if !buffer.trim().is_empty() {
                let mut new_line = line.clone();
                new_line.text = buffer.trim().to_string();
                result.push(new_line);
            }
        }
    }
    result
}

fn load_speaker_styles(path: &str) -> HashMap<String, Style> {
    let data = std::fs::read_to_string(path).expect("Failed to read speaker styles file");
    serde_json::from_str::<HashMap<String, Style>>(&data).expect("Failed to parse speaker_styles.json")
}

#[tokio::main]
async fn main() {
    let speaker_styles = load_speaker_styles("src/speaker_styles.json");
    // broadcast channel for pushing LineMessage to all connected clients.
    let (tx, _rx) = broadcast::channel::<LineMessage>(16);
    //let mut lines = Vec::new();
    // for i in 1..=2 {
    //     let file = if i == 0 {
    //         format!("src/00_prologue.json")
    //     } else {
    //         format!("src/{:02}_scene{}.json", i, i)
    //     };
    //     lines.extend(load_lines_from_file(&file));
    // }
    let mut lines = load_lines_from_file("src/12_scene12.json");

    // Apply default style if missing
    for line in &mut lines {
        if line.style.is_none() {
            if let Some(speaker) = &line.speaker {
                if let Some(default_style) = speaker_styles.get(speaker) {
                    line.style = Some(default_style.clone());
                }
            }
        }
        // Apply default media if missing and available in speaker_styles
        if line.media.is_none() {
            if let Some(speaker) = &line.speaker {
                if let Some(default_style) = speaker_styles.get(speaker) {
                    if let Some(default_media) = &default_style.media {
                        line.media = Some(default_media.clone());
                    }
                }
            }
        }
    }

    // Use a watch channel to track the current index
    let (idx_tx, mut idx_rx) = watch::channel(0usize);
    // Channel for keyboard commands
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<String>(8);

    // Spawn a blocking thread for single-key input using crossterm
    {
        let cmd_tx = cmd_tx.clone();
        std::thread::spawn(move || {
            enable_raw_mode().unwrap();
            raw_println!("Press 'n' or → for next, 'p' or ← for previous, 'q' to quit.");
            loop {
                if event::poll(std::time::Duration::from_millis(100)).unwrap() {
                    if let Event::Key(key_event) = event::read().unwrap() {
                        if let KeyEventKind::Press = key_event.kind {
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
            }
        });
    }

    // Async task to process keyboard commands and update idx
    {
        let idx_tx = idx_tx.clone();
        let lines = lines.clone();
        tokio::spawn(async move {
            let max_idx = lines.len().saturating_sub(1);

            while let Some(line) = cmd_rx.recv().await {
                let mut idx = *idx_tx.borrow();
                raw_println!("Keyboard input: {} (current idx: {})", line.trim(), idx);
                match line.trim() {
                    "n" => {
                        if idx < max_idx {
                            idx += 1;
                        }
                        raw_println!("Next: idx = {}", idx);
                        let _ = idx_tx.send(idx);
                    }
                    "p" => {
                        if idx > 0 {
                            idx -= 1;
                        }
                        raw_println!("Prev: idx = {}", idx);
                        let _ = idx_tx.send(idx);
                    }
                    "q" => {
                        raw_println!("Quitting.");
                        std::process::exit(0);
                    }
                    _ => {
                        raw_println!("Unknown command. Use 'n', 'p', or 'q'.");
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
                raw_println!("Producer: sending initial idx = 0");
                let _ = tx.send(lm.clone());
            }
            loop {
                idx_rx.changed().await.unwrap();
                let idx = *idx_rx.borrow();
                raw_println!("Producer: sending idx = {}", idx);
                if let Some(lm) = lines.get(idx) {
                    let _ = tx.send(lm.clone());
                }
            }
        });
    }

    // Spawn TCP receiver task (listens on 127.0.0.1:5000 for newline-delimited JSON)
    {
        let tx = tx.clone();
        let lines = lines.clone();
        let idx_tx = idx_tx.clone();
        tokio::spawn(async move {
            let tcp_addr = "127.0.0.1:5000";
            let listener = TokioTcpListener::bind(tcp_addr).await.expect("Failed to bind TCP listener");
            raw_println!("TCP receiver listening on {}", tcp_addr);
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        raw_println!("TCP connected by {}", addr);
                        let tx = tx.clone();
                        let lines = lines.clone();
                        let idx_tx = idx_tx.clone();
                        tokio::spawn(async move {
                            let reader = BufReader::new(stream);
                            let mut tcp_lines = reader.lines();
                            let mut accumulated = String::new();
                            let max_accumulated_chars = 150;
                            while let Ok(Some(line)) = tcp_lines.next_line().await {
                                match serde_json::from_str::<TcpSegment>(&line) {
                                    Ok(seg) => {
                                        raw_println!("[{} -> {}] {}", seg.start, seg.end, seg.text);

                                        // Accumulate text into rolling buffer
                                        if !accumulated.is_empty() {
                                            accumulated.push(' ');
                                        }
                                        accumulated.push_str(&seg.text);
                                        // Trim to last max_accumulated_chars
                                        if accumulated.len() > max_accumulated_chars {
                                            let start = accumulated.len() - max_accumulated_chars;
                                            // Find next word boundary to avoid cutting mid-word
                                            let trim_at = accumulated[start..].find(' ').map(|p| start + p + 1).unwrap_or(start);
                                            accumulated = accumulated[trim_at..].to_string();
                                        }

                                        let current = *idx_tx.borrow();
                                        let current_score = word_jaccard(&accumulated, &lines[current].text);
                                        let next_score = if current + 1 < lines.len() {
                                            word_jaccard(&accumulated, &lines[current + 1].text)
                                        } else {
                                            0.0
                                        };

                                        raw_println!("  buf: \"{}...\"", &accumulated[..accumulated.len().min(80)]);
                                        raw_println!("  idx {} score: {:.3} | idx {} score: {:.3}",
                                            current, current_score, current + 1, next_score);

                                        // Advance when next line matches better than current (with hysteresis)
                                        if next_score > current_score + 0.05 && next_score > 0.1 {
                                            let next_idx = current + 1;
                                            raw_println!("  >> Crossover: advancing to idx {}: {}",
                                                next_idx, lines[next_idx].text);
                                            let _ = idx_tx.send(next_idx);
                                        }
                                    }
                                    Err(e) => {
                                        eprint!("TCP JSON parse error: {}\r\n", e);
                                    }
                                }
                            }
                            raw_println!("TCP connection closed.");
                        });
                    }
                    Err(e) => {
                        eprint!("TCP accept error: {}\r\n", e);
                    }
                }
            }
        });
    }

    let app = Router::new()
        .route("/", get(html_handler))
        .route("/ws", get(ws_handler))
        .nest_service("/src", get_service(ServeDir::new("src")))
        .layer(Extension(tx));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3159));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    raw_println!("Listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}


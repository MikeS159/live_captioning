use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Extension},
    response::Html,
    routing::get,
    Router,
};
use axum::extract::ws::Utf8Bytes;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, time::Duration};
use std::time::Instant;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    animate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    animate_word: Option<String>,
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

/// What fraction of `line`'s words appear in `buffer`?
/// Recent words (last `recent_len` chars) count as a full match,
/// older words count as a partial (0.3) match.
fn containment(buffer: &str, line: &str) -> f64 {
    let recent_len = 60;
    let split_at = buffer.len().saturating_sub(recent_len);
    // Find word boundary for the split
    let split_at = if split_at == 0 { 0 } else {
        buffer[split_at..].find(' ').map(|p| split_at + p + 1).unwrap_or(split_at)
    };
    let older = &buffer[..split_at];
    let recent = &buffer[split_at..];

    let recent_words: HashSet<String> = recent.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    let older_words: HashSet<String> = older.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    let line_words: HashSet<String> = line.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    if line_words.is_empty() { return 0.0; }

    let mut score = 0.0;
    for w in &line_words {
        if recent_words.contains(w) {
            score += 1.0;
        } else if older_words.contains(w) {
            score += 0.3;
        }
    }
    score / line_words.len() as f64
}

fn truncate_line(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let half = (max_len - 3) / 2;
        format!("{}...{}", &text[..half], &text[text.len() - half..])
    }
}

fn print_context_window(lines: &[LineMessage], current: usize, auto_advance: bool) {
    let display_max = 100;
    let marker = if auto_advance { ">>>" } else { "xxx" };
    raw_println!("──────────────────────────────────────────────────────");
    let start = current.saturating_sub(10);
    let end = (current + 10).min(lines.len().saturating_sub(1));
    for i in start..=end {
        let truncated = truncate_line(&lines[i].text, display_max);
        if i == current {
            raw_println!("{} {:>3}: {}\n", marker, i, truncated);
        } else {
            raw_println!("    {:>3}: {}\n", i, truncated);
        }
    }
    raw_println!("──────────────────────────────────────────────────────");
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
    use std::env;
    let args: Vec<String> = env::args().collect();
    let (start, stop) = if args.len() >= 3 {
        let s = args[1].parse::<usize>().unwrap_or(0);
        let e = args[2].parse::<usize>().unwrap_or(8);
        (s, e)
    } else {
        (0, 8)
    };
    let speaker_styles = load_speaker_styles("src/speaker_styles.json");
    // broadcast channel for pushing LineMessage to all connected clients.
    let (tx, _rx) = broadcast::channel::<LineMessage>(16);
    // let mut lines = Vec::new();
    // for i in start..=stop {
    //     let file = if i == 0 {
    //         format!("src/00_prologue.json")
    //     } else {
    //         format!("src/{:02}_scene{}.json", i, i)
    //     };
    //     lines.extend(load_lines_from_file(&file));
    // }
    let mut lines = load_lines_from_file("src/colour_test.json");

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
    // Watch channel to toggle auto-advance on/off
    let (auto_tx, auto_rx) = watch::channel(true);
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
                                KeyCode::Char('h') => "h",
                                KeyCode::Char('g') => "g",
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
        let auto_tx = auto_tx.clone();
        let auto_rx_kb = auto_rx.clone();
        let lines = lines.clone();
        tokio::spawn(async move {
            let max_idx = lines.len().saturating_sub(1);

            while let Some(line) = cmd_rx.recv().await {
                let mut idx = *idx_tx.borrow();
                let auto = *auto_rx_kb.borrow();
                match line.trim() {
                    "n" => {
                        if idx < max_idx {
                            idx += 1;
                        }
                        let _ = idx_tx.send(idx);
                        print_context_window(&lines, idx, auto);
                    }
                    "p" => {
                        if idx > 0 {
                            idx -= 1;
                        }
                        let _ = idx_tx.send(idx);
                        print_context_window(&lines, idx, auto);
                    }
                    "h" => {
                        let _ = auto_tx.send(false);
                        raw_println!("Auto-advance DISABLED");
                        print_context_window(&lines, idx, false);
                    }
                    "g" => {
                        let _ = auto_tx.send(true);
                        raw_println!("Auto-advance ENABLED");
                        print_context_window(&lines, idx, true);
                    }
                    "q" => {
                        raw_println!("Quitting.");
                        std::process::exit(0);
                    }
                    _ => {}
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
                //raw_println!("Producer: sending initial idx = 0");
                let _ = tx.send(lm.clone());
                print_context_window(&lines, 0, true);
            }
            loop {
                idx_rx.changed().await.unwrap();
                let idx = *idx_rx.borrow();
                //raw_println!("Producer: sending idx = {}", idx);
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
        let auto_rx_tcp = auto_rx.clone();
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
                        let auto_rx_conn = auto_rx_tcp.clone();
                        tokio::spawn(async move {
                            let reader = BufReader::new(stream);
                            let mut tcp_lines = reader.lines();
                            let mut accumulated = String::new();
                            let max_accumulated_chars = 150;
                            let mut last_advance = Instant::now();
                            let mut prev_next_score: f64 = 0.0;
                            let mut prev_skip_score: f64 = 0.0;
                            let mut have_baseline = false; // Need one measurement before detecting rises
                            let mut last_known_idx: usize = *idx_tx.borrow();
                            while let Ok(Some(line)) = tcp_lines.next_line().await {
                                match serde_json::from_str::<TcpSegment>(&line) {
                                    Ok(seg) => {
                                        // raw_println!("[{} -> {}] {}", seg.start, seg.end, seg.text);

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
                                        // Detect external index change (keyboard override)
                                        if current != last_known_idx {
                                            // raw_println!("  (external idx change {} -> {}, resetting baseline)", last_known_idx, current);
                                            prev_next_score = 0.0;
                                            prev_skip_score = 0.0;
                                            have_baseline = false;
                                            last_advance = Instant::now();
                                            last_known_idx = current;
                                        }
                                        // Dwell time scales with word count: ~100ms per word, minimum 1s
                                        let word_count = lines[current].text.split_whitespace().count() as u64;
                                        let min_dwell = Duration::from_millis((word_count * 150).max(1000));
                                        let current_score = containment(&accumulated, &lines[current].text);
                                        let next_score = if current + 1 < lines.len() {
                                            containment(&accumulated, &lines[current + 1].text)
                                        } else {
                                            0.0
                                        };
                                        let skip_score = if current + 2 < lines.len() {
                                            containment(&accumulated, &lines[current + 2].text)
                                        } else {
                                            0.0
                                        };

                                        // raw_println!("  buf: \"{}...\"", &accumulated[..accumulated.len().min(80)]);
                                        // raw_println!("  idx {} score: {:.3} | idx+1 score: {:.3} (prev: {:.3}) | idx+2 score: {:.3} (prev: {:.3}, baseline: {})",
                                        //     current, current_score, next_score, prev_next_score, skip_score, prev_skip_score, have_baseline);

                                        let auto_on = *auto_rx_conn.borrow();
                                        if auto_on && last_advance.elapsed() >= min_dwell {
                                            if !have_baseline {
                                                // First measurement after advance — record baseline, don't act
                                                prev_next_score = next_score;
                                                prev_skip_score = skip_score;
                                                have_baseline = true;
                                                // raw_println!("  (baseline set: next {:.3}, skip {:.3})", next_score, skip_score);
                                            } else if next_score > prev_next_score && next_score > 0.15 {
                                                // Score is rising above baseline — advance
                                                let next_idx = current + 1;
                                                // raw_println!("  >> Next score rising ({:.3} -> {:.3}), advancing to idx {}: {}",
                                                //     prev_next_score, next_score, next_idx, lines[next_idx].text);
                                                last_advance = Instant::now();
                                                prev_next_score = 0.0;
                                                prev_skip_score = 0.0;
                                                have_baseline = false; // Need new baseline after advance
                                                last_known_idx = next_idx;
                                                let _ = idx_tx.send(next_idx);
                                            } else if skip_score > prev_skip_score && skip_score > next_score + 0.2 && skip_score > 0.3 {
                                                // idx+2 is rising and beats both current and next — line was skipped
                                                let skip_idx = current + 2;
                                                // raw_println!("  >> Skip detected ({:.3} -> {:.3}), jumping to idx {}: {}",
                                                //     prev_skip_score, skip_score, skip_idx, lines[skip_idx].text);
                                                last_advance = Instant::now();
                                                prev_next_score = 0.0;
                                                prev_skip_score = 0.0;
                                                have_baseline = false;
                                                last_known_idx = skip_idx;
                                                let _ = idx_tx.send(skip_idx);
                                            } else {
                                                prev_next_score = next_score;
                                                prev_skip_score = skip_score;
                                            }
                                        } else {
                                            // Still in dwell period — just track scores
                                            prev_next_score = next_score;
                                            prev_skip_score = skip_score;
                                        }
                                        print_context_window(&lines, *idx_tx.borrow(), auto_on);
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


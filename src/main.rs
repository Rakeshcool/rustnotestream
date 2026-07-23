mod keyboard;

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    Router,
    routing::get,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{IntoResponse, Json},
    http::{HeaderMap, Method, StatusCode, header},
};
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
};
use tracing::{info, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------
const PORT: u16 = 8765;
const RATE_LIMIT_ACTIONS: usize = 50;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(1);
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_AUTH_ATTEMPTS: u32 = 5;

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------
struct AppState {
    pin: Mutex<String>,
    locked: AtomicBool,
    rate_limit_enabled: bool,
    authorized_session: Mutex<Option<Uuid>>,
    sessions: Mutex<HashMap<Uuid, SessionState>>,
    rate_counters: Mutex<HashMap<Uuid, Vec<Instant>>>,
    webclient_dir: PathBuf,
}

#[derive(Clone, Copy)]
struct SessionState {
    authenticated: bool,
    auth_attempts: u32,
    last_activity: Instant,
}

impl SessionState {
    fn new() -> Self {
        Self {
            authenticated: false,
            auth_attempts: 0,
            last_activity: Instant::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// JSON message types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ClientMessage {
    action: String,
    #[serde(default)]
    pin: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    enter: Option<bool>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    keys: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct ServerMessage {
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    locked: Option<bool>,
    #[serde(rename = "final", skip_serializing_if = "Option::is_none")]
    final_: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    authenticated: Option<bool>,
}

impl ServerMessage {
    fn auth_required(msg: &str) -> Self {
        Self {
            action: "auth_required".into(),
            message: Some(msg.into()),
            reason: None,
            locked: None,
            final_: None,
            authenticated: None,
        }
    }

    fn auth_ok(locked: bool) -> Self {
        Self {
            action: "auth_ok".into(),
            message: Some("Authenticated".into()),
            reason: None,
            locked: Some(locked),
            final_: None,
            authenticated: None,
        }
    }

    fn auth_failed(msg: &str, final_: bool) -> Self {
        Self {
            action: "auth_failed".into(),
            message: Some(msg.into()),
            reason: None,
            locked: None,
            final_: Some(final_),
            authenticated: None,
        }
    }

    fn error(msg: &str) -> Self {
        Self {
            action: "error".into(),
            message: Some(msg.into()),
            reason: None,
            locked: None,
            final_: None,
            authenticated: None,
        }
    }

    fn pong(locked: bool) -> Self {
        Self {
            action: "pong".into(),
            message: None,
            reason: None,
            locked: Some(locked),
            final_: None,
            authenticated: None,
        }
    }

    fn lock_status(locked: bool) -> Self {
        Self {
            action: "lock_status".into(),
            message: None,
            reason: None,
            locked: Some(locked),
            final_: None,
            authenticated: None,
        }
    }

    fn disconnect(reason: &str, msg: &str) -> Self {
        Self {
            action: "disconnect".into(),
            message: Some(msg.into()),
            reason: Some(reason.into()),
            locked: None,
            final_: None,
            authenticated: None,
        }
    }

    fn status(locked: bool, authenticated: bool) -> Self {
        Self {
            action: "status".into(),
            message: None,
            reason: None,
            locked: Some(locked),
            final_: None,
            authenticated: Some(authenticated),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_local_ip() -> String {
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                return addr.ip().to_string();
            }
        }
    }
    "127.0.0.1".into()
}

fn check_origin(origin: Option<&str>, host: Option<&str>, ip: &str, port: u16) -> bool {
    let (origin, host) = match (origin, host) {
        (Some(o), Some(h)) if !o.is_empty() && !h.is_empty() => (o, h),
        _ => return true,
    };

    let allowed = vec![
        format!("http://{host}"),
        format!("https://{host}"),
        format!("http://{ip}:{port}"),
        format!("https://{ip}:{port}"),
    ];

    allowed.iter().any(|a| origin.starts_with(a.as_str()))
}

// ---------------------------------------------------------------------------
// Rate limiting
// ---------------------------------------------------------------------------

fn check_rate_limit(session_id: Uuid, counters: &mut HashMap<Uuid, Vec<Instant>>) -> bool {
    let now = Instant::now();
    let cutoff = now - RATE_LIMIT_WINDOW;
    let times = counters.entry(session_id).or_default();

    times.retain(|t| *t > cutoff);

    if times.len() >= RATE_LIMIT_ACTIONS {
        return false;
    }

    times.push(now);
    true
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

fn print_banner(ip: &str, port: u16, pin: &str, rate_limit_enabled: bool) {
    let line = "=".repeat(52);
    let rate_line = if rate_limit_enabled {
        format!("   - Rate limiting ({} actions/sec)", RATE_LIMIT_ACTIONS)
    } else {
        "   - Rate limiting DISABLED".into()
    };
    println!(
        r#"
{line}
  NoteStream Server  (SECURE MODE)
  Type from Android -> Any Windows App
{line}
  Server:   http://{ip}:{port}
  WebSock:  ws://{ip}:{port}/ws

  PIN:      {pin}    <-- enter this on your phone
  Keyboard: Windows SendInput (Rust)

  SECURITY:
   - PIN-based authentication enabled
   - Single active session
   {rate_line}
   - Idle timeout ({idle}s)
   - Auth timeout ({auth}s)
   - Origin validation

  Extra:    Live-typing supported (toggle on phone)

  Commands from phone:  lock / unlock (toggle typing)

  Press Ctrl+C to stop
{line}
"#,
        ip = ip,
        port = port,
        pin = pin,
        rate_line = rate_line,
        idle = IDLE_TIMEOUT.as_secs(),
        auth = AUTH_TIMEOUT.as_secs(),
    );
}

// ---------------------------------------------------------------------------
// WebSocket handler
// ---------------------------------------------------------------------------

async fn ws_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    state: Arc<AppState>,
) -> axum::response::Response {
    let ip = get_local_ip();
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    if !check_origin(origin.as_deref(), host.as_deref(), &ip, PORT) {
        warn!("Rejected WebSocket connection from origin: {:?}", origin);
        return StatusCode::FORBIDDEN.into_response();
    }

    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
        .into_response()
}

async fn handle_ws_connection(ws: WebSocket, state: Arc<AppState>) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    let session_id = Uuid::new_v4();
    let mut session = SessionState::new();

    // Register session
    {
        let mut sessions = state.sessions.lock().await;
        sessions.insert(session_id, session);
    }

    info!("[+] New connection: {}", session_id);

    // Send auth challenge
    if send_json(&mut ws_tx, &ServerMessage::auth_required("Enter the PIN shown on the server"))
        .await
        .is_err()
    {
        cleanup_session(&state, session_id).await;
        return;
    }

    let mut awaiting_auth = true;
    let auth_deadline = Instant::now() + AUTH_TIMEOUT;

    // ---- Main message loop ----
    loop {
        // Check auth timeout
        if awaiting_auth && Instant::now() >= auth_deadline {
            warn!("[!] Auth timeout for session {}", session_id);
            let _ = send_json(
                &mut ws_tx,
                &ServerMessage::disconnect("timeout", "Authentication timed out"),
            )
            .await;
            break;
        }

        // Check idle timeout
        if !awaiting_auth {
            let idle = session.last_activity.elapsed();
            if idle >= IDLE_TIMEOUT {
                warn!("[!] Idle timeout for session {}", session_id);
                let _ = send_json(
                    &mut ws_tx,
                    &ServerMessage::disconnect("idle", "Disconnected due to inactivity"),
                )
                .await;
                break;
            }
        }

        // Wait for next message (with a short timeout to allow periodic checks)
        let timeout_dur = Duration::from_secs(5);
        let msg = tokio::time::timeout(timeout_dur, ws_rx.next()).await;

        let msg = match msg {
            Ok(Some(Ok(msg))) => msg,
            Ok(Some(Err(e))) => {
                warn!("[!] WebSocket error for session {}: {:?}", session_id, e);
                break;
            }
            Ok(None) => break,
            Err(_) => continue, // Timeout — loop back to check timeouts
        };

        match msg {
            Message::Text(text) => {
                let client_msg: ClientMessage = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("[!] Invalid JSON from {}: {}", session_id, e);
                        continue;
                    }
                };

                let action = client_msg.action.as_str();

                // ---- Auth handshake ----
                if action == "auth" {
                    if !awaiting_auth {
                        let _ =
                            send_json(&mut ws_tx, &ServerMessage::error("Already authenticated"))
                                .await;
                        continue;
                    }

                    let pin = client_msg.pin.unwrap_or_default();
                    session.auth_attempts += 1;

                    let server_pin = state.pin.lock().await.clone();

                    if pin == server_pin {
                        awaiting_auth = false;
                        session.authenticated = true;
                        session.last_activity = Instant::now();

                        // Update session state
                        {
                            let mut sessions = state.sessions.lock().await;
                            sessions.insert(session_id, session);
                        }

                        // Displace any existing authorized session
                        let mut authorized = state.authorized_session.lock().await;
                        if let Some(old_id) = authorized.replace(session_id) {
                            if old_id != session_id {
                                info!("[+] Session {} displaced by {}", old_id, session_id);
                            }
                        }
                        drop(authorized);

                        let locked = state.locked.load(Ordering::Relaxed);
                        info!("[+] Authorized: {} (PIN OK)", session_id);
                        let _ = send_json(&mut ws_tx, &ServerMessage::auth_ok(locked)).await;
                    } else {
                        let remaining = MAX_AUTH_ATTEMPTS.saturating_sub(session.auth_attempts);
                        warn!(
                            "[!] Wrong PIN from {} ({} attempts left)",
                            session_id, remaining
                        );

                        if remaining == 0 {
                            let _ =
                                send_json(&mut ws_tx, &ServerMessage::auth_failed(
                                    "Too many attempts",
                                    true,
                                ))
                                .await;
                            break;
                        } else {
                            let _ =
                                send_json(
                                    &mut ws_tx,
                                    &ServerMessage::auth_failed(
                                        &format!("Wrong PIN. {} attempt(s) left", remaining),
                                        false,
                                    ),
                                )
                                .await;
                        }
                    }
                    continue;
                }

                // ---- All actions below require auth ----
                if awaiting_auth {
                    let _ = send_json(&mut ws_tx, &ServerMessage::error("Authenticate first"))
                        .await;
                    continue;
                }

                // ---- Ping ----
                if action == "ping" {
                    let locked = state.locked.load(Ordering::Relaxed);
                    let _ = send_json(&mut ws_tx, &ServerMessage::pong(locked)).await;
                    session.last_activity = Instant::now();
                    continue;
                }

                // ---- Rate limit (for non-ping actions) ----
                if state.rate_limit_enabled {
                    let mut counters = state.rate_counters.lock().await;
                    if !check_rate_limit(session_id, &mut counters) {
                        let _ =
                            send_json(&mut ws_tx, &ServerMessage::error("Rate limited. Slow down."))
                                .await;
                        continue;
                    }
                }

                session.last_activity = Instant::now();

                match action {
                    "type" | "live_type" => {
                        if state.locked.load(Ordering::Relaxed) {
                            let _ =
                                send_json(&mut ws_tx, &ServerMessage::error("Server is locked"))
                                    .await;
                            continue;
                        }

                        let text = client_msg.text.unwrap_or_default();
                        let press_enter = client_msg.enter.unwrap_or(false);

                        let preview: String = text.chars().take(60).collect();
                        let preview = if text.len() > 60 {
                            format!("{}...", preview.replace('\n', "\\n"))
                        } else {
                            preview.replace('\n', "\\n")
                        };
                        info!("[>] Typing: {}", preview);

                        let text_clone = text;
                        tokio::task::spawn_blocking(move || {
                            keyboard::simulate_typing(&text_clone, press_enter);
                        });
                    }

                    "key" => {
                        if state.locked.load(Ordering::Relaxed) {
                            let _ =
                                send_json(&mut ws_tx, &ServerMessage::error("Server is locked"))
                                    .await;
                            continue;
                        }

                        let key = client_msg.key.unwrap_or_default();
                        info!("[>] Key: {}", key);

                        tokio::task::spawn_blocking(move || {
                            keyboard::simulate_special_key(&key);
                        });
                    }

                    "hotkey" => {
                        if state.locked.load(Ordering::Relaxed) {
                            let _ =
                                send_json(&mut ws_tx, &ServerMessage::error("Server is locked"))
                                    .await;
                            continue;
                        }

                        let keys = client_msg.keys.unwrap_or_default();
                        info!("[>] Hotkey: {}", keys.join("+"));

                        tokio::task::spawn_blocking(move || {
                            keyboard::simulate_key_combo(&keys);
                        });
                    }

                    "lock" => {
                        state.locked.store(true, Ordering::Relaxed);
                        info!("[!] Server LOCKED by {}", session_id);
                        let _ = send_json(&mut ws_tx, &ServerMessage::lock_status(true)).await;
                    }

                    "unlock" => {
                        state.locked.store(false, Ordering::Relaxed);
                        info!("[!] Server UNLOCKED by {}", session_id);
                        let _ = send_json(&mut ws_tx, &ServerMessage::lock_status(false)).await;
                    }

                    "get_status" => {
                        let locked = state.locked.load(Ordering::Relaxed);
                        let _ = send_json(&mut ws_tx, &ServerMessage::status(locked, true)).await;
                    }

                    other => {
                        let _ = send_json(
                            &mut ws_tx,
                            &ServerMessage::error(&format!("Unknown action: {}", other)),
                        )
                        .await;
                    }
                }
            }

            Message::Close(_) => break,
            _ => {}
        }

        // After processing a message, check if this session was displaced.
        // If the authorized_session no longer points to us, we've been replaced.
        if !awaiting_auth {
            let authorized = state.authorized_session.lock().await;
            let displaced = *authorized != Some(session_id);
            drop(authorized);
            if displaced {
                let _ = send_json(
                    &mut ws_tx,
                    &ServerMessage::disconnect("replaced", "Another device authenticated"),
                )
                .await;
                break;
            }
        }
    }

    // Send a close frame for clean disconnection
    let _ = ws_tx.send(Message::Close(None)).await;

    cleanup_session(&state, session_id).await;
}

async fn send_json<S>(sender: &mut S, msg: &ServerMessage) -> Result<(), ()>
where
    S: futures_util::sink::Sink<Message> + std::marker::Unpin,
    S::Error: std::fmt::Debug,
{
    let json = serde_json::to_string(msg).map_err(|_| ())?;
    sender
        .send(Message::Text(json.into()))
        .await
        .map_err(|_| ())
}

async fn cleanup_session(state: &Arc<AppState>, session_id: Uuid) {
    let mut authorized = state.authorized_session.lock().await;
    if *authorized == Some(session_id) {
        *authorized = None;
        info!("[-] Authorized session released: {}", session_id);
    }
    drop(authorized);

    let mut sessions = state.sessions.lock().await;
    sessions.remove(&session_id);
    drop(sessions);

    let mut counters = state.rate_counters.lock().await;
    counters.remove(&session_id);
    drop(counters);

    info!("[-] Disconnected: {}", session_id);
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

async fn index_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let index_path = state.webclient_dir.join("index.html");
    if index_path.exists() {
        match tokio::fs::read_to_string(&index_path).await {
            Ok(html) => axum::response::Html(html).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    } else {
        axum::response::Html(
            "<h1>NoteStream Server Running</h1><p>Client HTML not found at webclient/index.html</p>"
                .to_string(),
        )
        .into_response()
    }
}

async fn status_handler(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let authorized = state.authorized_session.lock().await;
    let client_count = if authorized.is_some() { 1 } else { 0 };
    drop(authorized);

    let locked = state.locked.load(Ordering::Relaxed);

    Json(serde_json::json!({
        "status": "running",
        "clients": client_count,
        "host": get_local_ip(),
        "port": PORT,
        "locked": locked,
    }))
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    // Parse CLI arguments
    let args: Vec<String> = std::env::args().collect();
    let rate_limit_enabled = !args.iter().any(|a| a == "--no-rate-limit");

    // Generate 6-digit PIN
    let pin = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000));

    // Determine webclient directory
    let webclient_dir = resolve_webclient_dir();

    let state = Arc::new(AppState {
        pin: Mutex::new(pin.clone()),
        locked: AtomicBool::new(false),
        rate_limit_enabled,
        authorized_session: Mutex::new(None),
        sessions: Mutex::new(HashMap::new()),
        rate_counters: Mutex::new(HashMap::new()),
        webclient_dir: webclient_dir.clone(),
    });

    let ip = get_local_ip();
    print_banner(&ip, PORT, &pin, rate_limit_enabled);

    // Build routes
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/status", get(status_handler))
        .route(
            "/ws",
            get(
                |headers: HeaderMap,
                 ws: WebSocketUpgrade,
                 State(state): State<Arc<AppState>>| async move {
                    ws_handler(headers, ws, state).await
                },
            ),
        )
        .nest_service("/static", ServeDir::new(&webclient_dir))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::CONTENT_TYPE, header::ORIGIN]),
        )
        .with_state(state);

    // Start server
    let addr = std::net::SocketAddr::new(
        IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
        PORT,
    );
    info!("Server listening on http://{}:{}", ip, PORT);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Locate the webclient directory from the expected locations.
fn resolve_webclient_dir() -> PathBuf {
    let candidates = [
        PathBuf::from("webclient"),
        PathBuf::from("../webclient"),
        PathBuf::from(r"D:\build by ai\notestream\webclient"),
    ];

    for path in &candidates {
        if path.join("index.html").exists() {
            info!("Using webclient directory: {:?}", path);
            return path.clone();
        }
    }

    // Fallback: create directory with basic file
    let fallback = PathBuf::from("webclient");
    let _ = std::fs::create_dir_all(fallback.join("icons"));
    let basic_html = r#"<!DOCTYPE html>
<html>
<head><meta charset="UTF-8"><title>NoteStream</title></head>
<body>
<h1>NoteStream Server Running</h1>
<p>Client HTML not found at webclient/index.html</p>
</body>
</html>
"#;
    let _ = std::fs::write(fallback.join("index.html"), basic_html);
    info!("Created fallback webclient directory");
    fallback
}

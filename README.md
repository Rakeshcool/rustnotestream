# NoteStream build in rust 📱 → 💻

**Type from your Android device into any Windows application** — notepad, browser,
file explorer, terminal, anything. Zero installation on Android — just open a URL.

> Rust port of the Python NoteStream server. PIN-authenticated, single-session,
> rate-limited, with live streaming support.

## Demo

```
$ cargo run --release

====================================================
  NoteStream Server  (SECURE MODE)
  Type from Android -> Any Windows App
====================================================
  Server:   http://192.168.1.x:8765
  WebSock:  ws://192.168.1.x:8765/ws

  PIN:      487291    <-- enter this on your phone
  Keyboard: Windows SendInput (Rust)

  SECURITY:
   - PIN-based authentication enabled
   - Single active session
   - Rate limiting (50 actions/sec)
   - Idle timeout (300s)
   - Auth timeout (30s)
   - Origin validation

  Extra:    Live-typing supported (toggle on phone)

  Commands from phone:  lock / unlock (toggle typing)

  Press Ctrl+C to stop
====================================================
```

## Quick Start

### Prerequisites

- **Windows** (for keyboard simulation via Win32 API)
- **Rust** 1.75+ ([rustup.rs](https://rustup.rs))
- Both devices on the same WiFi network

### 1. Install & Build

```bash
git clone https://github.com/Rakeshcool/rustnotestream
cd notestream
cargo build --release
```

### 2. Start the server

```bash
cargo run --release
```

Look for the **6-digit PIN** in the terminal output.

### 3. Connect your Android device

1. Make sure your phone is on **the same WiFi network** as your PC
2. Open Chrome (or any browser) on your phone
3. Go to `http://192.168.1.x:8765` (the URL from the server output)
4. **Enter the 6-digit PIN** shown in the server terminal
5. Once authenticated, the typing screen appears

### 4. Start typing

- **Type text** and press **Send** — it instantly types into whatever window is active on your PC
- Toggle **🔁 LIVE** for streaming mode — characters are sent as you type (60ms debounce)
- Toggle **↵ Enter** to append Enter after each message
- Use **Tab**, **Esc**, **Enter**, **⌫ Bksp**, **⌦ Del** for special keys
- Use **🔒 Lock** to freeze typing remotely

## Features

### Core
- **PIN authentication** — random 6-digit PIN on each server start
- **Single active session** — new connections displace old ones
- **Rate limiting** — 50 actions/sec sliding window (disable with `--no-rate-limit`)
- **Origin validation** — blocks cross-site WebSocket hijacking
- **Idle timeout** — auto-disconnects after 5 minutes of inactivity
- **Auth timeout** — drops unauthenticated connections after 30 seconds
- **Max 5 PIN attempts** — brute-force protection
- **Lock/unlock** — freeze typing from your phone instantly
- **Session displacement** — old session gets notified when a new one authenticates

### Live Typing (NEW)
- Toggle **LIVE** mode for real-time character streaming
- Text stays visible in the textarea (no disappearing text)
- 60ms debounce batches rapid keystrokes
- Pending text flushes on **Send** or **Enter**
- Pre-existing text is sent immediately when live mode is enabled

### Keyboard Simulation
- **Windows SendInput** (Win32 API via the `windows` crate)
- Supports all standard characters, including shift-key punctuation
- Special keys: Backspace, Delete, Tab, Escape, Enter, Space, arrows, Home, End, Page Up/Down, Caps Lock
- Hotkeys: Ctrl+C, Alt+Tab, Win+R, etc.

### Web Client
- Dark-mode PWA with mobile-first design
- PIN entry screen with 6-digit input
- Sent history (persisted in localStorage)
- Auto-reconnect with exponential backoff
- Copy URL to clipboard
- Connection status indicator
- Quick-access special keys

## CLI Options

| Flag | Description | Default |
|------|-------------|---------|
| `--no-rate-limit` | Disable rate limiting | Rate limiting enabled |
| `-h`, `--help` | Show help | — |

## Project Structure

```
notestream/
├── Cargo.toml            # Dependencies & metadata
├── src/
│   ├── main.rs           # HTTP/WebSocket server (axum)
│   └── keyboard.rs       # Windows keyboard simulation (Win32)
├── webclient/
│   ├── index.html        # Android web client
│   ├── manifest.json     # PWA manifest
│   └── icons/icon.svg    # App icon
└── README.md             # This file
```

## Tech Stack

| Component | Technology |
|-----------|-----------|
| HTTP/WebSocket server | [axum](https://github.com/tokio-rs/axum) 0.7 |
| Async runtime | [tokio](https://tokio.rs) 1.x |
| Keyboard simulation | [windows](https://github.com/microsoft/windows-rs) 0.58 (Win32 SendInput) |
| Serialization | [serde](https://serde.rs) / [serde_json](https://github.com/serde-rs/json) |
| Session IDs | [uuid](https://github.com/uuid-rs/uuid) |
| Static files | [tower-http](https://github.com/tower-rs/tower-http) ServeDir |
| Logging | [tracing](https://github.com/tokio-rs/tracing) |

## Troubleshooting

| Problem | Solution |
|---------|----------|
| Can't connect from phone | Check both devices are on the same WiFi |
| PIN not shown | Look in the server terminal output |
| Wrong PIN error | PIN is 6 digits, case-sensitive digits |
| "Another device auth'd" | Someone else connected — reconnect |
| Text doesn't type | Click the target window first to focus it |
| "Rate limited" error | Slow down, or use `--no-rate-limit` |
| Firewall blocking | Allow the executable through Windows Firewall |
| Typing is slow | Use `--release` build for best performance |

## License

Free to use, modify, and share.

//! Phase 11 (Local Congregant Companion View): a tiny, LAN-only,
//! read-only HTTP server that mirrors whatever CIP's Stage display is
//! currently showing to a congregant's phone browser - no app to
//! install, no cloud, never leaves the LAN. See `docs/phase-11-audit.md`
//! for the full design record and `docs/congregant-companion.md` for the
//! permanent reference.
//!
//! Deliberately Tauri-agnostic: [`spawn_server`]/[`stop_server`] operate
//! on a plain `Arc<Mutex<Option<CompanionSnapshot>>>`, never an
//! `AppHandle`/`State` - unlike almost everything else in this codebase,
//! that makes the actual server logic directly testable with a real
//! `TcpListener` on an OS-assigned port and real `TcpStream` requests,
//! not just pure-function proxies for untestable Tauri commands (see
//! this module's own tests). The thin Tauri-specific wiring
//! (`enable`/`disable`/`status`/`update_snapshot`, each taking an
//! `&AppHandle`) stays in this module too, but is a thin wrapper over
//! the tested core, matching `presentation.rs`/`presentation_display.rs`'s
//! own split.
//!
//! This server is long-running, unlike every other worker thread this
//! codebase already has (`production.rs`'s fire-and-forget OBS/vMix
//! pushes) - it needs a real stop mechanism, not just "let it finish."
//! [`stop_server`] sets a shared `AtomicBool` and then connects to the
//! listener itself once, the standard dependency-free way to unblock a
//! thread blocked in `TcpListener::accept()` so it can observe the flag
//! and exit.

use chrono::{DateTime, Utc};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use thiserror::Error;

use crate::state::AppState;
use cip_presentation_renderer::RenderedSlide;

/// Fixed port for the companion server - from IANA's dynamic/private
/// range (49152-65535), chosen to avoid colliding with common dev-server
/// ports (3000/5000/8000/8080) an operator's machine might also be
/// running. See `docs/phase-11-audit.md`'s "Fixed port, no TLS."
pub const COMPANION_PORT: u16 = 49876;

#[derive(Debug, Error)]
pub enum CompanionError {
    #[error("could not start the congregant companion server on port {0}: {1}")]
    BindFailed(u16, String),
}

/// A plain, Tauri-agnostic snapshot of "what CIP currently displays" -
/// deliberately the same shape as [`RenderedSlide`] rather than that
/// type itself, so this module never needs `cip_presentation_renderer`
/// to add `Serialize`/`Clone` derives it doesn't otherwise need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanionSnapshot {
    pub heading: String,
    pub body_lines: Vec<String>,
    pub footer: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl CompanionSnapshot {
    pub fn from_slide(slide: &RenderedSlide) -> Self {
        CompanionSnapshot {
            heading: slide.heading.clone(),
            body_lines: slide.body_lines.clone(),
            footer: slide.footer.clone(),
            updated_at: Utc::now(),
        }
    }
}

/// Shared cell the HTTP server thread reads from and
/// [`update_snapshot`]/commands write to - `Arc` so it can be cloned into
/// the spawned thread while `AppState` keeps its own handle.
pub type SharedSnapshot = Arc<Mutex<Option<CompanionSnapshot>>>;

/// A running server's stop handle - `Arc<AtomicBool>` so [`stop_server`]
/// can be called from a different thread than the one that called
/// [`spawn_server`] (always true here: a Tauri command handler stops a
/// server a prior command handler started).
pub struct CompanionServerHandle {
    stop_flag: Arc<AtomicBool>,
    pub port: u16,
}

/// The operator-facing status returned by `enable`/`disable`/`status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanionStatus {
    pub running: bool,
    pub port: u16,
    /// Candidate `http://` URLs a phone on the same LAN could open - may
    /// be empty (never fabricated) if no local network route could be
    /// detected. See [`detect_local_ip`].
    pub urls: Vec<String>,
}

/// Binds a `TcpListener` on `port` (`0` lets the OS assign an ephemeral
/// port - used by this module's own tests) and spawns the accept loop on
/// a dedicated thread. Returns immediately; the handle's real bound port
/// is always populated from the listener itself, never the requested
/// value, so a `0` caller can read back what was actually bound.
pub fn spawn_server(
    snapshot: SharedSnapshot,
    port: u16,
) -> Result<CompanionServerHandle, CompanionError> {
    let listener = TcpListener::bind(("0.0.0.0", port))
        .map_err(|e| CompanionError::BindFailed(port, e.to_string()))?;
    let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_thread = stop_flag.clone();

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if stop_flag_thread.load(Ordering::SeqCst) {
                break;
            }
            if let Ok(stream) = stream {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                handle_connection(stream, &snapshot);
            }
        }
    });

    Ok(CompanionServerHandle {
        stop_flag,
        port: bound_port,
    })
}

/// Stops a server started by [`spawn_server`] - sets the shared flag and
/// wakes the blocking `accept()` call with a single self-connection so
/// the accept loop can observe the flag and return, dropping (and so
/// closing) the listener.
pub fn stop_server(handle: &CompanionServerHandle) {
    handle.stop_flag.store(true, Ordering::SeqCst);
    let _ = TcpStream::connect(("127.0.0.1", handle.port));
}

fn handle_connection(stream: TcpStream, snapshot: &SharedSnapshot) {
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    // Drain and discard headers up to the blank line - this server never
    // reads a request body (both routes are GET-only).
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if line == "\r\n" || line == "\n" => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }

    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");

    let mut stream = stream;
    let response = match path {
        "/" => http_ok("text/html; charset=utf-8", companion_page_html()),
        "/api/current" => {
            let current = snapshot.lock().expect("companion snapshot mutex poisoned");
            http_ok("application/json", current_json(current.as_ref()))
        }
        _ => http_not_found(),
    };
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn http_ok(content_type: &str, body: String) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        content_type,
        body.len(),
        body
    )
}

fn http_not_found() -> String {
    let body = "not found";
    format!(
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// Builds the exact JSON body served at `/api/current` - a pure function
/// so it is directly unit-testable without a real socket.
fn current_json(snapshot: Option<&CompanionSnapshot>) -> String {
    match snapshot {
        None => serde_json::json!({
            "active": false,
            "heading": null,
            "bodyLines": [],
            "footer": null,
            "updatedAt": null,
        })
        .to_string(),
        Some(s) => serde_json::json!({
            "active": true,
            "heading": s.heading,
            "bodyLines": s.body_lines,
            "footer": s.footer,
            "updatedAt": s.updated_at.to_rfc3339(),
        })
        .to_string(),
    }
}

/// The single-page HTML this server ever serves at `GET /` - fully
/// self-contained (inline CSS/JS, no external assets), so it works with
/// zero network access beyond the LAN connection to this server itself.
/// Polls `/api/current` every two seconds; the notes textarea is
/// `localStorage`-only and is never sent anywhere - see the disclosure
/// text baked into the page itself (asserted on directly by this
/// module's own tests, not just documented separately).
fn companion_page_html() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Follow Along</title>
<style>
  :root { color-scheme: light dark; }
  body { font-family: -apple-system, system-ui, sans-serif; margin: 0; padding: 1.25rem;
         background: #111; color: #f5f5f5; line-height: 1.5; }
  h1 { font-size: 1.1rem; opacity: 0.7; margin: 0 0 1rem; font-weight: 600; }
  #current { min-height: 6rem; }
  .heading { font-size: 1.4rem; font-weight: 700; margin: 0 0 0.5rem; }
  .body-line { font-size: 1.15rem; margin: 0 0 0.4rem; }
  .footer { font-size: 0.95rem; opacity: 0.75; margin-top: 0.5rem; }
  .empty { opacity: 0.6; font-style: italic; }
  textarea { width: 100%; box-sizing: border-box; min-height: 8rem; margin-top: 1.5rem;
             font-size: 1rem; padding: 0.6rem; border-radius: 0.4rem; border: 1px solid #555;
             background: #1c1c1c; color: #f5f5f5; }
  .notes-label { font-size: 0.85rem; opacity: 0.7; margin-top: 1.5rem; }
  .disclosure { font-size: 0.8rem; opacity: 0.55; margin-top: 0.4rem; }
  button { margin-top: 0.5rem; padding: 0.4rem 0.8rem; }
</style>
</head>
<body>
<h1>Now displaying</h1>
<div id="current"><p class="empty">Loading&hellip;</p></div>

<p class="notes-label">Your notes</p>
<textarea id="notes" placeholder="Write anything you want to remember&hellip;"></textarea>
<div class="disclosure">Notes stay on this device only &mdash; Church Intelligence Platform never receives them, saved only on your phone.</div>
<button id="clear-notes" type="button">Clear notes</button>

<script>
(function () {
  var notesKey = "cip-companion-notes";
  var notes = document.getElementById("notes");
  try {
    notes.value = localStorage.getItem(notesKey) || "";
  } catch (e) {}
  notes.addEventListener("input", function () {
    try { localStorage.setItem(notesKey, notes.value); } catch (e) {}
  });
  document.getElementById("clear-notes").addEventListener("click", function () {
    notes.value = "";
    try { localStorage.removeItem(notesKey); } catch (e) {}
  });

  var current = document.getElementById("current");
  function render(data) {
    if (!data.active) {
      current.innerHTML = '<p class="empty">Nothing is currently being displayed.</p>';
      return;
    }
    var html = '<p class="heading"></p>';
    var frag = document.createElement("div");
    var h = document.createElement("p");
    h.className = "heading";
    h.textContent = data.heading || "";
    frag.appendChild(h);
    (data.bodyLines || []).forEach(function (line) {
      var p = document.createElement("p");
      p.className = "body-line";
      p.textContent = line;
      frag.appendChild(p);
    });
    if (data.footer) {
      var f = document.createElement("p");
      f.className = "footer";
      f.textContent = data.footer;
      frag.appendChild(f);
    }
    current.innerHTML = "";
    current.appendChild(frag);
  }

  function poll() {
    fetch("/api/current", { cache: "no-store" })
      .then(function (r) { return r.json(); })
      .then(render)
      .catch(function () {});
  }
  poll();
  setInterval(poll, 2000);
})();
</script>
</body>
</html>
"#
    .to_string()
}

/// Best-effort detection of a LAN-facing local IPv4 address, using the
/// standard dependency-free `UdpSocket::connect` routing-table trick:
/// `connect` on a UDP socket never transmits a packet, it only asks the
/// OS which local interface/address would be used to reach the given
/// remote address - so this makes no network request and works fully
/// offline as long as *some* route (even to a private gateway) is
/// configured. Returns `None`, honestly, on a machine with no route at
/// all, rather than fabricating an address.
fn detect_local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}

/// Starts the companion server if not already running (idempotent -
/// returns the existing status unchanged if it is), gates enforced by
/// the caller (`commands::enable_congregant_companion`).
pub fn enable(app: &AppHandle) -> Result<CompanionStatus, CompanionError> {
    let state = app.state::<AppState>();
    let mut server = state
        .companion_server
        .lock()
        .expect("companion_server mutex poisoned");
    if server.is_none() {
        let handle = spawn_server(state.companion_snapshot.clone(), COMPANION_PORT)?;
        *server = Some(handle);
    }
    let port = server.as_ref().map(|h| h.port).unwrap_or(COMPANION_PORT);
    drop(server);
    Ok(build_status(true, port))
}

/// Stops the companion server if running - a safe no-op if it wasn't.
pub fn disable(app: &AppHandle) -> CompanionStatus {
    let state = app.state::<AppState>();
    let mut server = state
        .companion_server
        .lock()
        .expect("companion_server mutex poisoned");
    if let Some(handle) = server.take() {
        stop_server(&handle);
    }
    drop(server);
    build_status(false, COMPANION_PORT)
}

/// Current status without changing anything - safe to call from any
/// logged-in operator, unlike `enable`/`disable`.
pub fn status(app: &AppHandle) -> CompanionStatus {
    let state = app.state::<AppState>();
    let server = state
        .companion_server
        .lock()
        .expect("companion_server mutex poisoned");
    let running = server.is_some();
    let port = server.as_ref().map(|h| h.port).unwrap_or(COMPANION_PORT);
    drop(server);
    build_status(running, port)
}

fn build_status(running: bool, port: u16) -> CompanionStatus {
    let urls = if running {
        detect_local_ip()
            .map(|ip| vec![format!("http://{ip}:{port}/")])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    CompanionStatus {
        running,
        port,
        urls,
    }
}

/// Updates what the companion server broadcasts - called from
/// `commands::display_presentation` (with `Some(slide)`) and
/// `commands::clear_active_presentation` (with `None`), the exact same
/// two call sites `production::push_to_configured_targets` already
/// hooks into, so the companion view and any configured OBS/vMix target
/// always change in lockstep with Stage.
pub fn update_snapshot(app: &AppHandle, slide: Option<&RenderedSlide>) {
    let state = app.state::<AppState>();
    let mut current = state
        .companion_snapshot
        .lock()
        .expect("companion_snapshot mutex poisoned");
    *current = slide.map(CompanionSnapshot::from_slide);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn http_get(port: u16, path: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let status_line = response.lines().next().unwrap_or("");
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let body = response.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    fn a_slide() -> RenderedSlide {
        RenderedSlide {
            template: "scripture-default".into(),
            heading: "JHN 3:16".into(),
            body_lines: vec!["For God so loved the world...".into()],
            footer: Some("ESV".into()),
        }
    }

    #[test]
    fn current_json_reports_inactive_when_nothing_is_displayed() {
        let json = current_json(None);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["active"], false);
        assert!(parsed["bodyLines"].as_array().unwrap().is_empty());
    }

    #[test]
    fn current_json_reports_active_content_when_present() {
        let snapshot = CompanionSnapshot::from_slide(&a_slide());
        let json = current_json(Some(&snapshot));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["active"], true);
        assert_eq!(parsed["heading"], "JHN 3:16");
        assert_eq!(parsed["bodyLines"][0], "For God so loved the world...");
        assert_eq!(parsed["footer"], "ESV");
    }

    #[test]
    fn companion_page_discloses_local_only_notes_storage() {
        let html = companion_page_html();
        assert!(html.contains("saved only on your phone"));
        assert!(html.contains("localStorage"));
    }

    #[test]
    fn real_server_serves_html_at_root() {
        let snapshot: SharedSnapshot = Arc::new(Mutex::new(None));
        let handle = spawn_server(snapshot, 0).expect("bind");
        let (status, body) = http_get(handle.port, "/");
        assert_eq!(status, 200);
        assert!(body.contains("Now displaying"));
        assert!(body.contains("saved only on your phone"));
        stop_server(&handle);
    }

    #[test]
    fn real_server_serves_current_json_and_reflects_live_updates() {
        let snapshot: SharedSnapshot = Arc::new(Mutex::new(None));
        let handle = spawn_server(snapshot.clone(), 0).expect("bind");

        let (status, body) = http_get(handle.port, "/api/current");
        assert_eq!(status, 200);
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["active"], false);

        *snapshot.lock().unwrap() = Some(CompanionSnapshot::from_slide(&a_slide()));

        let (status2, body2) = http_get(handle.port, "/api/current");
        assert_eq!(status2, 200);
        let parsed2: serde_json::Value = serde_json::from_str(&body2).unwrap();
        assert_eq!(parsed2["active"], true);
        assert_eq!(parsed2["heading"], "JHN 3:16");

        stop_server(&handle);
    }

    #[test]
    fn real_server_returns_404_for_unknown_path() {
        let snapshot: SharedSnapshot = Arc::new(Mutex::new(None));
        let handle = spawn_server(snapshot, 0).expect("bind");
        let (status, _) = http_get(handle.port, "/nope");
        assert_eq!(status, 404);
        stop_server(&handle);
    }

    #[test]
    fn stop_server_actually_stops_accepting_connections() {
        let snapshot: SharedSnapshot = Arc::new(Mutex::new(None));
        let handle = spawn_server(snapshot, 0).expect("bind");
        let port = handle.port;
        stop_server(&handle);

        let mut refused = false;
        for _ in 0..50 {
            if TcpStream::connect(("127.0.0.1", port)).is_err() {
                refused = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            refused,
            "expected the port to stop accepting connections after stop_server"
        );
    }

    #[test]
    fn spawn_server_reports_the_actual_bound_port_not_the_requested_zero() {
        let snapshot: SharedSnapshot = Arc::new(Mutex::new(None));
        let handle = spawn_server(snapshot, 0).expect("bind");
        assert_ne!(handle.port, 0);
        stop_server(&handle);
    }

    #[test]
    fn build_status_reports_stopped_with_no_urls() {
        let status = build_status(false, COMPANION_PORT);
        assert!(!status.running);
        assert!(status.urls.is_empty());
        assert_eq!(status.port, COMPANION_PORT);
    }
}

//! vMix integration - Phase 8 (Production Integration).
//!
//! A real vMix HTTP API client (<https://www.vmix.com/help27/index.htm?DeveloperAPI.html>):
//! a plain `GET http://host:port/api/?Function=SetText&Input=...&Value=...`
//! request (optionally `&SelectedName=...` to target one named text layer
//! inside a Title, rather than its first/default text field), pushing
//! CIP's currently-displayed slide text into an operator-configured vMix
//! title. Plain `http://` only - vMix runs on the same machine/LAN as the
//! media team, so no TLS backend is pulled in at all (see
//! `docs/phase-8-audit.md`'s "Protocol choice").
//!
//! Deliberately narrow in scope, matching `integrations/obs`'s own
//! discipline exactly: this crate never switches inputs, never controls
//! recording/streaming, and a connection is opened fresh for every push.
//!
//! **Honest limitation, unique to vMix's own API**: unlike
//! `obs-websocket`'s structured `RequestStatus` (which tells the caller
//! *why* a request failed - e.g. "no source by that name"), vMix's
//! `SetText` endpoint returns a bare HTTP 200 whether or not `Input`/
//! `SelectedName` actually named something real inside vMix - this is a
//! property of vMix's own API surface, not a gap in this client. A
//! successful [`push_text`] therefore proves vMix accepted and processed
//! the HTTP request, not that the named input/field exists.

use ureq::Error as UreqError;

/// Where and how to reach one vMix instance, and which title/text field
/// to update. `selected_name` targets one named text layer inside a
/// Title input (e.g. `"Heading.Text"`); `None` updates the input's first/
/// default text field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmixTarget {
    pub host: String,
    pub port: u16,
    pub input: String,
    pub selected_name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum VmixError {
    #[error("could not reach vMix at {0}: {1}")]
    Connect(String, String),
    #[error("vMix returned an unexpected HTTP status {0}")]
    UnexpectedStatus(u16),
}

/// Sends one `SetText` request to `target`, updating its input's text to
/// `text`. Blocking - callers on an async runtime should run this on a
/// dedicated thread (see `apps/desktop/src-tauri/src/production.rs`).
pub fn push_text(target: &VmixTarget, text: &str) -> Result<(), VmixError> {
    let url = format!("http://{}:{}/api/", target.host, target.port);

    let mut request = ureq::get(&url)
        .query("Function", "SetText")
        .query("Input", &target.input)
        .query("Value", text);
    if let Some(selected_name) = &target.selected_name {
        request = request.query("SelectedName", selected_name);
    }

    match request.call() {
        Ok(_response) => Ok(()),
        Err(UreqError::Status(code, _response)) => Err(VmixError::UnexpectedStatus(code)),
        Err(UreqError::Transport(e)) => Err(VmixError::Connect(url, e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    /// A minimal, real HTTP/1.1 server (hand-rolled over `TcpListener`,
    /// no server-framework dependency added for this) that returns a
    /// given status code and records the exact request line it received -
    /// proves this crate's client against real HTTP bytes, and lets tests
    /// assert on the real query string vMix would have seen. See
    /// `docs/phase-8-audit.md`'s "Testing boundary".
    fn spawn_fake_vmix(status_line: &'static str) -> (u16, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake vMix listener");
        let port = listener.local_addr().expect("local_addr").port();
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).expect("read request");
            let request_text = String::from_utf8_lossy(&buf[..n]).to_string();
            let request_line = request_text.lines().next().unwrap_or("").to_string();
            let _ = tx.send(request_line);

            let body = "OK";
            let response = format!(
                "{status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });

        (port, rx)
    }

    #[test]
    fn push_text_succeeds_against_a_real_http_200_and_sends_the_right_query_string() {
        let (port, rx) = spawn_fake_vmix("HTTP/1.1 200 OK");
        let target = VmixTarget {
            host: "127.0.0.1".to_string(),
            port,
            input: "LowerThird".to_string(),
            selected_name: None,
        };

        push_text(&target, "Romans 8:28").expect("push should succeed");

        let request_line = rx.recv().expect("fake vMix should have received a request");
        assert!(request_line.contains("Function=SetText"));
        assert!(request_line.contains("Input=LowerThird"));
        // ureq percent-encodes the space and colon.
        assert!(request_line.contains("Value=Romans"));
    }

    #[test]
    fn push_text_includes_selected_name_when_configured() {
        let (port, rx) = spawn_fake_vmix("HTTP/1.1 200 OK");
        let target = VmixTarget {
            host: "127.0.0.1".to_string(),
            port,
            input: "1".to_string(),
            selected_name: Some("Heading.Text".to_string()),
        };

        push_text(&target, "John 3:16").expect("push should succeed");

        let request_line = rx.recv().expect("fake vMix should have received a request");
        assert!(request_line.contains("SelectedName=Heading.Text"));
    }

    #[test]
    fn push_text_surfaces_a_non_2xx_status_honestly() {
        let (port, _rx) = spawn_fake_vmix("HTTP/1.1 500 Internal Server Error");
        let target = VmixTarget {
            host: "127.0.0.1".to_string(),
            port,
            input: "LowerThird".to_string(),
            selected_name: None,
        };

        let err = push_text(&target, "Romans 8:28").expect_err("500 should be an error");
        match err {
            VmixError::UnexpectedStatus(code) => assert_eq!(code, 500),
            other => panic!("expected UnexpectedStatus, got {other:?}"),
        }
    }

    #[test]
    fn push_text_reports_a_clear_error_when_nothing_is_listening() {
        let target = VmixTarget {
            host: "127.0.0.1".to_string(),
            port: 1, // reserved, nothing ever listens here
            input: "LowerThird".to_string(),
            selected_name: None,
        };

        let err = push_text(&target, "Romans 8:28").expect_err("should fail to connect");
        assert!(matches!(err, VmixError::Connect(_, _)));
    }
}

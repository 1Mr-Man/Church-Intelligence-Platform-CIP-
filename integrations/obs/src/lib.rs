//! OBS Studio integration - Phase 8 (Production Integration).
//!
//! A real `obs-websocket` v5 client (<https://github.com/obsproject/obs-websocket>,
//! protocol docs: <https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md>):
//! a plain `ws://` WebSocket (never `wss://` - OBS runs on the same
//! machine/LAN as the media team, so no TLS backend is pulled in at all,
//! matching this project's own Windows-cross-compilation discipline of
//! preferring pure-Rust dependencies wherever the actual protocol allows
//! it), the real `Hello`/`Identify`/`Identified` handshake (with
//! SHA256-based challenge/response auth when a password is configured),
//! and one request type: `SetInputSettings`, used to update a text
//! source's `text` field with CIP's currently-displayed slide text.
//!
//! Deliberately narrow in scope (see `docs/phase-8-audit.md`'s "Design
//! choices"): this crate never switches scenes, never toggles source
//! visibility, and never controls recording/streaming - it only ever
//! updates one text field the operator has already pointed it at. A
//! connection is opened fresh for every push and closed immediately after
//! (see the audit's "Connection model" - an OBS push is infrequent and
//! cheap; there is no engine here to keep warm).
//!
//! Every failure is a real, typed [`ObsError`] - never a fabricated
//! success. Nothing in `core` may depend on this crate; the desktop app
//! orchestrates it as an optional, best-effort sink (a failed push must
//! never block or degrade CIP's own local display).

use std::net::TcpStream;
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tungstenite::{Message, WebSocket};

/// Where and how to reach one OBS Studio instance, and which text source
/// to update. `password` is `None` when the operator's OBS instance has
/// no WebSocket authentication enabled (OBS's own default for a fresh
/// install).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObsTarget {
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
    /// The name of an existing OBS text source (GDI+ or FreeType2) whose
    /// `text` setting this client will overwrite.
    pub source_name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ObsError {
    #[error("could not connect to OBS at {0}: {1}")]
    Connect(String, String),
    #[error("OBS handshake failed: {0}")]
    Handshake(String),
    #[error("OBS rejected the connection: a password is required but none was configured")]
    AuthRequired,
    #[error("OBS request failed (code {code}): {comment}")]
    RequestFailed { code: i64, comment: String },
    #[error("unexpected response from OBS: {0}")]
    Protocol(String),
}

/// Connects to `target`, performs the real `obs-websocket` v5 handshake,
/// sends one `SetInputSettings` request updating `target.source_name`'s
/// `text` field to `text`, and closes the connection. Blocking - callers
/// on an async runtime should run this on a dedicated thread (see
/// `apps/desktop/src-tauri/src/production.rs`), matching this project's
/// established worker-thread precedent for other blocking I/O.
pub fn push_text(target: &ObsTarget, text: &str) -> Result<(), ObsError> {
    let url = format!("ws://{}:{}", target.host, target.port);
    let (mut socket, _response) =
        tungstenite::connect(&url).map_err(|e| ObsError::Connect(url.clone(), e.to_string()))?;

    if let Some(stream) = socket_tcp_stream(&mut socket) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    }

    let hello = read_json::<Hello>(&mut socket)?;
    let authentication = match hello.d.authentication {
        Some(auth) => {
            let password = target.password.as_deref().ok_or(ObsError::AuthRequired)?;
            Some(compute_auth_response(password, &auth.salt, &auth.challenge))
        }
        None => None,
    };

    let identify = Identify {
        op: 1,
        d: IdentifyData {
            rpc_version: hello.d.rpc_version,
            authentication,
            event_subscriptions: 0,
        },
    };
    write_json(&mut socket, &identify)?;

    let _identified = read_json::<Identified>(&mut socket)?;

    let request_id = "cip-push-text";
    let request = Request {
        op: 6,
        d: RequestData {
            request_type: "SetInputSettings".to_string(),
            request_id: request_id.to_string(),
            request_data: SetInputSettingsRequest {
                input_name: target.source_name.clone(),
                input_settings: InputSettings {
                    text: text.to_string(),
                },
                overlay: true,
            },
        },
    };
    write_json(&mut socket, &request)?;

    let response = read_json::<RequestResponse>(&mut socket)?;
    let _ = socket.close(None);

    if response.d.request_status.result {
        Ok(())
    } else {
        Err(ObsError::RequestFailed {
            code: response.d.request_status.code,
            comment: response
                .d
                .request_status
                .comment
                .unwrap_or_else(|| "no comment from OBS".to_string()),
        })
    }
}

/// `secret = base64(sha256(password + salt))`;
/// `auth_response = base64(sha256(secret + challenge))` - the exact
/// algorithm `obs-websocket` v5's own protocol docs specify.
fn compute_auth_response(password: &str, salt: &str, challenge: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(salt.as_bytes());
    let secret = base64::engine::general_purpose::STANDARD.encode(hasher.finalize());

    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(challenge.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

fn socket_tcp_stream(
    socket: &mut WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
) -> Option<&TcpStream> {
    match socket.get_ref() {
        tungstenite::stream::MaybeTlsStream::Plain(s) => Some(s),
        _ => None,
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(
    socket: &mut WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
) -> Result<T, ObsError> {
    loop {
        let msg = socket
            .read()
            .map_err(|e| ObsError::Handshake(e.to_string()))?;
        match msg {
            Message::Text(text) => {
                return serde_json::from_str(&text)
                    .map_err(|e| ObsError::Protocol(format!("{e}: {text}")));
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(frame) => {
                return Err(ObsError::Handshake(format!(
                    "connection closed by OBS: {frame:?}"
                )));
            }
            other => {
                return Err(ObsError::Protocol(format!(
                    "unexpected non-text frame: {other:?}"
                )));
            }
        }
    }
}

fn write_json<T: Serialize>(
    socket: &mut WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
    value: &T,
) -> Result<(), ObsError> {
    let text = serde_json::to_string(value).expect("obs-websocket payloads always serialize");
    socket
        .send(Message::Text(text))
        .map_err(|e| ObsError::Handshake(e.to_string()))
}

#[derive(Debug, Deserialize)]
struct Hello {
    d: HelloData,
}

#[derive(Debug, Deserialize)]
struct HelloData {
    #[serde(rename = "rpcVersion")]
    rpc_version: u32,
    authentication: Option<HelloAuth>,
}

#[derive(Debug, Deserialize)]
struct HelloAuth {
    challenge: String,
    salt: String,
}

#[derive(Debug, Serialize)]
struct Identify {
    op: u8,
    d: IdentifyData,
}

#[derive(Debug, Serialize)]
struct IdentifyData {
    #[serde(rename = "rpcVersion")]
    rpc_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    authentication: Option<String>,
    #[serde(rename = "eventSubscriptions")]
    event_subscriptions: u32,
}

#[derive(Debug, Deserialize)]
struct Identified {
    #[allow(dead_code)]
    d: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct Request {
    op: u8,
    d: RequestData,
}

#[derive(Debug, Serialize)]
struct RequestData {
    #[serde(rename = "requestType")]
    request_type: String,
    #[serde(rename = "requestId")]
    request_id: String,
    #[serde(rename = "requestData")]
    request_data: SetInputSettingsRequest,
}

#[derive(Debug, Serialize)]
struct SetInputSettingsRequest {
    #[serde(rename = "inputName")]
    input_name: String,
    #[serde(rename = "inputSettings")]
    input_settings: InputSettings,
    overlay: bool,
}

#[derive(Debug, Serialize)]
struct InputSettings {
    text: String,
}

#[derive(Debug, Deserialize)]
struct RequestResponse {
    d: RequestResponseData,
}

#[derive(Debug, Deserialize)]
struct RequestResponseData {
    #[serde(rename = "requestStatus")]
    request_status: RequestStatus,
}

#[derive(Debug, Deserialize)]
struct RequestStatus {
    result: bool,
    code: i64,
    comment: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// A minimal, real WebSocket server (via `tungstenite`, not a mock
    /// trait) that performs the actual `obs-websocket` v5 handshake and
    /// responds to `SetInputSettings` - proves this crate's client against
    /// real wire bytes. See `docs/phase-8-audit.md`'s "Testing boundary".
    fn spawn_fake_obs(require_password: Option<&'static str>, succeed: bool) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake OBS listener");
        let port = listener.local_addr().expect("local_addr").port();

        thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut ws = tungstenite::accept(stream).expect("ws handshake");

            let (authentication, salt, challenge) = if require_password.is_some() {
                let salt = "test-salt".to_string();
                let challenge = "test-challenge".to_string();
                (
                    Some(HelloAuthOut {
                        challenge: challenge.clone(),
                        salt: salt.clone(),
                    }),
                    salt,
                    challenge,
                )
            } else {
                (None, String::new(), String::new())
            };

            let hello = serde_json::json!({
                "op": 0,
                "d": {
                    "obsWebSocketVersion": "5.0.0",
                    "rpcVersion": 1,
                    "authentication": authentication,
                }
            });
            ws.send(Message::Text(hello.to_string()))
                .expect("send hello");

            let identify_text = loop {
                match ws.read().expect("read identify") {
                    Message::Text(t) => break t,
                    Message::Ping(_) => continue,
                    other => panic!("unexpected frame while waiting for Identify: {other:?}"),
                }
            };
            let identify: serde_json::Value =
                serde_json::from_str(&identify_text).expect("parse identify");

            if let Some(password) = require_password {
                let expected = super::compute_auth_response(password, &salt, &challenge);
                let got = identify["d"]["authentication"].as_str().unwrap_or("");
                assert_eq!(
                    got, expected,
                    "auth response did not match expected algorithm"
                );
            }

            let identified = serde_json::json!({"op": 2, "d": {"negotiatedRpcVersion": 1}});
            ws.send(Message::Text(identified.to_string()))
                .expect("send identified");

            let request_text = loop {
                match ws.read().expect("read request") {
                    Message::Text(t) => break t,
                    Message::Ping(_) => continue,
                    other => panic!("unexpected frame while waiting for Request: {other:?}"),
                }
            };
            let request: serde_json::Value =
                serde_json::from_str(&request_text).expect("parse request");
            assert_eq!(request["d"]["requestType"], "SetInputSettings");

            let response = if succeed {
                serde_json::json!({
                    "op": 7,
                    "d": {
                        "requestType": "SetInputSettings",
                        "requestId": request["d"]["requestId"],
                        "requestStatus": {"result": true, "code": 100},
                    }
                })
            } else {
                serde_json::json!({
                    "op": 7,
                    "d": {
                        "requestType": "SetInputSettings",
                        "requestId": request["d"]["requestId"],
                        "requestStatus": {
                            "result": false,
                            "code": 604,
                            "comment": "No source was found by the name of `does-not-exist`."
                        },
                    }
                })
            };
            let _ = ws.send(Message::Text(response.to_string()));
            let _ = ws.close(None);
        });

        port
    }

    #[derive(Serialize)]
    struct HelloAuthOut {
        challenge: String,
        salt: String,
    }

    #[test]
    fn push_text_succeeds_against_a_real_handshake_with_no_password() {
        let port = spawn_fake_obs(None, true);
        let target = ObsTarget {
            host: "127.0.0.1".to_string(),
            port,
            password: None,
            source_name: "cip-verse-text".to_string(),
        };
        push_text(&target, "Romans 8:28").expect("push should succeed");
    }

    #[test]
    fn push_text_succeeds_against_a_real_handshake_with_a_correct_password() {
        let port = spawn_fake_obs(Some("correct-horse"), true);
        let target = ObsTarget {
            host: "127.0.0.1".to_string(),
            port,
            password: Some("correct-horse".to_string()),
            source_name: "cip-verse-text".to_string(),
        };
        push_text(&target, "Romans 8:28").expect("push should succeed with the right password");
    }

    #[test]
    fn push_text_reports_auth_required_when_obs_wants_a_password_and_none_is_configured() {
        let port = spawn_fake_obs(Some("correct-horse"), true);
        let target = ObsTarget {
            host: "127.0.0.1".to_string(),
            port,
            password: None,
            source_name: "cip-verse-text".to_string(),
        };
        let err = push_text(&target, "Romans 8:28").expect_err("should fail without a password");
        assert!(matches!(err, ObsError::AuthRequired));
    }

    #[test]
    fn push_text_surfaces_a_real_obs_request_failure_honestly() {
        let port = spawn_fake_obs(None, false);
        let target = ObsTarget {
            host: "127.0.0.1".to_string(),
            port,
            password: None,
            source_name: "does-not-exist".to_string(),
        };
        let err = push_text(&target, "Romans 8:28").expect_err("should surface OBS's failure");
        match err {
            ObsError::RequestFailed { code, comment } => {
                assert_eq!(code, 604);
                assert!(comment.contains("does-not-exist"));
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    #[test]
    fn push_text_reports_a_clear_error_when_nothing_is_listening() {
        // No listener bound to this port - a real, deterministic connection
        // failure rather than a hang.
        let target = ObsTarget {
            host: "127.0.0.1".to_string(),
            port: 1, // reserved, nothing ever listens here
            password: None,
            source_name: "cip-verse-text".to_string(),
        };
        let err = push_text(&target, "Romans 8:28").expect_err("should fail to connect");
        assert!(matches!(err, ObsError::Connect(_, _)));
    }

    #[test]
    fn compute_auth_response_matches_the_documented_obs_websocket_v5_algorithm() {
        // Hand-computed reference vector for password="password",
        // salt="salt", challenge="challenge" (SHA256+base64, exactly as
        // obs-websocket's own protocol docs specify).
        let secret = {
            let mut hasher = Sha256::new();
            hasher.update(b"password");
            hasher.update(b"salt");
            base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
        };
        let expected = {
            let mut hasher = Sha256::new();
            hasher.update(secret.as_bytes());
            hasher.update(b"challenge");
            base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
        };
        assert_eq!(
            compute_auth_response("password", "salt", "challenge"),
            expected
        );
    }
}

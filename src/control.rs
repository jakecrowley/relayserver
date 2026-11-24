use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use serde::Serialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::{
    config::RelayConfig,
    relay::Relay,
    util::ResourceState,
};

const MAX_REQUEST_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct ControlState {
    pub relay: Relay,
    pub config_path: PathBuf,
    pub pending_restart: Arc<AtomicBool>,
    pub restart_fn: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
    error: Option<String>,
    retry_in_secs: Option<u64>,
    code: Option<String>,
    connected_since: Option<u64>,
    pid: u32,
    pending_restart: bool,
    config: RelayConfig,
}

pub async fn start_control_server(
    addr: &str,
    state: ControlState,
) -> std::io::Result<std::net::SocketAddr> {
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    println!("Control server listening on http://{local_addr}");

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let state = state.clone();
                    tokio::spawn(async move {
                        let _ = handle_connection(stream, state).await;
                    });
                }
                Err(err) => {
                    eprintln!("Control server accept error: {err}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    });

    Ok(local_addr)
}

async fn handle_connection(mut stream: TcpStream, state: ControlState) -> std::io::Result<()> {
    let Some(req) = read_http_request(&mut stream).await? else {
        return Ok(());
    };

    let path = req.path.split('?').next().unwrap_or(&req.path);

    let (status_line, body, trigger_restart) = match (req.method.as_str(), path) {
        ("GET", "/status") => {
            let resp = build_status(&state).await;
            (
                "200 OK",
                serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string()),
                false,
            )
        }
        ("GET", "/config") => {
            let cfg = current_config(&state).await;
            (
                "200 OK",
                serde_json::to_string(&cfg).unwrap_or_else(|_| "{}".to_string()),
                false,
            )
        }
        ("PUT", "/config") | ("POST", "/config") => match serde_json::from_slice::<RelayConfig>(&req.body) {
            Ok(incoming) => match incoming.normalized_and_validated() {
                Ok(validated) => match validated.save_atomic(&state.config_path) {
                    Ok(()) => {
                        state.pending_restart.store(true, Ordering::SeqCst);
                        let payload = serde_json::json!({
                            "ok": true,
                            "pending_restart": true,
                            "config": validated,
                        });
                        ("200 OK", payload.to_string(), false)
                    }
                    Err(e) => (
                        "500 Internal Server Error",
                        serde_json::json!({ "error": format!("Failed to write config: {e}") }).to_string(),
                        false,
                    ),
                },
                Err(msg) => (
                    "400 Bad Request",
                    serde_json::json!({ "error": msg }).to_string(),
                    false,
                ),
            },
            Err(e) => (
                "400 Bad Request",
                serde_json::json!({ "error": format!("Invalid JSON: {e}") }).to_string(),
                false,
            ),
        },
        ("POST", "/restart") => (
            "200 OK",
            serde_json::json!({ "ok": true, "restarting": true }).to_string(),
            true,
        ),
        _ => (
            "404 Not Found",
            serde_json::json!({ "error": "Not found" }).to_string(),
            false,
        ),
    };

    let response = format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    if trigger_restart {
        let restart_fn = state.restart_fn.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            (restart_fn)();
        });
    }

    Ok(())
}

async fn current_config(state: &ControlState) -> RelayConfig {
    if let Some(cfg) = RelayConfig::load_sync(&state.config_path) {
        return cfg;
    }
    RelayConfig::from_relay(&state.relay).await
}

async fn build_status(state: &ControlState) -> StatusResponse {
    let rs = state.relay.resource_state.lock().await.clone();
    let (status, error, retry_in_secs) = match rs {
        ResourceState::Generated => ("connected", None, None),
        ResourceState::Generating => ("connecting", None, None),
        ResourceState::Failed(f) => ("failed", Some(f.error.to_string()), f.retry_wait),
    };

    let live_code = state
        .relay
        .state
        .lock()
        .await
        .as_ref()
        .map(|s| s.code.clone());
    let connected_since = state.relay.connected_since_unix().await;
    let config = current_config(state).await;

    StatusResponse {
        status,
        error,
        retry_in_secs,
        code: live_code,
        connected_since,
        pid: std::process::id(),
        pending_restart: state.pending_restart.load(Ordering::SeqCst),
        config,
    }
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

async fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Option<HttpRequest>> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];

    let header_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
        if buf.len() > MAX_REQUEST_BYTES {
            return Ok(None);
        }
    };

    let header_str = String::from_utf8_lossy(&buf[..header_end]);
    let mut lines = header_str.split("\r\n");
    let request_line = match lines.next() {
        Some(l) => l,
        None => return Ok(None),
    };
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    let mut content_length: usize = 0;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                if let Ok(len) = v.trim().parse::<usize>() {
                    content_length = len.min(MAX_REQUEST_BYTES);
                }
            }
        }
    }

    let body_start = header_end + 4;
    let mut body = buf[body_start..].to_vec();
    while body.len() < content_length {
        let remaining = content_length - body.len();
        let to_read = remaining.min(chunk.len());
        let n = stream.read(&mut chunk[..to_read]).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);

    Ok(Some(HttpRequest { method, path, body }))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::{RelayResource, RelayState};

    async fn send_raw_http(addr: std::net::SocketAddr, raw: &str) -> (u16, serde_json::Value) {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(raw.as_bytes()).await.unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).await.unwrap();
        let (head, body) = resp.split_once("\r\n\r\n").unwrap();
        let code: u16 = head
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        (code, json)
    }

    #[tokio::test]
    async fn control_server_status_config_and_restart() {
        let dir = std::env::temp_dir().join(format!(
            "relayserver_ctrl_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.json");

        let initial_cfg = RelayConfig {
            url: "wss://127.0.0.1:1/unreachable".to_string(),
            state: Some(RelayState {
                code: "TEST-CODE".to_string(),
                secret: "TEST-SECRET".to_string(),
            }),
            serial_number: Some("C02KL2YKDNCR".to_string()),
            unique_device_id: Some("AABBCCDD-1234-5678-9ABC-DEF012345678".to_string()),
            use_mac_identity: false,
        };
        initial_cfg.save_atomic(&config_path).unwrap();

        let relay = RelayResource::new(
            initial_cfg.url.clone(),
            initial_cfg.state.clone(),
            "C02KL2YKDNCR".to_string(),
            "AABBCCDD-1234-5678-9ABC-DEF012345678".to_string(),
            "AABBCCDD-1234-5678-9ABC-DEF012345678".to_string(),
            false,
        );

        let restarted = Arc::new(AtomicBool::new(false));
        let restarted_clone = restarted.clone();
        let state = ControlState {
            relay,
            config_path: config_path.clone(),
            pending_restart: Arc::new(AtomicBool::new(false)),
            restart_fn: Arc::new(move || {
                restarted_clone.store(true, Ordering::SeqCst);
            }),
        };

        let addr = start_control_server("127.0.0.1:0", state).await.unwrap();

        // 1. GET /status
        let (code, body) = send_raw_http(addr, "GET /status HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
        assert_eq!(code, 200);
        assert!(matches!(
            body["status"].as_str(),
            Some("connecting") | Some("failed") | Some("connected")
        ));
        assert_eq!(body["code"], "TEST-CODE");
        assert_eq!(body["pending_restart"], false);
        assert_eq!(body["config"]["url"], "wss://127.0.0.1:1/unreachable");

        // 2. PUT /config with invalid URL -> 400
        let bad_body = r#"{"url":"http://not-ws","use_mac_identity":true}"#;
        let req = format!(
            "PUT /config HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            bad_body.len(),
            bad_body
        );
        let (code, _) = send_raw_http(addr, &req).await;
        assert_eq!(code, 400);

        // 3. PUT /config with valid payload -> 200 and persists to disk
        let good_body = serde_json::json!({
            "url": "wss://relay.example.com/api/v1/provider",
            "state": { "code": "NEW-CODE", "secret": "NEW-SECRET" },
            "serial_number": "C02KL2YKDNCR",
            "unique_device_id": "11111111-2222-3333-4444-555555555555",
            "use_mac_identity": true
        })
        .to_string();
        let req = format!(
            "PUT /config HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            good_body.len(),
            good_body
        );
        let (code, body) = send_raw_http(addr, &req).await;
        assert_eq!(code, 200);
        assert_eq!(body["ok"], true);
        assert_eq!(body["pending_restart"], true);

        let disk_cfg = RelayConfig::load_sync(&config_path).unwrap();
        assert_eq!(disk_cfg.url, "wss://relay.example.com/api/v1/provider");
        assert!(disk_cfg.use_mac_identity);
        assert_eq!(disk_cfg.state.unwrap().code, "NEW-CODE");

        // 4. POST /restart triggers restart_fn
        let (code, body) = send_raw_http(addr, "POST /restart HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n").await;
        assert_eq!(code, 200);
        assert_eq!(body["restarting"], true);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(restarted.load(Ordering::SeqCst));

        let _ = std::fs::remove_dir_all(&dir);
    }
}

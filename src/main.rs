mod c;
mod error;
mod nac;
mod relay;
mod util;

use std::sync::Arc;

use base64::engine::general_purpose;
use base64::Engine;
use relay::{generate_hybrid_serial, generate_hybrid_udid, Relay, RelayResource, RelayState};
use serde::{Deserialize, Serialize};
use tokio::{fs, sync::broadcast};


pub fn base64_encode(data: &[u8]) -> String {
    general_purpose::STANDARD.encode(data)
}

#[derive(Serialize, Deserialize)]
struct RelayConfig {
    url: String,
    state: Option<RelayState>,
    #[serde(default)]
    serial_number: Option<String>,
    #[serde(default)]
    unique_device_id: Option<String>,
}

impl RelayConfig {
    async fn from_relay(relay: &Relay) -> RelayConfig {
        RelayConfig {
            url: relay.url.lock().await.clone(),
            state: relay.state.lock().await.clone(),
            serial_number: Some(relay.serial_number.clone()),
            unique_device_id: Some(relay.unique_device_id.clone()),
        }
    }
}

#[tokio::main]
async fn main() {
    let config_path = "/var/mobile/config.json";

    let mut config: Option<RelayConfig> = None;
    if let Ok(read) = fs::read_to_string(config_path).await {
        if let Ok(item) = serde_json::from_str(&read) {
            config = Some(item);
        }
    }

    let url = config
        .as_ref()
        .map(|i| i.url.clone())
        .unwrap_or_else(|| "wss://registration-relay.beeper.com/api/v1/provider".to_string());

    let serial_number = config
        .as_ref()
        .and_then(|i| i.serial_number.clone())
        .filter(|s| s.len() == 12)
        .unwrap_or_else(generate_hybrid_serial);

    let unique_device_id = config
        .as_ref()
        .and_then(|i| i.unique_device_id.clone())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(generate_hybrid_udid);

    println!("Using hybrid identity: serial={}, udid={}", serial_number, unique_device_id);

    let relay = RelayResource::new(
        url,
        config.as_ref().and_then(|i| i.state.clone()),
        serial_number,
        unique_device_id,
    );

    let initial_state = RelayConfig::from_relay(&relay).await;
    if let Err(e) = std::fs::write(config_path, serde_json::to_string_pretty(&initial_state).unwrap()) {
        eprintln!("Failed to write initial config to {}: {}", config_path, e);
    } else {
        println!("Wrote initial config to {}", config_path);
    }

    let mut to_refresh = relay.generated_signal.subscribe();
    let reconn_conn = Arc::downgrade(&relay);
    tokio::spawn(async move {
        loop {
            match to_refresh.recv().await {
                Ok(()) => {
                    let Some(conn) = reconn_conn.upgrade() else { break };
                    // update keys
                    let state = RelayConfig::from_relay(&conn).await;
                    if let Err(e) = std::fs::write(config_path, serde_json::to_string_pretty(&state).unwrap()) {
                        eprintln!("Failed to write updated config to {}: {}", config_path, e);
                    } else {
                        println!("Updated config at {}", config_path);
                    }
                },
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    std::future::pending::<()>().await;
}

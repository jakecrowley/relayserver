mod c;
mod config;
mod control;
mod error;
mod nac;
mod relay;
mod util;

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use base64::engine::general_purpose;
use base64::Engine;
use config::{RelayConfig, DEFAULT_CONFIG_PATH, DEFAULT_CONTROL_ADDR, DEFAULT_RELAY_URL};
use control::{start_control_server, ControlState};
use relay::{generate_hybrid_serial, generate_hybrid_udid, get_persistent_device_uuid, RelayResource};
use tokio::sync::broadcast;

pub fn base64_encode(data: &[u8]) -> String {
    general_purpose::STANDARD.encode(data)
}

#[tokio::main]
async fn main() {
    let config_path = PathBuf::from(
        std::env::var("RELAY_CONFIG_PATH").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string()),
    );
    let control_addr =
        std::env::var("RELAY_CONTROL_ADDR").unwrap_or_else(|_| DEFAULT_CONTROL_ADDR.to_string());

    let config = RelayConfig::load_sync(&config_path);

    let url = config
        .as_ref()
        .map(|i| i.url.clone())
        .unwrap_or_else(|| DEFAULT_RELAY_URL.to_string());

    let serial_number = config
        .as_ref()
        .and_then(|i| i.serial_number.clone())
        .filter(|s| s.len() == 12)
        .unwrap_or_else(generate_hybrid_serial);

    // Client parity (getPersistentDeviceUUID): identifierForVendor wins, else
    // the persisted UUID from config, else a fresh UUID persisted on next write.
    let identifier_for_vendor = c::device_identifier_for_vendor_rs();
    let saved = config.as_ref().and_then(|i| i.unique_device_id.clone());
    let (persistent_device_uuid, needs_uuid_save) =
        get_persistent_device_uuid(Some(&identifier_for_vendor), saved.as_deref());
    if needs_uuid_save {
        println!("Generated persistent device UUID {persistent_device_uuid}; will be saved to config");
    }

    let unique_device_id = if needs_uuid_save {
        persistent_device_uuid.clone()
    } else {
        config
            .as_ref()
            .and_then(|i| i.unique_device_id.clone())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(generate_hybrid_udid)
    };

    let use_mac_identity = config
        .as_ref()
        .map(|i| i.use_mac_identity)
        .unwrap_or(false);

    println!(
        "Using hybrid identity: serial={}, udid={}, use_mac_identity={}",
        serial_number, unique_device_id, use_mac_identity
    );

    let relay = RelayResource::new(
        url,
        config.as_ref().and_then(|i| i.state.clone()),
        serial_number,
        unique_device_id,
        persistent_device_uuid,
        use_mac_identity,
    );

    let initial_state = RelayConfig::from_relay(&relay).await;
    if let Err(e) = initial_state.save_atomic(&config_path) {
        eprintln!(
            "Failed to write initial config to {}: {}",
            config_path.display(),
            e
        );
    } else {
        println!("Wrote initial config to {}", config_path.display());
    }

    let pending_restart = Arc::new(AtomicBool::new(false));

    let control_state = ControlState {
        relay: relay.clone(),
        config_path: config_path.clone(),
        pending_restart: pending_restart.clone(),
        restart_fn: Arc::new(|| {
            println!("Restart requested via control API; exiting for launchd respawn");
            std::process::exit(0);
        }),
    };
    if let Err(e) = start_control_server(&control_addr, control_state).await {
        eprintln!("Failed to start control server on {}: {}", control_addr, e);
    }

    let mut to_refresh = relay.generated_signal.subscribe();
    let reconn_conn = Arc::downgrade(&relay);
    let refresh_config_path = config_path.clone();
    let refresh_pending_restart = pending_restart.clone();
    tokio::spawn(async move {
        loop {
            match to_refresh.recv().await {
                Ok(()) => {
                    let Some(conn) = reconn_conn.upgrade() else { break };
                    if refresh_pending_restart.load(Ordering::SeqCst) {
                        println!(
                            "Skipping config write on reconnect because user edits are pending restart"
                        );
                        continue;
                    }
                    // update keys
                    let state = RelayConfig::from_relay(&conn).await;
                    if let Err(e) = state.save_atomic(&refresh_config_path) {
                        eprintln!(
                            "Failed to write updated config to {}: {}",
                            refresh_config_path.display(),
                            e
                        );
                    } else {
                        println!("Updated config at {}", refresh_config_path.display());
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    std::future::pending::<()>().await;
}

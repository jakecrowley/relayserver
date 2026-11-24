use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::relay::{Relay, RelayState};

pub const DEFAULT_RELAY_URL: &str = "wss://registration-relay.beeper.com/api/v1/provider";
pub const DEFAULT_CONFIG_PATH: &str = "/var/mobile/config.json";
pub const DEFAULT_CONTROL_ADDR: &str = "127.0.0.1:17420";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayConfig {
    pub url: String,
    pub state: Option<RelayState>,
    #[serde(default)]
    pub serial_number: Option<String>,
    #[serde(default)]
    pub unique_device_id: Option<String>,
    #[serde(default)]
    pub use_mac_identity: bool,
}

impl RelayConfig {
    pub async fn from_relay(relay: &Relay) -> RelayConfig {
        RelayConfig {
            url: relay.url.lock().await.clone(),
            state: relay.state.lock().await.clone(),
            serial_number: Some(relay.serial_number.clone()),
            unique_device_id: Some(relay.unique_device_id.clone()),
            use_mac_identity: relay.use_mac_identity,
        }
    }

    pub fn load_sync(path: &Path) -> Option<RelayConfig> {
        let read = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&read).ok()
    }

    pub fn save_atomic(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp_path = path.with_extension("json.tmp");
        if std::fs::write(&tmp_path, &json).is_ok() && std::fs::rename(&tmp_path, path).is_ok() {
            return Ok(());
        }
        let _ = std::fs::remove_file(&tmp_path);
        std::fs::write(path, json)
    }

    /// Trims fields, turns empty optional strings into `None`, and validates the config.
    pub fn normalized_and_validated(&self) -> Result<RelayConfig, String> {
        let url = self.url.trim().to_string();
        if url.is_empty() {
            return Err("Relay URL cannot be empty".to_string());
        }
        if !url.starts_with("ws://") && !url.starts_with("wss://") {
            return Err("Relay URL must start with ws:// or wss://".to_string());
        }

        let serial_number = self
            .serial_number
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        if let Some(ref s) = serial_number {
            if s.len() != 12 {
                return Err("Serial number must be 12 characters (or left blank to auto-generate)".to_string());
            }
        }

        let unique_device_id = self
            .unique_device_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let state = match &self.state {
            None => None,
            Some(st) => {
                let code = st.code.trim().to_string();
                let secret = st.secret.trim().to_string();
                match (code.is_empty(), secret.is_empty()) {
                    (true, true) => None,
                    (false, false) => Some(RelayState { code, secret }),
                    _ => {
                        return Err(
                            "State code and secret must both be provided, or both left blank"
                                .to_string(),
                        )
                    }
                }
            }
        };

        Ok(RelayConfig {
            url,
            state,
            serial_number,
            unique_device_id,
            use_mac_identity: self.use_mac_identity,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_config_use_mac_identity_defaults_and_roundtrips() {
        let without_flag: RelayConfig =
            serde_json::from_str(r#"{"url":"wss://example.com"}"#).unwrap();
        assert!(!without_flag.use_mac_identity);

        let with_true: RelayConfig =
            serde_json::from_str(r#"{"url":"wss://example.com","use_mac_identity":true}"#)
                .unwrap();
        assert!(with_true.use_mac_identity);

        let serialized = serde_json::to_value(&with_true).unwrap();
        assert_eq!(serialized["use_mac_identity"], serde_json::Value::Bool(true));
    }

    #[test]
    fn normalize_and_validate_checks_fields() {
        let cfg = RelayConfig {
            url: "  wss://relay.example.com/ws  ".to_string(),
            state: Some(RelayState {
                code: "  ".to_string(),
                secret: "".to_string(),
            }),
            serial_number: Some("   ".to_string()),
            unique_device_id: Some("  UUID-123  ".to_string()),
            use_mac_identity: true,
        };
        let norm = cfg.normalized_and_validated().unwrap();
        assert_eq!(norm.url, "wss://relay.example.com/ws");
        assert_eq!(norm.state, None);
        assert_eq!(norm.serial_number, None);
        assert_eq!(norm.unique_device_id.as_deref(), Some("UUID-123"));
        assert!(norm.use_mac_identity);

        // Invalid URL scheme
        let bad_url = RelayConfig {
            url: "https://example.com".to_string(),
            ..norm.clone()
        };
        assert!(bad_url.normalized_and_validated().is_err());

        // Partial state
        let partial_state = RelayConfig {
            state: Some(RelayState {
                code: "ABC".to_string(),
                secret: "".to_string(),
            }),
            ..norm.clone()
        };
        assert!(partial_state.normalized_and_validated().is_err());

        // Wrong serial length
        let bad_serial = RelayConfig {
            serial_number: Some("SHORT".to_string()),
            ..norm
        };
        assert!(bad_serial.normalized_and_validated().is_err());
    }

    #[test]
    fn save_atomic_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "relayserver_cfg_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");

        let cfg = RelayConfig {
            url: "wss://example.com".to_string(),
            state: Some(RelayState {
                code: "CODE-1".to_string(),
                secret: "SECRET-1".to_string(),
            }),
            serial_number: Some("C02KL2YKDNCR".to_string()),
            unique_device_id: Some("AABBCCDD-1234-5678-9ABC-DEF012345678".to_string()),
            use_mac_identity: true,
        };
        cfg.save_atomic(&path).unwrap();
        let loaded = RelayConfig::load_sync(&path).unwrap();
        assert_eq!(loaded, cfg);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

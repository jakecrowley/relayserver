use std::{sync::Arc, time::Duration};

use backon::ExponentialBuilder;
use futures::{SinkExt, StreamExt};
use nix::sys::utsname::uname;
use serde::{Deserialize, Serialize};
use tokio::{net::TcpStream, select, sync::Mutex, task::JoinHandle, time::{self, Instant}};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{base64_encode, c::mg_copy_answer_rs, error::RelayError, nac::generate_validation_data, util::{Resource, ResourceManager}};

const IMAC131_SUFFIXES: &[&str] = &[
    "DNCT", "DNCR", "DNML", "DNMM", "F8QM", "F8QN", "F9RN",
    "FC6M", "FC6N", "FC6P", "FD55", "FD5Y", "FF7L",
];
const SERIAL_YEARS: &[u8] = b"CDFGHJKLMNPQRSTUVWX";
const SERIAL_WEEKS: &[u8] = b"123456789CDFGHJKLMNPQRSTVW";
const SERIAL_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ0123456789";

fn random_index(upper_bound: usize) -> usize {
    #[cfg(target_os = "ios")]
    let r = unsafe { libc::arc4random_uniform(upper_bound as u32) };
    #[cfg(not(target_os = "ios"))]
    let r = {
        use crate::util::random_bytes;
        let b = random_bytes::<8>();
        u64::from_le_bytes(b) % upper_bound as u64
    };
    r as usize
}

pub fn generate_hybrid_serial() -> String {
    let year = SERIAL_YEARS[random_index(SERIAL_YEARS.len())] as char;
    let week = SERIAL_WEEKS[random_index(SERIAL_WEEKS.len())] as char;
    let u0 = SERIAL_ALPHABET[random_index(SERIAL_ALPHABET.len())] as char;
    let u1 = SERIAL_ALPHABET[random_index(SERIAL_ALPHABET.len())] as char;
    let u2 = SERIAL_ALPHABET[random_index(SERIAL_ALPHABET.len())] as char;
    let suffix = IMAC131_SUFFIXES[random_index(IMAC131_SUFFIXES.len())];
    format!("C02{}{}{}{}{}{}", year, week, u0, u1, u2, suffix)
}

pub fn generate_hybrid_udid() -> String {
    let mut b = crate::util::random_bytes::<16>();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        b[0], b[1], b[2], b[3],
        b[4], b[5],
        b[6], b[7],
        b[8], b[9],
        b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

pub fn build_activation_payload(serial_number: &str) -> serde_json::Value {
    serde_json::json!({
        "Serial": {
            "activation_randomness": crate::util::random_uuid_uppercase(),
            "build_version": "22G513",
            "activation_state": "Unactivated",
            "device_class": "MacOS",
            "product_type": "iMac13,1",
            "product_version": "13.6.4",
            "serial_number": serial_number,
            "unique_device_id": serde_json::Value::Null,
            "activation_device": "MacOS",
            "ua": serde_json::Value::Null,
        }
    })
}

pub fn build_activate_response(id: u64, serial_number: &str) -> serde_json::Value {
    serde_json::json!({
        "command": "response",
        "data": build_activation_payload(serial_number),
        "id": id,
    })
}

pub fn build_sign_data_response(id: u64) -> serde_json::Value {
    serde_json::json!({
        "command": "response",
        "data": {},
        "id": id,
    })
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RelayState {
    pub code: String,
    pub secret: String,
}

pub struct RelayResource {
    pub url: Mutex<String>,
    pub state: Mutex<Option<RelayState>>,
    pub serial_number: String,
    pub unique_device_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct RelayVersions {
    hardware_version: String,
    software_name: String,
    software_version: String,
    software_build_id: String,
    unique_device_id: String,
    serial_number: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_payload_shape() {
        let payload = build_activation_payload("C02KL2YKDNCR");
        let serial = &payload["Serial"];
        assert_eq!(serial["build_version"], "22G513");
        assert_eq!(serial["activation_state"], "Unactivated");
        assert_eq!(serial["device_class"], "MacOS");
        assert_eq!(serial["product_type"], "iMac13,1");
        assert_eq!(serial["product_version"], "13.6.4");
        assert_eq!(serial["serial_number"], "C02KL2YKDNCR");
        assert_eq!(serial["activation_device"], "MacOS");
        assert_eq!(serial["unique_device_id"], serde_json::Value::Null);
        assert_eq!(serial["ua"], serde_json::Value::Null);
        let r = serial["activation_randomness"].as_str().unwrap();
        assert_eq!(r.len(), 36);
        assert!(r.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-'));
        assert!(serial.as_object().unwrap().len() == 10);
    }

    #[test]
    fn activation_randomness_differs_per_call() {
        let a = build_activation_payload("C02KL2YKDNCR");
        let b = build_activation_payload("C02KL2YKDNCR");
        assert_ne!(a["Serial"]["activation_randomness"], b["Serial"]["activation_randomness"]);
    }

    #[test]
    fn activate_response_shape() {
        let resp = build_activate_response(42, "C02KL2YKDNCR");
        assert_eq!(resp["command"], "response");
        assert_eq!(resp["id"], 42);
        assert_eq!(resp["data"]["Serial"]["serial_number"], "C02KL2YKDNCR");
    }

    #[test]
    fn sign_data_response_shape() {
        let resp = build_sign_data_response(42);
        assert_eq!(resp["command"], "response");
        assert_eq!(resp["id"], 42);
        assert_eq!(resp["data"], serde_json::json!({}));
        assert_eq!(serde_json::to_string(&resp).unwrap(), r##"{"command":"response","data":{},"id":42}"##);
    }
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum CommandData {
    Code {
        #[serde(flatten)]
        code: RelayState
    },
    Versions {
        versions: RelayVersions
    },
    ValidationData {
        data: String,
    },
    Empty {},
}

#[derive(Deserialize, Serialize)]
struct RelayCommand {
    command: String,
    id: Option<u64>,
    data: Option<CommandData>,
}

impl RelayCommand {
    fn to_message(self) -> Message {
        Message::Text(serde_json::to_string(&self).unwrap())
    }

    fn respond(&self, data: CommandData) -> Message {
        RelayCommand {
            command: "response".to_string(),
            id: Some(self.id.unwrap()),
            data: Some(data)
        }.to_message()
    }
}

pub type Relay = Arc<ResourceManager<RelayResource>>;

impl Resource for RelayResource {
    async fn generate(self: &Arc<Self>) -> Result<JoinHandle<()>, RelayError> {
        let (mut ws_stream, _) = connect_async(&*self.url.lock().await).await?;

        let mut state = self.state.lock().await;
        
        let mapped = state.clone().map(|i| CommandData::Code { code: i }).unwrap_or(CommandData::Empty {  });

        ws_stream.send(RelayCommand { id: None, command: "register".to_string(), data: Some(mapped)}.to_message()).await?;

        let item: RelayCommand = serde_json::from_str(&ws_stream.next().await.unwrap()?.into_text()?)?;
        let Some(CommandData::Code { code }) = item.data else { panic!("bad response!") };

        println!("Connected with code {}", code.code);

        *state = Some(code);

        let serial_number = self.serial_number.clone();
        let unique_device_id = self.unique_device_id.clone();

        Ok(tokio::spawn(async move {
            match RelayResource::poll(ws_stream, serial_number, unique_device_id).await {
                Ok(_) => {},
                Err(err) => {
                    println!("error {err}");
                }
            }
        }))
    }
}

impl RelayResource {
    async fn poll(
        mut ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
        serial_number: String,
        unique_device_id: String,
    ) -> Result<(), RelayError> {
        let ping_interval = Duration::from_secs(60);
        let mut last_ping = Instant::now();
        loop {
            select! {
                msg = ws_stream.next() => {
                    let Some(msg) = msg else { continue };
                    let msg = match msg {
                        Ok(Message::Text(msg)) => msg,
                        _msg => {
                            println!("Bad msg! {_msg:?}!");
                            break;
                        }
                    };
                    
                    let command: RelayCommand = serde_json::from_str(&msg).unwrap();
                    match command.command.as_str() {
                        "get-version-info" => {
                            let uts = uname().unwrap();
                            ws_stream.send(command.respond(CommandData::Versions { versions: RelayVersions {
                                hardware_version: uts.machine().to_str().unwrap().to_string(),
                                software_name: "iPhone OS".to_string(),
                                software_version: mg_copy_answer_rs("ProductVersion"),
                                software_build_id: mg_copy_answer_rs("BuildVersion"),
                                unique_device_id: unique_device_id.clone(),
                                serial_number: serial_number.clone(),
                            } })).await?;
                        },
                        "get-validation-data" => {
                            println!("Generating validation data!");
                            ws_stream.send(command.respond(CommandData::ValidationData { data: base64_encode(&generate_validation_data().await?) })).await?;
                            println!("Sent validation data!");
                        },
                        "pong" => {},
                        _raw => panic!("bad command {_raw}"),
                    }
                },
                _ = time::sleep_until(last_ping + ping_interval) => {
                    ws_stream.send(RelayCommand {
                        command: "ping".to_string(),
                        id: None,
                        data: None,
                    }.to_message()).await?;
                    last_ping = Instant::now();
                }
            }
        }
        Ok(())
    }

    pub fn new(
        url: String,
        state: Option<RelayState>,
        serial_number: String,
        unique_device_id: String,
    ) -> Relay {
        let resource = RelayResource {
            url: Mutex::new(url),
            state: Mutex::new(state),
            serial_number,
            unique_device_id,
        };

        ResourceManager::new(
            Arc::new(resource),
            ExponentialBuilder::default()
                .with_max_delay(Duration::from_secs(30))
                .with_max_times(usize::MAX), None)
    }
}

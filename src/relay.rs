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

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    unsafe {
        libc::arc4random_buf(buf.as_mut_ptr() as *mut libc::c_void, N);
    }
    buf
}

fn random_index(upper_bound: usize) -> usize {
    let r = unsafe { libc::arc4random_uniform(upper_bound as u32) };
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
    let mut b = random_bytes::<16>();
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

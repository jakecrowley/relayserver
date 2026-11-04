
use std::io::Cursor;

use crate::c::{nac_init_rs, nac_key_establishment_rs, nac_sign_rs};
use crate::error::RelayError;
use plist::{Data, Error};
use serde::{Serialize, Deserialize};


#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct SessionInfoRequest {
    session_info_request: Data,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct SessionInfoResponse {
    session_info: Option<Data>,
}

#[derive(Deserialize)]
struct CertsResponse {
    cert: Data,
}

pub fn plist_to_buf<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    let mut buf: Vec<u8> = Vec::new();
    let writer = Cursor::new(&mut buf);
    plist::to_writer_xml(writer, &value)?;
    Ok(buf)
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct ValidationRequestBody {
    session_info_request: Data,
    #[serde(skip_serializing_if = "Option::is_none")]
    absinthe_udid: Option<String>,
}

pub fn build_validation_request_body(session_request: &[u8], udid: Option<&str>) -> Result<Vec<u8>, Error> {
    let body = ValidationRequestBody {
        session_info_request: Data::from(session_request.to_vec()),
        absinthe_udid: udid.filter(|u| !u.is_empty()).map(|u| u.to_string()),
    };
    plist_to_buf(&body)
}

pub fn parse_session_info(bytes: &[u8]) -> Result<Vec<u8>, RelayError> {
    let response: SessionInfoResponse = plist::from_bytes(bytes)
        .map_err(|_| RelayError::MissingSessionInfo)?;
    match response.session_info {
        Some(info) => Ok(info.into()),
        None => Err(RelayError::MissingSessionInfo),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_info_parse_ok_and_missing() {
        let ok = br#"<plist><dict><key>session-info</key><data>c2Vzc2lvbg==</data></dict></plist>"#;
        let info = parse_session_info(&ok[..]).unwrap();
        assert_eq!(info, b"session");
        assert!(matches!(parse_session_info(b"<plist/>"), Err(RelayError::MissingSessionInfo)));
        assert!(matches!(parse_session_info(b"not a plist"), Err(RelayError::MissingSessionInfo)));
    }

    #[test]
    fn request_body_embeds_udid_conditionally() {
        let bare = build_validation_request_body(b"req", None).unwrap();
        let xml = String::from_utf8(bare).unwrap();
        assert!(xml.contains("session-info-request"));
        assert!(!xml.contains("absinthe-udid"));

        let with = build_validation_request_body(b"req", Some("ABC-123")).unwrap();
        let xml = String::from_utf8(with).unwrap();
        assert!(xml.contains("absinthe-udid"));
        assert!(xml.contains("ABC-123"));

        let empty = build_validation_request_body(b"req", Some("")).unwrap();
        let xml = String::from_utf8(empty).unwrap();
        assert!(!xml.contains("absinthe-udid"));
    }

    #[test]
    fn validation_headers_match_client() {
        let base: Vec<(&str, String)> = vec![
            ("Content-Type", "application/x-apple-plist".into()),
            ("x-protocol-version", "1630".into()),
            ("User-Agent", "com.apple.invitation-registration".into()),
            ("Accept", "*/*".into()),
            ("Accept-Language", "en-US,en;q=0.9".into()),
        ];
        assert_eq!(validation_headers(None), base);
        // empty token is treated as absent (client: `!pushToken.isEmpty`)
        assert_eq!(validation_headers(Some("")), base);
        let with_token = validation_headers(Some("tok"));
        assert_eq!(with_token.last().unwrap(), &("x-push-token", "tok".to_string()));
        assert_eq!(with_token.len(), base.len() + 1);
        assert_eq!(&with_token[..5], &base[..]);
    }
}

/// Headers for the initializeValidation POST, matching the client's
/// ValidationData.swift (including Content-Type: application/x-apple-plist).
/// x-push-token only when a non-empty token is present.
pub fn validation_headers(push_token: Option<&str>) -> Vec<(&'static str, String)> {
    let mut headers: Vec<(&'static str, String)> = vec![
        ("Content-Type", "application/x-apple-plist".into()),
        ("x-protocol-version", "1630".into()),
        ("User-Agent", "com.apple.invitation-registration".into()),
        ("Accept", "*/*".into()),
        ("Accept-Language", "en-US,en;q=0.9".into()),
    ];
    if let Some(t) = push_token.filter(|t| !t.is_empty()) {
        headers.push(("x-push-token", t.to_string()));
    }
    headers
}

pub async fn generate_validation_data(udid: Option<String>, push_token: Option<String>) -> Result<Vec<u8>, RelayError> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .use_rustls_tls()
        .build()
        .unwrap();

    let key = client.get("http://static.ess.apple.com/identity/validation/cert-1.0.plist")
        .send().await?;
    let response: CertsResponse = plist::from_bytes(&key.bytes().await?)?;
    let certs: Vec<u8> = response.cert.into();

    let mut output_req = vec![];
    let ctx = nac_init_rs(&certs, &mut output_req)?;

    let info = build_validation_request_body(&output_req, udid.as_deref())?;
    let mut activation = client.post("https://identity.ess.apple.com/WebObjects/TDIdentityService.woa/wa/initializeValidation");
    for (name, value) in validation_headers(push_token.as_deref()) {
        activation = activation.header(name, value);
    }
    let activation = activation
        .body(info)
        .send().await?;

    let output = parse_session_info(&activation.bytes().await?)?;
    nac_key_establishment_rs(ctx, &output)?;

    
    Ok(nac_sign_rs(ctx, &[])?)
}
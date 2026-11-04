# Port ValidationRelay Changes to relayserver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the Rust relay server to parity with the Swift client's recent protocol changes: new `activate` and `sign-data` websocket commands, push-token/UDID-aware validation requests, and real device identity in `get-version-info` (hybrid iMac identity reserved for `activate`).

**Architecture:** All protocol handling lives in the `poll` loop of `src/relay.rs` (`match command.as_str()`), responses built as `serde_json` values; NAC/HTTP plumbing lives in `src/nac.rs`. `activate` reuses the persisted hybrid serial already on `RelayResource`; `get-version-info` switches to MobileGestalt real identity with hybrid fallback. New error variant `MissingSessionInfo` lets `generate_validation_data` fail cleanly (the resource manager retries) instead of panicking.

**Tech Stack:** Rust, tokio + tokio-tungstenite, serde/serde_json, reqwest (rustls), plist crate, MobileGestalt via existing `mg_copy_answer_rs` C binding.

**Spec:** `docs/superpowers/specs/2026-11-04-port-validationrelay-changes-design.md` (short design approved in conversation; see also client commits `925944d`, `53aa4fb`, `072682e`, `f33b2b4` in `/home/jake/Documents/ValidationRelay`)

## Global Constraints

- Match the client's wire behavior exactly: response JSON shapes, key names, and header values as in the Swift commits listed above.
- `activate` payload values verbatim: `build_version: "22G513"`, `activation_state: "Unactivated"`, `device_class: "MacOS"`, `product_type: "iMac13,1"`, `product_version: "13.6.4"`, `activation_device: "MacOS"`, `unique_device_id: null`, `ua: null`, `activation_randomness` = fresh uppercase UUID string.
- `sign-data` response: `{ "command": "response", "data": {}, "id": <id> }`.
- Push token accepted under `push-token`, `pushToken`, `push_token`, `token` — top-level or nested in `data`.
- Headers on `initializeValidation` POST: `x-protocol-version: 1630`, `User-Agent: com.apple.invitation-registration`, `Accept: */*`, `Accept-Language: en-US,en;q=0.9`; `x-push-token` only when a token was supplied. Plist body adds `absinthe-udid` only when a UDID was supplied.
- `get-version-info` reports real `SerialNumber`/`UniqueDeviceID` from MobileGestalt; falls back to the persisted hybrid identity when MobileGestalt returns empty/None.
- Hybrid serial/UDID continue to be generated and persisted in `/var/mobile/config.json` (existing `RelayConfig` fields, unchanged names).
- No new dependencies; no changes to the register/backoff machinery in `src/util.rs`.
- Build target is iOS (`aarch64-apple-ios` via Theos toolchain) — `cargo test` cannot run on-device code; unit tests must be pure Rust (no MobileGestalt/NAC calls) and run with `cargo test --target aarch64-apple-ios` only where linking permits, otherwise verified by `cargo build`.

## Review Focus

- Unknown/extra commands from the relay: `poll` currently panics (`panic!("bad command")`) on `_raw`; after this change unknown commands must log-and-continue instead of panicking, or one stray message kills the connection loop. Needs a live socket, so pinned by code review + the Task 5 build/spot-check, not a unit test.
- Requests without `id`: `activate`/`sign-data` without an `id` field must be ignored without panicking (the client would `unwrap` here). Pinned by review + Task 5 spot-check, same reason.
- `get-validation-data` without a push token (and/or without `data` nesting): must still generate and respond with `absinthe-udid` but no `x-push-token` header. Test: task 2 pins token extraction returns `None` for these shapes.
- Empty/failed MobileGestalt answers: `mg_copy_answer_rs` returns "" for missing properties; `get-version-info` must fall back to hybrid identity rather than send an empty serial. Test: task 3 pins the fallback choice function.
- `initializeValidation` response without `session-info`: must error (retryable), not panic/unwrap. Test: task 2 pins `generate_validation_data` returns `Err(MissingSessionInfo)` — HTTP stubbed in unit test by factoring the response-parse; the network call itself stays device-only.

---

### Task 1: Response payload helpers (activate + sign-data)

**Files:**
- Modify: `src/relay.rs` (add helpers near `RelayCommand`/`CommandData`, lines ~80-115)
- Test: `src/relay.rs` (inline `#[cfg(test)] mod tests`, pure-Rust only)

**Interfaces:**
- Consumes: `RelayCommand` (existing), `generate_hybrid_serial()` (existing, pub).
- Produces:
  - `fn build_activation_payload(serial_number: &str) -> serde_json::Value` — returns `{ "Serial": { ...9 fixed keys..., "activation_randomness": <uppercase uuid>, "serial_number": serial_number } }` with `unique_device_id`/`ua` = `null`.
  - `fn build_activate_response(id: u64, serial_number: &str) -> serde_json::Value` — `{ "command": "response", "data": build_activation_payload(serial_number), "id": id }`.
  - `fn build_sign_data_response(id: u64) -> serde_json::Value` — `{ "command": "response", "data": {}, "id": id }`.
  - UUID generation: implement `fn random_uuid_uppercase() -> String` in `src/util.rs` (pub, `use crate::random_bytes` via `crate::relay::random_bytes` or move helper — see Step 3 note).

- [ ] **Step 1: Write failing tests** in `src/relay.rs` `#[cfg(test)] mod tests`:

```rust
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
    assert!(r.chars().all(|c| c.is_ascii_hexdigit() && c.is_uppercase() || c == '-'));
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `source scripts/theos-env.sh && cargo test --target aarch64-apple-ios relay::tests`
Expected: compile error — `build_activation_payload` not defined (tests can't link/run device code, but must compile; if linking `test` profile fails on device-only symbols, fall back to `cargo build --target aarch64-apple-ios` compile-fail as the gate — see Global Constraints note; record which gate was used in the commit message).

- [ ] **Step 3: Implement the helpers**

In `src/util.rs`: add `pub fn random_uuid_uppercase() -> String` using the existing `arc4random_buf` approach (move `random_bytes` from `src/relay.rs` to `src/util.rs` and make it `pub`, or expose a `pub` wrapper — builder's choice, keep `random_index` private). Format per RFC 4224 style already used by `generate_hybrid_udid` (8-4-4-4-12, uppercase hex).

In `src/relay.rs`: implement the three `build_*` functions returning `serde_json::Value` with `serde_json::json!` (use `serde_json::Value::Null` for the two null fields).

- [ ] **Step 4: Run tests to verify they pass**

Run: `source scripts/theos-env.sh && cargo test --target aarch64-apple-ios relay::tests` (or the fallback gate from Step 2)
Expected: PASS (or clean release/debug build if test link is impossible)

- [ ] **Step 5: Commit**

```bash
git add src/relay.rs src/util.rs
git commit -m "feat: activation payload and sign-data response helpers"
```

### Task 2: UDID/push-token aware validation request

**Files:**
- Modify: `src/nac.rs` (`initializeValidation` equivalent: `generate_validation_data`, lines 34-64)
- Modify: `src/relay.rs` (`get-validation-data` arm, lines 183-187; call site only)
- Test: `src/nac.rs` (inline `#[cfg(test)] mod tests`, response-parse factored out)

**Interfaces:**
- Consumes: `generate_validation_data()` (existing) — signature becomes `pub async fn generate_validation_data(udid: Option<String>, push_token: Option<String>) -> Result<Vec<u8>, RelayError>`; call sites updated in Task 4.
- Produces:
  - `fn parse_session_info(bytes: &[u8]) -> Result<Vec<u8>, RelayError>` — new, pub, unit-testable: plist-parses `session-info` or `Err(RelayError::MissingSessionInfo)`.
  - `fn build_validation_request_body(session_request: &[u8], udid: Option<&str>) -> Vec<u8>` — new, pub, unit-testable: XML plist of `{ "session-info-request": <data> }` plus `"absinthe-udid"` key only when udid is Some(non-empty).
  - `RelayError::MissingSessionInfo` — new variant in `src/error.rs` (`#[error("No session-info in validation response")]`).
  - `fn extract_push_token(json: &serde_json::Value) -> Option<String>` — new, pub, lives in `src/relay.rs`: checks nested `data` object then top level for keys `push-token`, `pushToken`, `push_token`, `token` (in that order, nested first to match client); string values only, empty string counts as None. Takes the raw message `serde_json::Value`, not `RelayCommand` — the untagged `CommandData` deserializer would silently drop unknown fields from `data` objects (only `ValidationData { data: String }` or `Empty` can match), so token extraction must happen before/at the `Value` level.

- [ ] **Step 1: Write failing tests**

In `src/nac.rs` tests:

```rust
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
    let bare = build_validation_request_body(b"req", None);
    let xml = String::from_utf8(bare).unwrap();
    assert!(xml.contains("session-info-request"));
    assert!(!xml.contains("absinthe-udid"));

    let with = build_validation_request_body(b"req", Some("ABC-123"));
    let xml = String::from_utf8(with).unwrap();
    assert!(xml.contains("absinthe-udid"));
    assert!(xml.contains("ABC-123"));
}
```

In `src/relay.rs` tests:

```rust
#[test]
fn push_token_extraction() {
    let tok = |json: &str| extract_push_token(&serde_json::from_str::<serde_json::Value>(json).unwrap());
    assert_eq!(tok(r#"{"command":"get-validation-data","id":1,"data":{"data":"x","push-token":"tok1"}}"#).as_deref(), Some("tok1"));
    assert_eq!(tok(r#"{"command":"get-validation-data","id":1,"data":{"data":"x","pushToken":"tok2"}}"#).as_deref(), Some("tok2"));
    assert_eq!(tok(r#"{"command":"get-validation-data","id":1,"push_token":"tok3","data":{"data":"x"}}"#).as_deref(), Some("tok3"));
    assert_eq!(tok(r#"{"command":"get-validation-data","id":1,"data":{"data":"x"},"token":"tok4"}"#).as_deref(), Some("tok4"));
    assert_eq!(tok(r#"{"command":"get-validation-data","id":1,"data":{"data":"x"}}"#), None);
    assert_eq!(tok(r#"{"command":"get-validation-data","id":1}"#), None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `source scripts/theos-env.sh && cargo test --target aarch64-apple-ios nac::tests relay::tests` (or compile-fail fallback gate per Task 1 Step 2)
Expected: compile error — `parse_session_info`/`build_validation_request_body`/`extract_push_token` not defined

- [ ] **Step 3: Implement**

- `src/error.rs`: add `MissingSessionInfo` variant.
- `src/nac.rs`: factor response parsing into `parse_session_info` (`plist::from_bytes::<T>` on a one-key struct or `Value` walk; on absence return `Err(RelayError::MissingSessionInfo)`); factor body building into `build_validation_request_body` using existing `plist_to_buf` with a new `SessionInfoRequest`-like struct holding optional `absinthe-udid` (builder's choice: `#[serde(skip_serializing_if = "Option::is_none")]`). Rework `generate_validation_data(udid, push_token)` to: build body via helper, set the five headers (Global Constraints values) with `reqwest::RequestBuilder` `header()` calls (`.header("User-Agent", ...)` etc.; `x-push-token` only when `Some`), parse response via `parse_session_info` instead of unwrapping `session_info`.
- `src/relay.rs`: add `extract_push_token` per Interfaces; note `CommandData::ValidationData { data: String }` — extend to accept the raw `data` object (builder's choice: add a variant or make `data: serde_json::Value`), keeping existing uses compiling.

- [ ] **Step 4: Run tests to verify they pass**

Run: same gate as Step 2
Expected: PASS (or clean build)

- [ ] **Step 5: Commit**

```bash
git add src/nac.rs src/relay.rs src/error.rs
git commit -m "feat: UDID and push-token aware validation requests"
```

### Task 3: Real identity in get-version-info with hybrid fallback

**Files:**
- Modify: `src/relay.rs` (`RelayResource::poll` `get-version-info` arm, lines 172-182; `main.rs` config plumbing untouched)
- Test: `src/relay.rs` tests

**Interfaces:**
- Consumes: `mg_copy_answer_rs` (existing, device-only), `RelayResource.serial_number`/`unique_device_id` (existing hybrid fields).
- Produces:
  - `fn resolve_device_identity(hybrid_serial: &str, hybrid_udid: &str) -> (String, String)` — new, pub, in `src/relay.rs`: returns real MobileGestalt `SerialNumber`/`UniqueDeviceID` when non-empty, else `(hybrid_serial, hybrid_udid)`. Pure decision function (device call parameterized at call site via a closure-free split — builder's choice: `resolve_device_identity(fetch: fn(&str) -> Option<String>)`-style or two-arg helper tested without MobileGestalt).

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn identity_falls_back_to_hybrid_when_real_missing() {
    // stubbed MobileGestalt: real answers empty -> hybrid wins
    let got = resolve_device_identity_with("", None, "HYBRIDSER", "HYBRIDUDID");
    assert_eq!(got.0, "HYBRIDSER");
    assert_eq!(got.1, "HYBRIDUDID");

    let got = resolve_device_identity_with("C02REALSERIAL", Some("AABBCCDD-1234-5678-9ABC-DEF012345678"), "HYBRIDSER", "HYBRIDUDID");
    assert_eq!(got.0, "C02REALSERIAL");
    assert_eq!(got.1, "AABBCCDD-1234-5678-9ABC-DEF012345678");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `source scripts/theos-env.sh && cargo test --target aarch64-apple-ios relay::tests` (or compile-fail fallback gate)
Expected: compile error — `resolve_device_identity_with` not defined

- [ ] **Step 3: Implement**

`resolve_device_identity_with(real_serial: &str, real_udid: Option<&str>, hybrid_serial: &str, hybrid_udid: &str) -> (String, String)` returns real when `!real_serial.is_empty()` and `real_udid` is `Some(non-empty)`, else hybrid tuple. In the `get-version-info` arm, call `mg_copy_answer_rs("SerialNumber")` and `mg_copy_answer_rs("UniqueDeviceID")` (device code), pass through the resolver, and keep the rest of the `RelayVersions` construction unchanged. `hardware_version`/`software_*` fields stay exactly as-is (uname + `ProductVersion`/`BuildVersion`).

- [ ] **Step 4: Run test to verify it passes**

Run: same gate as Step 2
Expected: PASS (or clean build)

- [ ] **Step 5: Commit**

```bash
git add src/relay.rs
git commit -m "feat: real device identity in get-version-info with hybrid fallback"
```

### Task 4: Wire commands into the poll loop

**Files:**
- Modify: `src/relay.rs` (`RelayResource::poll` match, lines 171-189; `generate()` validation call site at line 185)
- Test: `src/relay.rs` tests

**Interfaces:**
- Consumes: `build_activate_response(id, serial)` (Task 1), `build_sign_data_response(id)` (Task 1), `extract_push_token` (Task 2), `generate_validation_data(udid, push_token)` (Task 2), `resolve_device_identity_with` (Task 3), `RelayResource.serial_number` (hybrid serial for activate).
- Produces: final `poll` behavior — this task's deliverable.

- [ ] **Step 1: Wire the arms**

No new unit tests: every arm's pure pieces are already pinned by Tasks 1-3, and the remaining wiring needs a live socket (not testable on this target). Changes in `RelayResource::poll`:

- `"activate"`: respond `build_activate_response(id, hybrid_serial)` using the hybrid serial (from `self`/param — see note), log "Sending activation info".
  - Note: `poll` currently takes `serial_number`/`unique_device_id` params — extend to receive `Arc<RelayResource>` (or add the hybrid serial as a param) so both real + hybrid identities are available in the loop.
  - id-less `activate`/`sign-data` messages: ignore (log), no panic. `RelayCommand.id` is already `Option<u64>`; only respond when `Some`.
- `"sign-data"`: respond `build_sign_data_response(id)` when `id` is `Some`, log "Sending sign-data response".
- `"get-validation-data"`: parse the message as `serde_json::Value` first, `extract_push_token(&value)` (Task 2), real UDID via `mg_copy_answer_rs("UniqueDeviceID")` (empty -> persisted hybrid UDID), call `generate_validation_data(udid, push_token)`, respond `CommandData::ValidationData` with base64 (existing shape), keep the "Generating/Sent validation data" logs.
- `_raw` arm: replace `panic!("bad command {_raw}")` with `println!("Unhandled command {_raw}")` and keep looping.
- Keep the `pong` arm and ping timing untouched.

- [ ] **Step 2: Verify tests still pass and the binary builds**

Run: `source scripts/theos-env.sh && cargo test --target aarch64-apple-ios && cargo build --release --target aarch64-apple-ios`
Expected: existing tests PASS (or clean build per the test-linking note); build `Finished release profile`

- [ ] **Step 3: Review the arms against the client**

Read the `webSocketDidReceiveMessage` arms in `/home/jake/Documents/ValidationRelay/ValidationRelay/Relay.swift` and confirm command names, response shapes, and id-less handling match. Fix any mismatch before committing.

- [ ] **Step 4: Commit**

```bash
git add src/relay.rs
git commit -m "feat: handle activate and sign-data commands, no panic on unknown commands"
```

### Task 5: End-to-end verification

**Files:**
- Verify only: no new code.

**Interfaces:**
- Consumes: all prior tasks.
- Produces: evidence the port compiles and behaves per spec.

- [ ] **Step 1: Full test suite**

Run: `source scripts/theos-env.sh && cargo test --target aarch64-apple-ios`
Expected: PASS (all `relay::tests`, `nac::tests`, `util` if any)

- [ ] **Step 2: Release build + package smoke**

Run: `source scripts/theos-env.sh && cargo build --release --target aarch64-apple-ios && cd relay-package && make package THEOS_PACKAGE_SCHEME=rootless 2>/dev/null || cd ..`
Expected: release build `Finished`; deb packaging attempt produces `relay-package/*.deb` or, if packaging is env-gated, the build step alone suffices — record actual outcome.

- [ ] **Step 3: Behavioral spot-check against the client**

Compare each new response shape against the client-side Swift tests in `tests/ActivationPayloadTest/main.swift` (`/home/jake/Documents/ValidationRelay`): payload key set (10 keys), null `unique_device_id`/`ua`, `"data":{}` sign-data body, push-token key order, header values. Note any mismatch and fix before commit.

- [ ] **Step 4: Final commit of plan notes**

```bash
git add docs/superpowers/plans/2026-11-04-port-validationrelay-changes.md
git commit -m "docs: implementation plan for ValidationRelay port" --or-if-empty "docs: implementation plan"
```

---

## Notes

- **Test-linking reality:** this crate targets `aarch64-apple-ios` and links MobileGestalt/CoreFoundation; `cargo test` may fail at link time even for pure-Rust tests. If so, the accepted gate per task is `cargo build --target aarch64-apple-ios` compile success plus review of test bodies (they still must exist and compile as `cfg(test)` code). Task 1 Step 2 records which gate applied; reuse that decision in Tasks 2-4.
- **Client parity references:** Swift sources for exact shapes live in the sibling repo (`/home/jake/Documents/ValidationRelay/ValidationRelay/ActivationPayload.swift`, `ValidationData.swift`, `Relay.swift`); the Swift tests assert the same shapes pinned here.

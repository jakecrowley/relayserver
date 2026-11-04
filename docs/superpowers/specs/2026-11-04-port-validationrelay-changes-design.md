# Port ValidationRelay Changes to relayserver — Short Design

**Date:** 2026-11-04
**Status:** Approved in conversation (approach: "Match the client exactly")
**Client reference:** `/home/jake/Documents/ValidationRelay` commits `925944d` (activation payload helpers), `53aa4fb` (`activate` command), `072682e` (`sign-data` command), `f33b2b4` (Mac identity required / real identity in get-version-info, validation headers).

## Intent

Bring the Rust relay server (`/home/jake/Documents/ios-stuff/relayserver`) to wire parity with the recent Swift client changes so both sides of the Beeper registration relay behave identically.

## Goals

1. **`activate` command** — respond with the client's Mac activation payload: `"Serial"` map with `activation_randomness` (fresh uppercase UUID), `build_version: 22G513`, `activation_state: Unactivated`, `device_class`/`activation_device: MacOS`, `product_type: iMac13,1`, `product_version: 13.6.4`, `serial_number` = the **hybrid iMac serial**, `unique_device_id`/`ua` = `null`.
2. **`sign-data` command** — dummy response `{ "command": "response", "data": {}, "id": <id> }`, matching the client.
3. **`get-validation-data`** — extract push token from the request (`push-token`/`pushToken`/`push_token`/`token`, top-level or nested under `data`); pass UDID + token into `generate_validation_data`; `initializeValidation` POST gains optional `absinthe-udid` in the plist body and headers `x-protocol-version: 1630`, `User-Agent: com.apple.invitation-registration`, `Accept: */*`, `Accept-Language: en-US,en;q=0.9`, plus `x-push-token` when a token exists.
4. **Real identity in `get-version-info`** — report `SerialNumber` and `UniqueDeviceID` from MobileGestalt (real device, which now works because of `activate`), falling back to the persisted hybrid identity when MobileGestalt returns empty. `hardware_version`/`software_*` unchanged.

## Decisions (from the conversation)

- Identity behavior: **match the client exactly** — real serial + persistent UUID in `get-version-info`; the hybrid iMac serial continues to be generated/persisted in `config.json` and feeds the `activate` payload. Config field names stay the same; semantics shift from "spoofed identity" to "hybrid identity for activate + fallback".
- No UI work, no protocol changes beyond what the client does, no changes to the register/backoff machinery.

## Non-goals

- No changes to the WebSocket registration handshake or reconnection logic.
- No new dependencies.
- No behavioral divergence from the Swift client's wire shapes.

## What's left to the builder

- Exact Rust structure of the helpers (pure functions, unit-testable without device calls).
- Handling of id-less `activate`/`sign-data` messages (must not panic) and unknown commands (must not panic).
- Whether `cargo test` can link on this target; if not, compile-success is the gate (see plan Notes).

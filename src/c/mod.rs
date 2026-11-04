use std::ffi::{c_char, c_int, c_void, CStr, CString};

use crate::error::RelayError;

// Device-only C bindings (absd NAC mach RPC, MobileGestalt, UIDevice).
// iOS target: linked against the real implementations compiled by build.rs.
// Host target: panicking stubs so `cargo test --target x86_64-unknown-linux-gnu`
// can run the pure-Rust unit tests; no host test may call these.

#[cfg(target_os = "ios")]
mod device {
    use std::ffi::{c_char, c_int, c_void};

    extern "C" {
        pub fn nac_init(
            certificate_bytes: *const c_void,
            certificate_len: usize,
            out_ctx: *mut u64,
            out_session_request: *mut *mut c_void,
            session_requestCnt: *mut usize
        ) -> c_int;

        pub fn nac_key_establishment(
            val_ctx: u64,
            session_response: *const c_void,
            session_response_len: usize
        ) -> c_int;

        pub fn nac_sign(
            val_ctx: u64,
            data: *const c_void,
            data_len: usize,
            out_signature: *mut *mut c_void,
            out_sig_len: *mut usize
        ) -> c_int;

        pub fn mig_deallocate(
            data: *mut c_void,
            data_len: usize,
        );

        pub fn mg_copy_answer(property: *const c_char) -> *mut c_char;

        pub fn device_identifier_for_vendor() -> *mut c_char;
    }
}

#[cfg(not(target_os = "ios"))]
mod device {
    use std::ffi::{c_char, c_int, c_void};

    // Panicking host stubs: never touch real device services.
    pub unsafe fn nac_init(
        _certificate_bytes: *const c_void,
        _certificate_len: usize,
        _out_ctx: *mut u64,
        _out_session_request: *mut *mut c_void,
        _session_requestCnt: *mut usize
    ) -> c_int {
        panic!("nac_init called on host (device-only binding)")
    }

    pub unsafe fn nac_key_establishment(
        _val_ctx: u64,
        _session_response: *const c_void,
        _session_response_len: usize
    ) -> c_int {
        panic!("nac_key_establishment called on host (device-only binding)")
    }

    pub unsafe fn nac_sign(
        _val_ctx: u64,
        _data: *const c_void,
        _data_len: usize,
        _out_signature: *mut *mut c_void,
        _out_sig_len: *mut usize
    ) -> c_int {
        panic!("nac_sign called on host (device-only binding)")
    }

    pub unsafe fn mig_deallocate(
        _data: *mut c_void,
        _data_len: usize,
    ) {
        panic!("mig_deallocate called on host (device-only binding)")
    }

    pub unsafe fn mg_copy_answer(_property: *const c_char) -> *mut c_char {
        panic!("mg_copy_answer called on host (device-only binding)")
    }

    pub unsafe fn device_identifier_for_vendor() -> *mut c_char {
        panic!("device_identifier_for_vendor called on host (device-only binding)")
    }
}

use device::{nac_init, nac_key_establishment, nac_sign, mig_deallocate, mg_copy_answer, device_identifier_for_vendor};

pub fn nac_init_rs(cert: &[u8], output: &mut Vec<u8>) -> Result<u64, RelayError> {
    unsafe {
        let mut out_req: *mut c_void = std::ptr::null_mut();
        let mut out_req_cnt: usize = 0;
        let mut ctx_out: u64 = 0;
        let resp = nac_init(cert.as_ptr() as *const c_void, cert.len(), &mut ctx_out, &mut out_req, &mut out_req_cnt);
        if resp == 0 {
            output.extend_from_slice(std::slice::from_raw_parts(out_req as *mut u8, out_req_cnt));
            mig_deallocate(out_req, out_req_cnt);
            Ok(ctx_out)
        } else {
            Err(RelayError::NacError(resp as u64))
        }
    }
}

pub fn nac_key_establishment_rs(ctx: u64, response: &[u8]) -> Result<(), RelayError> {
    unsafe {
        let resp = nac_key_establishment(ctx, response.as_ptr() as *const c_void, response.len());
        if resp == 0 {
            Ok(())
        } else {
            Err(RelayError::NacError(resp as u64))
        }
    }
}

pub fn nac_sign_rs(ctx: u64, data: &[u8]) -> Result<Vec<u8>, RelayError> {
    unsafe {
        let mut out_sig: *mut c_void = std::ptr::null_mut();
        let mut out_sig_cnt: usize = 0;
        let resp = nac_sign(ctx, data.as_ptr() as *const c_void, data.len(), &mut out_sig, &mut out_sig_cnt);
        if resp == 0 {
            let vec = std::slice::from_raw_parts(out_sig as *mut u8, out_sig_cnt).to_vec();
            mig_deallocate(out_sig, out_sig_cnt);
            Ok(vec)
        } else {
            Err(RelayError::NacError(resp as u64))
        }
    }
}

fn copy_answer_to_string(answer: *mut c_char) -> String {
    unsafe {
        let c_str = CStr::from_ptr(answer).to_str().unwrap().to_string();
        libc::free(answer as *mut c_void);
        c_str
    }
}

/// MobileGestalt answer; "" for a missing property (matches the client's
/// optional-bridging semantics that the fallback logic relies on).
pub fn mg_copy_answer_rs(item: &str) -> String {
    unsafe {
        let c_str = CString::new(item).unwrap();
        copy_answer_to_string(mg_copy_answer(c_str.as_ptr()))
    }
}

/// UIDevice.current.identifierForVendor?.uuidString ?? "" (device-only).
pub fn device_identifier_for_vendor_rs() -> String {
    unsafe { copy_answer_to_string(device_identifier_for_vendor()) }
}

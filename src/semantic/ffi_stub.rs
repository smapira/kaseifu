//! Rust fallback with the same C ABI as the Swift bridge.
//!
//! `build.rs` compiles this module instead of the Swift bridge when the build
//! machine has no Swift toolchain or an SDK without Foundation Models (CI, for
//! example). It never infers anything: every answer says the feature is
//! unavailable and names the reason. The request validation mirrors the Swift
//! side so the same tests run against both implementations.

use std::ffi::{CStr, CString, c_char, c_void};

use super::contract::SCHEMA_VERSION;
use super::ffi::{CODE_INVALID_REQUEST, CODE_NULL, CODE_OK, CODE_VERSION, FmCallback};

/// The reason every answer carries in this build.
#[cfg(kaseifu_fm_stub = "no-sdk")]
pub const REASON: &str = "foundation-models-sdk-missing";
/// The reason every answer carries in this build.
#[cfg(kaseifu_fm_stub = "no-toolchain")]
pub const REASON: &str = "swift-toolchain-missing";

/// Marker allocation: `create` / `destroy` keep the same ownership shape as
/// the Swift context.
struct StubContext;

/// Availability JSON (caller frees it with [`kaseifu_fm_free_result`]).
///
/// # Safety
/// The returned pointer must be freed exactly once with
/// `kaseifu_fm_free_result`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kaseifu_fm_available() -> *mut c_char {
    let json = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "status": "unavailable",
        "reason": REASON,
    })
    .to_string();
    into_c_string(json)
}

/// Create a context (null when `options_json` is not a JSON object).
///
/// # Safety
/// `options_json` must be null or a valid NUL-terminated string, and the
/// returned context must be destroyed exactly once with
/// `kaseifu_fm_destroy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kaseifu_fm_create(options_json: *const c_char) -> *mut c_void {
    if !options_json.is_null() {
        let text = unsafe { CStr::from_ptr(options_json) }
            .to_string_lossy()
            .into_owned();
        if serde_json::from_str::<serde_json::Value>(&text).is_err() {
            return std::ptr::null_mut();
        }
    }
    Box::into_raw(Box::new(StubContext)) as *mut c_void
}

/// Answer one request through `callback` (accepted requests call back once).
///
/// # Safety
/// `context` must come from [`kaseifu_fm_create`] and stay alive, and
/// `request_json` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kaseifu_fm_generate(
    context: *mut c_void,
    request_json: *const c_char,
    callback: Option<FmCallback>,
    user_data: *mut c_void,
) -> i32 {
    let Some(callback) = callback else {
        return CODE_NULL;
    };
    if context.is_null() || request_json.is_null() {
        return CODE_NULL;
    }
    let request = unsafe { CStr::from_ptr(request_json) }
        .to_string_lossy()
        .into_owned();
    let Ok(serde_json::Value::Object(object)) = serde_json::from_str::<serde_json::Value>(&request)
    else {
        return CODE_INVALID_REQUEST;
    };
    if object
        .get("schema_version")
        .and_then(|value| value.as_str())
        != Some(SCHEMA_VERSION)
    {
        return CODE_VERSION;
    }
    let Some(request_id) = object.get("request_id").and_then(|value| value.as_str()) else {
        return CODE_INVALID_REQUEST;
    };
    if request_id.is_empty() {
        return CODE_INVALID_REQUEST;
    }
    let payload = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "request_id": request_id,
        "status": "error",
        "semantic": serde_json::Value::Null,
        "error": {"kind": "unavailable", "reason": REASON},
    })
    .to_string();
    // Accepted: the callback runs exactly once, as the Swift bridge does.
    unsafe { callback(into_c_string(payload), user_data) };
    CODE_OK
}

/// No-op: the fallback has no running generation to cancel.
///
/// # Safety
/// `_context` must be null or a context from [`kaseifu_fm_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kaseifu_fm_cancel(_context: *mut c_void) {}

/// Free a result string this fallback produced.
///
/// # Safety
/// `result` must be null or a pointer from this fallback that was not freed
/// before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kaseifu_fm_free_result(result: *mut c_char) {
    if result.is_null() {
        return;
    }
    drop(unsafe { CString::from_raw(result) });
}

/// Destroy a context created by this fallback.
///
/// # Safety
/// `context` must be null or a context from [`kaseifu_fm_create`] that was not
/// destroyed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kaseifu_fm_destroy(context: *mut c_void) {
    if context.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(context as *mut StubContext) });
}

/// Hand a `String` to the caller as an owned C string.
fn into_c_string(value: String) -> *mut c_char {
    CString::new(value)
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

//! The raw C ABI and the safe Rust wrapper around the Swift bridge.
//!
//! Ownership and threading contract (mirrored by `swift/kaseifu_fm_bridge.swift`):
//!
//! - `generate` returns a code; `0` means the callback will be invoked exactly
//!   once, any other code means it will never be invoked,
//! - the callback owns the result string and frees it with
//!   `kaseifu_fm_free_result` (the trampoline below does this),
//! - `destroy` never lets a callback run after it returned,
//! - a context is not shared between concurrent `generate` and `destroy` calls;
//!   the borrow rules of [`FmContext`] enforce that, and the bridge is
//!   internally synchronized for `generate` + `cancel`.
//!
//! The per-request channel lives in a process-wide registry keyed by a small
//! integer, not in a pointer the bridge holds: a timeout (or a callback that
//! never arrives) removes the entry, so **no per-request allocation can leak**
//! even when a generation outlives the call. The `user_data` pointer is that
//! key, and the bridge only ever passes it back untouched.
//!
//! Everything `unsafe` in this subsystem is confined to this module and to
//! `ffi_stub.rs`.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::future::Future;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

/// `kaseifu_fm_generate`: the request was accepted; the callback will run.
pub const CODE_OK: i32 = 0;
/// `kaseifu_fm_generate`: a null argument.
pub const CODE_NULL: i32 = 1;
/// `kaseifu_fm_generate`: the request is not a valid `fm-rename-v1` request.
pub const CODE_INVALID_REQUEST: i32 = 2;
/// `kaseifu_fm_generate`: the request carries another `schema_version`.
pub const CODE_VERSION: i32 = 3;
/// `kaseifu_fm_generate`: a generation is already active on this context.
pub const CODE_BUSY: i32 = 4;
/// `kaseifu_fm_generate`: the context is being destroyed.
pub const CODE_DESTROYED: i32 = 5;
/// `kaseifu_fm_generate`: an internal failure.
pub const CODE_INTERNAL: i32 = 7;

/// The callback type shared with the bridge.
pub type FmCallback = unsafe extern "C" fn(result: *mut c_char, user_data: *mut c_void);

/// Which implementation of the C ABI this build linked.
pub const BRIDGE_KIND: &str = if cfg!(any(
    kaseifu_fm_stub = "no-sdk",
    kaseifu_fm_stub = "no-toolchain"
)) {
    "stub"
} else {
    "swift"
};

/// Raw C ABI declarations.
///
/// Prefer the safe wrappers in this module; the raw surface is public so the
/// integration tests and the developer smoke can exercise the boundary as the
/// bridge sees it.
pub mod raw {
    use std::ffi::{c_char, c_void};

    use super::FmCallback;

    unsafe extern "C" {
        /// Availability JSON owned by the caller (`kaseifu_fm_free_result`).
        pub fn kaseifu_fm_available() -> *mut c_char;
        /// Perception JSON for one image path, owned by the caller
        /// (`kaseifu_fm_free_result`). Synchronous: Vision classifies a
        /// bounded thumbnail in well under a second (measured 0.078-0.385 s).
        pub fn kaseifu_vision_perceive(image_path: *const c_char) -> *mut c_char;
        /// Create a context, or null when `options_json` is not a JSON object.
        pub fn kaseifu_fm_create(options_json: *const c_char) -> *mut c_void;
        /// Start one generation; see the module contract for the return codes.
        pub fn kaseifu_fm_generate(
            context: *mut c_void,
            request_json: *const c_char,
            callback: Option<FmCallback>,
            user_data: *mut c_void,
        ) -> i32;
        /// Request cancellation of the active generation (a no-op when idle).
        pub fn kaseifu_fm_cancel(context: *mut c_void);
        /// Free a result string returned by the bridge.
        pub fn kaseifu_fm_free_result(result: *mut c_char);
        /// Destroy a context; no callback runs after it returns.
        pub fn kaseifu_fm_destroy(context: *mut c_void);
    }
}

/// Why a call at the FFI boundary failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfiError {
    /// `generate` refused the request immediately (no callback will follow).
    Rejected { code: i32, detail: String },
    /// No result arrived within the timeout (the generation was cancelled).
    Timeout,
    /// The boundary itself misbehaved (null result, non-UTF-8, ...).
    Internal(String),
}

impl FfiError {
    /// Map a `kaseifu_fm_generate` return code.
    pub fn from_code(code: i32) -> Self {
        let detail = match code {
            CODE_NULL => "null argument",
            CODE_INVALID_REQUEST => "invalid request JSON",
            CODE_VERSION => "unsupported schema version",
            CODE_BUSY => "a generation is already active on this context",
            CODE_DESTROYED => "the context is being destroyed",
            _ => "internal error",
        };
        Self::Rejected {
            code,
            detail: detail.to_string(),
        }
    }
}

impl std::fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected { code, detail } => write!(f, "Rejected({code}: {detail})"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Internal(message) => write!(f, "Internal({message})"),
        }
    }
}

/// The availability payload as JSON (parsed by `contract::parse_availability`).
pub fn available() -> Result<String, FfiError> {
    take_string(unsafe { raw::kaseifu_fm_available() })
}

/// The perception payload for one image path (parsed by `perception::parse`).
///
/// The path travels as a C string and Rust never opens the file: the bridge
/// decodes a bounded thumbnail with ImageIO and classifies it. A rejected
/// path (not UTF-8, NUL byte) is an explicit error, never a panic.
pub fn vision_perceive(path: &Path) -> Result<String, FfiError> {
    let text = path
        .to_str()
        .ok_or_else(|| FfiError::Internal("the image path is not valid UTF-8".to_string()))?;
    let argument = CString::new(text)
        .map_err(|_| FfiError::Internal("the image path contains a NUL byte".to_string()))?;
    take_string(unsafe { raw::kaseifu_vision_perceive(argument.as_ptr()) })
}

/// An inference context. Dropping it destroys the bridge context.
pub struct FmContext {
    pointer: NonNull<c_void>,
}

// SAFETY: the bridge is internally synchronized (a lock plus a semaphore) and
// the C ABI contract allows calls from any thread. `generate` and `destroy`
// cannot race for one context because `FmContext` is only ever accessed through
// `&self`, and `Drop` needs exclusive ownership.
unsafe impl Send for FmContext {}
unsafe impl Sync for FmContext {}

impl FmContext {
    /// Create a context (the bridge allocates; `None` means failure).
    pub fn create() -> Result<Self, FfiError> {
        let pointer = unsafe { raw::kaseifu_fm_create(std::ptr::null()) };
        NonNull::new(pointer)
            .map(|pointer| Self { pointer })
            .ok_or_else(|| FfiError::Internal("the bridge could not create a context".to_string()))
    }

    /// Run one generation and return the raw response JSON.
    ///
    /// On timeout the generation is cancelled and a late result is discarded
    /// (the trampoline frees it).
    pub async fn generate(
        &self,
        request_json: &str,
        timeout: Duration,
    ) -> Result<String, FfiError> {
        let request = CString::new(request_json).map_err(|_| FfiError::Rejected {
            code: CODE_INVALID_REQUEST,
            detail: "the request contains a NUL byte".to_string(),
        })?;
        let (sender, receiver) = oneshot::channel::<Result<String, FfiError>>();
        let key = register_delivery(sender);
        let code = unsafe {
            raw::kaseifu_fm_generate(
                self.pointer.as_ptr(),
                request.as_ptr(),
                Some(trampoline),
                key as *mut c_void,
            )
        };
        if code != CODE_OK {
            // Contract: a rejected call never invokes the callback.
            drop(take_delivery(key));
            return Err(FfiError::from_code(code));
        }
        let outcome = async move {
            match receiver.await {
                Ok(result) => result,
                Err(_) => Err(FfiError::Internal(
                    "the bridge dropped the callback without a result".to_string(),
                )),
            }
        };
        let result = await_with_timeout(timeout, outcome, || self.cancel()).await;
        if result.is_err() {
            // The call is over: drop the sender so a late callback finds
            // nothing (and frees its own payload).
            drop(take_delivery(key));
        }
        result
    }

    /// Ask the bridge to cancel the active generation.
    pub fn cancel(&self) {
        unsafe { raw::kaseifu_fm_cancel(self.pointer.as_ptr()) };
    }

    /// The raw context pointer.
    ///
    /// For diagnostics and the raw-ABI tests only: the caller must not destroy
    /// the context through this pointer (the `Drop` of this value does that).
    pub fn as_raw_pointer(&self) -> *mut c_void {
        self.pointer.as_ptr()
    }
}

impl Drop for FmContext {
    fn drop(&mut self) {
        unsafe { raw::kaseifu_fm_destroy(self.pointer.as_ptr()) };
    }
}

/// Await `future`, cancelling through `on_timeout` when it runs too long.
pub(crate) async fn await_with_timeout<T, F, C>(
    timeout: Duration,
    future: F,
    on_timeout: C,
) -> Result<T, FfiError>
where
    F: Future<Output = Result<T, FfiError>>,
    C: FnOnce(),
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => {
            on_timeout();
            Err(FfiError::Timeout)
        }
    }
}

/// Take ownership of a bridge string and copy it into a `String`.
fn take_string(pointer: *mut c_char) -> Result<String, FfiError> {
    if pointer.is_null() {
        return Err(FfiError::Internal(
            "the bridge returned a null result".to_string(),
        ));
    }
    let owned = OwnedResult(pointer);
    let text = unsafe { CStr::from_ptr(owned.0) }
        .to_str()
        .map_err(|error| FfiError::Internal(format!("the bridge returned non-UTF-8: {error}")))?;
    Ok(text.to_string())
}

/// A result string owned by the bridge.
struct OwnedResult(*mut c_char);

impl Drop for OwnedResult {
    fn drop(&mut self) {
        unsafe { raw::kaseifu_fm_free_result(self.0) };
    }
}

/// The callback the bridge calls exactly once per accepted request.
///
/// It always frees the result string and always reclaims the delivery entry;
/// a closed receiver (timeout, cancellation) simply drops the payload.
unsafe extern "C" fn trampoline(result: *mut c_char, user_data: *mut c_void) {
    let payload = take_string(result);
    if user_data.is_null() {
        return;
    }
    if let Some(sender) = take_delivery(user_data as u64) {
        let _ = sender.send(payload);
    }
}

/// One in-flight delivery: the channel back to the awaiting caller.
type Delivery = oneshot::Sender<Result<String, FfiError>>;

/// In-flight deliveries: request key -> channel to the awaiting caller.
///
/// An entry lives only while the call that created it is waiting; a timeout
/// removes it, so a callback that never arrives cannot leak it.
static DELIVERIES: LazyLock<Mutex<HashMap<u64, Delivery>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Keys are handed to the bridge as `user_data`; 0 is never used so a null
/// `user_data` stays distinguishable.
static NEXT_DELIVERY_KEY: AtomicU64 = AtomicU64::new(1);

/// Store a delivery channel and return its key.
fn register_delivery(sender: Delivery) -> u64 {
    let key = NEXT_DELIVERY_KEY.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut deliveries) = DELIVERIES.lock() {
        deliveries.insert(key, sender);
    }
    key
}

/// Remove a delivery channel (whoever removes it owns it).
fn take_delivery(key: u64) -> Option<Delivery> {
    DELIVERIES.lock().ok()?.remove(&key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn bridge_kind_matches_the_build() {
        assert!(matches!(BRIDGE_KIND, "swift" | "stub"));
    }

    #[test]
    fn available_returns_a_contract_payload() {
        let json = available().expect("the bridge answers");
        assert!(
            json.contains("\"schema_version\":\"fm-rename-v1\""),
            "{json}"
        );
        assert!(json.contains("\"status\""), "{json}");
    }

    #[test]
    fn rejected_codes_map_to_ffi_errors() {
        assert_eq!(
            FfiError::from_code(CODE_NULL),
            FfiError::Rejected {
                code: 1,
                detail: "null argument".to_string()
            }
        );
        assert_eq!(
            FfiError::from_code(CODE_BUSY),
            FfiError::Rejected {
                code: 4,
                detail: "a generation is already active on this context".to_string()
            }
        );
        assert!(matches!(
            FfiError::from_code(CODE_VERSION),
            FfiError::Rejected { code: 3, .. }
        ));
    }

    #[tokio::test]
    async fn a_timeout_cancels_and_reports_the_timeout() {
        let cancelled = Arc::new(AtomicUsize::new(0));
        let flag = Arc::clone(&cancelled);
        let outcome = await_with_timeout(
            Duration::from_millis(1),
            pending::<Result<(), FfiError>>(),
            move || {
                flag.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;
        assert_eq!(outcome, Err(FfiError::Timeout));
        assert_eq!(cancelled.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_result_before_the_timeout_is_returned() {
        let outcome = await_with_timeout(
            Duration::from_secs(30),
            async { Ok::<_, FfiError>(42) },
            || panic!("must not cancel"),
        )
        .await;
        assert_eq!(outcome, Ok(42));
    }

    #[test]
    fn a_null_result_is_an_internal_error() {
        assert!(matches!(
            take_string(std::ptr::null_mut()),
            Err(FfiError::Internal(_))
        ));
    }

    #[test]
    fn a_delivery_is_removed_exactly_once() {
        let (sender, _receiver) = oneshot::channel::<Result<String, FfiError>>();
        let key = register_delivery(sender);
        assert!(take_delivery(key).is_some(), "the first taker owns it");
        assert!(
            take_delivery(key).is_none(),
            "the second taker finds nothing"
        );
    }

    #[tokio::test]
    async fn the_trampoline_delivers_a_null_result_as_an_error() {
        let (sender, receiver) = oneshot::channel::<Result<String, FfiError>>();
        let key = register_delivery(sender);
        unsafe { trampoline(std::ptr::null_mut(), key as *mut c_void) };
        let payload = receiver.await.expect("the trampoline sent");
        assert!(matches!(payload, Err(FfiError::Internal(_))));
    }

    #[tokio::test]
    async fn the_trampoline_without_a_key_frees_nothing() {
        // No user data and no result: nothing to deliver, nothing to free.
        unsafe { trampoline(std::ptr::null_mut(), std::ptr::null_mut()) };
    }

    #[tokio::test]
    async fn the_trampoline_without_a_delivery_still_frees_the_result() {
        // The key is gone (the caller timed out): the payload must be freed
        // here and must not panic.
        let payload = std::ffi::CString::new("{\"status\":\"error\"}").expect("cstring");
        unsafe { trampoline(payload.into_raw(), 424242 as *mut c_void) };
    }
}

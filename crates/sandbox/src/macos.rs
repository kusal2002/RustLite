//! macOS implementation: the renderer enters a `sandbox_init` profile (the
//! Seatbelt SBPL language; later rules take precedence over earlier ones).
//!
//! The profile denies reading and writing files outside system library
//! locations, every network operation, and process creation. Pipes
//! inherited from the browser have no path and stay usable.

use std::ffi::{CStr, c_char, c_int};

use crate::Lockdown;

#[allow(unsafe_code)]
unsafe extern "C" {
    fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> c_int;
    fn sandbox_free_error(errorbuf: *mut c_char);
}

const PROFILE: &CStr = c"(version 1)
(allow default)
(deny network*)
(deny process-exec* process-fork)
(deny file-read* file-write* (subpath \"/\"))
(allow file-read*
    (subpath \"/System\")
    (subpath \"/usr/lib\")
    (subpath \"/usr/share\")
    (subpath \"/Library/Apple\")
    (subpath \"/private/var/db/dyld\")
    (subpath \"/private/var/db/timezone\")
    (literal \"/dev/urandom\")
    (literal \"/dev/null\"))
(allow file-write* (literal \"/dev/null\"))
";

#[allow(unsafe_code)]
pub(crate) fn lockdown() -> Lockdown {
    let mut error: *mut c_char = std::ptr::null_mut();
    // SAFETY: `PROFILE` is a NUL-terminated string; flags 0 means the
    // argument is profile text; `error` is a valid out-pointer.
    let r = unsafe { sandbox_init(PROFILE.as_ptr(), 0, &mut error) };
    if r != 0 {
        let message = if error.is_null() {
            String::from("unknown error")
        } else {
            // SAFETY: on failure `error` is a NUL-terminated string owned by
            // the sandbox library until freed below.
            let m = unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned();
            // SAFETY: frees the buffer returned above, exactly once.
            unsafe { sandbox_free_error(error) };
            m
        };
        tracing::warn!(error = %message, "sandbox_init failed");
        return Lockdown::default();
    }
    Lockdown {
        filesystem: true,
        network: true,
        processes: true,
        // A sandboxed process cannot leave or loosen its sandbox.
        privileges: true,
        integrity: None,
    }
}

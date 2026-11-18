//! The entry of the Android `:compat` process (ADR-0066). Kotlin's `CompatService` gets one end of a socket pair
//! from the app process, detaches its descriptor and calls [`Java_io_wonmor_vutric_1spike_CompatNative_serve`]
//! on a background thread. The adapter loop runs there until the app process sends `Stop` or closes its end.
//!
//! This process never opens the Vutric identity, `trusted.json` or `settings.json`: it is given only the
//! adapters' own identity files, a staging folder and ports, in the `Start` line the app process writes first.

use std::ffi::c_void;

/// `CompatNative.serve(fd)`: runs the compat loop on `fd` until it ends. Returns 0 when the loop ended cleanly,
/// 1 when it failed (the reason goes to the log; the app process sees its end of the socket close).
///
/// # Safety
///
/// Called only by the JVM through JNI with a socket descriptor that `ParcelFileDescriptor.detachFd()` handed over:
/// open, connected, and owned by nobody else from then on. The JNI environment and class pointers are not used.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_io_wonmor_vutric_1spike_CompatNative_serve(
    _env: *mut c_void,
    _class: *mut c_void,
    fd: i32,
) -> i32 {
    if fd < 0 {
        return 1;
    }
    // A panic must not unwind into the JVM: it ends this call (and the loop) instead.
    let ran = std::panic::catch_unwind(|| {
        // SAFETY: the caller hands over an open, connected stream socket it no longer uses (see above).
        unsafe { vutric_compat_localsend::serve::serve_socket_fd(fd) }
    });
    match ran {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => {
            vutric_net::say(format!("compat process: {e:#}"));
            1
        }
        Err(_) => {
            vutric_net::say("compat process: the adapter loop panicked".into());
            1
        }
    }
}

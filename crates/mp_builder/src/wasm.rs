//! The exports `web/builder-worker.js` calls: the builder as its own wasm
//! in a worker (no wasm-bindgen; bytes in and out of linear memory).
//!
//! The shim allocates with [`mpb_alloc`], writes the level's id, calls
//! [`mpb_plan`], then [`mpb_build`] per section; a result (the packed
//! section, or the error's text) sits in the builder's own buffer until
//! the next call: [`mpb_out_ptr`] and [`mpb_out_len`] find it. A panic
//! writes its message through the shim's `mpb_log` and traps.

use std::sync::Mutex;

use mp_worldgen::stream::Planned;

static PLAN: Mutex<Option<Planned>> = Mutex::new(None);
static OUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

#[link(wasm_import_module = "env")]
unsafe extern "C" {
    /// The shim's: a line to the console.
    fn mpb_log(ptr: *const u8, len: u32);
}

fn log(s: &str) {
    unsafe { mpb_log(s.as_ptr(), s.len() as u32) }
}

fn set_out(bytes: Vec<u8>) {
    *OUT.lock().unwrap_or_else(|e| e.into_inner()) = bytes;
}

/// `n` bytes the shim may write (the level's id); freed by [`mpb_free`].
#[unsafe(no_mangle)]
pub extern "C" fn mpb_alloc(n: u32) -> *mut u8 {
    let mut v: Vec<u8> = Vec::with_capacity(n as usize);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `p` and `n` must come from [`mpb_alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpb_free(p: *mut u8, n: u32) {
    if !p.is_null() {
        drop(unsafe { Vec::from_raw_parts(p, 0, n as usize) });
    }
}

/// Plans the level whose id is the `len` bytes at `p` (UTF-8). Returns 1,
/// or 0 with the error's text as the output.
///
/// # Safety
/// `p` must hold `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpb_plan(p: *const u8, len: u32) -> u32 {
    std::panic::set_hook(Box::new(|info| log(&format!("builder panicked: {info}"))));
    let id = unsafe { std::slice::from_raw_parts(p, len as usize) };
    let id = String::from_utf8_lossy(id);
    match crate::plan(&id) {
        Ok(plan) => {
            *PLAN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plan);
            set_out(Vec::new());
            1
        }
        Err(e) => {
            set_out(e.into_bytes());
            0
        }
    }
}

/// Builds section `k` of the planned level into the output. Returns 1, or
/// 0 with the error's text as the output.
#[unsafe(no_mangle)]
pub extern "C" fn mpb_build(k: i32) -> u32 {
    let plan = PLAN.lock().unwrap_or_else(|e| e.into_inner());
    let Some(p) = plan.as_ref() else {
        set_out(b"no level planned".to_vec());
        return 0;
    };
    match crate::build(p, i64::from(k)) {
        Ok(bytes) => {
            set_out(bytes);
            1
        }
        Err(e) => {
            set_out(e.into_bytes());
            0
        }
    }
}

/// The last result's bytes (valid until the next call).
#[unsafe(no_mangle)]
pub extern "C" fn mpb_out_ptr() -> *const u8 {
    OUT.lock().unwrap_or_else(|e| e.into_inner()).as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn mpb_out_len() -> u32 {
    OUT.lock().unwrap_or_else(|e| e.into_inner()).len() as u32
}

/// Frees the last result (the shim has copied it out).
#[unsafe(no_mangle)]
pub extern "C" fn mpb_out_clear() {
    set_out(Vec::new());
}

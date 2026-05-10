use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_ulonglong};
use std::ptr::null_mut;
use std::sync::Mutex;

use crate::player_core::{PlayerCore, PlayerState};
type PlayerHandle = Mutex<PlayerCore>;

const OK: c_int = 0;
const ERR_NULL: c_int = -1;
const ERR_UTF8: c_int = -2;
const ERR_OP: c_int = -3;

fn cstr_to_string(ptr: *const c_char) -> Result<String, c_int> {
    if ptr.is_null() {
        return Err(ERR_NULL);
    }
    // SAFETY: pointer validity is checked by caller contract and null check above.
    let c = unsafe { CStr::from_ptr(ptr) };
    c.to_str().map(|s| s.to_string()).map_err(|_| ERR_UTF8)
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_create(
    device_name: *const c_char,
    music_dir: *const c_char,
    index_path: *const c_char,
) -> *mut PlayerHandle {
    let device_name = match cstr_to_string(device_name) {
        Ok(v) => v,
        Err(_) => return null_mut(),
    };
    let music_dir = match cstr_to_string(music_dir) {
        Ok(v) => v,
        Err(_) => return null_mut(),
    };
    let index_path = match cstr_to_string(index_path) {
        Ok(v) => v,
        Err(_) => return null_mut(),
    };
    Box::into_raw(Box::new(Mutex::new(PlayerCore::new(
        &device_name,
        &music_dir,
        &index_path,
    ))))
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_destroy(handle: *mut PlayerHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: handle was created by Box::into_raw in liplayer_create and is consumed exactly once here.
    unsafe {
        drop(Box::from_raw(handle));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_scan_local_library(
    handle: *mut PlayerHandle,
    music_dir: *const c_char,
) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    let music_dir = match cstr_to_string(music_dir) {
        Ok(v) => v,
        Err(code) => return code,
    };
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &*handle };
    if let Ok(core) = core.lock() {
        core.scan_local_library(&music_dir);
    } else {
        return ERR_OP;
    }
    OK
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_track_count(handle: *mut PlayerHandle) -> usize {
    if handle.is_null() {
        return 0;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &*handle };
    core.lock().map(|c| c.list_tracks().len()).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_list_tracks_json(handle: *mut PlayerHandle) -> *mut c_char {
    if handle.is_null() {
        return null_mut();
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &*handle };
    let tracks = match core.lock() {
        Ok(c) => c.list_tracks(),
        Err(_) => return null_mut(),
    };
    let json = match serde_json::to_string(&tracks) {
        Ok(v) => v,
        Err(_) => return null_mut(),
    };
    match CString::new(json) {
        Ok(v) => v.into_raw(),
        Err(_) => null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    // SAFETY: pointer was allocated by CString::into_raw in this module.
    unsafe {
        drop(CString::from_raw(s));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_play_track_at(handle: *mut PlayerHandle, index: usize) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &*handle };
    match core.lock() {
        Ok(mut c) => c.play_track_at(index).map(|_| OK).unwrap_or(ERR_OP),
        Err(_) => ERR_OP,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_stop(handle: *mut PlayerHandle) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &*handle };
    if let Ok(mut c) = core.lock() {
        c.stop();
        OK
    } else {
        ERR_OP
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_pause(handle: *mut PlayerHandle) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &*handle };
    if let Ok(mut c) = core.lock() {
        c.pause();
        OK
    } else {
        ERR_OP
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_resume(handle: *mut PlayerHandle) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &*handle };
    if let Ok(mut c) = core.lock() {
        c.resume();
        OK
    } else {
        ERR_OP
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_elapsed_millis(handle: *mut PlayerHandle) -> c_ulonglong {
    if handle.is_null() {
        return 0;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &*handle };
    core.lock()
        .map(|c| c.elapsed_millis() as c_ulonglong)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_state(handle: *mut PlayerHandle) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &*handle };
    let state = match core.lock() {
        Ok(c) => c.state(),
        Err(_) => return ERR_OP,
    };
    match state {
        PlayerState::Idle => 0,
        PlayerState::Playing => 1,
        PlayerState::Paused => 2,
    }
}

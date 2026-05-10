use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_ulonglong};
use std::ptr::null_mut;

use crate::player_core::{PlayerCore, PlayerState};

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
) -> *mut PlayerCore {
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
    Box::into_raw(Box::new(PlayerCore::new(
        &device_name,
        &music_dir,
        &index_path,
    )))
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_destroy(handle: *mut PlayerCore) {
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
    handle: *mut PlayerCore,
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
    let core = unsafe { &mut *handle };
    core.scan_local_library(&music_dir);
    OK
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_track_count(handle: *mut PlayerCore) -> usize {
    if handle.is_null() {
        return 0;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &mut *handle };
    core.list_tracks().len()
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_list_tracks_json(handle: *mut PlayerCore) -> *mut c_char {
    if handle.is_null() {
        return null_mut();
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &mut *handle };
    let tracks = core.list_tracks();
    let json = match serde_json::to_string(&tracks.iter().map(|t| &t.path).collect::<Vec<_>>()) {
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
pub extern "C" fn liplayer_play_track_at(handle: *mut PlayerCore, index: usize) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &mut *handle };
    match core.play_track_at(index) {
        Ok(()) => OK,
        Err(_) => ERR_OP,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_stop(handle: *mut PlayerCore) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &mut *handle };
    core.stop();
    OK
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_pause(handle: *mut PlayerCore) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &mut *handle };
    core.pause();
    OK
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_resume(handle: *mut PlayerCore) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &mut *handle };
    core.resume();
    OK
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_elapsed_millis(handle: *mut PlayerCore) -> c_ulonglong {
    if handle.is_null() {
        return 0;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &mut *handle };
    core.elapsed_millis() as c_ulonglong
}

#[unsafe(no_mangle)]
pub extern "C" fn liplayer_state(handle: *mut PlayerCore) -> c_int {
    if handle.is_null() {
        return ERR_NULL;
    }
    // SAFETY: handle is checked for null and points to a valid PlayerCore by API contract.
    let core = unsafe { &mut *handle };
    match core.state() {
        PlayerState::Idle => 0,
        PlayerState::Playing => 1,
        PlayerState::Paused => 2,
    }
}

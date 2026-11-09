//! C ABI over `city-core`. The same exports serve WebAssembly and native
//! engines (Unity via P/Invoke). All strings are NUL-terminated UTF-8 JSON;
//! strings returned here must be released with `city_string_free`.

use city_core::sim::{Event, Simulation};
use city_core::{District, DistrictInput, district_hash, generate};
use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};

pub struct Session {
    district: District,
    simulation: Simulation,
}

thread_local! {
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

fn fail(message: String) {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = message);
}

fn to_c(text: String) -> *mut c_char {
    CString::new(text)
        .expect("JSON output has no interior NUL")
        .into_raw()
}

unsafe fn read(text: *const c_char) -> Option<String> {
    if text.is_null() {
        fail("null string".into());
        return None;
    }
    // SAFETY: the caller guarantees `text` is a valid NUL-terminated string.
    match unsafe { CStr::from_ptr(text) }.to_str() {
        Ok(value) => Some(value.to_owned()),
        Err(error) => {
            fail(error.to_string());
            None
        }
    }
}

/// Returns a session, or null with `city_last_error` set.
///
/// # Safety
/// `input` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_generate(input: *const c_char) -> *mut Session {
    let Some(text) = (unsafe { read(input) }) else {
        return std::ptr::null_mut();
    };
    let parsed: DistrictInput = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            fail(error.to_string());
            return std::ptr::null_mut();
        }
    };
    match generate(&parsed) {
        Ok(district) => {
            let simulation = Simulation::new(&district);
            Box::into_raw(Box::new(Session {
                district,
                simulation,
            }))
        }
        Err(error) => {
            fail(error.to_string());
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `session` must come from `city_generate` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_district_json(session: *const Session) -> *mut c_char {
    let session = unsafe { &*session };
    to_c(serde_json::to_string(&session.district).expect("district serializes"))
}

/// Hash as a hex string: 64-bit integers do not survive JSON in JS or C#.
///
/// # Safety
/// `session` must come from `city_generate` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_hash(session: *const Session) -> *mut c_char {
    let session = unsafe { &*session };
    to_c(format!("{:016x}", district_hash(&session.district)))
}

/// Returns a snapshot of the simulation, or null with `city_last_error` set.
///
/// # Safety
/// `session` must come from `city_generate`; `event` must be NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_apply_event(
    session: *mut Session,
    event: *const c_char,
) -> *mut c_char {
    let session = unsafe { &mut *session };
    let Some(text) = (unsafe { read(event) }) else {
        return std::ptr::null_mut();
    };
    let parsed: Event = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            fail(error.to_string());
            return std::ptr::null_mut();
        }
    };
    match session.simulation.apply(&session.district, &parsed) {
        Ok(snapshot) => to_c(serde_json::to_string(&snapshot).expect("snapshot serializes")),
        Err(error) => {
            fail(error.to_string());
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `session` must come from `city_generate` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_reset(session: *mut Session) -> *mut c_char {
    let session = unsafe { &mut *session };
    session.simulation = Simulation::new(&session.district);
    let snapshot = session.simulation.snapshot(&session.district);
    to_c(serde_json::to_string(&snapshot).expect("snapshot serializes"))
}

#[unsafe(no_mangle)]
pub extern "C" fn city_last_error() -> *mut c_char {
    to_c(LAST_ERROR.with(|slot| slot.borrow().replace('\0', " ")))
}

/// # Safety
/// `session` must come from `city_generate` and be freed only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_free(session: *mut Session) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// # Safety
/// `text` must be a string returned by this library, freed only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_string_free(text: *mut c_char) {
    if !text.is_null() {
        drop(unsafe { CString::from_raw(text) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn city_alloc(length: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(length);
    let pointer = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    pointer
}

/// # Safety
/// `pointer` must come from `city_alloc` with the same `length`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_dealloc(pointer: *mut u8, length: usize) {
    drop(unsafe { Vec::from_raw_parts(pointer, 0, length) });
}

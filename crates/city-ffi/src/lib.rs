//! C ABI over `city-core`. The same exports serve WebAssembly and native
//! engines (Unity via P/Invoke). All strings are NUL-terminated UTF-8 JSON;
//! strings returned here must be released with `city_string_free`.

use city_core::city::{CityEvent, CityInput, CitySession, city_hash, generate_city};
use city_core::regions::regions;
use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::ptr::null_mut;

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

unsafe fn parse<T: serde::de::DeserializeOwned>(text: *const c_char) -> Option<T> {
    if text.is_null() {
        fail("null string".into());
        return None;
    }
    // SAFETY: the caller guarantees `text` is a valid NUL-terminated string.
    let bytes = unsafe { CStr::from_ptr(text) }.to_bytes();
    match serde_json::from_slice(bytes) {
        Ok(value) => Some(value),
        Err(error) => {
            fail(error.to_string());
            None
        }
    }
}

fn json<T: serde::Serialize>(value: &T) -> *mut c_char {
    to_c(serde_json::to_string(value).expect("model serializes"))
}

/// The regional presets, each with the profile to send to `city_generate`.
#[unsafe(no_mangle)]
pub extern "C" fn city_regions() -> *mut c_char {
    json(&regions())
}

/// Returns a session, or null with `city_last_error` set.
///
/// # Safety
/// `input` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_generate(input: *const c_char) -> *mut CitySession {
    let Some(parsed) = (unsafe { parse::<CityInput>(input) }) else {
        return null_mut();
    };
    match generate_city(&parsed) {
        Ok(city) => Box::into_raw(Box::new(CitySession::new(city))),
        Err(error) => {
            fail(error.to_string());
            null_mut()
        }
    }
}

/// # Safety
/// `session` must come from `city_generate` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_json(session: *const CitySession) -> *mut c_char {
    json(unsafe { &*session }.city())
}

/// Hash as a hex string: 64-bit integers do not survive JSON in JS or C#.
///
/// # Safety
/// `session` must come from `city_generate` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_hash_hex(session: *const CitySession) -> *mut c_char {
    to_c(format!("{:016x}", city_hash(unsafe { &*session }.city())))
}

/// Generates the district if needed. Returns null with `city_last_error` set
/// when the id is unknown.
///
/// # Safety
/// `session` must come from `city_generate` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_district_json(
    session: *mut CitySession,
    district: usize,
) -> *mut c_char {
    match unsafe { &mut *session }.detail(district) {
        Ok(detailed) => json(detailed),
        Err(error) => {
            fail(error.to_string());
            null_mut()
        }
    }
}

/// # Safety
/// `session` must come from `city_generate` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_snapshot(session: *const CitySession) -> *mut c_char {
    json(&unsafe { &*session }.snapshot())
}

/// Returns a snapshot, or null with `city_last_error` set.
///
/// # Safety
/// `session` must come from `city_generate`; `event` must be NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_apply_event(
    session: *mut CitySession,
    event: *const c_char,
) -> *mut c_char {
    let Some(parsed) = (unsafe { parse::<CityEvent>(event) }) else {
        return null_mut();
    };
    match unsafe { &mut *session }.apply(&parsed) {
        Ok(snapshot) => json(&snapshot),
        Err(error) => {
            fail(error.to_string());
            null_mut()
        }
    }
}

/// # Safety
/// `session` must come from `city_generate` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_reset(session: *mut CitySession) -> *mut c_char {
    json(&unsafe { &mut *session }.reset())
}

#[unsafe(no_mangle)]
pub extern "C" fn city_last_error() -> *mut c_char {
    to_c(LAST_ERROR.with(|slot| slot.borrow().replace('\0', " ")))
}

/// # Safety
/// `session` must come from `city_generate` and be freed only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn city_free(session: *mut CitySession) {
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

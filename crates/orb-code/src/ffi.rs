use std::{
    ffi::{CStr, CString, c_char},
    ptr,
    sync::Mutex,
};

use crate::{
    GenerateOrbRequest, OrbId, derive_orb_id, generate_orb_image, scan_orb_image,
    scan_orb_image_from_luma8, verify_orb_image,
};

static LAST_ERROR_MESSAGE: Mutex<Option<CString>> = Mutex::new(None);

#[repr(C)]
pub struct OrbCodeBuffer {
    pub data: *mut u8,
    pub len: usize,
}

#[unsafe(no_mangle)]
pub extern "C" fn orb_code_generate_png_from_id(
    orb_id: *const c_char,
    image_size: u32,
) -> OrbCodeBuffer {
    wrap_buffer_result(|| {
        let orb_id = string_from_ptr(orb_id)?;
        let request = GenerateOrbRequest::new(OrbId::parse(orb_id)?).with_image_size(image_size)?;
        let orb_image = generate_orb_image(&request)?;
        orb_image.to_png_bytes()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn orb_code_generate_png_from_data(
    input: *const c_char,
    image_size: u32,
) -> OrbCodeBuffer {
    wrap_buffer_result(|| {
        let input = string_from_ptr(input)?;
        let request = GenerateOrbRequest::new(derive_orb_id(&input)).with_image_size(image_size)?;
        let orb_image = generate_orb_image(&request)?;
        orb_image.to_png_bytes()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn orb_code_derive_id_from_data(input: *const c_char) -> *mut c_char {
    wrap_string_result(|| {
        let input = string_from_ptr(input)?;
        Ok(derive_orb_id(&input).to_string())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn orb_code_scan_png(data: *const u8, len: usize) -> *mut c_char {
    wrap_string_result(|| {
        let bytes = bytes_from_raw_parts(data, len)?;
        let scan_result = scan_orb_image(bytes)?;
        Ok(scan_result.orb_id.to_string())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn orb_code_scan_luma8(
    data: *const u8,
    len: usize,
    width: u32,
    height: u32,
) -> *mut c_char {
    wrap_string_result(|| {
        let bytes = bytes_from_raw_parts(data, len)?;
        let scan_result = scan_orb_image_from_luma8(bytes, width, height)?;
        Ok(scan_result.orb_id.to_string())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn orb_code_verify_png(data: *const u8, len: usize) -> bool {
    match bytes_from_raw_parts(data, len).and_then(verify_orb_image) {
        Ok(result) => {
            clear_last_error();
            result.is_match
        }
        Err(error) => {
            store_last_error(error.to_string());
            false
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn orb_code_last_error_message() -> *mut c_char {
    let guard = LAST_ERROR_MESSAGE
        .lock()
        .expect("last error mutex should not be poisoned");
    match guard.as_ref() {
        Some(message) => message.clone().into_raw(),
        None => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn orb_code_string_free(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    // SAFETY: `value` must be a pointer previously returned by `CString::into_raw`.
    unsafe {
        let _ = CString::from_raw(value);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn orb_code_buffer_free(buffer: OrbCodeBuffer) {
    if buffer.data.is_null() || buffer.len == 0 {
        return;
    }
    // SAFETY: `data` and `len` must come from `Vec::into_raw_parts` in this crate.
    unsafe {
        let _ = Vec::from_raw_parts(buffer.data, buffer.len, buffer.len);
    }
}

fn wrap_buffer_result(operation: impl FnOnce() -> crate::Result<Vec<u8>>) -> OrbCodeBuffer {
    match operation() {
        Ok(bytes) => {
            clear_last_error();
            let mut bytes = bytes;
            let buffer = OrbCodeBuffer {
                data: bytes.as_mut_ptr(),
                len: bytes.len(),
            };
            std::mem::forget(bytes);
            buffer
        }
        Err(error) => {
            store_last_error(error.to_string());
            OrbCodeBuffer {
                data: ptr::null_mut(),
                len: 0,
            }
        }
    }
}

fn wrap_string_result(operation: impl FnOnce() -> crate::Result<String>) -> *mut c_char {
    match operation() {
        Ok(value) => {
            clear_last_error();
            match CString::new(value) {
                Ok(value) => value.into_raw(),
                Err(error) => {
                    store_last_error(format!("string contains interior null byte: {error}"));
                    ptr::null_mut()
                }
            }
        }
        Err(error) => {
            store_last_error(error.to_string());
            ptr::null_mut()
        }
    }
}

fn string_from_ptr(value: *const c_char) -> crate::Result<String> {
    if value.is_null() {
        return Err(crate::OrbError::MalformedPayload);
    }
    // SAFETY: `value` must be a valid NUL-terminated C string owned by the caller.
    let c_str = unsafe { CStr::from_ptr(value) };
    Ok(c_str.to_string_lossy().into_owned())
}

fn bytes_from_raw_parts<'a>(data: *const u8, len: usize) -> crate::Result<&'a [u8]> {
    if data.is_null() || len == 0 {
        return Err(crate::OrbError::PayloadNotFound);
    }
    // SAFETY: caller promises `data` points to `len` readable bytes for the duration of the call.
    Ok(unsafe { std::slice::from_raw_parts(data, len) })
}

fn store_last_error(message: String) {
    let sanitized = message.replace('\0', " ");
    let mut guard = LAST_ERROR_MESSAGE
        .lock()
        .expect("last error mutex should not be poisoned");
    *guard = CString::new(sanitized).ok();
}

fn clear_last_error() {
    let mut guard = LAST_ERROR_MESSAGE
        .lock()
        .expect("last error mutex should not be poisoned");
    *guard = None;
}

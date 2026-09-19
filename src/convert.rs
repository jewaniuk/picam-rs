use std::ffi::CStr;

use crate::error::{Error, Result};
use crate::sys;

/// Converts a fixed-size, nul-terminated C character array into an owned,
/// lossily-converted `String`.
pub(crate) fn c_char_array_to_string(chars: &[sys::pichar]) -> String {
    let bytes: Vec<u8> = chars.iter().map(|&c| c as u8).collect();
    match CStr::from_bytes_until_nul(&bytes) {
        Ok(c_str) => c_str.to_string_lossy().into_owned(),
        Err(_) => String::new(), // no nul terminator found; shouldn't happen
    }
}

/// Converts a `&str` into a fixed-size, nul-terminated C character array.
///
/// `N` is inferred from context at each call site (e.g. `PicamCameraID`'s
/// own field types), so there's no magic number to keep in sync by hand.
pub(crate) fn string_to_c_char_array<const N: usize>(value: &str) -> Result<[sys::pichar; N]> {
    if value.len() >= N {
        return Err(Error::ValueTooLong);
    }
    let mut array = [0 as sys::pichar; N];
    for (dst, &byte) in array.iter_mut().zip(value.as_bytes()) {
        *dst = byte as sys::pichar;
    }
    Ok(array)
}

/// A single firmware component's name and version/detail string, as
/// reported by a camera or accessory.
#[derive(Debug, Clone)]
pub struct FirmwareDetail {
    pub name: String,
    pub detail: String,
}

impl FirmwareDetail {
    pub(crate) fn from_raw(raw: &sys::PicamFirmwareDetail) -> Self {
        Self {
            name: c_char_array_to_string(&raw.name),
            detail: c_char_array_to_string(&raw.detail),
        }
    }
}

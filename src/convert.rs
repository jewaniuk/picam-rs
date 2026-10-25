use std::ffi::CStr;

use crate::error::{Error, Result, picam_call};
use crate::library::Library;
use crate::sys;

macro_rules! define_picam_enum {
    (
        $(#[$enum_meta:meta])*
        $enum_name:ident,
        $raw_ty:path,
        $enumerated:ident,
        $($(#[$meta:meta])* $name:ident = $raw:ident),* $(,)?
    ) => {
        $(#[$enum_meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $enum_name {
            $($(#[$meta])* $name,)*
            /// A value this crate doesn't recognize, most likely because
            /// the installed PICam runtime is newer than the SDK version
            /// these bindings were generated against.
            Unknown(i32),
        }

        impl $enum_name {
            pub(crate) fn from_raw(raw: $raw_ty) -> Self {
                match raw {
                    $(<$raw_ty>::$raw => $enum_name::$name,)*
                    other => $enum_name::Unknown(other.0),
                }
            }

            pub(crate) fn to_raw(self) -> $raw_ty {
                match self {
                    $($enum_name::$name => <$raw_ty>::$raw,)*
                    $enum_name::Unknown(code) => $raw_ty(code),
                }
            }

            /// Gets PICam's name for this value.
            ///
            /// # Errors
            ///
            /// Returns an error if PICam is unable to name this value.
            pub fn name(self, library: &$crate::Library) -> $crate::Result<String> {
                $crate::convert::enumeration_string(
                    library,
                    $crate::sys::PicamEnumeratedType::$enumerated,
                    self.to_raw().0,
                )
            }
        }
    };
}
pub(crate) use define_picam_enum;

/// Checks that every raw value in a range survives a round trip through
/// a wrapper, and that exactly `known` of them map to a named variant.
#[cfg(test)]
macro_rules! assert_round_trips {
    ($enum_name:ident, $raw_ty:path, $known:expr) => {
        let mut named = 0;
        for code in -10..1000 {
            let value = $enum_name::from_raw($raw_ty(code));
            assert_eq!(value.to_raw().0, code, "{value:?} should keep {code}");
            if !matches!(value, $enum_name::Unknown(_)) {
                named += 1;
            }
        }
        assert_eq!(named, $known, "every value in picam.h should be named");
    };
}
#[cfg(test)]
pub(crate) use assert_round_trips;

/// Gets PICam's name for `value`, a value of the enum `enumerated_type`
/// identifies.
///
/// `_library` is only there as proof that PICam is initialized, which
/// `Picam_GetEnumerationString` needs.
pub(crate) fn enumeration_string(
    _library: &Library,
    enumerated_type: sys::PicamEnumeratedType,
    value: sys::piint,
) -> Result<String> {
    let mut ptr: *const sys::pichar = std::ptr::null();
    picam_call!(sys::Picam_GetEnumerationString(
        enumerated_type,
        value,
        &mut ptr
    ))?;

    if ptr.is_null() {
        return Err(Error::UnexpectedNullPointer);
    }

    // SAFETY: just checked `ptr` is non-null, and on success PICam guarantees
    // it points to a nul-terminated string it allocated (PICam manual 3.2.6),
    // which stays valid until `Picam_DestroyString` below
    let name = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    picam_call!(sys::Picam_DestroyString(ptr))?;
    Ok(name)
}

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

/// Converts a PICam-allocated array into an owned `Vec`, treating a null
/// pointer (PICam's convention for "nothing to report" in at least some
/// functions) as an empty `Vec` rather than dereferencing it.
///
/// # Safety
///
/// If `array` is non-null, it must point to `count` valid, contiguous
/// `T`s for the duration of this call.
pub(crate) unsafe fn collect_raw_array<T, U>(
    array: *const T,
    count: sys::piint,
    convert: impl Fn(&T) -> U,
) -> Vec<U> {
    if array.is_null() {
        return Vec::new();
    }

    // SAFETY: caller guarantees `array`/`count` are valid when non-null
    unsafe { std::slice::from_raw_parts(array, count as usize) }
        .iter()
        .map(convert)
        .collect()
}

/// A single firmware component's name and version/detail string, as
/// reported by a camera or accessory.
#[derive(Debug, Clone)]
pub struct FirmwareDetail {
    /// The name of the firmware component.
    pub name: String,
    /// The component's version or other detail.
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

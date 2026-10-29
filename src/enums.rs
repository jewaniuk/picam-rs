//! The value enums of PICam's enum parameters (shutter timing, ADC quality,
//! trigger source and so on), generated from the headers into
//! `generated/enums.rs`.
//!
//! Each one is a parameter value type: it is read and written through
//! PICam's integer calls, which accept enum parameters (PICam manual 5.4.2.1,
//! 5.4.2.3, 5.4.2.7 and 5.4.2.12). 5.4.2.2 lists no value types for
//! can-set, but `tests/handles.rs` checks it accepts every enum handle on the
//! demo cameras. None is online-settable, since PICam only sets integer
//! parameters online (5.4.1.20).

use std::sync::mpsc::Receiver;

use crate::convert::define_picam_enum;
use crate::error::{Error, Result};
use crate::parameter::sealed::{self, RawHandle};
use crate::parameter::{
    EnumeratedType, Parameter, PicamValue, ReadablePicamValue, WaitablePicamValue, enumerated_type,
};
use crate::sys;
use crate::watch::{self, Event, Kind, Watch};

/// Checks that `parameter`'s values belong to `expected`, for an untyped call
/// that names an enum PICam can't check itself.
///
/// # Errors
///
/// Returns [`Error::EnumeratedTypeMismatch`] if they belong to another enum,
/// or an error if PICam is unable to say which enum they belong to.
fn check_enumerated_type(
    handle: RawHandle,
    parameter: Parameter,
    expected: sys::PicamEnumeratedType,
) -> Result<()> {
    let found = enumerated_type(handle.0, parameter)?;
    if found == expected {
        Ok(())
    } else {
        Err(Error::EnumeratedTypeMismatch {
            expected: EnumeratedType::from_raw(expected),
            found: EnumeratedType::from_raw(found),
        })
    }
}

/// Makes a value enum a parameter value type, converting to and from the
/// `i32` PICam's integer calls use with `$from` and `$to`.
macro_rules! impl_value_via_i32 {
    ($ty:ident, $enumerated:ident, $from:expr, $to:expr) => {
        impl sealed::RawPicamValue for $ty {
            fn get(handle: RawHandle, parameter: Parameter) -> Result<Self> {
                <i32 as sealed::RawPicamValue>::get(handle, parameter).map($from)
            }

            fn set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<()> {
                <i32 as sealed::RawPicamValue>::set(handle, parameter, $to(value))
            }

            fn default_value(handle: RawHandle, parameter: Parameter) -> Result<Self> {
                <i32 as sealed::RawPicamValue>::default_value(handle, parameter).map($from)
            }

            fn can_set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<bool> {
                <i32 as sealed::RawPicamValue>::can_set(handle, parameter, $to(value))
            }

            fn check_type(handle: RawHandle, parameter: Parameter) -> Result<()> {
                check_enumerated_type(handle, parameter, sys::PicamEnumeratedType::$enumerated)
            }

            fn watch(handle: RawHandle, parameter: Parameter) -> Result<(Watch, Receiver<Self>)> {
                watch::subscribe(handle.0, parameter, Kind::Integer, |event| match event {
                    Event::Integer(code) => Some(($from)(*code)),
                    _ => None,
                })
            }
        }

        impl PicamValue for $ty {}

        impl sealed::RawReadablePicamValue for $ty {
            fn read(handle: RawHandle, parameter: Parameter) -> Result<Self> {
                <i32 as sealed::RawReadablePicamValue>::read(handle, parameter).map($from)
            }

            fn watch_extrinsic(
                handle: RawHandle,
                parameter: Parameter,
            ) -> Result<(Watch, Receiver<Self>)> {
                watch::subscribe(
                    handle.0,
                    parameter,
                    Kind::ExtrinsicInteger,
                    |event| match event {
                        Event::Integer(code) => Some(($from)(*code)),
                        _ => None,
                    },
                )
            }
        }

        impl ReadablePicamValue for $ty {}

        impl sealed::RawWaitablePicamValue for $ty {
            fn to_status(self) -> i32 {
                $to(self)
            }

            fn from_status(code: i32) -> Self {
                ($from)(code)
            }
        }

        impl WaitablePicamValue for $ty {}
    };
}

/// Makes a `define_picam_enum!` enum a parameter value type.
macro_rules! impl_enum_value {
    ($ty:ident, $raw_ty:path, $enumerated:ident) => {
        impl_value_via_i32!(
            $ty,
            $enumerated,
            |code| $ty::from_raw($raw_ty(code)),
            |value: $ty| value.to_raw().0
        );
    };
}

/// Makes a `bitflags!` mask a parameter value type, keeping bits this crate
/// doesn't recognize.
macro_rules! impl_mask_value {
    ($ty:ident, $enumerated:ident) => {
        impl_value_via_i32!($ty, $enumerated, $ty::from_bits_retain, |value: $ty| value
            .bits());
    };
}

include!("generated/enums.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::assert_round_trips;

    #[test]
    fn generated_enums_round_trip() {
        assert_round_trips!(AdcQuality, sys::PicamAdcQuality, 6);
        assert_round_trips!(ShutterTimingMode, sys::PicamShutterTimingMode, 4);
    }

    #[test]
    fn masks_keep_unknown_bits() {
        // a bit picam 5.15 doesn't define, as a newer runtime might send
        let unknown = 0x40;
        let code = Orientation::FLIPPED_HORIZONTALLY.bits() | unknown;
        let orientation = Orientation::from_bits_retain(code);
        assert!(orientation.contains(Orientation::FLIPPED_HORIZONTALLY));
        assert!(!orientation.contains(Orientation::FLIPPED_VERTICALLY));
        assert_eq!(orientation.bits(), code);
    }
}

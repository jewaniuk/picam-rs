//! PICam parameter identifiers and value access.

use crate::convert::{collect_raw_array, define_picam_enum, enumeration_string};
use crate::error::{Result, picam_call};
use crate::library::Library;
use crate::sys;

define_picam_enum!(
    /// How a camera connects to the computer (PICam manual 4.1).
    ComputerInterface,
    sys::PicamComputerInterface,
    PicamEnumeratedType_ComputerInterface,
    /// USB 2.0.
    Usb2 = PicamComputerInterface_Usb2,
    /// USB 3.0.
    Usb3 = PicamComputerInterface_Usb3,
    /// USB 3.1.
    Usb31 = PicamComputerInterface_Usb31,
    /// IEEE 1394a (FireWire).
    Ieee1394A = PicamComputerInterface_1394A,
    /// Gigabit Ethernet.
    GigabitEthernet = PicamComputerInterface_GigabitEthernet,
);
define_picam_enum!(
    /// The kind of value a parameter holds, which decides the Rust type used
    /// to get and set it (PICam manual 5.1.1.42).
    ValueType,
    sys::PicamValueType,
    PicamEnumeratedType_ValueType,
    /// A whole number, used as `i32`.
    Integer = PicamValueType_Integer,
    /// A true/false value, used as `i32` (zero is false).
    Boolean = PicamValueType_Boolean,
    /// One value of a PICam enum, used as `i32`. [`crate::Camera::enumerated_type`]
    /// says which enum.
    Enumeration = PicamValueType_Enumeration,
    /// A 64-bit whole number, used as `i64`.
    LargeInteger = PicamValueType_LargeInteger,
    /// A floating-point number, used as `f64`.
    FloatingPoint = PicamValueType_FloatingPoint,
    /// A set of regions of interest, used as `Vec<Roi>`.
    Rois = PicamValueType_Rois,
    /// A gate pulse, used as [`Pulse`].
    Pulse = PicamValueType_Pulse,
    /// A custom modulation sequence, used as `Vec<Modulation>`.
    Modulations = PicamValueType_Modulations,
);
define_picam_enum!(
    /// How a parameter's value may be accessed (PICam manual 5.1.2.1).
    ValueAccess,
    sys::PicamValueAccess,
    PicamEnumeratedType_ValueAccess,
    /// The value can only be read.
    ReadOnly = PicamValueAccess_ReadOnly,
    /// The value can be read and written, but only one value is allowed.
    ReadWriteTrivial = PicamValueAccess_ReadWriteTrivial,
    /// The value can be read and written.
    ReadWrite = PicamValueAccess_ReadWrite,
);
include!("generated/enumerated_types.rs");

macro_rules! define_parameters {
    ($($(#[$meta:meta])* $name:ident = $raw:ident),* $(,)?) => {
        /// Identifies a specific camera parameter (exposure time, ROI,
        /// gain, and so on).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Parameter {
            $($(#[$meta])* $name,)*
            /// A parameter this crate doesn't recognize, most likely
            /// because the installed PICam runtime is newer than the
            /// SDK version these bindings were generated against.
            Unknown(i32),
        }

        impl Parameter {
            pub(crate) fn from_raw(raw: sys::PicamParameter) -> Self {
                match raw {
                    $(sys::PicamParameter::$raw => Parameter::$name,)*
                    other => Parameter::Unknown(other.0),
                }
            }

            pub(crate) fn to_raw(self) -> sys::PicamParameter {
                match self {
                    $(Parameter::$name => sys::PicamParameter::$raw,)*
                    Parameter::Unknown(code) => sys::PicamParameter(code),
                }
            }

            /// Gets PICam's name for this parameter.
            ///
            /// # Errors
            ///
            /// Returns an error if PICam is unable to name this parameter.
            pub fn name(self, library: &Library) -> Result<String> {
                enumeration_string(
                    library,
                    sys::PicamEnumeratedType::PicamEnumeratedType_Parameter,
                    self.to_raw().0,
                )
            }
        }
    };
}

include!("generated/parameters.rs");

/// The crate-private halves of the value traits, which seal them.
///
/// The public traits ([`PicamValue`], [`ReadablePicamValue`] and
/// [`OnlineSettablePicamValue`]) have no methods of their own, they only
/// require the supertraits here. Users can't name these supertraits, so they
/// can't implement the public traits for their own types:
///
/// ```compile_fail
/// struct Mine;
/// impl picam_rs::PicamValue for Mine {}
/// ```
///
/// A generic bound still makes the supertrait methods visible, but they take a
/// [`RawHandle`], which only this crate can construct, so users can't call
/// them with an arbitrary pointer either:
///
/// ```compile_fail
/// fn get<T: picam_rs::PicamValue>() -> picam_rs::Result<T> {
///     T::get(std::ptr::null_mut(), picam_rs::Parameter::ExposureTime)
/// }
/// ```
pub(crate) mod sealed {
    use super::Parameter;
    use crate::error::Result;
    use crate::sys;

    /// A raw camera handle that only this crate can construct.
    ///
    /// Taking this instead of a bare `sys::PicamHandle` keeps the supertrait
    /// methods below from being called outside the crate.
    #[derive(Debug, Clone, Copy)]
    pub struct RawHandle(pub(crate) sys::PicamHandle);

    /// The handle-taking half of [`super::PicamValue`].
    pub trait RawPicamValue: Sized {
        /// Gets the current value of `parameter` on `handle`.
        ///
        /// # Errors
        ///
        /// Returns an error if PICam is unable to get the value.
        fn get(handle: RawHandle, parameter: Parameter) -> Result<Self>;

        /// Sets `parameter` on `handle` to `value`.
        ///
        /// # Errors
        ///
        /// Returns an error if PICam is unable to set the value.
        fn set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<()>;

        /// Gets the factory default value of `parameter` on `handle`.
        ///
        /// # Errors
        ///
        /// Returns an error if PICam is unable to get the default value.
        fn default_value(handle: RawHandle, parameter: Parameter) -> Result<Self>;

        /// Reports whether `value` could currently be set for `parameter` on
        /// `handle`, without actually setting it.
        ///
        /// # Errors
        ///
        /// Returns an error if PICam is unable to check this.
        fn can_set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<bool>;
    }

    /// The handle-taking half of [`super::ReadablePicamValue`].
    pub trait RawReadablePicamValue: RawPicamValue {
        /// Reads the current value of `parameter` directly from hardware.
        ///
        /// # Errors
        ///
        /// Returns an error if PICam is unable to read the value.
        fn read(handle: RawHandle, parameter: Parameter) -> Result<Self>;
    }

    /// The handle-taking half of [`super::OnlineSettablePicamValue`].
    pub trait RawOnlineSettablePicamValue: RawPicamValue {
        /// Sets `parameter` to `value` and applies it immediately.
        ///
        /// # Errors
        ///
        /// Returns an error if PICam is unable to set the value online.
        fn set_online(handle: RawHandle, parameter: Parameter, value: Self) -> Result<()>;
    }
}

use sealed::RawHandle;

/// A value type PICam can get or set for a parameter.
///
/// This trait is sealed: it is implemented for `i32`, `i64`, `f64`,
/// `Vec<Roi>`, `Vec<Modulation>` and [`Pulse`], and can't be implemented
/// outside this crate. It is meant to be used as a bound:
///
/// ```no_run
/// use picam_rs::{Camera, Parameter, PicamValue, Result};
///
/// fn current<T: PicamValue>(camera: &Camera, parameter: Parameter) -> Result<T> {
///     camera.get(parameter)
/// }
/// ```
pub trait PicamValue: sealed::RawPicamValue {}

macro_rules! impl_picam_value {
    ($ty:ty, $get:ident, $set:ident, $default:ident, $can_set:ident) => {
        impl sealed::RawPicamValue for $ty {
            fn get(handle: RawHandle, parameter: Parameter) -> Result<Self> {
                let mut value: $ty = Default::default();
                picam_call!(sys::$get(handle.0, parameter.to_raw(), &mut value))?;
                Ok(value)
            }

            fn set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<()> {
                picam_call!(sys::$set(handle.0, parameter.to_raw(), value))
            }

            fn default_value(handle: RawHandle, parameter: Parameter) -> Result<Self> {
                let mut value: $ty = Default::default();
                picam_call!(sys::$default(handle.0, parameter.to_raw(), &mut value))?;
                Ok(value)
            }

            fn can_set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<bool> {
                let mut settable: sys::pibln = 0;
                picam_call!(sys::$can_set(
                    handle.0,
                    parameter.to_raw(),
                    value,
                    &mut settable
                ))?;
                Ok(settable != 0)
            }
        }

        impl PicamValue for $ty {}
    };
}

impl_picam_value!(
    sys::piint,
    Picam_GetParameterIntegerValue,
    Picam_SetParameterIntegerValue,
    Picam_GetParameterIntegerDefaultValue,
    Picam_CanSetParameterIntegerValue
);
impl_picam_value!(
    sys::pi64s,
    Picam_GetParameterLargeIntegerValue,
    Picam_SetParameterLargeIntegerValue,
    Picam_GetParameterLargeIntegerDefaultValue,
    Picam_CanSetParameterLargeIntegerValue
);
impl_picam_value!(
    sys::piflt,
    Picam_GetParameterFloatingPointValue,
    Picam_SetParameterFloatingPointValue,
    Picam_GetParameterFloatingPointDefaultValue,
    Picam_CanSetParameterFloatingPointValue
);

macro_rules! impl_bool_query {
    ($name:ident, $raw:ident) => {
        pub(crate) fn $name(handle: sys::PicamHandle, parameter: Parameter) -> Result<bool> {
            let mut result: sys::pibln = 0;
            picam_call!(sys::$raw(handle, parameter.to_raw(), &mut result))?;
            Ok(result != 0)
        }
    };
}

macro_rules! impl_parameter_info {
    ($name:ident, $raw:ident, $ty:ty) => {
        pub(crate) fn $name(handle: sys::PicamHandle, parameter: Parameter) -> Result<$ty> {
            let mut result = std::mem::MaybeUninit::<$ty>::uninit();
            picam_call!(sys::$raw(handle, parameter.to_raw(), result.as_mut_ptr()))?;
            // SAFETY: on success, PICam guarantees this out-parameter was
            // fully written with a valid value.
            Ok(unsafe { result.assume_init() })
        }
    };
}

impl_bool_query!(can_set_online, Picam_CanSetParameterOnline);
impl_bool_query!(can_read, Picam_CanReadParameter);
impl_bool_query!(can_wait_for_status, Picam_CanWaitForStatusParameter);
impl_bool_query!(is_relevant, Picam_IsParameterRelevant);

impl_parameter_info!(value_type, Picam_GetParameterValueType, sys::PicamValueType);
impl_parameter_info!(
    enumerated_type,
    Picam_GetParameterEnumeratedType,
    sys::PicamEnumeratedType
);
impl_parameter_info!(
    value_access,
    Picam_GetParameterValueAccess,
    sys::PicamValueAccess
);
impl_parameter_info!(
    constraint_type,
    Picam_GetParameterConstraintType,
    sys::PicamConstraintType
);

/// A value type that can be read directly from hardware, bypassing PICam's
/// committed-value cache.
///
/// This trait is sealed: it is implemented only for `i32` and `f64`, the two
/// value types PICam supports this for, and can't be implemented outside this
/// crate.
pub trait ReadablePicamValue: PicamValue + sealed::RawReadablePicamValue {}

macro_rules! impl_readable_value {
    ($ty:ty, $read:ident) => {
        impl sealed::RawReadablePicamValue for $ty {
            fn read(handle: RawHandle, parameter: Parameter) -> Result<Self> {
                let mut value: $ty = Default::default();
                picam_call!(sys::$read(handle.0, parameter.to_raw(), &mut value))?;
                Ok(value)
            }
        }

        impl ReadablePicamValue for $ty {}
    };
}

impl_readable_value!(sys::piint, Picam_ReadParameterIntegerValue);
impl_readable_value!(sys::piflt, Picam_ReadParameterFloatingPointValue);

/// A value type that can be set and applied to hardware immediately,
/// without needing [`crate::camera::Camera::commit_parameters`].
///
/// This trait is sealed: it is implemented only for `i32`, `f64` and
/// [`Pulse`], the three value types PICam supports this for, and can't be
/// implemented outside this crate.
pub trait OnlineSettablePicamValue: PicamValue + sealed::RawOnlineSettablePicamValue {}

macro_rules! impl_online_settable_scalar {
    ($ty:ty, $set_online:ident) => {
        impl sealed::RawOnlineSettablePicamValue for $ty {
            fn set_online(handle: RawHandle, parameter: Parameter, value: Self) -> Result<()> {
                picam_call!(sys::$set_online(handle.0, parameter.to_raw(), value))
            }
        }

        impl OnlineSettablePicamValue for $ty {}
    };
}

impl_online_settable_scalar!(sys::piint, Picam_SetParameterIntegerValueOnline);
impl_online_settable_scalar!(sys::piflt, Picam_SetParameterFloatingPointValueOnline);

/// A single region of interest.
///
/// Field meanings and units follow PICam manual 5.2.1.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roi {
    /// The left-most column, counting from zero.
    pub x: sys::piint,
    /// How many columns the region spans.
    pub width: sys::piint,
    /// How many adjacent columns are summed into one.
    pub x_binning: sys::piint,
    /// The top-most row, counting from zero.
    pub y: sys::piint,
    /// How many rows the region spans.
    pub height: sys::piint,
    /// How many adjacent rows are summed into one.
    pub y_binning: sys::piint,
}

impl Roi {
    fn from_raw(raw: &sys::PicamRoi) -> Self {
        Self {
            x: raw.x,
            width: raw.width,
            x_binning: raw.x_binning,
            y: raw.y,
            height: raw.height,
            y_binning: raw.y_binning,
        }
    }

    fn to_raw(self) -> sys::PicamRoi {
        sys::PicamRoi {
            x: self.x,
            width: self.width,
            x_binning: self.x_binning,
            y: self.y,
            height: self.height,
            y_binning: self.y_binning,
        }
    }
}

impl sealed::RawPicamValue for Vec<Roi> {
    fn get(handle: RawHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamRois = std::ptr::null();
        picam_call!(sys::Picam_GetParameterRoisValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ))?;

        let rois = if ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: on success with a non-null pointer, PICam guarantees
            // `roi_array` has `roi_count` valid, contiguous entries
            unsafe {
                let raw = &*ptr;
                collect_raw_array(raw.roi_array, raw.roi_count, Roi::from_raw)
            }
        };

        if !ptr.is_null() {
            picam_call!(sys::Picam_DestroyRois(ptr))?;
        }
        Ok(rois)
    }

    fn set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<()> {
        // `raw` holds a pointer into the buffer for `raw_rois` via `.as_mut_ptr()`,
        // which the borrow checker can't see through; `raw_rois` must stay alive
        // and unmodified (no push/reallocation, which would invalidate the
        // pointer) until after the call below returns; PICam only reads through
        // `raw` for the duration of this one call and doesn't retain the pointer
        let mut raw_rois: Vec<sys::PicamRoi> = value.iter().map(|roi| roi.to_raw()).collect();
        let raw = sys::PicamRois {
            roi_array: raw_rois.as_mut_ptr(),
            roi_count: raw_rois.len() as sys::piint,
        };
        picam_call!(sys::Picam_SetParameterRoisValue(
            handle.0,
            parameter.to_raw(),
            &raw
        ))
    }

    fn default_value(handle: RawHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamRois = std::ptr::null();
        picam_call!(sys::Picam_GetParameterRoisDefaultValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ))?;

        let rois = if ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: on success with a non-null pointer, PICam guarantees
            // `roi_array` has `roi_count` valid, contiguous entries
            unsafe {
                let raw = &*ptr;
                collect_raw_array(raw.roi_array, raw.roi_count, Roi::from_raw)
            }
        };

        if !ptr.is_null() {
            picam_call!(sys::Picam_DestroyRois(ptr))?;
        }
        Ok(rois)
    }

    fn can_set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<bool> {
        let mut raw_rois: Vec<sys::PicamRoi> = value.iter().map(|roi| roi.to_raw()).collect();
        let raw = sys::PicamRois {
            roi_array: raw_rois.as_mut_ptr(),
            roi_count: raw_rois.len() as sys::piint,
        };
        let mut settable: sys::pibln = 0;
        picam_call!(sys::Picam_CanSetParameterRoisValue(
            handle.0,
            parameter.to_raw(),
            &raw,
            &mut settable
        ))?;
        Ok(settable != 0)
    }
}

impl PicamValue for Vec<Roi> {}

/// A single step in a custom intensifier modulation sequence.
///
/// Field meanings and units follow PICam manual 5.2.1.4.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Modulation {
    /// How long the intensifier modulates for, in ms.
    pub duration: sys::piflt,
    /// The intensifier modulation frequency, in MHz.
    pub frequency: sys::piflt,
    /// The intensifier's phase relative to the modulation output signal, in degrees.
    pub phase: sys::piflt,
    /// The frequency of the RF output signal, in MHz.
    pub output_signal_frequency: sys::piflt,
}

impl Modulation {
    fn from_raw(raw: &sys::PicamModulation) -> Self {
        Self {
            duration: raw.duration,
            frequency: raw.frequency,
            phase: raw.phase,
            output_signal_frequency: raw.output_signal_frequency,
        }
    }

    fn to_raw(self) -> sys::PicamModulation {
        sys::PicamModulation {
            duration: self.duration,
            frequency: self.frequency,
            phase: self.phase,
            output_signal_frequency: self.output_signal_frequency,
        }
    }
}

impl sealed::RawPicamValue for Vec<Modulation> {
    fn get(handle: RawHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamModulations = std::ptr::null();
        picam_call!(sys::Picam_GetParameterModulationsValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ))?;

        let modulations = if ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: on success with a non-null pointer, PICam guarantees
            // `modulation_array` has `modulation_count` valid, contiguous entries
            unsafe {
                let raw = &*ptr;
                collect_raw_array(
                    raw.modulation_array,
                    raw.modulation_count,
                    Modulation::from_raw,
                )
            }
        };

        if !ptr.is_null() {
            picam_call!(sys::Picam_DestroyModulations(ptr))?;
        }
        Ok(modulations)
    }

    fn set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<()> {
        // `raw` holds a pointer into the buffer for `raw_modulations` via `.as_mut_ptr()`,
        // which the borrow checker can't see through; `raw_modulations` must stay alive
        // and unmodified (no push/reallocation, which would invalidate the
        // pointer) until after the call below returns; PICam only reads through
        // `raw` for the duration of this one call and doesn't retain the pointer
        let mut raw_modulations: Vec<sys::PicamModulation> =
            value.iter().map(|m| m.to_raw()).collect();
        let raw = sys::PicamModulations {
            modulation_array: raw_modulations.as_mut_ptr(),
            modulation_count: raw_modulations.len() as sys::piint,
        };
        picam_call!(sys::Picam_SetParameterModulationsValue(
            handle.0,
            parameter.to_raw(),
            &raw
        ))
    }

    fn default_value(handle: RawHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamModulations = std::ptr::null();
        picam_call!(sys::Picam_GetParameterModulationsDefaultValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ))?;

        let modulations = if ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: on success with a non-null pointer, PICam guarantees
            // `modulation_array` has `modulation_count` valid, contiguous entries
            unsafe {
                let raw = &*ptr;
                collect_raw_array(
                    raw.modulation_array,
                    raw.modulation_count,
                    Modulation::from_raw,
                )
            }
        };

        if !ptr.is_null() {
            picam_call!(sys::Picam_DestroyModulations(ptr))?;
        }
        Ok(modulations)
    }

    fn can_set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<bool> {
        let mut raw_modulations: Vec<sys::PicamModulation> =
            value.iter().map(|m| m.to_raw()).collect();
        let raw = sys::PicamModulations {
            modulation_array: raw_modulations.as_mut_ptr(),
            modulation_count: raw_modulations.len() as sys::piint,
        };
        let mut settable: sys::pibln = 0;
        picam_call!(sys::Picam_CanSetParameterModulationsValue(
            handle.0,
            parameter.to_raw(),
            &raw,
            &mut settable
        ))?;
        Ok(settable != 0)
    }
}

impl PicamValue for Vec<Modulation> {}

/// A single pulse's delay and width.
///
/// Field meanings and units follow PICam manual 5.2.1.3.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pulse {
    /// The delay before the gate pulse starts, in ns.
    pub delay: sys::piflt,
    /// The width of the gate pulse, in ns.
    pub width: sys::piflt,
}

impl Pulse {
    fn from_raw(raw: &sys::PicamPulse) -> Self {
        Self {
            delay: raw.delay,
            width: raw.width,
        }
    }

    fn to_raw(self) -> sys::PicamPulse {
        sys::PicamPulse {
            delay: self.delay,
            width: self.width,
        }
    }
}

impl sealed::RawPicamValue for Pulse {
    fn get(handle: RawHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamPulse = std::ptr::null();
        picam_call!(sys::Picam_GetParameterPulseValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(crate::error::Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees it points to
        // one valid `PicamPulse` when non-null
        let value = Pulse::from_raw(unsafe { &*ptr });

        picam_call!(sys::Picam_DestroyPulses(ptr))?;
        Ok(value)
    }

    fn set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<()> {
        let raw = value.to_raw();
        picam_call!(sys::Picam_SetParameterPulseValue(
            handle.0,
            parameter.to_raw(),
            &raw
        ))
    }

    fn default_value(handle: RawHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamPulse = std::ptr::null();
        picam_call!(sys::Picam_GetParameterPulseDefaultValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(crate::error::Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees it points to
        // one valid `PicamPulse` when non-null
        let value = Pulse::from_raw(unsafe { &*ptr });

        picam_call!(sys::Picam_DestroyPulses(ptr))?;
        Ok(value)
    }

    fn can_set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<bool> {
        let raw = value.to_raw();
        let mut settable: sys::pibln = 0;
        picam_call!(sys::Picam_CanSetParameterPulseValue(
            handle.0,
            parameter.to_raw(),
            &raw,
            &mut settable
        ))?;
        Ok(settable != 0)
    }
}

impl PicamValue for Pulse {}

impl sealed::RawOnlineSettablePicamValue for Pulse {
    fn set_online(handle: RawHandle, parameter: Parameter, value: Self) -> Result<()> {
        let raw = value.to_raw();
        picam_call!(sys::Picam_SetParameterPulseValueOnline(
            handle.0,
            parameter.to_raw(),
            &raw
        ))
    }
}

impl OnlineSettablePicamValue for Pulse {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::assert_round_trips;

    #[test]
    fn parameter_enums_round_trip() {
        assert_round_trips!(ComputerInterface, sys::PicamComputerInterface, 5);
        assert_round_trips!(ValueType, sys::PicamValueType, 8);
        assert_round_trips!(ValueAccess, sys::PicamValueAccess, 3);
        assert_round_trips!(EnumeratedType, sys::PicamEnumeratedType, 57);
    }

    #[test]
    fn parameters_round_trip() {
        // picam packs a parameter's constraint type, value type and number
        // into its code (`PI_V` in picam.h), so cover every combination
        let mut named = 0;
        for constraint_type in 0..8 {
            for value_type in 0..10 {
                for number in 0..512 {
                    let code = (constraint_type << 24) + (value_type << 16) + number;
                    let parameter = Parameter::from_raw(sys::PicamParameter(code));
                    assert_eq!(
                        parameter.to_raw().0,
                        code,
                        "{parameter:?} should keep {code}"
                    );
                    if !matches!(parameter, Parameter::Unknown(_)) {
                        named += 1;
                    }
                }
            }
        }
        assert_eq!(named, 171, "every parameter in picam.h should be named");
    }
}

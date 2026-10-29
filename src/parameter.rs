//! PICam parameter identifiers and value access.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::mpsc::Receiver;

use crate::convert::{collect_raw_array, define_picam_enum, enumeration_string};
use crate::error::{Result, free_on_error, picam_call};
use crate::library::Library;
use crate::sys;
use crate::watch::{self, Event, Kind, Watch};

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
    /// A true/false value, used as `bool`.
    Boolean = PicamValueType_Boolean,
    /// One value of a PICam enum, used as that enum's own type, such as
    /// [`ShutterTimingMode`](crate::ShutterTimingMode).
    /// [`crate::Camera::enumerated_type`] says which enum.
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

/// Defines `Parameter` and the typed handles in `params`. Each entry gives
/// the variant, its raw name, its handle's name and value type, and PICam's
/// value type, plus for an enum parameter its `EnumeratedType` variant. The
/// last two are only used in docs here, but tests compare them with PICam's.
macro_rules! define_parameters {
    ($(
        $(#[$meta:meta])*
        $name:ident = $raw:ident, $handle:ident: $ty:ty, $value_type:ident $(($enumerated:ident))?;
    )*) => {
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
            /// Returns [`crate::Error::EnumerationValueNotDefined`] for a
            /// parameter PICam doesn't know, such as a [`Parameter::Unknown`]
            /// one (seen with SDK 5.15), or an error if PICam is unable to name
            /// this parameter.
            pub fn name(self, library: &Library) -> Result<String> {
                enumeration_string(
                    library,
                    sys::PicamEnumeratedType::PicamEnumeratedType_Parameter,
                    self.to_raw().0,
                )
            }
        }

        /// A typed handle for every parameter in the PICam SDK this crate was
        /// generated from (5.15), to pass to [`Camera::get`](crate::Camera::get)
        /// and the other value methods.
        ///
        /// Each handle's value type comes from the parameter's definition in
        /// PICam's header, so a wrong type doesn't compile. A parameter with no
        /// handle here, such as one a newer PICam runtime added, can still be
        /// used through [`Camera::get_untyped`](crate::Camera::get_untyped) and
        /// the other `_untyped` methods.
        pub mod params {
            use super::{Modulation, Parameter, Pulse, Roi, TypedParameter};
            use crate::enums::*;

            $(
                #[doc = concat!(
                    "[`Parameter::", stringify!($name), "`](crate::Parameter::",
                    stringify!($name), "), with values of type `", stringify!($ty),
                    "` (PICam value type `", stringify!($value_type), "`)."
                )]
                pub const $handle: TypedParameter<$ty> = TypedParameter::new(Parameter::$name);
            )*
        }
    };
}

include!("generated/parameters.rs");

/// A parameter together with the Rust type of its value, so the compiler
/// checks the type when getting or setting it. Every parameter has one in
/// [`params`], and none can be made any other way.
///
/// ```no_run
/// use picam_rs::{Camera, Result, ShutterTimingMode, params};
///
/// fn exposure_time(camera: &Camera) -> Result<f64> {
///     camera.get(params::EXPOSURE_TIME)
/// }
///
/// fn set_up(camera: &Camera) -> Result<()> {
///     camera.set(params::EXPOSURE_TIME, 5.0)?;
///     camera.set(params::ENABLE_INTENSIFIER, true)?;
///     camera.set(params::SHUTTER_TIMING_MODE, ShutterTimingMode::AlwaysOpen)?;
///     camera.set_online(params::EXPOSURE_TIME, 5.0)
/// }
/// ```
///
/// A value of the wrong type doesn't compile:
///
/// ```compile_fail,E0308
/// use picam_rs::{Camera, Result, params};
///
/// fn exposure_time(camera: &Camera) -> Result<i32> {
///     camera.get(params::EXPOSURE_TIME)
/// }
/// ```
///
/// ```compile_fail,E0308
/// use picam_rs::{Camera, Result, params};
///
/// fn set_exposure_time(camera: &Camera) -> Result<()> {
///     camera.set(params::EXPOSURE_TIME, 5_i32)
/// }
/// ```
///
/// Neither does an operation PICam doesn't support for the parameter's type,
/// such as setting an on/off or enum parameter online:
///
/// ```compile_fail,E0277
/// use picam_rs::{Camera, Result, params};
///
/// fn enable_intensifier(camera: &Camera) -> Result<()> {
///     camera.set_online(params::ENABLE_INTENSIFIER, true)
/// }
/// ```
///
/// ```compile_fail,E0277
/// use picam_rs::{Camera, Result, ShutterTimingMode, params};
///
/// fn open_shutter(camera: &Camera) -> Result<()> {
///     camera.set_online(params::SHUTTER_TIMING_MODE, ShutterTimingMode::AlwaysOpen)
/// }
/// ```
pub struct TypedParameter<T> {
    parameter: Parameter,
    value: PhantomData<fn() -> T>,
}

impl<T> TypedParameter<T> {
    /// Makes a handle. Only the generated constants in `params` call this, so
    /// every handle's type matches its parameter.
    const fn new(parameter: Parameter) -> Self {
        TypedParameter {
            parameter,
            value: PhantomData,
        }
    }

    /// Returns the untyped parameter this handle refers to.
    pub const fn parameter(self) -> Parameter {
        self.parameter
    }
}

impl<T> From<TypedParameter<T>> for Parameter {
    fn from(handle: TypedParameter<T>) -> Self {
        handle.parameter
    }
}

// written by hand rather than derived, since deriving would require `T` to
// implement each trait too, and `Vec<Roi>` isn't `Copy`
impl<T> Clone for TypedParameter<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for TypedParameter<T> {}

impl<T> fmt::Debug for TypedParameter<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TypedParameter")
            .field(&self.parameter)
            .finish()
    }
}

impl<T> PartialEq for TypedParameter<T> {
    fn eq(&self, other: &Self) -> bool {
        self.parameter == other.parameter
    }
}

impl<T> Eq for TypedParameter<T> {}

impl<T> Hash for TypedParameter<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.parameter.hash(state);
    }
}

/// The crate-private halves of the value traits, which seal them.
///
/// The public traits ([`PicamValue`], [`ReadablePicamValue`],
/// [`OnlineSettablePicamValue`] and [`WaitablePicamValue`]) have no methods of
/// their own, they only require the supertraits here. Users can't name these supertraits, so they
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
    use std::sync::mpsc::Receiver;

    use super::Parameter;
    use crate::error::Result;
    use crate::sys;
    use crate::watch::Watch;

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

        /// Checks, before an untyped call, that `parameter`'s values can be
        /// this type where PICam itself wouldn't notice a mismatch. Only enums
        /// need this, since PICam reads every enum through the same integer
        /// call. Typed handles skip it, their type is already right.
        ///
        /// # Errors
        ///
        /// Returns an error if the values can't be this type, or PICam is
        /// unable to check.
        fn check_type(_handle: RawHandle, _parameter: Parameter) -> Result<()> {
            Ok(())
        }

        /// Watches `parameter`'s value on `handle`, sending each value it's set to
        /// (PICam manual 7.4.8 and 7.4.9.1).
        ///
        /// # Errors
        ///
        /// Returns an error if PICam is unable to register the callback.
        fn watch(handle: RawHandle, parameter: Parameter) -> Result<(Watch, Receiver<Self>)>;
    }

    /// The handle-taking half of [`super::ReadablePicamValue`].
    pub trait RawReadablePicamValue: RawPicamValue {
        /// Reads the current value of `parameter` directly from hardware.
        ///
        /// # Errors
        ///
        /// Returns an error if PICam is unable to read the value.
        fn read(handle: RawHandle, parameter: Parameter) -> Result<Self>;

        /// Watches `parameter`'s value on `handle`, sending each value the hardware
        /// changes it to (PICam manual 7.4.9.3 and 7.4.9.7).
        ///
        /// # Errors
        ///
        /// Returns an error if PICam is unable to register the callback.
        fn watch_extrinsic(
            handle: RawHandle,
            parameter: Parameter,
        ) -> Result<(Watch, Receiver<Self>)>;
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

    /// The status half of [`super::WaitablePicamValue`]: converting to and
    /// from the `i32` PICam's status calls use.
    pub trait RawWaitablePicamValue: RawPicamValue {
        /// The value as PICam's status calls take it.
        fn to_status(self) -> i32;

        /// A value from PICam's status calls.
        fn from_status(code: i32) -> Self;
    }
}

use sealed::RawHandle;

/// A value type PICam can get or set for a parameter.
///
/// This trait is sealed: it is implemented for `i32`, `bool`, `i64`, `f64`,
/// `Vec<Roi>`, `Vec<Modulation>`, [`Pulse`] and the value enums of PICam's
/// enum parameters (such as [`ShutterTimingMode`](crate::ShutterTimingMode)),
/// and can't be implemented outside this crate. It is meant to be used as a
/// bound:
///
/// ```no_run
/// use picam_rs::{Camera, PicamValue, Result, TypedParameter};
///
/// fn current<T: PicamValue>(camera: &Camera, parameter: TypedParameter<T>) -> Result<T> {
///     camera.get(parameter)
/// }
/// ```
pub trait PicamValue: sealed::RawPicamValue {}

macro_rules! impl_picam_value {
    ($ty:ty, $get:ident, $set:ident, $default:ident, $can_set:ident, $kind:ident, $event:ident) => {
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

            fn watch(handle: RawHandle, parameter: Parameter) -> Result<(Watch, Receiver<Self>)> {
                watch::subscribe(handle.0, parameter, Kind::$kind, |event| match event {
                    Event::$event(value) => Some(*value),
                    _ => None,
                })
            }
        }

        impl PicamValue for $ty {}
    };
}

impl_picam_value!(
    i32,
    Picam_GetParameterIntegerValue,
    Picam_SetParameterIntegerValue,
    Picam_GetParameterIntegerDefaultValue,
    Picam_CanSetParameterIntegerValue,
    Integer,
    Integer
);
impl_picam_value!(
    i64,
    Picam_GetParameterLargeIntegerValue,
    Picam_SetParameterLargeIntegerValue,
    Picam_GetParameterLargeIntegerDefaultValue,
    Picam_CanSetParameterLargeIntegerValue,
    LargeInteger,
    LargeInteger
);
impl_picam_value!(
    f64,
    Picam_GetParameterFloatingPointValue,
    Picam_SetParameterFloatingPointValue,
    Picam_GetParameterFloatingPointDefaultValue,
    Picam_CanSetParameterFloatingPointValue,
    FloatingPoint,
    FloatingPoint
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
/// This trait is sealed: it is implemented only for `i32`, `bool`, `f64` and
/// the value enums of PICam's enum parameters, the value types PICam supports
/// this for, and can't be implemented outside this crate.
pub trait ReadablePicamValue: PicamValue + sealed::RawReadablePicamValue {}

macro_rules! impl_readable_value {
    ($ty:ty, $read:ident, $kind:ident, $event:ident) => {
        impl sealed::RawReadablePicamValue for $ty {
            fn read(handle: RawHandle, parameter: Parameter) -> Result<Self> {
                let mut value: $ty = Default::default();
                picam_call!(sys::$read(handle.0, parameter.to_raw(), &mut value))?;
                Ok(value)
            }

            fn watch_extrinsic(
                handle: RawHandle,
                parameter: Parameter,
            ) -> Result<(Watch, Receiver<Self>)> {
                watch::subscribe(handle.0, parameter, Kind::$kind, |event| match event {
                    Event::$event(value) => Some(*value),
                    _ => None,
                })
            }
        }

        impl ReadablePicamValue for $ty {}
    };
}

impl_readable_value!(
    i32,
    Picam_ReadParameterIntegerValue,
    ExtrinsicInteger,
    Integer
);
impl_readable_value!(
    f64,
    Picam_ReadParameterFloatingPointValue,
    ExtrinsicFloatingPoint,
    FloatingPoint
);

// picam gets and sets booleans through its integer calls, as 0 for false and
// 1 for true (PICam manual 5.1.1.42). 5.4.2.1, 5.4.2.3, 5.4.2.7 and 5.4.2.12
// list boolean parameters for get, set, default and read. 5.4.2.2 lists no
// types for can-set, but tests/handles.rs shows picam accepts them there

impl sealed::RawPicamValue for bool {
    fn get(handle: RawHandle, parameter: Parameter) -> Result<Self> {
        <i32 as sealed::RawPicamValue>::get(handle, parameter).map(|value| value != 0)
    }

    fn set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<()> {
        <i32 as sealed::RawPicamValue>::set(handle, parameter, i32::from(value))
    }

    fn default_value(handle: RawHandle, parameter: Parameter) -> Result<Self> {
        <i32 as sealed::RawPicamValue>::default_value(handle, parameter).map(|value| value != 0)
    }

    fn can_set(handle: RawHandle, parameter: Parameter, value: Self) -> Result<bool> {
        <i32 as sealed::RawPicamValue>::can_set(handle, parameter, i32::from(value))
    }

    fn watch(handle: RawHandle, parameter: Parameter) -> Result<(Watch, Receiver<Self>)> {
        watch::subscribe(handle.0, parameter, Kind::Integer, |event| match event {
            Event::Integer(value) => Some(*value != 0),
            _ => None,
        })
    }
}

impl PicamValue for bool {}

impl sealed::RawReadablePicamValue for bool {
    fn read(handle: RawHandle, parameter: Parameter) -> Result<Self> {
        <i32 as sealed::RawReadablePicamValue>::read(handle, parameter).map(|value| value != 0)
    }

    fn watch_extrinsic(handle: RawHandle, parameter: Parameter) -> Result<(Watch, Receiver<Self>)> {
        watch::subscribe(
            handle.0,
            parameter,
            Kind::ExtrinsicInteger,
            |event| match event {
                Event::Integer(value) => Some(*value != 0),
                _ => None,
            },
        )
    }
}

impl ReadablePicamValue for bool {}

/// A value type that can be set and applied to hardware immediately,
/// without needing [`crate::Camera::commit_parameters`].
///
/// This trait is sealed: it is implemented only for `i32`, `f64` and
/// [`Pulse`], the three value types PICam supports this for, and can't be
/// implemented outside this crate. `bool` isn't one, since PICam only sets
/// integer parameters online, not on/off ones (PICam manual 5.4.1.20).
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

impl_online_settable_scalar!(i32, Picam_SetParameterIntegerValueOnline);
impl_online_settable_scalar!(f64, Picam_SetParameterFloatingPointValueOnline);

/// A value type a waitable status parameter can have, so the value waited
/// for is checked against the parameter at compile time.
///
/// This trait is sealed: it is implemented for the value enums of PICam's
/// enum parameters, and can't be implemented outside this crate. The manual
/// doesn't list which parameters are waitable statuses, so every value enum
/// implements it, and
/// [`Camera::can_wait_for_status`](crate::Camera::can_wait_for_status) says
/// which parameters are. PICam refuses the rest.
///
/// ```no_run
/// use std::time::Duration;
/// use picam_rs::{Camera, Result, SensorTemperatureStatus, params};
///
/// fn wait_until_cold(camera: &Camera) -> Result<()> {
///     camera.wait_for_status_parameter_value(
///         params::SENSOR_TEMPERATURE_STATUS,
///         SensorTemperatureStatus::Locked,
///         Some(Duration::from_secs(600)),
///     )
/// }
/// ```
///
/// A value of another enum doesn't compile:
///
/// ```compile_fail,E0308
/// use picam_rs::{Camera, CoolingFanStatus, Result, params};
///
/// fn wait(camera: &Camera) -> Result<()> {
///     camera.wait_for_status_parameter_value(
///         params::SENSOR_TEMPERATURE_STATUS,
///         CoolingFanStatus::On,
///         None,
///     )
/// }
/// ```
///
/// Neither does a parameter whose values aren't an enum:
///
/// ```compile_fail,E0277
/// use picam_rs::{Camera, Result, params};
///
/// fn wait(camera: &Camera) -> Result<()> {
///     camera.wait_for_status_parameter_value(params::EXPOSURE_TIME, 5.0, None)
/// }
/// ```
pub trait WaitablePicamValue: PicamValue + sealed::RawWaitablePicamValue {}

/// A single region of interest.
///
/// Field meanings and units follow PICam manual 5.2.1.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roi {
    /// The left-most column, counting from zero.
    pub x: i32,
    /// How many columns the region spans.
    pub width: i32,
    /// How many adjacent columns are summed into one.
    pub x_binning: i32,
    /// The top-most row, counting from zero.
    pub y: i32,
    /// How many rows the region spans.
    pub height: i32,
    /// How many adjacent rows are summed into one.
    pub y_binning: i32,
}

impl Roi {
    pub(crate) fn from_raw(raw: &sys::PicamRoi) -> Self {
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
        let result = picam_call!(sys::Picam_GetParameterRoisValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::Picam_DestroyRois)?;

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
        let result = picam_call!(sys::Picam_GetParameterRoisDefaultValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::Picam_DestroyRois)?;

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

    fn watch(handle: RawHandle, parameter: Parameter) -> Result<(Watch, Receiver<Self>)> {
        watch::subscribe(handle.0, parameter, Kind::Rois, |event| match event {
            Event::Rois(rois) => Some(rois.clone()),
            _ => None,
        })
    }
}

impl PicamValue for Vec<Roi> {}

/// A single step in a custom intensifier modulation sequence.
///
/// Field meanings and units follow PICam manual 5.2.1.4.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Modulation {
    /// How long the intensifier modulates for, in ms.
    pub duration: f64,
    /// The intensifier modulation frequency, in MHz.
    pub frequency: f64,
    /// The intensifier's phase relative to the modulation output signal, in degrees.
    pub phase: f64,
    /// The frequency of the RF output signal, in MHz.
    pub output_signal_frequency: f64,
}

impl Modulation {
    pub(crate) fn from_raw(raw: &sys::PicamModulation) -> Self {
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
        let result = picam_call!(sys::Picam_GetParameterModulationsValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::Picam_DestroyModulations)?;

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
        let result = picam_call!(sys::Picam_GetParameterModulationsDefaultValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::Picam_DestroyModulations)?;

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

    fn watch(handle: RawHandle, parameter: Parameter) -> Result<(Watch, Receiver<Self>)> {
        watch::subscribe(
            handle.0,
            parameter,
            Kind::Modulations,
            |event| match event {
                Event::Modulations(modulations) => Some(modulations.clone()),
                _ => None,
            },
        )
    }
}

impl PicamValue for Vec<Modulation> {}

/// A single pulse's delay and width.
///
/// Field meanings and units follow PICam manual 5.2.1.3.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pulse {
    /// The delay before the gate pulse starts, in ns.
    pub delay: f64,
    /// The width of the gate pulse, in ns.
    pub width: f64,
}

impl Pulse {
    pub(crate) fn from_raw(raw: &sys::PicamPulse) -> Self {
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
        let result = picam_call!(sys::Picam_GetParameterPulseValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::Picam_DestroyPulses)?;

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
        let result = picam_call!(sys::Picam_GetParameterPulseDefaultValue(
            handle.0,
            parameter.to_raw(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::Picam_DestroyPulses)?;

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

    fn watch(handle: RawHandle, parameter: Parameter) -> Result<(Watch, Receiver<Self>)> {
        watch::subscribe(handle.0, parameter, Kind::Pulse, |event| match event {
            Event::Pulse(pulse) => Some(*pulse),
            _ => None,
        })
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

bitflags::bitflags! {
    /// Which of a parameter's attributes may change, from `dynamics` and
    /// `extrinsic_dynamics` on [`Camera`](crate::Camera) and
    /// [`Accessory`](crate::Accessory) (PICam manual 7.1.3.1).
    ///
    /// No flags set means none of them may change. Bits this crate doesn't
    /// recognize are kept, not dropped.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Dynamics: i32 {
        /// The parameter's value may change.
        const VALUE = sys::PicamDynamicsMask::PicamDynamicsMask_Value.0;
        /// The parameter's value access may change.
        const VALUE_ACCESS = sys::PicamDynamicsMask::PicamDynamicsMask_ValueAccess.0;
        /// Whether the parameter is relevant may change.
        const IS_RELEVANT = sys::PicamDynamicsMask::PicamDynamicsMask_IsRelevant.0;
        /// The parameter's dependent constraints may change.
        const CONSTRAINT = sys::PicamDynamicsMask::PicamDynamicsMask_Constraint.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::assert_round_trips;

    #[test]
    fn dynamics_keep_unknown_bits() {
        let none = sys::PicamDynamicsMask::PicamDynamicsMask_None;
        assert!(Dynamics::from_bits_retain(none.0).is_empty());
        let raw = Dynamics::VALUE.bits() | Dynamics::CONSTRAINT.bits() | 0x40;
        let dynamics = Dynamics::from_bits_retain(raw);
        assert!(dynamics.contains(Dynamics::VALUE | Dynamics::CONSTRAINT));
        assert_eq!(dynamics.bits(), raw);
    }

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

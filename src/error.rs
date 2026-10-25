//! Error types for `picam-rs`

use crate::parameter::EnumeratedType;
use crate::sys;

/// Result type used throughout `picam-rs`.
pub type Result<T> = std::result::Result<T, Error>;

/// A PICam error, translated from the raw `PicamError` code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    // --- General Errors ---
    /// An unexpected error occurred in PICam.
    #[error("an unexpected error occurred")]
    UnexpectedError,
    /// An unexpected null pointer was encountered. This crate also returns it
    /// when PICam gives back a null pointer where a single struct was expected.
    #[error("an unexpected null pointer was encountered")]
    UnexpectedNullPointer,
    /// PICam was passed a pointer it considers invalid.
    #[error("an invalid pointer was supplied")]
    InvalidPointer,
    /// PICam was passed an invalid element count.
    #[error("an invalid count was supplied")]
    InvalidCount,
    /// PICam was passed an enumerated type it doesn't recognize.
    #[error("an invalid enumerated type was supplied")]
    InvalidEnumeratedType,
    /// An enumeration value isn't defined for its enumerated type.
    #[error("an enumeration value was not defined")]
    EnumerationValueNotDefined,
    /// The operation is invalid.
    #[error("the operation is invalid")]
    InvalidOperation,
    /// The operation was canceled before it finished.
    #[error("the operation was canceled")]
    OperationCanceled,

    // --- Library Initialization Errors ---
    /// The call needs an initialized library, but no [`crate::Library`] is alive.
    #[error("the PICam library has not been initialized")]
    LibraryNotInitialized,
    /// The library is already initialized, usually because another [`crate::Library`] is alive.
    #[error("the PICam library has already been initialized")]
    LibraryAlreadyInitialized,

    // --- Camera/Accessory Plug & Play Discovery Errors ---
    /// Camera discovery was expected to be running, but isn't.
    #[error("not currently discovering cameras")]
    NotDiscoveringCameras,
    /// Camera discovery is already running.
    #[error("already discovering cameras")]
    AlreadyDiscoveringCameras,
    /// Accessory discovery was expected to be running, but isn't.
    #[error("not currently discovering accessories")]
    NotDiscoveringAccessories,
    /// Accessory discovery is already running.
    #[error("already discovering accessories")]
    AlreadyDiscoveringAccessories,

    // --- Camera/Accessory Access Errors ---
    /// No cameras are available to open.
    #[error("no cameras are available")]
    NoCamerasAvailable,
    /// The camera is already open in this process.
    #[error("the camera is already open")]
    CameraAlreadyOpened,
    /// The camera ID doesn't identify an available camera.
    #[error("an invalid camera ID was supplied")]
    #[doc(alias = "InvalidCameraID")]
    InvalidCameraId,
    /// No accessories are available to open.
    #[error("no accessories are available")]
    NoAccessoriesAvailable,
    /// The accessory is already open in this process.
    #[error("the accessory is already open")]
    AccessoryAlreadyOpened,
    /// The accessory ID doesn't identify an available accessory.
    #[error("an invalid accessory ID was supplied")]
    #[doc(alias = "InvalidAccessoryID")]
    InvalidAccessoryId,
    /// The handle is invalid. This is expected for a handle that has since been
    /// closed, but that is not yet confirmed (see the README's "to test" list).
    #[error("an invalid handle was supplied")]
    InvalidHandle,
    /// PICam couldn't communicate with the device.
    #[error("communication with the device failed")]
    DeviceCommunicationFailed,
    /// The device has been disconnected.
    #[error("the device was disconnected")]
    DeviceDisconnected,
    /// The device is already open in another process.
    #[error("the device is already open elsewhere")]
    DeviceOpenElsewhere,

    // --- Demo Camera Errors ---
    /// The model can't be used to create a demo camera.
    #[error("an invalid demo camera model was supplied")]
    InvalidDemoModel,
    /// The serial number can't be used for a demo camera.
    #[error("an invalid demo camera serial number was supplied")]
    InvalidDemoSerialNumber,
    /// A demo camera with this ID is already connected.
    #[error("a demo camera with this ID is already connected")]
    DemoAlreadyConnected,
    /// The operation doesn't support demo cameras.
    #[error("demo cameras are not supported")]
    DemoNotSupported,
    /// The operation only supports demo cameras.
    #[error("only demo cameras are supported")]
    OnlyDemoSupported,

    // --- Camera/Accessory Parameter Access Errors ---
    /// The parameter's value type doesn't match the one the call works with.
    #[error("the parameter has an invalid value type")]
    ParameterHasInvalidValueType,
    /// The parameter's constraint type doesn't match the one the call works with.
    #[error("the parameter has an invalid constraint type")]
    ParameterHasInvalidConstraintType,
    /// The parameter doesn't exist on this camera.
    #[error("the parameter does not exist")]
    ParameterDoesNotExist,
    /// The parameter is read-only and can't be set.
    #[error("the parameter value is read-only")]
    ParameterValueIsReadOnly,
    /// The value isn't valid for the parameter.
    #[error("an invalid parameter value was supplied")]
    InvalidParameterValue,
    /// The constraint category isn't valid.
    #[error("an invalid constraint category was supplied")]
    InvalidConstraintCategory,
    /// The parameter isn't relevant in the camera's current configuration.
    #[error("the parameter value is irrelevant in the current configuration")]
    ParameterValueIsIrrelevant,
    /// The parameter can't be set online.
    #[error("the parameter cannot be set online (i.e. during acquisition)")]
    ParameterIsNotOnlineable,
    /// The parameter can't be read directly from hardware.
    #[error("the parameter is not readable")]
    ParameterIsNotReadable,
    /// The parameter isn't a status parameter that can be waited on.
    #[error("the parameter is not a waitable status parameter")]
    ParameterIsNotWaitableStatus,
    /// The timeout given for waiting on a status parameter isn't valid.
    #[error("an invalid timeout was supplied for a waitable status parameter")]
    InvalidWaitableStatusParameterTimeOut,

    // --- Camera Data Acquisition Errors ---
    /// One or more of the camera's parameter values are invalid.
    #[error("one or more parameter values are invalid")]
    InvalidParameterValues,
    /// The camera has parameter changes that haven't been committed.
    #[error("parameters have not been committed")]
    ParametersNotCommitted,
    /// The acquisition buffer isn't valid.
    #[error("an invalid acquisition buffer was supplied")]
    InvalidAcquisitionBuffer,
    /// The readout count isn't valid.
    #[error("an invalid readout count was supplied")]
    InvalidReadoutCount,
    /// The readout timeout isn't valid.
    #[error("an invalid readout timeout was supplied")]
    InvalidReadoutTimeOut,
    /// There isn't enough memory for the operation.
    #[error("insufficient memory is available")]
    InsufficientMemory,
    /// An acquisition is already running.
    #[error("an acquisition is already in progress")]
    AcquisitionInProgress,
    /// No acquisition is running.
    #[error("no acquisition is in progress")]
    AcquisitionNotInProgress,
    /// The operation timed out.
    #[error("the operation timed out")]
    TimeOutOccurred,
    /// An acquisition-updated handler is already registered.
    #[error("an acquisition-updated handler is already registered")]
    AcquisitionUpdatedHandlerRegistered,
    /// The acquisition is in a state that doesn't allow the operation.
    #[error("the acquisition state is invalid for this operation")]
    InvalidAcquisitionState,
    /// Nondestructive readout is enabled, which doesn't allow the operation.
    #[error("nondestructive readout is enabled, which is incompatible with this operation")]
    NondestructiveReadoutEnabled,
    /// The shutter has overheated.
    #[error("the shutter has overheated")]
    ShutterOverheated,
    /// The center wavelength drive has faulted.
    #[error("the center wavelength drive has faulted")]
    CenterWavelengthFaulted,
    /// The camera has faulted.
    #[error("the camera has faulted")]
    CameraFaulted,

    /// A PICam error code this crate doesn't recognize.
    #[error("unrecognized PICam error code: {0}")]
    Unknown(i32),

    // --- crate-side errors ---
    /// A value is too long for the fixed-size buffer PICam uses for it.
    #[error("value too long to fit in PICam's fixed-size buffer")]
    ValueTooLong,
    /// A string contains an embedded null byte, so it can't be passed to PICam.
    #[error("string contains an embedded null byte")]
    InvalidString,
    /// This [`Library`](crate::Library) has already connected 64 different
    /// demo cameras (model and serial number pairs). PICam 5.15 corrupts its
    /// own memory when one more is connected, so
    /// [`CameraId::connect_demo`](crate::CameraId::connect_demo) refuses
    /// instead. Dropping the `Library` and making a new one resets the count.
    #[error("too many different demo cameras connected with this library")]
    TooManyDemoCameras,
    /// An untyped call such as
    /// [`Camera::get_untyped`](crate::Camera::get_untyped) asked for an enum
    /// parameter's value as a different enum from the one PICam says its
    /// values belong to. PICam reads every enum through the same integer
    /// call, so it can't catch this itself.
    #[error("the parameter's values are {found:?}, not {expected:?}")]
    EnumeratedTypeMismatch {
        /// The enum the call asked for.
        expected: EnumeratedType,
        /// The enum PICam says the parameter's values belong to.
        found: EnumeratedType,
    },
}

impl Error {
    /// Converts a raw `PicamError` (as returned by the SDK) into an [`Error`].
    pub(crate) fn from_raw(code: sys::PicamError) -> Self {
        match code {
            sys::PicamError::PicamError_UnexpectedError => Error::UnexpectedError,
            sys::PicamError::PicamError_UnexpectedNullPointer => Error::UnexpectedNullPointer,
            sys::PicamError::PicamError_InvalidPointer => Error::InvalidPointer,
            sys::PicamError::PicamError_InvalidCount => Error::InvalidCount,
            sys::PicamError::PicamError_InvalidEnumeratedType => Error::InvalidEnumeratedType,
            sys::PicamError::PicamError_EnumerationValueNotDefined => {
                Error::EnumerationValueNotDefined
            }
            sys::PicamError::PicamError_InvalidOperation => Error::InvalidOperation,
            sys::PicamError::PicamError_OperationCanceled => Error::OperationCanceled,
            sys::PicamError::PicamError_LibraryNotInitialized => Error::LibraryNotInitialized,
            sys::PicamError::PicamError_LibraryAlreadyInitialized => {
                Error::LibraryAlreadyInitialized
            }
            sys::PicamError::PicamError_NotDiscoveringCameras => Error::NotDiscoveringCameras,
            sys::PicamError::PicamError_AlreadyDiscoveringCameras => {
                Error::AlreadyDiscoveringCameras
            }
            sys::PicamError::PicamError_NotDiscoveringAccessories => {
                Error::NotDiscoveringAccessories
            }
            sys::PicamError::PicamError_AlreadyDiscoveringAccessories => {
                Error::AlreadyDiscoveringAccessories
            }
            sys::PicamError::PicamError_NoCamerasAvailable => Error::NoCamerasAvailable,
            sys::PicamError::PicamError_CameraAlreadyOpened => Error::CameraAlreadyOpened,
            sys::PicamError::PicamError_InvalidCameraID => Error::InvalidCameraId,
            sys::PicamError::PicamError_NoAccessoriesAvailable => Error::NoAccessoriesAvailable,
            sys::PicamError::PicamError_AccessoryAlreadyOpened => Error::AccessoryAlreadyOpened,
            sys::PicamError::PicamError_InvalidAccessoryID => Error::InvalidAccessoryId,
            sys::PicamError::PicamError_InvalidHandle => Error::InvalidHandle,
            sys::PicamError::PicamError_DeviceCommunicationFailed => {
                Error::DeviceCommunicationFailed
            }
            sys::PicamError::PicamError_DeviceDisconnected => Error::DeviceDisconnected,
            sys::PicamError::PicamError_DeviceOpenElsewhere => Error::DeviceOpenElsewhere,
            sys::PicamError::PicamError_InvalidDemoModel => Error::InvalidDemoModel,
            sys::PicamError::PicamError_InvalidDemoSerialNumber => Error::InvalidDemoSerialNumber,
            sys::PicamError::PicamError_DemoAlreadyConnected => Error::DemoAlreadyConnected,
            sys::PicamError::PicamError_DemoNotSupported => Error::DemoNotSupported,
            sys::PicamError::PicamError_OnlyDemoSupported => Error::OnlyDemoSupported,
            sys::PicamError::PicamError_ParameterHasInvalidValueType => {
                Error::ParameterHasInvalidValueType
            }
            sys::PicamError::PicamError_ParameterHasInvalidConstraintType => {
                Error::ParameterHasInvalidConstraintType
            }
            sys::PicamError::PicamError_ParameterDoesNotExist => Error::ParameterDoesNotExist,
            sys::PicamError::PicamError_ParameterValueIsReadOnly => Error::ParameterValueIsReadOnly,
            sys::PicamError::PicamError_InvalidParameterValue => Error::InvalidParameterValue,
            sys::PicamError::PicamError_InvalidConstraintCategory => {
                Error::InvalidConstraintCategory
            }
            sys::PicamError::PicamError_ParameterValueIsIrrelevant => {
                Error::ParameterValueIsIrrelevant
            }
            sys::PicamError::PicamError_ParameterIsNotOnlineable => Error::ParameterIsNotOnlineable,
            sys::PicamError::PicamError_ParameterIsNotReadable => Error::ParameterIsNotReadable,
            sys::PicamError::PicamError_ParameterIsNotWaitableStatus => {
                Error::ParameterIsNotWaitableStatus
            }
            sys::PicamError::PicamError_InvalidWaitableStatusParameterTimeOut => {
                Error::InvalidWaitableStatusParameterTimeOut
            }
            sys::PicamError::PicamError_InvalidParameterValues => Error::InvalidParameterValues,
            sys::PicamError::PicamError_ParametersNotCommitted => Error::ParametersNotCommitted,
            sys::PicamError::PicamError_InvalidAcquisitionBuffer => Error::InvalidAcquisitionBuffer,
            sys::PicamError::PicamError_InvalidReadoutCount => Error::InvalidReadoutCount,
            sys::PicamError::PicamError_InvalidReadoutTimeOut => Error::InvalidReadoutTimeOut,
            sys::PicamError::PicamError_InsufficientMemory => Error::InsufficientMemory,
            sys::PicamError::PicamError_AcquisitionInProgress => Error::AcquisitionInProgress,
            sys::PicamError::PicamError_AcquisitionNotInProgress => Error::AcquisitionNotInProgress,
            sys::PicamError::PicamError_TimeOutOccurred => Error::TimeOutOccurred,
            sys::PicamError::PicamError_AcquisitionUpdatedHandlerRegistered => {
                Error::AcquisitionUpdatedHandlerRegistered
            }
            sys::PicamError::PicamError_InvalidAcquisitionState => Error::InvalidAcquisitionState,
            sys::PicamError::PicamError_NondestructiveReadoutEnabled => {
                Error::NondestructiveReadoutEnabled
            }
            sys::PicamError::PicamError_ShutterOverheated => Error::ShutterOverheated,
            sys::PicamError::PicamError_CenterWavelengthFaulted => Error::CenterWavelengthFaulted,
            sys::PicamError::PicamError_CameraFaulted => Error::CameraFaulted,

            // `PicamError` is generated by bindgen as a "newtype" (a thin wrapper
            // around a plain integer, rather than a real Rust `enum`) specifically so
            // that a code this crate doesn't recognize (for example, one introduced
            // by a PICam runtime newer than the SDK version these bindings were
            // generated against) can still be represented safely, as
            // [`Error::Unknown`], instead of constructing an invalid enum
            // discriminant (which would be undefined behavior)
            other => Error::Unknown(other.0),
        }
    }
}

/// Calls a raw PICam sys function and converts its `PicamError` return
/// value into a [`Result`].
///
/// # Safety
/// The wrapped expression must be a valid call to a PICam sys function.
/// This macro makes the return value safe to handle, not the call itself.
macro_rules! picam_call {
    ($expr:expr) => {{
        // SAFETY: upheld by the caller, see the `# Safety` section above
        let code = unsafe { $expr };
        if code == $crate::sys::PicamError::PicamError_None {
            Ok(())
        } else {
            Err($crate::error::Error::from_raw(code))
        }
    }};
}
pub(crate) use picam_call;

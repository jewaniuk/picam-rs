//! Error types for `picam-rs`

use crate::sys;

/// Result type used throughout `picam-rs`.
pub type Result<T> = std::result::Result<T, Error>;

/// A PICam error, translated from the raw `PicamError` code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    // --- General Errors ---
    #[error("an unexpected error occurred")]
    UnexpectedError,
    #[error("an unexpected null pointer was encountered")]
    UnexpectedNullPointer,
    #[error("an invalid pointer was supplied")]
    InvalidPointer,
    #[error("an invalid count was supplied")]
    InvalidCount,
    #[error("an invalid enumerated type was supplied")]
    InvalidEnumeratedType,
    #[error("an enumeration value was not defined")]
    EnumerationValueNotDefined,
    #[error("the operation is invalid")]
    InvalidOperation,
    #[error("the operation was canceled")]
    OperationCanceled,

    // --- Library Initialization Errors ---
    #[error("the PICam library has not been initialized")]
    LibraryNotInitialized,
    #[error("the PICam library has already been initialized")]
    LibraryAlreadyInitialized,

    // --- Camera/Accessory Plug & Play Discovery Errors ---
    #[error("not currently discovering cameras")]
    NotDiscoveringCameras,
    #[error("already discovering cameras")]
    AlreadyDiscoveringCameras,
    #[error("not currently discovering accessories")]
    NotDiscoveringAccessories,
    #[error("already discovering accessories")]
    AlreadyDiscoveringAccessories,

    // --- Camera/Accessory Access Errors ---
    #[error("no cameras are available")]
    NoCamerasAvailable,
    #[error("the camera is already open")]
    CameraAlreadyOpened,
    #[error("an invalid camera ID was supplied")]
    InvalidCameraID,
    #[error("no accessories are available")]
    NoAccessoriesAvailable,
    #[error("the accessory is already open")]
    AccessoryAlreadyOpened,
    #[error("an invalid accessory ID was supplied")]
    InvalidAccessoryID,
    #[error("an invalid handle was supplied")]
    InvalidHandle,
    #[error("communication with the device failed")]
    DeviceCommunicationFailed,
    #[error("the device was disconnected")]
    DeviceDisconnected,
    #[error("the device is already open elsewhere")]
    DeviceOpenElsewhere,

    // --- Demo Camera Errors ---
    #[error("an invalid demo camera model was supplied")]
    InvalidDemoModel,
    #[error("an invalid demo camera serial number was supplied")]
    InvalidDemoSerialNumber,
    #[error("a demo camera with this ID is already connected")]
    DemoAlreadyConnected,
    #[error("demo cameras are not supported")]
    DemoNotSupported,
    #[error("only demo cameras are supported")]
    OnlyDemoSupported,

    // --- Camera/Accessory Parameter Access Errors ---
    #[error("the parameter has an invalid value type")]
    ParameterHasInvalidValueType,
    #[error("the parameter has an invalid constraint type")]
    ParameterHasInvalidConstraintType,
    #[error("the parameter does not exist")]
    ParameterDoesNotExist,
    #[error("the parameter value is read-only")]
    ParameterValueIsReadOnly,
    #[error("an invalid parameter value was supplied")]
    InvalidParameterValue,
    #[error("an invalid constraint category was supplied")]
    InvalidConstraintCategory,
    #[error("the parameter value is irrelevant in the current configuration")]
    ParameterValueIsIrrelevant,
    #[error("the parameter cannot be set online (i.e. during acquisition)")]
    ParameterIsNotOnlineable,
    #[error("the parameter is not readable")]
    ParameterIsNotReadable,
    #[error("the parameter is not a waitable status parameter")]
    ParameterIsNotWaitableStatus,
    #[error("an invalid timeout was supplied for a waitable status parameter")]
    InvalidWaitableStatusParameterTimeOut,

    // --- Camera Data Acquisition Errors ---
    #[error("one or more parameter values are invalid")]
    InvalidParameterValues,
    #[error("parameters have not been committed")]
    ParametersNotCommitted,
    #[error("an invalid acquisition buffer was supplied")]
    InvalidAcquisitionBuffer,
    #[error("an invalid readout count was supplied")]
    InvalidReadoutCount,
    #[error("an invalid readout timeout was supplied")]
    InvalidReadoutTimeOut,
    #[error("insufficient memory is available")]
    InsufficientMemory,
    #[error("an acquisition is already in progress")]
    AcquisitionInProgress,
    #[error("no acquisition is in progress")]
    AcquisitionNotInProgress,
    #[error("the operation timed out")]
    TimeOutOccurred,
    #[error("an acquisition-updated handler is already registered")]
    AcquisitionUpdatedHandlerRegistered,
    #[error("the acquisition state is invalid for this operation")]
    InvalidAcquisitionState,
    #[error("nondestructive readout is enabled, which is incompatible with this operation")]
    NondestructiveReadoutEnabled,
    #[error("the shutter has overheated")]
    ShutterOverheated,
    #[error("the center wavelength drive has faulted")]
    CenterWavelengthFaulted,
    #[error("the camera has faulted")]
    CameraFaulted,

    /// A PICam error code this crate doesn't recognize.
    #[error("unrecognized PICam error code: {0}")]
    Unknown(i32),

    /// Errors that do not come from the PICam API.
    #[error("value too long to fit in PICam's fixed-size buffer")]
    ValueTooLong,
    #[error("string contains an embedded null byte")]
    InvalidString,
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
            sys::PicamError::PicamError_InvalidCameraID => Error::InvalidCameraID,
            sys::PicamError::PicamError_NoAccessoriesAvailable => Error::NoAccessoriesAvailable,
            sys::PicamError::PicamError_AccessoryAlreadyOpened => Error::AccessoryAlreadyOpened,
            sys::PicamError::PicamError_InvalidAccessoryID => Error::InvalidAccessoryID,
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
        let code = unsafe { $expr };
        if code == $crate::sys::PicamError::PicamError_None {
            Ok(())
        } else {
            Err($crate::error::Error::from_raw(code))
        }
    }};
}
pub(crate) use picam_call;

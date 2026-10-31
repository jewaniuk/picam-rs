//! A safe Rust wrapper around the Teledyne Princeton Instruments PICam C API,
//! which controls their scientific cameras.
//!
//! It wraps all 199 of the API's functions and all 58 of its enums.
//!
//! **This crate is experimental.** Much of it has only been tested against
//! PICam's demo cameras, with only part checked on real hardware (a PIXIS
//! 100BR), and its API may change.
//!
//! It only runs on Windows, and needs the PICam SDK installed: the build
//! links `Picam.lib`, from the `Libraries` directory beside `PICAM_DIR` or
//! from the SDK's default install path. The raw bindings come with the
//! crate, so building doesn't read the SDK's headers, and no SDK files are
//! shipped with it.
//!
//! Everything starts with a [`Library`], which initializes PICam and keeps it
//! initialized for as long as it or any [`Camera`] opened with it is alive:
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use picam_rs::{Camera, Library, params};
//!
//! # fn main() -> picam_rs::Result<()> {
//! let library = Arc::new(Library::new()?);
//! let camera = Camera::open_first(library)?;
//! let exposure_time = camera.get(params::EXPOSURE_TIME)?;
//! println!("exposure time: {exposure_time}");
//! # Ok(())
//! # }
//! ```

mod accessory;
mod acquisition;
mod camera;
mod camera_device;
mod constraint;
mod convert;
mod defect;
mod device;
mod discovery;
mod em_calibration;
mod enums;
mod error;
mod library;
mod model;
mod parameter;
mod sys;
mod user_state;
mod validation;
mod watch;

pub use accessory::{Accessory, AccessoryHandle, AccessoryId, CalibrationPoint};
pub use acquisition::{
    Acquisition, AcquisitionErrors, AcquisitionState, AcquisitionStateErrors,
    AcquisitionStateUpdate, AcquisitionStream, Frame, Metadata, Pixels, Readout, Readouts, RoiData,
    StreamedUpdate, TimeStamp, Update,
};
pub use camera::{Camera, CameraHandle, CameraId, HandleType};
pub use camera_device::CameraDevice;
pub use constraint::{
    CollectionConstraint, ConstraintCategory, ConstraintScope, ConstraintSeverity, ConstraintType,
    ModulationsConstraint, PulseConstraint, RangeConstraint, RoisConstraint, RoisConstraintRules,
};
pub use convert::FirmwareDetail;
pub use defect::{
    ColumnDefect, PixelDefectMap, PixelLocation, RowDefect, StarDefect, StarDefectMap,
};
pub use discovery::{AccessoryDiscovery, CameraDiscovery, DiscoveryAction};
pub use em_calibration::{EmCalibration, EmCalibrationDate, EmCalibrationHandle};
pub use enums::*;
pub use error::{Error, Result};
pub use library::{Library, Version};
pub use model::Model;
pub use parameter::{
    ComputerInterface, Dynamics, EnumeratedType, Modulation, OnlineSettablePicamValue, Parameter,
    PicamValue, Pulse, ReadablePicamValue, Roi, TypedParameter, ValueAccess, ValueType,
    WaitablePicamValue, params,
};
pub use validation::{
    DependentValidationResult, FailedDependentParameter, ValidationResult, ValidationResults,
};
pub use watch::Watch;

/// Compiles the README's example with the doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
